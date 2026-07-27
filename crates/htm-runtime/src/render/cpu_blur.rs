use super::{BackendError, BackendErrorKind, MAX_EFFECT_SURFACE_BYTES};

const DIRECT_GAUSSIAN_THRESHOLD: f64 = 2.0;
const CHANNELS_PER_PIXEL: usize = 4;
const MASK_CHANNELS: usize = 1;
const BOX_FIXED_SCALE: f64 = 257.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CpuBlurAlgorithm {
    DirectGaussian,
    ThreeBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BoxBlurPass {
    pub before: u32,
    pub after: u32,
}

impl BoxBlurPass {
    fn width(self) -> u32 {
        self.before + self.after + 1
    }
}

#[derive(Debug, Default)]
pub(super) struct CpuBlurScratch {
    pixels: Vec<u8>,
    dimensions: Option<(u32, u32)>,
    plane: Vec<u16>,
    plane_dimensions: Option<(u32, u32)>,
    line_a: Vec<f64>,
    line_b: Vec<f64>,
    line_length: usize,
}

impl CpuBlurScratch {
    pub(super) fn allocated_bytes(&self) -> usize {
        self.pixels
            .capacity()
            .saturating_add(auxiliary_storage_bytes(
                self.plane.capacity(),
                self.line_a.capacity(),
                self.line_b.capacity(),
            ))
    }

    fn take(
        &mut self,
        width: u32,
        height: u32,
        byte_len: usize,
        committed_surface_bytes: usize,
    ) -> Result<(Vec<u8>, bool), BackendError> {
        let available = MAX_EFFECT_SURFACE_BYTES
            .checked_sub(committed_surface_bytes)
            .ok_or_else(|| blur_error("CPU blur scratch budget underflowed"))?;
        if byte_len > available {
            return Err(blur_error(
                "CPU blur scratch exceeds the per-surface effect budget",
            ));
        }

        if byte_len
            .checked_add(auxiliary_storage_bytes(
                self.plane.capacity(),
                self.line_a.capacity(),
                self.line_b.capacity(),
            ))
            .is_none_or(|total| total > available)
        {
            self.plane = Vec::new();
            self.plane_dimensions = None;
            self.line_a = Vec::new();
            self.line_b = Vec::new();
            self.line_length = 0;
        }
        let reusable = self.dimensions == Some((width, height))
            && self.pixels.capacity() >= byte_len
            && self.allocated_bytes() <= available;
        let mut pixels = if reusable {
            std::mem::take(&mut self.pixels)
        } else {
            self.pixels = Vec::new();
            self.dimensions = None;
            let mut pixels = Vec::new();
            pixels
                .try_reserve_exact(byte_len)
                .map_err(|_| blur_error("CPU blur scratch allocation failed"))?;
            if pixels.capacity() > available {
                return Err(blur_error(
                    "CPU blur scratch allocation exceeds the per-surface effect budget",
                ));
            }
            if pixels
                .capacity()
                .checked_add(auxiliary_storage_bytes(
                    self.plane.capacity(),
                    self.line_a.capacity(),
                    self.line_b.capacity(),
                ))
                .is_none_or(|total| total > available)
            {
                return Err(blur_error(
                    "CPU blur scratch allocation exceeds the per-surface effect budget",
                ));
            }
            pixels
        };
        pixels.resize(byte_len, 0);
        pixels.fill(0);
        Ok((pixels, reusable))
    }

    fn put(&mut self, mut pixels: Vec<u8>, width: u32, height: u32) {
        pixels.fill(0);
        self.pixels = pixels;
        self.dimensions = Some((width, height));
    }

    fn prepare_three_box(
        &mut self,
        width: u32,
        height: u32,
        pixel_capacity: usize,
        committed_surface_bytes: usize,
    ) -> Result<bool, BackendError> {
        let available = MAX_EFFECT_SURFACE_BYTES
            .checked_sub(committed_surface_bytes)
            .ok_or_else(|| blur_error("CPU blur scratch budget underflowed"))?;
        let plane_length = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| blur_error("CPU blur plane size overflowed"))?;
        let line_length = usize::try_from(width.max(height))
            .map_err(|_| blur_error("CPU blur line length overflowed"))?;
        let required_auxiliary_bytes = plane_length
            .checked_mul(std::mem::size_of::<u16>())
            .and_then(|plane_bytes| {
                line_length
                    .checked_mul(std::mem::size_of::<f64>())
                    .and_then(|line_bytes| line_bytes.checked_mul(2))
                    .and_then(|line_bytes| plane_bytes.checked_add(line_bytes))
            })
            .ok_or_else(|| blur_error("CPU blur spatial scratch size overflowed"))?;
        if pixel_capacity
            .checked_add(required_auxiliary_bytes)
            .is_none_or(|total| total > available)
        {
            return Err(blur_error(
                "CPU blur spatial scratch exceeds the per-surface effect budget",
            ));
        }

        let reusable = self.plane_dimensions == Some((width, height))
            && self.plane.capacity() >= plane_length
            && self.line_length == line_length
            && self.line_a.capacity() >= line_length
            && self.line_b.capacity() >= line_length
            && pixel_capacity
                .checked_add(auxiliary_storage_bytes(
                    self.plane.capacity(),
                    self.line_a.capacity(),
                    self.line_b.capacity(),
                ))
                .is_some_and(|total| total <= available);
        if !reusable {
            let mut plane = Vec::new();
            let mut line_a = Vec::new();
            let mut line_b = Vec::new();
            plane
                .try_reserve_exact(plane_length)
                .map_err(|_| blur_error("CPU blur plane scratch allocation failed"))?;
            line_a
                .try_reserve_exact(line_length)
                .map_err(|_| blur_error("CPU blur line scratch allocation failed"))?;
            line_b
                .try_reserve_exact(line_length)
                .map_err(|_| blur_error("CPU blur line scratch allocation failed"))?;
            if pixel_capacity
                .checked_add(auxiliary_storage_bytes(
                    plane.capacity(),
                    line_a.capacity(),
                    line_b.capacity(),
                ))
                .is_none_or(|total| total > available)
            {
                return Err(blur_error(
                    "CPU blur spatial scratch allocation exceeds the per-surface effect budget",
                ));
            }
            self.plane = plane;
            self.line_a = line_a;
            self.line_b = line_b;
            self.plane_dimensions = Some((width, height));
            self.line_length = line_length;
        }
        self.plane.resize(plane_length, 0);
        self.line_a.resize(line_length, 0.0);
        self.line_b.resize(line_length, 0.0);
        self.plane.fill(0);
        self.line_a.fill(0.0);
        self.line_b.fill(0.0);
        Ok(reusable)
    }
}

