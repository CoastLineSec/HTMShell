use super::{BackendError, BackendErrorKind, GPU_WAIT_TIMEOUT, GpuStatistics};
use crate::render::{DropShadowEffect, MAX_EFFECT_PIPELINE_VARIANTS};
use vello::wgpu;

pub(super) const SHADOW_PARAMETER_BYTES: usize = 32;
const _: () = assert!(6 <= MAX_EFFECT_PIPELINE_VARIANTS);

const SHADOW_SHADER: &str = r#"
struct ShadowParameters {
    offset: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var<uniform> parameters: ShadowParameters;

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

fn load_mask_or_zero(coordinate: vec2<i32>) -> f32 {
    let dimensions = vec2<i32>(textureDimensions(mask));
    if (any(coordinate < vec2<i32>(0)) || any(coordinate >= dimensions)) {
        return 0.0;
    }
    return textureLoad(mask, coordinate, 0).a;
}

fn sample_mask_bilinear(coordinate: vec2<f32>) -> f32 {
    let base_value = floor(coordinate);
    let fraction = coordinate - base_value;
    let base = vec2<i32>(base_value);
    let top = mix(
        load_mask_or_zero(base),
        load_mask_or_zero(base + vec2<i32>(1, 0)),
        fraction.x,
    );
    let bottom = mix(
        load_mask_or_zero(base + vec2<i32>(0, 1)),
        load_mask_or_zero(base + vec2<i32>(1, 1)),
        fraction.x,
    );
    return mix(top, bottom, fraction.y);
}

@fragment
fn fs_extract(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let alpha = clamp(textureLoad(source, vec2<i32>(position.xy), 0).a, 0.0, 1.0);
    return vec4<f32>(0.0, 0.0, 0.0, alpha);
}

@fragment
fn fs_composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel_coordinate = vec2<i32>(position.xy);
    let source_value = normalized_premultiplied(textureLoad(source, pixel_coordinate, 0));
    let mask_coordinate = vec2<f32>(pixel_coordinate) - parameters.offset.xy;
    let mask_alpha = clamp(sample_mask_bilinear(mask_coordinate), 0.0, 1.0);
    let shadow_alpha = mask_alpha * parameters.color.a;
    let shadow = vec4<f32>(parameters.color.rgb * shadow_alpha, shadow_alpha);
    return normalized_premultiplied(
        source_value + shadow * (1.0 - source_value.a),
    );
}
"#;

pub(super) struct ShadowEffectPipelines {
    bind_group_layout: wgpu::BindGroupLayout,
    extraction: wgpu::RenderPipeline,
    composition: wgpu::RenderPipeline,
}

impl ShadowEffectPipelines {
    pub(super) fn new(
        device: &wgpu::Device,
        statistics: &mut GpuStatistics,
    ) -> Result<Self, BackendError> {
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HTMShell Vello drop shadow resources"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
            label: Some("HTMShell bounded Vello drop shadow shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HTMShell Vello drop shadow pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let extraction = create_pipeline(
            device,
            &layout,
            &shader,
            "fs_extract",
            "HTMShell bounded Vello alpha-mask extraction pipeline",
        );
        let composition = create_pipeline(
            device,
            &layout,
            &shader,
            "fs_composite",
            "HTMShell bounded Vello shadow composition pipeline",
        );
        let pipeline_error = super::bounded_block_on(error_scope.pop(), GPU_WAIT_TIMEOUT)
            .inspect_err(|_| {
                statistics.gpu_shadow_pipeline_failures =
                    statistics.gpu_shadow_pipeline_failures.saturating_add(1);
            })?;
        if let Some(error) = pipeline_error {
            statistics.gpu_shadow_pipeline_failures =
                statistics.gpu_shadow_pipeline_failures.saturating_add(1);
            return Err(shadow_error(
                BackendErrorKind::PipelineCreation,
                format!("GPU drop shadow pipeline creation failed: {error}"),
                true,
            ));
        }
        Ok(Self {
            bind_group_layout,
            extraction,
            composition,
        })
    }

