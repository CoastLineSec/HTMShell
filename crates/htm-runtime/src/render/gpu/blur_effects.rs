use super::{BackendError, BackendErrorKind, GPU_WAIT_TIMEOUT, GpuStatistics};
use crate::render::MAX_EFFECT_PIPELINE_VARIANTS;
use crate::render::cpu_blur::{BlurParameters, BoxBlurPass};
use vello::wgpu;

const PARAMETER_BYTES: usize = 80;
const MAX_GAUSSIAN_WEIGHTS: usize = 16;
const MAX_BOX_WIDTH: u32 = 1_024;
const _: () = assert!(4 <= MAX_EFFECT_PIPELINE_VARIANTS);

const BLUR_SHADER: &str = r#"
struct Parameters {
    header: vec4<u32>,
    values: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> parameters: Parameters;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

fn normalized_premultiplied(value: vec4<f32>) -> vec4<f32> {
    let bounded = clamp(value, vec4<f32>(0.0), vec4<f32>(1.0));
    if (bounded.a == 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(min(bounded.rgb, vec3<f32>(bounded.a)), bounded.a);
}

fn load_or_zero(coordinate: vec2<i32>) -> vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(source));
    if (any(coordinate < vec2<i32>(0)) || any(coordinate >= dimensions)) {
        return vec4<f32>(0.0);
    }
    return textureLoad(source, coordinate, 0);
}

fn weight(index: u32) -> f32 {
    return parameters.values[index / 4u][index % 4u];
}

@fragment
fn fs_gaussian(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let coordinate = vec2<i32>(position.xy);
    let radius = i32(parameters.header.x);
    let horizontal = parameters.header.y == 0u;
    let length = parameters.header.z;
    var result = vec4<f32>(0.0);
    for (var index = 0u; index < 16u; index = index + 1u) {
        if (index >= length) {
            break;
        }
        let delta = i32(index) - radius;
        let sample_coordinate = select(
            coordinate + vec2<i32>(0, delta),
            coordinate + vec2<i32>(delta, 0),
            horizontal,
        );
        result += load_or_zero(sample_coordinate) * weight(index);
    }
    return normalized_premultiplied(result);
}

@fragment
fn fs_box(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let coordinate = vec2<i32>(position.xy);
    let before = i32(parameters.header.x);
    let after = i32(parameters.header.y);
    let horizontal = parameters.header.z == 0u;
    let width = parameters.header.w;
    var result = vec4<f32>(0.0);
    for (var index = 0u; index < 1024u; index = index + 1u) {
        if (index >= width) {
            break;
        }
        let delta = i32(index) - before;
        let sample_coordinate = select(
            coordinate + vec2<i32>(0, delta),
            coordinate + vec2<i32>(delta, 0),
            horizontal,
        );
        result += load_or_zero(sample_coordinate);
    }
    return normalized_premultiplied(result / f32(width));
}

