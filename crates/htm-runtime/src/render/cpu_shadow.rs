use super::cpu_blur::{CpuBlurAlgorithm, CpuBlurScratch, apply_cpu_blur_mask};
use super::{BackendError, BackendErrorKind, DropShadowEffect, MAX_EFFECT_SURFACE_BYTES};

const RGBA_CHANNELS: usize = 4;

#[derive(Debug, Default)]
pub(super) struct CpuShadowScratch {
    mask: Vec<u8>,
    dimensions: Option<(u32, u32)>,
}

impl CpuShadowScratch {
    pub(super) fn allocated_bytes(&self) -> usize {
        self.mask.capacity()
    }

    fn take(
        &mut self,
        width: u32,
        height: u32,
        sample_count: usize,
        committed_surface_bytes: usize,
        blur_scratch_bytes: usize,
    ) -> Result<(Vec<u8>, bool), BackendError> {
        let available = MAX_EFFECT_SURFACE_BYTES
            .checked_sub(committed_surface_bytes)
            .ok_or_else(|| shadow_error("CPU shadow scratch budget underflowed"))?;
        if sample_count
            .checked_add(blur_scratch_bytes)
            .is_none_or(|total| total > available)
        {
            return Err(shadow_error(
                "CPU shadow mask exceeds the per-surface effect budget",
            ));
        }

        let reusable = self.dimensions == Some((width, height))
            && self.mask.capacity() >= sample_count
            && self
                .mask
                .capacity()
                .checked_add(blur_scratch_bytes)
                .is_some_and(|total| total <= available);
        let mut mask = if reusable {
            std::mem::take(&mut self.mask)
        } else {
            self.mask = Vec::new();
            self.dimensions = None;
            let mut mask = Vec::new();
            mask.try_reserve_exact(sample_count)
                .map_err(|_| shadow_error("CPU shadow mask allocation failed"))?;
            if mask
                .capacity()
                .checked_add(blur_scratch_bytes)
                .is_none_or(|total| total > available)
            {
                return Err(shadow_error(
                    "CPU shadow mask allocation exceeds the per-surface effect budget",
                ));
            }
            mask
        };
        mask.resize(sample_count, 0);
        mask.fill(0);
        Ok((mask, reusable))
    }

    fn put(&mut self, mut mask: Vec<u8>, width: u32, height: u32) {
        mask.fill(0);
        self.mask = mask;
        self.dimensions = Some((width, height));
    }
}