    pub(super) fn extract_mask(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        output: &wgpu::TextureView,
        statistics: &mut GpuStatistics,
    ) -> Result<(), BackendError> {
        let parameters = create_parameter_buffer(device, &[0; SHADOW_PARAMETER_BYTES]);
        encode_pass(
            device,
            queue,
            &self.bind_group_layout,
            &self.extraction,
            source,
            source,
            output,
            &parameters,
            "HTMShell Vello alpha-mask extraction pass",
        )?;
        statistics.gpu_shadow_mask_extractions =
            statistics.gpu_shadow_mask_extractions.saturating_add(1);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        output: &wgpu::TextureView,
        effect: DropShadowEffect,
        physical_scale: f64,
        statistics: &mut GpuStatistics,
    ) -> Result<(), BackendError> {
        let offset_x = f64::from(effect.offset_x.get()) * physical_scale;
        let offset_y = f64::from(effect.offset_y.get()) * physical_scale;
        let color = [
            effect.color.red.get(),
            effect.color.green.get(),
            effect.color.blue.get(),
            effect.color.alpha.get(),
        ];
        if !offset_x.is_finite()
            || !offset_y.is_finite()
            || color
                .into_iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(shadow_error(
                BackendErrorKind::ResourcePreparation,
                "GPU drop shadow parameters are invalid",
                false,
            ));
        }
        let offset_x = offset_x as f32;
        let offset_y = offset_y as f32;
        if !offset_x.is_finite() || !offset_y.is_finite() {
            return Err(shadow_error(
                BackendErrorKind::ResourcePreparation,
                "GPU drop shadow offset cannot be represented",
                false,
            ));
        }
        let mut bytes = [0; SHADOW_PARAMETER_BYTES];
        write_f32(&mut bytes, 0, offset_x);
        write_f32(&mut bytes, 4, offset_y);
        for (index, value) in color.into_iter().enumerate() {
            write_f32(&mut bytes, 16 + index * 4, value);
        }
        let parameters = create_parameter_buffer(device, &bytes);
        encode_pass(
            device,
            queue,
            &self.bind_group_layout,
            &self.composition,
            source,
            mask,
            output,
            &parameters,
            "HTMShell Vello drop shadow composition pass",
        )?;
        if offset_x.fract() != 0.0 || offset_y.fract() != 0.0 {
            statistics.gpu_shadow_fractional_offset_samples = statistics
                .gpu_shadow_fractional_offset_samples
                .saturating_add(1);
        }
        statistics.gpu_shadow_colorization_passes =
            statistics.gpu_shadow_colorization_passes.saturating_add(1);
        statistics.gpu_shadow_composition_passes =
            statistics.gpu_shadow_composition_passes.saturating_add(1);
        statistics.gpu_shadow_parameter_uploads =
            statistics.gpu_shadow_parameter_uploads.saturating_add(1);
        Ok(())
    }
}

pub(super) fn create_mask_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &'static str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry_point: &'static str,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
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
    mask: &wgpu::TextureView,
    output: &wgpu::TextureView,
    parameters: &wgpu::Buffer,
    label: &'static str,
) -> Result<(), BackendError> {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("HTMShell Vello drop shadow bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(mask),
            },
            wgpu::BindGroupEntry {
                binding: 2,
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
        return Err(shadow_error(
            BackendErrorKind::Submission,
            format!("GPU drop shadow pass submission failed: {error}"),
            true,
        ));
    }
    Ok(())
}

fn create_parameter_buffer(
    device: &wgpu::Device,
    bytes: &[u8; SHADOW_PARAMETER_BYTES],
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("HTMShell Vello drop shadow parameters"),
        size: SHADOW_PARAMETER_BYTES as u64,
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

fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
}

fn shadow_error(
    kind: BackendErrorKind,
    message: impl Into<String>,
    recoverable: bool,
) -> BackendError {
    BackendError::new(kind, message, recoverable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{CanonicalF32, EffectColor};

    fn canonical(value: f32) -> CanonicalF32 {
        CanonicalF32::new(value).unwrap()
    }

    #[test]
    fn shadow_parameter_packing_is_initialized_and_deterministic() {
        let effect = DropShadowEffect {
            offset_x: canonical(0.5),
            offset_y: canonical(-1.25),
            sigma: canonical(2.0),
            color: EffectColor {
                red: canonical(0.25),
                green: canonical(0.5),
                blue: canonical(0.75),
                alpha: canonical(0.6),
            },
        };
        let scale = 1.5;
        let mut first = [0; SHADOW_PARAMETER_BYTES];
        write_f32(&mut first, 0, effect.offset_x.get() * scale);
        write_f32(&mut first, 4, effect.offset_y.get() * scale);
        for (index, value) in [
            effect.color.red.get(),
            effect.color.green.get(),
            effect.color.blue.get(),
            effect.color.alpha.get(),
        ]
        .into_iter()
        .enumerate()
        {
            write_f32(&mut first, 16 + index * 4, value);
        }
        let second = first;
        assert_eq!(first, second);
        assert!(first[8..16].iter().all(|byte| *byte == 0));
        assert_eq!(
            f32::from_bits(u32::from_le_bytes(first[0..4].try_into().unwrap())),
            0.75
        );
        assert_eq!(
            f32::from_bits(u32::from_le_bytes(first[4..8].try_into().unwrap())),
            -1.875
        );
    }

    #[test]
    fn shader_uses_alpha_only_and_manual_transparent_bilinear_sampling() {
        assert!(SHADOW_SHADER.contains("textureLoad(source"));
        assert!(SHADOW_SHADER.contains(".a"));
        assert!(SHADOW_SHADER.contains("load_mask_or_zero"));
        assert!(SHADOW_SHADER.contains("sample_mask_bilinear"));
        assert!(SHADOW_SHADER.contains("source_value + shadow * (1.0 - source_value.a)"));
    }
}