@fragment
fn fs_convert(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let value = clamp(
        textureLoad(source, vec2<i32>(position.xy), 0),
        vec4<f32>(0.0),
        vec4<f32>(1.0),
    );
    if (value.a == 0.0) {
        return vec4<f32>(0.0);
    }
    if (parameters.header.x == 0u) {
        return vec4<f32>(min(value.rgb * value.a, vec3<f32>(value.a)), value.a);
    }
    return vec4<f32>(clamp(value.rgb / value.a, vec3<f32>(0.0), vec3<f32>(1.0)), value.a);
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AlphaConversion {
    Premultiply,
    Unpremultiply,
}

pub(super) struct BlurEffectPipelines {
    bind_group_layout: wgpu::BindGroupLayout,
    gaussian: wgpu::RenderPipeline,
    box_blur: wgpu::RenderPipeline,
    conversion: wgpu::RenderPipeline,
    cached_kernel: Option<(Vec<u64>, wgpu::Buffer)>,
}

impl BlurEffectPipelines {
    pub(super) fn new(
        device: &wgpu::Device,
        statistics: &mut GpuStatistics,
    ) -> Result<Self, BackendError> {
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HTMShell Vello blur resources"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HTMShell bounded Vello blur shader"),
            source: wgpu::ShaderSource::Wgsl(BLUR_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HTMShell Vello blur pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let gaussian = create_pipeline(device, &layout, &shader, "fs_gaussian", "Gaussian");
        let box_blur = create_pipeline(device, &layout, &shader, "fs_box", "three-box");
        let conversion =
            create_pipeline(device, &layout, &shader, "fs_convert", "alpha conversion");
        let pipeline_error = super::bounded_block_on(error_scope.pop(), GPU_WAIT_TIMEOUT)
            .inspect_err(|_| {
                statistics.gpu_blur_pipeline_failures =
                    statistics.gpu_blur_pipeline_failures.saturating_add(1);
            })?;
        if let Some(error) = pipeline_error {
            statistics.gpu_blur_pipeline_failures =
                statistics.gpu_blur_pipeline_failures.saturating_add(1);
            return Err(effect_error(
                BackendErrorKind::PipelineCreation,
                format!("GPU blur pipeline creation failed: {error}"),
                true,
            ));
        }
        Ok(Self {
            bind_group_layout,
            gaussian,
            box_blur,
            conversion,
            cached_kernel: None,
        })
    }

    pub(super) fn apply_conversion(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        output: &wgpu::TextureView,
        conversion: AlphaConversion,
        statistics: &mut GpuStatistics,
    ) -> Result<(), BackendError> {
        let mut parameters = [0; PARAMETER_BYTES];
        write_u32(
            &mut parameters,
            0,
            u32::from(conversion == AlphaConversion::Unpremultiply),
        );
        let buffer = create_parameter_buffer(device, "HTMShell blur alpha conversion", &parameters);
        encode_pass(
            device,
            queue,
            &self.bind_group_layout,
            &self.conversion,
            source,
            output,
            &buffer,
            "HTMShell Vello blur alpha conversion pass",
        )?;
        match conversion {
            AlphaConversion::Premultiply => {
                statistics.gpu_blur_premultiply_conversions = statistics
                    .gpu_blur_premultiply_conversions
                    .saturating_add(1);
            }
            AlphaConversion::Unpremultiply => {
                statistics.gpu_blur_unpremultiply_conversions = statistics
                    .gpu_blur_unpremultiply_conversions
                    .saturating_add(1);
            }
        }
        Ok(())
    }

    pub(super) fn apply_blur(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source_index: &mut usize,
        views: &[wgpu::TextureView; 2],
        parameters: &BlurParameters,
        statistics: &mut GpuStatistics,
    ) -> Result<(), BackendError> {
        match parameters {
            BlurParameters::Identity => Ok(()),
            BlurParameters::DirectGaussian { kernel } => {
                if kernel.len() > MAX_GAUSSIAN_WEIGHTS {
                    return Err(effect_error(
                        BackendErrorKind::UnsupportedCapability,
                        "GPU Gaussian kernel exceeds the bounded weight limit",
                        true,
                    ));
                }
                let key: Vec<_> = kernel.iter().map(|weight| weight.to_bits()).collect();
                if self
                    .cached_kernel
                    .as_ref()
                    .is_some_and(|(cached, _)| *cached == key)
                {
                    statistics.gpu_blur_kernel_cache_hits =
                        statistics.gpu_blur_kernel_cache_hits.saturating_add(1);
                } else {
                    let parameters = gaussian_parameters(kernel)?;
                    self.cached_kernel = Some((
                        key,
                        create_parameter_buffer(
                            device,
                            "HTMShell Vello Gaussian kernel",
                            &parameters,
                        ),
                    ));
                    statistics.gpu_blur_kernel_uploads =
                        statistics.gpu_blur_kernel_uploads.saturating_add(1);
                }
                for direction in 0..2u32 {
                    let (_, buffer) = self
                        .cached_kernel
                        .as_mut()
                        .expect("Gaussian kernel was cached");
                    let mut parameters = gaussian_parameters(kernel)?;
                    write_u32(&mut parameters, 4, direction);
                    let directional = create_parameter_buffer(
                        device,
                        "HTMShell Vello directional Gaussian kernel",
                        &parameters,
                    );
                    let output_index = 1 - *source_index;
                    encode_pass(
                        device,
                        queue,
                        &self.bind_group_layout,
                        &self.gaussian,
                        &views[*source_index],
                        &views[output_index],
                        if direction == 0 { buffer } else { &directional },
                        "HTMShell Vello Gaussian pass",
                    )?;
                    *source_index = output_index;
                    statistics.gpu_blur_gaussian_passes =
                        statistics.gpu_blur_gaussian_passes.saturating_add(1);
                }
                Ok(())
            }
            BlurParameters::ThreeBox { passes } => {
                for direction in 0..2u32 {
                    for pass in passes {
                        if pass.width() > MAX_BOX_WIDTH {
                            return Err(effect_error(
                                BackendErrorKind::UnsupportedCapability,
                                "GPU three-box width exceeds the bounded shader limit",
                                true,
                            ));
                        }
                        let parameters = box_parameters(*pass, direction);
                        let buffer = create_parameter_buffer(
                            device,
                            "HTMShell Vello box parameters",
                            &parameters,
                        );
                        let output_index = 1 - *source_index;
                        encode_pass(
                            device,
                            queue,
                            &self.bind_group_layout,
                            &self.box_blur,
                            &views[*source_index],
                            &views[output_index],
                            &buffer,
                            "HTMShell Vello three-box pass",
                        )?;
                        *source_index = output_index;
                        statistics.gpu_blur_box_passes =
                            statistics.gpu_blur_box_passes.saturating_add(1);
                        statistics.gpu_blur_box_parameter_uploads =
                            statistics.gpu_blur_box_parameter_uploads.saturating_add(1);
                    }
                }
                Ok(())
            }
        }
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry_point: &'static str,
    kind: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(match kind {
            "Gaussian" => "HTMShell bounded Vello Gaussian pipeline",
            "three-box" => "HTMShell bounded Vello three-box pipeline",
            _ => "HTMShell bounded Vello blur conversion pipeline",
        }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry_point),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn encode_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    pipeline: &wgpu::RenderPipeline,
    source: &wgpu::TextureView,
    output: &wgpu::TextureView,
    parameters: &wgpu::Buffer,
    label: &'static str,
) -> Result<(), BackendError> {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("HTMShell Vello blur bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: parameters.as_entire_binding(),
            },
        ],
    });
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
    let submission_error = super::bounded_block_on(error_scope.pop(), GPU_WAIT_TIMEOUT)?;
    if let Some(error) = submission_error {
        return Err(effect_error(
            BackendErrorKind::Submission,
            format!("GPU blur pass submission failed: {error}"),
            true,
        ));
    }
    Ok(())
}