pub(super) struct CpuBlurResult {
    pub pixels: Vec<u8>,
    pub algorithm: CpuBlurAlgorithm,
    pub scratch_reused: bool,
    pub pass_count: u32,
}

struct ThreeBoxScratch<'a> {
    plane: &'a mut [u16],
    line_a: &'a mut [f64],
    line_b: &'a mut [f64],
}

pub(super) fn apply_cpu_blur(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    sigma: f64,
    scratch: &mut CpuBlurScratch,
    committed_surface_bytes: usize,
) -> Result<CpuBlurResult, BackendError> {
    apply_cpu_blur_channels(
        pixels,
        width,
        height,
        sigma,
        CHANNELS_PER_PIXEL,
        true,
        scratch,
        committed_surface_bytes,
    )
}

pub(super) fn apply_cpu_blur_mask(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    sigma: f64,
    scratch: &mut CpuBlurScratch,
    committed_surface_bytes: usize,
) -> Result<CpuBlurResult, BackendError> {
    apply_cpu_blur_channels(
        pixels,
        width,
        height,
        sigma,
        MASK_CHANNELS,
        false,
        scratch,
        committed_surface_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_cpu_blur_channels(
    mut pixels: Vec<u8>,
    width: u32,
    height: u32,
    sigma: f64,
    channels: usize,
    premultiplied_rgba: bool,
    scratch: &mut CpuBlurScratch,
    committed_surface_bytes: usize,
) -> Result<CpuBlurResult, BackendError> {
    let byte_len = checked_sample_len(width, height, channels)?;
    if pixels.len() != byte_len {
        return Err(blur_error(
            "CPU blur input does not match its declared dimensions",
        ));
    }
    if !sigma.is_finite() || sigma < 0.0 {
        return Err(blur_error("CPU blur sigma is invalid"));
    }
    if sigma == 0.0 {
        return Ok(CpuBlurResult {
            pixels,
            algorithm: CpuBlurAlgorithm::DirectGaussian,
            scratch_reused: false,
            pass_count: 0,
        });
    }

    let (mut work, pixel_scratch_reused) =
        scratch.take(width, height, byte_len, committed_surface_bytes)?;
    let algorithm;
    let pass_count;
    let scratch_reused;
    if sigma < DIRECT_GAUSSIAN_THRESHOLD {
        algorithm = CpuBlurAlgorithm::DirectGaussian;
        let kernel = gaussian_kernel(sigma)?;
        convolve_gaussian_horizontal(
            &pixels,
            &mut work,
            width,
            height,
            channels,
            premultiplied_rgba,
            &kernel,
        );
        std::mem::swap(&mut pixels, &mut work);
        convolve_gaussian_vertical(
            &pixels,
            &mut work,
            width,
            height,
            channels,
            premultiplied_rgba,
            &kernel,
        );
        std::mem::swap(&mut pixels, &mut work);
        pass_count = 2;
        scratch_reused = pixel_scratch_reused;
    } else {
        algorithm = CpuBlurAlgorithm::ThreeBox;
        let passes = three_box_blur_passes(sigma)?;
        let spatial_scratch_reused =
            scratch.prepare_three_box(width, height, work.capacity(), committed_surface_bytes)?;
        convolve_three_boxes(
            &pixels,
            &mut work,
            width,
            height,
            channels,
            premultiplied_rgba,
            passes,
            ThreeBoxScratch {
                plane: &mut scratch.plane,
                line_a: &mut scratch.line_a,
                line_b: &mut scratch.line_b,
            },
        );
        std::mem::swap(&mut pixels, &mut work);
        pass_count = 6;
        scratch_reused = pixel_scratch_reused && spatial_scratch_reused;
    }
    scratch.put(work, width, height);
    Ok(CpuBlurResult {
        pixels,
        algorithm,
        scratch_reused,
        pass_count,
    })
}

pub(super) fn gaussian_kernel(sigma: f64) -> Result<Vec<f64>, BackendError> {
    if !sigma.is_finite() || !(0.0..DIRECT_GAUSSIAN_THRESHOLD).contains(&sigma) {
        return Err(blur_error(
            "direct CPU Gaussian sigma is outside its algorithm range",
        ));
    }
    let radius = (3.0 * sigma).ceil() as usize;
    let length = radius
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| blur_error("CPU Gaussian kernel length overflowed"))?;
    let mut kernel = Vec::new();
    kernel
        .try_reserve_exact(length)
        .map_err(|_| blur_error("CPU Gaussian kernel allocation failed"))?;
    let denominator = 2.0 * sigma * sigma;
    for index in 0..length {
        let distance = index as f64 - radius as f64;
        kernel.push((-(distance * distance) / denominator).exp());
    }
    let sum: f64 = kernel.iter().sum();
    if !sum.is_finite() || sum <= 0.0 {
        return Err(blur_error("CPU Gaussian kernel normalization failed"));
    }
    for weight in &mut kernel {
        *weight /= sum;
    }
    Ok(kernel)
}

pub(super) fn three_box_blur_passes(sigma: f64) -> Result<[BoxBlurPass; 3], BackendError> {
    if !sigma.is_finite() || sigma < DIRECT_GAUSSIAN_THRESHOLD {
        return Err(blur_error(
            "three-box CPU blur sigma is outside its algorithm range",
        ));
    }
    let diameter = (sigma * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5).floor();
    if diameter < 1.0 || diameter > f64::from(u32::MAX - 1) {
        return Err(blur_error("CPU three-box width is invalid"));
    }
    let diameter = diameter as u32;
    let half = diameter / 2;
    if diameter % 2 == 1 {
        Ok([BoxBlurPass {
            before: half,
            after: half,
        }; 3])
    } else {
        Ok([
            BoxBlurPass {
                before: half,
                after: half - 1,
            },
            BoxBlurPass {
                before: half - 1,
                after: half,
            },
            BoxBlurPass {
                before: half,
                after: half,
            },
        ])
    }
}

fn checked_sample_len(width: u32, height: u32, channels: usize) -> Result<usize, BackendError> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(channels))
        .filter(|length| *length > 0)
        .ok_or_else(|| blur_error("CPU blur dimensions overflowed"))
}