#[derive(Debug)]
pub(super) struct CpuDropShadowResult {
    pub pixels: Vec<u8>,
    pub blur_algorithm: Option<CpuBlurAlgorithm>,
    pub blur_pass_count: u32,
    pub blur_scratch_reused: bool,
    pub mask_scratch_reused: bool,
    pub identity_fast_path: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_cpu_drop_shadow(
    mut pixels: Vec<u8>,
    width: u32,
    height: u32,
    effect: DropShadowEffect,
    scale: f64,
    blur_scratch: &mut CpuBlurScratch,
    shadow_scratch: &mut CpuShadowScratch,
    committed_surface_bytes: usize,
) -> Result<CpuDropShadowResult, BackendError> {
    let sample_count = checked_sample_count(width, height)?;
    let byte_len = sample_count
        .checked_mul(RGBA_CHANNELS)
        .ok_or_else(|| shadow_error("CPU shadow input byte size overflowed"))?;
    if pixels.len() != byte_len {
        return Err(shadow_error(
            "CPU shadow input does not match its declared dimensions",
        ));
    }
    if !scale.is_finite() || scale <= 0.0 {
        return Err(shadow_error("CPU shadow scale is invalid"));
    }
    let sigma = f64::from(effect.sigma.get()) * scale;
    let offset_x = f64::from(effect.offset_x.get()) * scale;
    let offset_y = f64::from(effect.offset_y.get()) * scale;
    let color = [
        f64::from(effect.color.red.get()),
        f64::from(effect.color.green.get()),
        f64::from(effect.color.blue.get()),
        f64::from(effect.color.alpha.get()),
    ];
    if [sigma, offset_x, offset_y]
        .into_iter()
        .chain(color)
        .any(|value| !value.is_finite())
        || sigma < 0.0
        || color.into_iter().any(|value| !(0.0..=1.0).contains(&value))
    {
        return Err(shadow_error(
            "CPU shadow descriptor contains an invalid value",
        ));
    }
    if color[3] == 0.0
        || pixels
            .chunks_exact(RGBA_CHANNELS)
            .all(|pixel| pixel[3] == 0)
    {
        return Ok(CpuDropShadowResult {
            pixels,
            blur_algorithm: None,
            blur_pass_count: 0,
            blur_scratch_reused: false,
            mask_scratch_reused: false,
            identity_fast_path: true,
        });
    }

    if committed_surface_bytes
        .checked_add(sample_count)
        .and_then(|bytes| bytes.checked_add(blur_scratch.allocated_bytes()))
        .is_none_or(|bytes| bytes > MAX_EFFECT_SURFACE_BYTES)
    {
        *blur_scratch = CpuBlurScratch::default();
    }
    let (mut mask, mask_scratch_reused) = shadow_scratch.take(
        width,
        height,
        sample_count,
        committed_surface_bytes,
        blur_scratch.allocated_bytes(),
    )?;
    for (mask_alpha, pixel) in mask.iter_mut().zip(pixels.chunks_exact(RGBA_CHANNELS)) {
        *mask_alpha = pixel[3];
    }

    let mut blur_algorithm = None;
    let mut blur_pass_count = 0;
    let mut blur_scratch_reused = false;
    if sigma > 0.0 {
        let mask_capacity = mask.capacity();
        let blur_committed_bytes = committed_surface_bytes
            .checked_add(mask_capacity)
            .ok_or_else(|| shadow_error("CPU shadow byte accounting overflowed"))?;
        let result = apply_cpu_blur_mask(
            mask,
            width,
            height,
            sigma,
            blur_scratch,
            blur_committed_bytes,
        )?;
        mask = result.pixels;
        blur_algorithm = Some(result.algorithm);
        blur_pass_count = result.pass_count;
        blur_scratch_reused = result.scratch_reused;
    }

    composite_shadow_under_source(&mut pixels, &mask, width, height, offset_x, offset_y, color);
    shadow_scratch.put(mask, width, height);
    Ok(CpuDropShadowResult {
        pixels,
        blur_algorithm,
        blur_pass_count,
        blur_scratch_reused,
        mask_scratch_reused,
        identity_fast_path: false,
    })
}

fn composite_shadow_under_source(
    pixels: &mut [u8],
    mask: &[u8],
    width: u32,
    height: u32,
    offset_x: f64,
    offset_y: f64,
    color: [f64; 4],
) {
    let width_usize = width as usize;
    for y in 0..height as usize {
        for x in 0..width_usize {
            let mask_alpha = sample_mask(
                mask,
                width,
                height,
                x as f64 - offset_x,
                y as f64 - offset_y,
            );
            if mask_alpha <= 0.0 {
                continue;
            }
            let shadow_alpha_unit = (mask_alpha / 255.0) * color[3];
            let shadow_alpha = quantize_unit(shadow_alpha_unit);
            if shadow_alpha == 0 {
                continue;
            }
            let shadow = [
                quantize_unit(color[0] * shadow_alpha_unit).min(shadow_alpha),
                quantize_unit(color[1] * shadow_alpha_unit).min(shadow_alpha),
                quantize_unit(color[2] * shadow_alpha_unit).min(shadow_alpha),
                shadow_alpha,
            ];
            let pixel = &mut pixels
                [(y * width_usize + x) * RGBA_CHANNELS..(y * width_usize + x + 1) * RGBA_CHANNELS];
            let inverse_source_alpha = u16::from(255 - pixel[3]);
            for channel in 0..RGBA_CHANNELS {
                let shadow_contribution =
                    (u16::from(shadow[channel]) * inverse_source_alpha + 127) / 255;
                pixel[channel] = u16::from(pixel[channel])
                    .saturating_add(shadow_contribution)
                    .min(255) as u8;
            }
            let alpha = pixel[3];
            for channel in &mut pixel[..3] {
                *channel = (*channel).min(alpha);
            }
            if alpha == 0 {
                pixel[..3].fill(0);
            }
        }
    }
}

fn sample_mask(mask: &[u8], width: u32, height: u32, x: f64, y: f64) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let fraction_x = x - x0;
    let fraction_y = y - y0;
    let x0 = x0 as i64;
    let y0 = y0 as i64;
    let sample = |sample_x: i64, sample_y: i64| {
        if sample_x < 0
            || sample_y < 0
            || sample_x >= i64::from(width)
            || sample_y >= i64::from(height)
        {
            0.0
        } else {
            f64::from(mask[sample_y as usize * width as usize + sample_x as usize])
        }
    };
    let top = sample(x0, y0) * (1.0 - fraction_x) + sample(x0 + 1, y0) * fraction_x;
    let bottom = sample(x0, y0 + 1) * (1.0 - fraction_x) + sample(x0 + 1, y0 + 1) * fraction_x;
    top * (1.0 - fraction_y) + bottom * fraction_y
}