fn gaussian_parameters(kernel: &[f64]) -> Result<[u8; PARAMETER_BYTES], BackendError> {
    if kernel.is_empty() || kernel.len() > MAX_GAUSSIAN_WEIGHTS || kernel.len().is_multiple_of(2) {
        return Err(effect_error(
            BackendErrorKind::ResourcePreparation,
            "GPU Gaussian kernel shape is invalid",
            false,
        ));
    }
    let mut bytes = [0; PARAMETER_BYTES];
    write_u32(
        &mut bytes,
        0,
        u32::try_from(kernel.len() / 2).expect("bounded Gaussian radius"),
    );
    write_u32(
        &mut bytes,
        8,
        u32::try_from(kernel.len()).expect("bounded Gaussian length"),
    );
    for (index, weight) in kernel.iter().enumerate() {
        let weight = *weight as f32;
        if !weight.is_finite() || weight < 0.0 {
            return Err(effect_error(
                BackendErrorKind::ResourcePreparation,
                "GPU Gaussian kernel contains an invalid weight",
                false,
            ));
        }
        write_f32(&mut bytes, 16 + index * 4, weight);
    }
    Ok(bytes)
}

fn box_parameters(pass: BoxBlurPass, direction: u32) -> [u8; PARAMETER_BYTES] {
    let mut bytes = [0; PARAMETER_BYTES];
    write_u32(&mut bytes, 0, pass.before);
    write_u32(&mut bytes, 4, pass.after);
    write_u32(&mut bytes, 8, direction);
    write_u32(&mut bytes, 12, pass.width());
    bytes
}

fn create_parameter_buffer(
    device: &wgpu::Device,
    label: &'static str,
    bytes: &[u8; PARAMETER_BYTES],
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: PARAMETER_BYTES as u64,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: true,
    });
    {
        let mut mapped = buffer.slice(..).get_mapped_range_mut();
        mapped.copy_from_slice(bytes);
    }
    buffer.unmap();
    buffer
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
}

fn effect_error(
    kind: BackendErrorKind,
    message: impl Into<String>,
    recoverable: bool,
) -> BackendError {
    BackendError::new(kind, message, recoverable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::cpu_blur::derive_blur_parameters;

    #[test]
    fn gaussian_parameter_packing_is_fixed_and_deterministic() {
        let BlurParameters::DirectGaussian { kernel } =
            derive_blur_parameters(1.0).expect("derive Gaussian")
        else {
            panic!("sigma one must use direct Gaussian");
        };
        let first = gaussian_parameters(&kernel).expect("pack Gaussian");
        let second = gaussian_parameters(&kernel).expect("pack Gaussian");
        assert_eq!(first, second);
        assert_eq!(first.len(), PARAMETER_BYTES);
        assert_eq!(u32::from_le_bytes(first[8..12].try_into().unwrap()), 7);
        assert!(first[16 + kernel.len() * 4..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn three_box_parameters_preserve_pass_order_and_direction() {
        let BlurParameters::ThreeBox { passes } =
            derive_blur_parameters(2.0).expect("derive boxes")
        else {
            panic!("sigma two must use three boxes");
        };
        assert_ne!(box_parameters(passes[0], 0), box_parameters(passes[1], 0));
        assert_ne!(box_parameters(passes[0], 0), box_parameters(passes[0], 1));
        assert!(passes.iter().all(|pass| pass.width() <= MAX_BOX_WIDTH));
    }
}