fn convolve_gaussian_horizontal(
    source: &[u8],
    target: &mut [u8],
    width: u32,
    height: u32,
    channels: usize,
    premultiplied_rgba: bool,
    kernel: &[f64],
) {
    let radius = (kernel.len() / 2) as i64;
    let width = i64::from(width);
    for y in 0..i64::from(height) {
        for x in 0..width {
            let mut weighted = [0.0; CHANNELS_PER_PIXEL];
            for (kernel_index, weight) in kernel.iter().enumerate() {
                let sample_x = x + kernel_index as i64 - radius;
                if sample_x < 0 || sample_x >= width {
                    continue;
                }
                let offset = ((y * width + sample_x) * channels as i64) as usize;
                for channel in 0..channels {
                    weighted[channel] += f64::from(source[offset + channel]) * weight;
                }
            }
            let offset = ((y * width + x) * channels as i64) as usize;
            store_weighted_channels(
                &mut target[offset..offset + channels],
                &weighted[..channels],
                premultiplied_rgba,
            );
        }
    }
}

fn convolve_gaussian_vertical(
    source: &[u8],
    target: &mut [u8],
    width: u32,
    height: u32,
    channels: usize,
    premultiplied_rgba: bool,
    kernel: &[f64],
) {
    let radius = (kernel.len() / 2) as i64;
    let width = i64::from(width);
    let height = i64::from(height);
    for y in 0..height {
        for x in 0..width {
            let mut weighted = [0.0; CHANNELS_PER_PIXEL];
            for (kernel_index, weight) in kernel.iter().enumerate() {
                let sample_y = y + kernel_index as i64 - radius;
                if sample_y < 0 || sample_y >= height {
                    continue;
                }
                let offset = ((sample_y * width + x) * channels as i64) as usize;
                for channel in 0..channels {
                    weighted[channel] += f64::from(source[offset + channel]) * weight;
                }
            }
            let offset = ((y * width + x) * channels as i64) as usize;
            store_weighted_channels(
                &mut target[offset..offset + channels],
                &weighted[..channels],
                premultiplied_rgba,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn convolve_three_boxes(
    source: &[u8],
    target: &mut [u8],
    width: u32,
    height: u32,
    channels: usize,
    premultiplied_rgba: bool,
    passes: [BoxBlurPass; 3],
    scratch: ThreeBoxScratch<'_>,
) {
    let width = width as usize;
    let height = height as usize;
    let ThreeBoxScratch {
        plane,
        line_a,
        line_b,
    } = scratch;
    for channel in 0..channels {
        for y in 0..height {
            for x in 0..width {
                line_a[x] = f64::from(source[(y * width + x) * channels + channel]);
            }
            convolve_box_line(&line_a[..width], &mut line_b[..width], passes[0]);
            convolve_box_line(&line_b[..width], &mut line_a[..width], passes[1]);
            convolve_box_line(&line_a[..width], &mut line_b[..width], passes[2]);
            for (x, value) in line_b[..width].iter().enumerate() {
                plane[y * width + x] = quantize_fixed(*value);
            }
        }
        for x in 0..width {
            for y in 0..height {
                line_a[y] = f64::from(plane[y * width + x]) / BOX_FIXED_SCALE;
            }
            convolve_box_line(&line_a[..height], &mut line_b[..height], passes[0]);
            convolve_box_line(&line_b[..height], &mut line_a[..height], passes[1]);
            convolve_box_line(&line_a[..height], &mut line_b[..height], passes[2]);
            for (y, value) in line_b[..height].iter().enumerate() {
                target[(y * width + x) * channels + channel] = quantize_f64(*value);
            }
        }
    }

    if premultiplied_rgba {
        normalize_premultiplied_rgba(target);
    }
}

fn convolve_box_line(source: &[f64], target: &mut [f64], pass: BoxBlurPass) {
    let length = source.len() as i64;
    let mut sum = 0.0;
    for sample in -(i64::from(pass.before))..=i64::from(pass.after) {
        if (0..length).contains(&sample) {
            sum += source[sample as usize];
        }
    }
    let divisor = f64::from(pass.width());
    for index in 0..length {
        target[index as usize] = sum / divisor;
        let outgoing = index - i64::from(pass.before);
        if (0..length).contains(&outgoing) {
            sum -= source[outgoing as usize];
        }
        let incoming = index + i64::from(pass.after) + 1;
        if (0..length).contains(&incoming) {
            sum += source[incoming as usize];
        }
    }
}

fn store_weighted_channels(target: &mut [u8], weighted: &[f64], premultiplied_rgba: bool) {
    for (target, weighted) in target.iter_mut().zip(weighted) {
        *target = quantize_f64(*weighted);
    }
    if premultiplied_rgba {
        normalize_premultiplied_rgba(target);
    }
}

fn normalize_premultiplied_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(CHANNELS_PER_PIXEL) {
        let alpha = pixel[3];
        for channel in &mut pixel[..3] {
            *channel = (*channel).min(alpha);
        }
        if alpha == 0 {
            pixel[..3].fill(0);
        }
    }
}

fn quantize_fixed(value: f64) -> u16 {
    (value * BOX_FIXED_SCALE).round().clamp(0.0, 65_535.0) as u16
}

fn quantize_f64(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

fn auxiliary_storage_bytes(
    plane_capacity: usize,
    line_a_capacity: usize,
    line_b_capacity: usize,
) -> usize {
    plane_capacity
        .saturating_mul(std::mem::size_of::<u16>())
        .saturating_add(
            line_a_capacity
                .saturating_add(line_b_capacity)
                .saturating_mul(std::mem::size_of::<f64>()),
        )
}

fn blur_error(message: &'static str) -> BackendError {
    BackendError::new(BackendErrorKind::TargetAllocation, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn impulse(width: u32, height: u32, x: u32, y: u32, color: [u8; 4]) -> Vec<u8> {
        let mut pixels = vec![0; checked_sample_len(width, height, CHANNELS_PER_PIXEL).unwrap()];
        let offset = ((y * width + x) * 4) as usize;
        pixels[offset..offset + 4].copy_from_slice(&color);
        pixels
    }

    fn blurred(pixels: Vec<u8>, width: u32, height: u32, sigma: f64) -> CpuBlurResult {
        apply_cpu_blur(
            pixels,
            width,
            height,
            sigma,
            &mut CpuBlurScratch::default(),
            checked_sample_len(width, height, CHANNELS_PER_PIXEL).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn direct_gaussian_kernel_is_symmetric_normalized_and_bounded() {
        for sigma in [0.5, 1.0, 1.999] {
            let kernel = gaussian_kernel(sigma).unwrap();
            assert_eq!(kernel.len(), 2 * (3.0 * sigma).ceil() as usize + 1);
            for index in 0..kernel.len() {
                assert_eq!(
                    kernel[index].to_bits(),
                    kernel[kernel.len() - index - 1].to_bits()
                );
                assert!(kernel[index].is_finite());
                assert!(kernel[index] >= 0.0);
            }
            assert!((kernel.iter().sum::<f64>() - 1.0).abs() <= f64::EPSILON * 4.0);
        }
        assert!(gaussian_kernel(0.0).is_err());
        assert!(gaussian_kernel(2.0).is_err());
    }

    #[test]
    fn three_box_widths_follow_the_filter_effects_rounding_rule() {
        assert_eq!(
            three_box_blur_passes(2.0).unwrap(),
            [
                BoxBlurPass {
                    before: 2,
                    after: 1,
                },
                BoxBlurPass {
                    before: 1,
                    after: 2,
                },
                BoxBlurPass {
                    before: 2,
                    after: 2,
                },
            ]
        );
        assert_eq!(
            three_box_blur_passes(8.0).unwrap(),
            [BoxBlurPass {
                before: 7,
                after: 7,
            }; 3]
        );
        for sigma in [4.0, 16.0, 64.0] {
            let passes = three_box_blur_passes(sigma).unwrap();
            assert!(passes.into_iter().all(|pass| pass.width() > 0));
        }
        assert!(three_box_blur_passes(1.999).is_err());
    }

    #[test]
    fn zero_sigma_is_byte_exact_and_does_not_allocate_scratch() {
        let input = impulse(3, 3, 1, 1, [64, 32, 16, 128]);
        let mut scratch = CpuBlurScratch::default();
        let output = apply_cpu_blur(input.clone(), 3, 3, 0.0, &mut scratch, input.len()).unwrap();
        assert_eq!(output.pixels, input);
        assert_eq!(output.pass_count, 0);
        assert_eq!(scratch.allocated_bytes(), 0);
    }

    #[test]
    fn gaussian_and_box_paths_use_transparent_black_edges() {
        for sigma in [0.5, 1.0, 1.999, 2.0, 4.0] {
            let output = blurred(vec![255, 0, 0, 255], 1, 1, sigma);
            assert!(output.pixels[0] < 255, "{sigma}");
            assert_eq!(output.pixels[1], 0);
            assert_eq!(output.pixels[2], 0);
            assert_eq!(output.pixels[0], output.pixels[3]);
        }
    }

    #[test]
    fn algorithm_transition_at_sigma_two_is_visually_bounded() {
        let input = impulse(65, 65, 32, 32, [255; 4]);
        let direct = blurred(input.clone(), 65, 65, 1.999);
        let boxes = blurred(input, 65, 65, 2.0);
        assert_eq!(direct.algorithm, CpuBlurAlgorithm::DirectGaussian);
        assert_eq!(boxes.algorithm, CpuBlurAlgorithm::ThreeBox);
        let maximum_difference = direct
            .pixels
            .iter()
            .zip(&boxes.pixels)
            .map(|(left, right)| left.abs_diff(*right))
            .max()
            .unwrap();
        assert!(maximum_difference <= 8, "{maximum_difference}");
        let direct_alpha: u64 = direct
            .pixels
            .chunks_exact(4)
            .map(|pixel| u64::from(pixel[3]))
            .sum();
        let box_alpha: u64 = boxes
            .pixels
            .chunks_exact(4)
            .map(|pixel| u64::from(pixel[3]))
            .sum();
        assert!(
            direct_alpha.abs_diff(box_alpha) <= 64,
            "direct_alpha={direct_alpha} box_alpha={box_alpha}"
        );
    }

    #[test]
    fn spatial_passes_preserve_premultiplied_alpha_invariant() {
        for (width, height) in [(1, 7), (7, 1), (2, 2), (9, 9)] {
            for sigma in [0.5, 1.0, 1.999, 2.0, 4.0, 64.0] {
                let input = impulse(width, height, width / 2, height / 2, [64, 32, 16, 128]);
                let output = blurred(input, width, height, sigma);
                for pixel in output.pixels.chunks_exact(4) {
                    assert!(pixel[0] <= pixel[3]);
                    assert!(pixel[1] <= pixel[3]);
                    assert!(pixel[2] <= pixel[3]);
                    if pixel[3] == 0 {
                        assert_eq!(pixel, [0, 0, 0, 0]);
                    }
                }
            }
        }
    }

    #[test]
    fn scalar_mask_blur_matches_the_rgba_alpha_channel() {
        let width = 17;
        let height = 13;
        let sample_count = checked_sample_len(width, height, MASK_CHANNELS).unwrap();
        for sigma in [0.0, 0.5, 1.999, 2.0, 4.0, 16.0, 64.0] {
            let mut mask = vec![0; sample_count];
            for (index, alpha) in mask.iter_mut().enumerate() {
                *alpha = ((index * 37 + 11) % 256) as u8;
            }
            let rgba: Vec<_> = mask
                .iter()
                .flat_map(|alpha| [*alpha / 2, *alpha / 3, *alpha / 4, *alpha])
                .collect();
            let rgba_len = rgba.len();
            let rgba = apply_cpu_blur(
                rgba,
                width,
                height,
                sigma,
                &mut CpuBlurScratch::default(),
                rgba_len,
            )
            .unwrap();
            let mask = apply_cpu_blur_mask(
                mask,
                width,
                height,
                sigma,
                &mut CpuBlurScratch::default(),
                sample_count,
            )
            .unwrap();
            assert_eq!(
                rgba.pixels
                    .chunks_exact(CHANNELS_PER_PIXEL)
                    .map(|pixel| pixel[3])
                    .collect::<Vec<_>>(),
                mask.pixels,
                "sigma={sigma}"
            );
        }
    }

    #[test]
    fn scratch_is_reused_cleared_and_replaced_for_new_dimensions() {
        let mut scratch = CpuBlurScratch::default();
        let byte_len = checked_sample_len(9, 9, CHANNELS_PER_PIXEL).unwrap();
        let first = apply_cpu_blur(
            impulse(9, 9, 4, 4, [255; 4]),
            9,
            9,
            1.0,
            &mut scratch,
            byte_len,
        )
        .unwrap();
        assert!(!first.scratch_reused);
        let second = apply_cpu_blur(vec![0; byte_len], 9, 9, 1.0, &mut scratch, byte_len).unwrap();
        assert!(second.scratch_reused);
        assert!(second.pixels.iter().all(|value| *value == 0));

        let smaller_len = checked_sample_len(2, 2, CHANNELS_PER_PIXEL).unwrap();
        let replaced =
            apply_cpu_blur(vec![0; smaller_len], 2, 2, 1.0, &mut scratch, smaller_len).unwrap();
        assert!(!replaced.scratch_reused);
    }

    #[test]
    fn invalid_inputs_and_surface_budget_fail_without_partial_output() {
        let mut scratch = CpuBlurScratch::default();
        assert!(apply_cpu_blur(vec![0; 3], 1, 1, 1.0, &mut scratch, 4).is_err());
        assert!(apply_cpu_blur(vec![0; 4], 1, 1, f64::NAN, &mut scratch, 4).is_err());
        assert!(
            apply_cpu_blur(
                vec![0; 4],
                1,
                1,
                1.0,
                &mut scratch,
                MAX_EFFECT_SURFACE_BYTES
            )
            .is_err()
        );
    }

    #[test]
    fn one_thousand_sigma_changes_reuse_bounded_scratch_without_alpha_leakage() {
        let width = 17;
        let height = 17;
        let byte_len = checked_sample_len(width, height, CHANNELS_PER_PIXEL).unwrap();
        let mut scratch = CpuBlurScratch::default();
        let sigmas = [0.5, 1.0, 1.999, 2.0, 4.0, 8.0, 16.0, 64.0];

        for iteration in 0..1_000 {
            let sigma = sigmas[iteration % sigmas.len()];
            let color = if iteration % 2 == 0 {
                [96, 48, 24, 128]
            } else {
                [0, 0, 0, 0]
            };
            let result = apply_cpu_blur(
                impulse(width, height, 8, 8, color),
                width,
                height,
                sigma,
                &mut scratch,
                byte_len,
            )
            .unwrap();
            assert_eq!(
                result.algorithm,
                if sigma < DIRECT_GAUSSIAN_THRESHOLD {
                    CpuBlurAlgorithm::DirectGaussian
                } else {
                    CpuBlurAlgorithm::ThreeBox
                }
            );
            for pixel in result.pixels.chunks_exact(4) {
                assert!(pixel[0] <= pixel[3]);
                assert!(pixel[1] <= pixel[3]);
                assert!(pixel[2] <= pixel[3]);
                if pixel[3] == 0 {
                    assert_eq!(pixel, [0, 0, 0, 0]);
                }
            }
        }

        assert_eq!(
            scratch.allocated_bytes(),
            byte_len + 17 * 17 * std::mem::size_of::<u16>() + 2 * 17 * std::mem::size_of::<f64>()
        );
    }

    #[test]
    fn cpu_blur_measurement_probe_is_bounded() {
        const ITERATIONS: usize = 100;
        let width = 96;
        let height = 96;
        let byte_len = checked_sample_len(width, height, CHANNELS_PER_PIXEL).unwrap();

        for sigma in [0.0, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 64.0] {
            let mut scratch = CpuBlurScratch::default();
            let started = Instant::now();
            let mut alpha_sum = 0_u64;
            for _ in 0..ITERATIONS {
                let mut input = vec![0; byte_len];
                for y in 40..56 {
                    for x in 40..56 {
                        let offset = (y * width as usize + x) * CHANNELS_PER_PIXEL;
                        input[offset..offset + CHANNELS_PER_PIXEL]
                            .copy_from_slice(&[96, 48, 24, 128]);
                    }
                }
                let result =
                    apply_cpu_blur(input, width, height, sigma, &mut scratch, byte_len).unwrap();
                alpha_sum = alpha_sum.saturating_add(
                    result
                        .pixels
                        .chunks_exact(4)
                        .map(|pixel| u64::from(pixel[3]))
                        .sum::<u64>(),
                );
                std::hint::black_box(result.pixels);
            }
            let average_us = started.elapsed().as_micros() / ITERATIONS as u128;
            assert!(alpha_sum > 0);
            eprintln!(
                "cpu_blur_measurement sigma={sigma} dimensions={width}x{height} average_us={average_us} scratch_bytes={}",
                scratch.allocated_bytes()
            );
        }
    }
}