fn quantize_unit(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn checked_sample_count(width: u32, height: u32) -> Result<usize, BackendError> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .filter(|samples| *samples > 0)
        .ok_or_else(|| shadow_error("CPU shadow dimensions overflowed"))
}

fn shadow_error(message: &'static str) -> BackendError {
    BackendError::new(BackendErrorKind::TargetAllocation, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{CanonicalF32, EffectColor};
    use std::time::Instant;

    fn canonical(value: f32) -> CanonicalF32 {
        CanonicalF32::new(value).unwrap()
    }

    fn effect(offset_x: f32, offset_y: f32, sigma: f32, color: [f32; 4]) -> DropShadowEffect {
        DropShadowEffect {
            offset_x: canonical(offset_x),
            offset_y: canonical(offset_y),
            sigma: canonical(sigma),
            color: EffectColor {
                red: canonical(color[0]),
                green: canonical(color[1]),
                blue: canonical(color[2]),
                alpha: canonical(color[3]),
            },
        }
    }

    fn shadowed(
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        effect: DropShadowEffect,
        scale: f64,
    ) -> CpuDropShadowResult {
        let committed = pixels.len();
        apply_cpu_drop_shadow(
            pixels,
            width,
            height,
            effect,
            scale,
            &mut CpuBlurScratch::default(),
            &mut CpuShadowScratch::default(),
            committed,
        )
        .unwrap()
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = pixel_offset(width, x, y);
        pixels[offset..offset + 4].try_into().unwrap()
    }

    fn pixel_offset(width: u32, x: u32, y: u32) -> usize {
        ((y * width + x) * 4) as usize
    }

    #[test]
    fn mask_uses_only_exact_source_alpha() {
        let mut red = vec![0; 5 * 3 * 4];
        let mut blue = red.clone();
        let source_offset = pixel_offset(5, 1, 1);
        red[source_offset..source_offset + 4].copy_from_slice(&[128, 0, 0, 128]);
        blue[source_offset..source_offset + 4].copy_from_slice(&[0, 0, 128, 128]);
        let effect = effect(2.0, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0]);
        let red = shadowed(red, 5, 3, effect, 1.0);
        let blue = shadowed(blue, 5, 3, effect, 1.0);
        assert_eq!(pixel(&red.pixels, 5, 3, 1), [0, 0, 0, 128]);
        assert_eq!(pixel(&red.pixels, 5, 3, 1), pixel(&blue.pixels, 5, 3, 1));
    }

    #[test]
    fn every_u8_alpha_boundary_is_preserved_by_mask_extraction() {
        for alpha in [0, 1, 64, 128, 254, 255] {
            let mut source = vec![0; 3 * 4];
            source[..4].copy_from_slice(&[alpha / 2, alpha / 3, alpha / 4, alpha]);
            let output = shadowed(
                source,
                3,
                1,
                effect(1.0, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0]),
                1.0,
            );
            assert_eq!(pixel(&output.pixels, 3, 1, 0), [0, 0, 0, alpha]);
        }
    }

    #[test]
    fn shadow_color_alpha_is_premultiplied_once() {
        let mut source = vec![0; 5 * 3 * 4];
        let source_offset = pixel_offset(5, 1, 1);
        source[source_offset..source_offset + 4].copy_from_slice(&[255, 255, 255, 255]);
        let output = shadowed(
            source,
            5,
            3,
            effect(2.0, 0.0, 0.0, [1.0, 0.0, 0.0, 0.5]),
            1.0,
        );
        assert_eq!(pixel(&output.pixels, 5, 3, 1), [128, 0, 0, 128]);
    }

    #[test]
    fn source_is_composited_over_the_shadow() {
        let source = vec![64, 0, 0, 128];
        let output = shadowed(
            source,
            1,
            1,
            effect(0.0, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0]),
            1.0,
        );
        assert_eq!(output.pixels, [64, 0, 0, 192]);

        let opaque = shadowed(
            vec![255, 0, 0, 255],
            1,
            1,
            effect(0.0, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0]),
            1.0,
        );
        assert_eq!(opaque.pixels, [255, 0, 0, 255]);
    }

    #[test]
    fn signed_and_fractional_offsets_use_transparent_sampling() {
        let mut source = vec![0; 5 * 3 * 4];
        let source_offset = pixel_offset(5, 2, 1);
        source[source_offset..source_offset + 4].copy_from_slice(&[255; 4]);
        for (offset, expected_x) in [(1.0, 3), (-1.0, 1)] {
            let output = shadowed(
                source.clone(),
                5,
                3,
                effect(offset, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0]),
                1.0,
            );
            assert_eq!(pixel(&output.pixels, 5, expected_x, 1), [0, 0, 0, 255]);
        }
        let fractional = shadowed(
            source,
            5,
            3,
            effect(0.5, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0]),
            1.0,
        );
        assert_eq!(pixel(&fractional.pixels, 5, 3, 1), [0, 0, 0, 128]);

        let vertical = shadowed(
            vec![255, 255, 255, 255]
                .into_iter()
                .chain(std::iter::repeat_n(0, 5 * 5 * 4 - 4))
                .collect(),
            5,
            5,
            effect(0.0, 2.0, 0.0, [0.0, 0.0, 0.0, 1.0]),
            1.0,
        );
        assert_eq!(pixel(&vertical.pixels, 5, 0, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn shadow_reuses_both_blur_algorithms() {
        let mut source = vec![0; 17 * 17 * 4];
        source[(8 * 17 + 8) * 4..(8 * 17 + 9) * 4].copy_from_slice(&[255; 4]);
        for (sigma, algorithm) in [
            (0.5, CpuBlurAlgorithm::DirectGaussian),
            (2.0, CpuBlurAlgorithm::ThreeBox),
        ] {
            let output = shadowed(
                source.clone(),
                17,
                17,
                effect(1.0, 1.0, sigma, [0.0, 0.0, 0.0, 1.0]),
                1.0,
            );
            assert_eq!(output.blur_algorithm, Some(algorithm));
            assert!(output.blur_pass_count > 0);
            assert!(output.pixels.chunks_exact(4).all(|pixel| {
                pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3]
            }));
        }
    }

    #[test]
    fn transparent_color_and_zero_alpha_are_allocation_free_identities() {
        for (source, color) in [
            (vec![255; 4], [1.0, 0.0, 0.0, 0.0]),
            (vec![0; 4], [1.0, 0.0, 0.0, 1.0]),
        ] {
            let original = source.clone();
            let output = shadowed(source, 1, 1, effect(1.0, 1.0, 4.0, color), 1.0);
            assert!(output.identity_fast_path);
            assert_eq!(output.pixels, original);
        }
    }

    #[test]
    fn scratch_is_reused_cleared_and_budgeted() {
        let mut blur = CpuBlurScratch::default();
        let mut shadow = CpuShadowScratch::default();
        let effect = effect(1.0, 0.0, 1.0, [0.0, 0.0, 0.0, 1.0]);
        let mut source = vec![0; 9 * 9 * 4];
        source[(4 * 9 + 4) * 4..(4 * 9 + 5) * 4].copy_from_slice(&[255; 4]);
        let first =
            apply_cpu_drop_shadow(source, 9, 9, effect, 1.0, &mut blur, &mut shadow, 9 * 9 * 4)
                .unwrap();
        assert!(!first.mask_scratch_reused);
        let second = apply_cpu_drop_shadow(
            vec![0, 0, 0, 255]
                .into_iter()
                .chain(std::iter::repeat_n(0, 9 * 9 * 4 - 4))
                .collect(),
            9,
            9,
            effect,
            1.0,
            &mut blur,
            &mut shadow,
            9 * 9 * 4,
        )
        .unwrap();
        assert!(second.mask_scratch_reused);
        assert!(shadow.allocated_bytes() >= 9 * 9);
        assert!(
            apply_cpu_drop_shadow(
                vec![255; 4],
                1,
                1,
                effect,
                1.0,
                &mut blur,
                &mut shadow,
                MAX_EFFECT_SURFACE_BYTES,
            )
            .is_err()
        );
    }

    #[test]
    fn one_thousand_shadow_changes_remain_canonical_and_bounded() {
        let width = 17;
        let height = 17;
        let mut blur = CpuBlurScratch::default();
        let mut shadow = CpuShadowScratch::default();
        let sigmas = [0.0, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 64.0];
        for iteration in 0..1_000 {
            let mut source = vec![0; width * height * 4];
            let alpha = [1, 64, 128, 254, 255][iteration % 5];
            let source_offset = (8 * width + 8) * 4;
            source[source_offset..source_offset + 4].copy_from_slice(&[
                alpha / 2,
                alpha / 3,
                alpha / 4,
                alpha,
            ]);
            let sigma = sigmas[iteration % sigmas.len()];
            let offset = (iteration as i32 % 7 - 3) as f32 * 0.5;
            let result = apply_cpu_drop_shadow(
                source,
                width as u32,
                height as u32,
                effect(offset, -offset, sigma, [0.2, 0.6, 1.0, 0.75]),
                1.0,
                &mut blur,
                &mut shadow,
                width * height * 4,
            )
            .unwrap();
            assert!(result.pixels.chunks_exact(4).all(|pixel| {
                pixel[0] <= pixel[3]
                    && pixel[1] <= pixel[3]
                    && pixel[2] <= pixel[3]
                    && (pixel[3] != 0 || pixel[..3] == [0, 0, 0])
            }));
        }
        assert!(
            blur.allocated_bytes()
                .saturating_add(shadow.allocated_bytes())
                < MAX_EFFECT_SURFACE_BYTES
        );
    }

    #[test]
    fn cpu_drop_shadow_measurement_probe_is_bounded() {
        const ITERATIONS: usize = 25;
        let width = 96;
        let height = 96;
        let byte_len = width * height * 4;
        for sigma in [0.0, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 64.0] {
            let mut blur = CpuBlurScratch::default();
            let mut shadow = CpuShadowScratch::default();
            let started = Instant::now();
            let mut alpha_sum = 0_u64;
            for _ in 0..ITERATIONS {
                let mut source = vec![0; byte_len];
                for y in 40..56 {
                    for x in 40..56 {
                        let source_offset = (y * width + x) * 4;
                        source[source_offset..source_offset + 4]
                            .copy_from_slice(&[96, 48, 24, 128]);
                    }
                }
                let result = apply_cpu_drop_shadow(
                    source,
                    width as u32,
                    height as u32,
                    effect(4.5, -2.5, sigma, [0.2, 0.6, 1.0, 0.75]),
                    1.0,
                    &mut blur,
                    &mut shadow,
                    byte_len,
                )
                .unwrap();
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
                "cpu_drop_shadow_measurement sigma={sigma} dimensions={width}x{height} average_us={average_us} scratch_bytes={}",
                blur.allocated_bytes()
                    .saturating_add(shadow.allocated_bytes())
            );
        }
    }
}
