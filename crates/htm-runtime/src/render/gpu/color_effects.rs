use super::blur_effects::{AlphaConversion, BlurEffectPipelines};
use super::{BackendError, BackendErrorKind, GPU_WAIT_TIMEOUT, GpuPreparedScene, GpuStatistics};
use crate::render::cpu_blur::derive_blur_parameters;
use crate::render::cpu_effects::{
    CpuEffectExecution, CpuEffectPlan, RecordedNode, command_has_foreground_filter,
    filter_space_transforms, flatten_nodes, include_outset_box_shadows, parse_commands,
    physical_bounds, remove_identity_filter,
};
use crate::render::{
    ColorEffectKind, ForegroundEffect, ForegroundEffectList, MAX_EFFECT_IMAGE_BYTES,
    MAX_EFFECT_LAYER_DIMENSION, MAX_EFFECT_PIPELINE_VARIANTS, MAX_EFFECT_SURFACE_BYTES,
    MAX_FOREGROUND_EFFECT_FUNCTIONS,
};
use anyrender::recording::RenderCommand;
use anyrender::{ImageRenderer, PaintScene, Scene};
use kurbo::{Affine, Rect, Shape};
use peniko::{ImageBrush, ImageData};
use vello::wgpu;

use super::painter::VelloScenePainter;

const PACKED_HEADER_BYTES: usize = 16;
const PACKED_VECTORS_PER_OPERATION: usize = 6;
const PACKED_OPERATION_BYTES: usize = PACKED_VECTORS_PER_OPERATION * 16;
const PACKED_OPERATION_BUFFER_BYTES: usize =
    PACKED_HEADER_BYTES + MAX_FOREGROUND_EFFECT_FUNCTIONS * PACKED_OPERATION_BYTES;
const _: () = assert!(1 <= MAX_EFFECT_PIPELINE_VARIANTS);

const COLOR_EFFECT_SHADER: &str = r#"
struct ColorOperations {
    header: vec4<u32>,
    data: array<vec4<f32>, 96>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> operations: ColorOperations;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    var value = textureLoad(source, vec2<i32>(position.xy), 0);
    if (value.a == 0.0) {
        value = vec4<f32>(0.0);
    }
    for (var index = 0u; index < 16u; index = index + 1u) {
        if (index >= operations.header.x) {
            break;
        }
        let base = index * 6u;
        let row0 = operations.data[base + 1u];
        let row1 = operations.data[base + 2u];
        let row2 = operations.data[base + 3u];
        let row3 = operations.data[base + 4u];
        let offset = operations.data[base + 5u];
        value = clamp(vec4<f32>(
            dot(row0, value) + offset.x,
            dot(row1, value) + offset.y,
            dot(row2, value) + offset.z,
            dot(row3, value) + offset.w,
        ), vec4<f32>(0.0), vec4<f32>(1.0));
    }
    if (value.a == 0.0) {
        value = vec4<f32>(0.0);
    }
    return value;
}
"#;

pub(super) struct ColorEffectPipeline {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PackedColorOperations {
    bytes: [u8; PACKED_OPERATION_BUFFER_BYTES],
    operation_count: usize,
}

impl PackedColorOperations {
    pub(super) fn from_list(list: &ForegroundEffectList) -> Result<Self, BackendError> {
        Self::from_effects(&list.functions)
    }

    pub(super) fn from_effects(effects: &[ForegroundEffect]) -> Result<Self, BackendError> {
        if effects.len() > MAX_FOREGROUND_EFFECT_FUNCTIONS {
            return Err(effect_error(
                BackendErrorKind::ResourcePreparation,
                "GPU foreground effect list exceeds the operation limit",
                false,
            ));
        }
        let mut packed = Self {
            bytes: [0; PACKED_OPERATION_BUFFER_BYTES],
            operation_count: effects.len(),
        };
        packed.bytes[..4].copy_from_slice(
            &u32::try_from(effects.len())
                .map_err(|_| {
                    effect_error(
                        BackendErrorKind::ResourcePreparation,
                        "GPU foreground operation count cannot be represented",
                        false,
                    )
                })?
                .to_le_bytes(),
        );
        for (index, effect) in effects.iter().enumerate() {
            let ForegroundEffect::Color(color) = effect else {
                return Err(effect_error(
                    BackendErrorKind::FallbackRequired,
                    "spatial foreground effects require complete CPU fallback",
                    true,
                ));
            };
            let matrix = effect.color_matrix().map_err(|_| {
                effect_error(
                    BackendErrorKind::ResourcePreparation,
                    "normalized GPU color matrix is invalid",
                    false,
                )
            })?;
            let coefficients = matrix
                .expect("color effects always produce a finite matrix")
                .coefficients();
            let base = PACKED_HEADER_BYTES + index * PACKED_OPERATION_BYTES;
            write_f32(&mut packed.bytes, base, operation_kind(color.kind) as f32);
            write_f32(&mut packed.bytes, base + 4, color.value.get());
            for (row_index, row) in coefficients.iter().enumerate() {
                for (column_index, value) in row[..4].iter().enumerate() {
                    write_f32(
                        &mut packed.bytes,
                        base + 16 + (row_index * 4 + column_index) * 4,
                        *value,
                    );
                }
            }
            for (row_index, row) in coefficients.iter().enumerate() {
                write_f32(&mut packed.bytes, base + 80 + row_index * 4, row[4]);
            }
        }
        Ok(packed)
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn operation_count(&self) -> usize {
        self.operation_count
    }
}

impl ColorEffectPipeline {
    pub(super) fn new(
        device: &wgpu::Device,
        statistics: &mut GpuStatistics,
    ) -> Result<Self, BackendError> {
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HTMShell Vello color effect resources"),
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
            label: Some("HTMShell bounded Vello color effect shader"),
            source: wgpu::ShaderSource::Wgsl(COLOR_EFFECT_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HTMShell Vello color effect pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("HTMShell bounded Vello color effect pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
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
        });
        let pipeline_error = super::bounded_block_on(error_scope.pop(), GPU_WAIT_TIMEOUT)
            .inspect_err(|_error| {
                statistics.gpu_color_filter_pipeline_failures = statistics
                    .gpu_color_filter_pipeline_failures
                    .saturating_add(1);
            })?;
        if let Some(error) = pipeline_error {
            statistics.gpu_color_filter_pipeline_failures = statistics
                .gpu_color_filter_pipeline_failures
                .saturating_add(1);
            return Err(effect_error(
                BackendErrorKind::PipelineCreation,
                format!("GPU color effect pipeline creation failed: {error}"),
                true,
            ));
        }
        Ok(Self {
            bind_group_layout,
            pipeline,
        })
    }

    pub(super) fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        output: &wgpu::TextureView,
        operations: &PackedColorOperations,
        statistics: &mut GpuStatistics,
    ) -> Result<(), BackendError> {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HTMShell Vello color effect operations"),
            size: u64::try_from(PACKED_OPERATION_BUFFER_BYTES).expect("bounded operation buffer"),
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        });
        {
            let mut mapped = buffer.slice(..).get_mapped_range_mut();
            mapped.copy_from_slice(operations.bytes());
        }
        buffer.unmap();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HTMShell Vello color effect bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("HTMShell Vello color effect encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("HTMShell Vello color effect pass"),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        let submission_error = super::bounded_block_on(error_scope.pop(), GPU_WAIT_TIMEOUT)?;
        if let Some(error) = submission_error {
            return Err(effect_error(
                BackendErrorKind::Submission,
                format!("GPU color effect submission failed: {error}"),
                true,
            ));
        }
        statistics.gpu_color_filter_passes = statistics.gpu_color_filter_passes.saturating_add(1);
        statistics.gpu_color_filter_operation_uploads = statistics
            .gpu_color_filter_operation_uploads
            .saturating_add(1);
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_prepared_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut vello::Renderer,
    color_pipeline: &mut Option<ColorEffectPipeline>,
    blur_pipelines: &mut Option<BlurEffectPipelines>,
    prepared: &GpuPreparedScene,
    root_transform: Affine,
    physical_scale: f64,
    target_view: &wgpu::TextureView,
    target_width: u32,
    target_height: u32,
    statistics: &mut GpuStatistics,
) -> Result<(), BackendError> {
    let fallback_requests_before = statistics.gpu_color_filter_fallback_requests;
    let has_native_foreground_effect = prepared.effect_plans.iter().any(|plan| {
        matches!(
            &plan.execution,
            CpuEffectExecution::Ready(list)
                if !list.is_visual_identity()
                    && list
                        .functions
                        .iter()
                        .all(|effect| matches!(effect, ForegroundEffect::Color(_) | ForegroundEffect::Blur(_)))
        )
    });
    let mut transformed = Scene::with_tolerance(prepared.recording.tolerance);
    transformed.append_scene(prepared.recording.clone(), root_transform);
    let mut nodes = parse_commands(&transformed.commands)?;
    include_outset_box_shadows(&mut nodes);
    let mut plan_index = 0usize;
    let mut registered_images = Vec::new();
    let mut allocated_bytes = 0usize;
    let transformed_result = transform_nodes(
        &mut nodes,
        &prepared.effect_plans,
        &mut plan_index,
        root_transform,
        target_width,
        target_height,
        device,
        queue,
        renderer,
        color_pipeline,
        blur_pipelines,
        physical_scale,
        &mut registered_images,
        &mut allocated_bytes,
        statistics,
    );
    let result = match transformed_result {
        Ok(()) if plan_index != prepared.effect_plans.len() => Err(effect_error(
            BackendErrorKind::ResourcePreparation,
            "GPU effect plan count does not match the recorded filter layers",
            true,
        )),
        Ok(()) => {
            let mut recording = Scene::with_tolerance(prepared.recording.tolerance);
            flatten_nodes(&nodes, &mut recording.commands);
            let mut scene = vello::Scene::new();
            let mut painter = VelloScenePainter::new(&mut scene);
            painter.append_scene(recording, Affine::IDENTITY);
            if painter.unsupported() {
                Err(effect_error(
                    BackendErrorKind::FallbackRequired,
                    "GPU recording contains an unsupported brush or unresolved filter",
                    true,
                ))
            } else {
                renderer
                    .render_to_texture(
                        device,
                        queue,
                        &scene,
                        target_view,
                        &vello::RenderParams {
                            base_color: vello::peniko::Color::TRANSPARENT,
                            width: target_width,
                            height: target_height,
                            antialiasing_method: vello::AaConfig::Area,
                        },
                    )
                    .map_err(|error| {
                        effect_error(
                            BackendErrorKind::Render,
                            format!("Vello rendering with color effects failed: {error}"),
                            true,
                        )
                    })
            }
        }
        Err(error) => Err(error),
    };
    for image in registered_images {
        renderer.unregister_texture(image);
    }
    if result.is_err()
        && has_native_foreground_effect
        && statistics.gpu_color_filter_fallback_requests == fallback_requests_before
    {
        statistics.gpu_color_filter_fallback_requests = statistics
            .gpu_color_filter_fallback_requests
            .saturating_add(1);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn transform_nodes(
    nodes: &mut Vec<RecordedNode>,
    plans: &[CpuEffectPlan],
    plan_index: &mut usize,
    root_transform: Affine,
    target_width: u32,
    target_height: u32,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut vello::Renderer,
    color_pipeline: &mut Option<ColorEffectPipeline>,
    blur_pipelines: &mut Option<BlurEffectPipelines>,
    physical_scale: f64,
    registered_images: &mut Vec<ImageData>,
    allocated_bytes: &mut usize,
    statistics: &mut GpuStatistics,
) -> Result<(), BackendError> {
    for node in nodes {
        let RecordedNode::Group { push, children } = node else {
            continue;
        };
        let own_filter = command_has_foreground_filter(push);
        let own_plan = if own_filter {
            let plan = plans.get(*plan_index).cloned().ok_or_else(|| {
                effect_error(
                    BackendErrorKind::ResourcePreparation,
                    "recorded GPU filter layer has no retained effect plan",
                    true,
                )
            })?;
            *plan_index += 1;
            Some(plan)
        } else {
            None
        };

        transform_nodes(
            children,
            plans,
            plan_index,
            root_transform,
            target_width,
            target_height,
            device,
            queue,
            renderer,
            color_pipeline,
            blur_pipelines,
            physical_scale,
            registered_images,
            allocated_bytes,
            statistics,
        )?;

        let Some(plan) = own_plan else {
            continue;
        };
        match plan.execution {
            CpuEffectExecution::Identity => {
                remove_identity_filter(push, target_width, target_height);
                statistics.gpu_color_filter_identity_suppressions = statistics
                    .gpu_color_filter_identity_suppressions
                    .saturating_add(1);
            }
            CpuEffectExecution::Deferred => {
                statistics.gpu_color_filter_fallback_requests = statistics
                    .gpu_color_filter_fallback_requests
                    .saturating_add(1);
                return Err(effect_error(
                    BackendErrorKind::FallbackRequired,
                    "unresolved foreground filter requires complete CPU fallback",
                    true,
                ));
            }
            CpuEffectExecution::Ready(list) => {
                if list
                    .functions
                    .iter()
                    .any(|effect| matches!(effect, ForegroundEffect::DropShadow(_)))
                {
                    statistics.gpu_color_filter_fallback_requests = statistics
                        .gpu_color_filter_fallback_requests
                        .saturating_add(1);
                    statistics.gpu_spatial_cpu_fallbacks =
                        statistics.gpu_spatial_cpu_fallbacks.saturating_add(1);
                    return Err(effect_error(
                        BackendErrorKind::FallbackRequired,
                        "drop-shadow foreground filters require complete CPU fallback",
                        true,
                    ));
                }
                let (to_filter_space, from_filter_space) =
                    filter_space_transforms(push, plan.element_transform)?;
                let filtered_bounds = plan.filtered_bounds.as_ref().ok_or_else(|| {
                    effect_error(
                        BackendErrorKind::ResourcePreparation,
                        "GPU foreground effect plan has no filtered bounds",
                        true,
                    )
                })?;
                let Some(bounds) = physical_bounds(
                    filtered_bounds,
                    target_width,
                    target_height,
                    root_transform,
                    to_filter_space,
                )?
                else {
                    // Retained bounds already include every spatial stage. If
                    // they do not intersect this replay target, the filtered
                    // subtree cannot contribute to its authoritative pixels.
                    remove_identity_filter(push, target_width, target_height);
                    continue;
                };
                let has_blur = list
                    .functions
                    .iter()
                    .any(|effect| matches!(effect, ForegroundEffect::Blur(blur) if blur.sigma.get() > 0.0));
                let image_bytes = checked_effect_image_bytes(bounds.width, bounds.height)
                    .inspect_err(|_| {
                        if has_blur {
                            statistics.gpu_blur_allocation_failures =
                                statistics.gpu_blur_allocation_failures.saturating_add(1);
                        }
                    })?;
                let layer_bytes = image_bytes
                    .checked_mul(2)
                    .and_then(|bytes| bytes.checked_add(PACKED_OPERATION_BUFFER_BYTES))
                    .ok_or_else(|| {
                        effect_error(
                            BackendErrorKind::TargetAllocation,
                            "GPU foreground effect layer byte accounting overflowed",
                            true,
                        )
                    })?;
                *allocated_bytes = allocated_bytes.checked_add(layer_bytes).ok_or_else(|| {
                    effect_error(
                        BackendErrorKind::TargetAllocation,
                        "GPU foreground effect surface byte accounting overflowed",
                        true,
                    )
                })?;
                if *allocated_bytes > MAX_EFFECT_SURFACE_BYTES {
                    statistics.gpu_color_filter_allocation_failures = statistics
                        .gpu_color_filter_allocation_failures
                        .saturating_add(1);
                    if has_blur {
                        statistics.gpu_blur_allocation_failures =
                            statistics.gpu_blur_allocation_failures.saturating_add(1);
                    }
                    return Err(effect_error(
                        BackendErrorKind::TargetAllocation,
                        "GPU foreground effect layers exceed the per-surface byte limit",
                        true,
                    ));
                }

                let mut recorded_source = Scene::new();
                flatten_nodes(children, &mut recorded_source.commands);
                let mut source_recording = Scene::new();
                source_recording.append_scene(recorded_source, to_filter_space);
                let mut source_scene = vello::Scene::new();
                let mut painter = VelloScenePainter::new(&mut source_scene);
                painter.append_scene(
                    source_recording,
                    Affine::translate((-bounds.x0, -bounds.y0)),
                );
                if painter.unsupported() {
                    return Err(effect_error(
                        BackendErrorKind::FallbackRequired,
                        "nested SourceGraphic contains an unresolved GPU effect",
                        true,
                    ));
                }

                let source_texture = create_effect_texture(
                    device,
                    bounds.width,
                    bounds.height,
                    "HTMShell Vello foreground effect SourceGraphic",
                );
                let source_view =
                    source_texture.create_view(&wgpu::TextureViewDescriptor::default());
                renderer
                    .render_to_texture(
                        device,
                        queue,
                        &source_scene,
                        &source_view,
                        &vello::RenderParams {
                            base_color: vello::peniko::Color::TRANSPARENT,
                            width: bounds.width,
                            height: bounds.height,
                            antialiasing_method: vello::AaConfig::Area,
                        },
                    )
                    .map_err(|error| {
                        effect_error(
                            BackendErrorKind::Render,
                            format!("Vello SourceGraphic rendering failed: {error}"),
                            true,
                        )
                    })?;

                let filtered_texture = create_effect_texture(
                    device,
                    bounds.width,
                    bounds.height,
                    "HTMShell Vello foreground effect output",
                );
                let filtered_view =
                    filtered_texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut textures = [Some(source_texture), Some(filtered_texture)];
                let views = [source_view, filtered_view];
                let final_index = apply_ordered_gpu_effects(
                    device,
                    queue,
                    color_pipeline,
                    blur_pipelines,
                    &list,
                    physical_scale,
                    &views,
                    statistics,
                )?;
                let image = renderer.register_texture(
                    textures[final_index]
                        .take()
                        .expect("final GPU effect texture remains owned"),
                );
                if let Some(unused) = textures[1 - final_index].take() {
                    unused.destroy();
                }
                let brush = ImageBrush::new(image.clone());
                registered_images.push(image);

                let mut replacement = Scene::new();
                replacement.draw_image(
                    brush.as_ref(),
                    from_filter_space * Affine::translate((bounds.x0, bounds.y0)),
                );
                *children = parse_commands(&replacement.commands)?;
                if let RenderCommand::PushLayer(layer) = push {
                    layer.filter = None;
                    layer.transform = from_filter_space;
                    layer.clip = Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1).to_path(0.1);
                }
                statistics.gpu_color_filter_layer_creations = statistics
                    .gpu_color_filter_layer_creations
                    .saturating_add(1);
                if has_blur {
                    statistics.gpu_blur_layer_creations =
                        statistics.gpu_blur_layer_creations.saturating_add(1);
                    statistics.gpu_blur_output_pixels = statistics
                        .gpu_blur_output_pixels
                        .saturating_add(u64::from(bounds.width) * u64::from(bounds.height));
                }
                statistics.gpu_color_filter_pixels = statistics
                    .gpu_color_filter_pixels
                    .saturating_add(u64::from(bounds.width) * u64::from(bounds.height));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectAlphaRepresentation {
    Straight,
    Premultiplied,
}

#[allow(clippy::too_many_arguments)]
fn apply_ordered_gpu_effects(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color_pipeline: &mut Option<ColorEffectPipeline>,
    blur_pipelines: &mut Option<BlurEffectPipelines>,
    list: &ForegroundEffectList,
    physical_scale: f64,
    views: &[wgpu::TextureView; 2],
    statistics: &mut GpuStatistics,
) -> Result<usize, BackendError> {
    if !physical_scale.is_finite() || physical_scale <= 0.0 {
        return Err(effect_error(
            BackendErrorKind::InvalidPlan,
            "GPU foreground effect scale is invalid",
            false,
        ));
    }
    let mut source_index = 0usize;
    let mut representation = EffectAlphaRepresentation::Straight;
    let mut index = 0usize;
    while index < list.functions.len() {
        match &list.functions[index] {
            ForegroundEffect::Color(_) => {
                if representation == EffectAlphaRepresentation::Premultiplied {
                    ensure_blur_pipelines(device, blur_pipelines, statistics)?.apply_conversion(
                        device,
                        queue,
                        &views[source_index],
                        &views[1 - source_index],
                        AlphaConversion::Unpremultiply,
                        statistics,
                    )?;
                    source_index = 1 - source_index;
                    representation = EffectAlphaRepresentation::Straight;
                }
                let start = index;
                while index < list.functions.len()
                    && matches!(list.functions[index], ForegroundEffect::Color(_))
                {
                    index += 1;
                }
                let operations =
                    PackedColorOperations::from_effects(&list.functions[start..index])?;
                if color_pipeline.is_none() {
                    *color_pipeline = Some(ColorEffectPipeline::new(device, statistics)?);
                } else {
                    statistics.gpu_color_filter_cache_hits =
                        statistics.gpu_color_filter_cache_hits.saturating_add(1);
                }
                color_pipeline
                    .as_ref()
                    .expect("GPU color effect pipeline was initialized")
                    .apply(
                        device,
                        queue,
                        &views[source_index],
                        &views[1 - source_index],
                        &operations,
                        statistics,
                    )?;
                source_index = 1 - source_index;
            }
            ForegroundEffect::Blur(blur) => {
                index += 1;
                let physical_sigma = f64::from(blur.sigma.get()) * physical_scale;
                let parameters = derive_blur_parameters(physical_sigma)?;
                if matches!(
                    parameters,
                    crate::render::cpu_blur::BlurParameters::Identity
                ) {
                    continue;
                }
                if representation == EffectAlphaRepresentation::Straight {
                    ensure_blur_pipelines(device, blur_pipelines, statistics)?.apply_conversion(
                        device,
                        queue,
                        &views[source_index],
                        &views[1 - source_index],
                        AlphaConversion::Premultiply,
                        statistics,
                    )?;
                    source_index = 1 - source_index;
                    representation = EffectAlphaRepresentation::Premultiplied;
                }
                ensure_blur_pipelines(device, blur_pipelines, statistics)?.apply_blur(
                    device,
                    queue,
                    &mut source_index,
                    views,
                    &parameters,
                    statistics,
                )?;
            }
            ForegroundEffect::DropShadow(_) => {
                return Err(effect_error(
                    BackendErrorKind::FallbackRequired,
                    "drop-shadow requires complete CPU fallback",
                    true,
                ));
            }
        }
    }
    if representation == EffectAlphaRepresentation::Premultiplied {
        ensure_blur_pipelines(device, blur_pipelines, statistics)?.apply_conversion(
            device,
            queue,
            &views[source_index],
            &views[1 - source_index],
            AlphaConversion::Unpremultiply,
            statistics,
        )?;
        source_index = 1 - source_index;
    }
    Ok(source_index)
}

fn ensure_blur_pipelines<'a>(
    device: &wgpu::Device,
    pipelines: &'a mut Option<BlurEffectPipelines>,
    statistics: &mut GpuStatistics,
) -> Result<&'a mut BlurEffectPipelines, BackendError> {
    if pipelines.is_none() {
        *pipelines = Some(BlurEffectPipelines::new(device, statistics)?);
    }
    Ok(pipelines
        .as_mut()
        .expect("GPU blur pipelines were initialized"))
}

fn create_effect_texture(
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
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn checked_effect_image_bytes(width: u32, height: u32) -> Result<usize, BackendError> {
    if width == 0
        || height == 0
        || width > MAX_EFFECT_LAYER_DIMENSION
        || height > MAX_EFFECT_LAYER_DIMENSION
    {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "GPU color effect layer exceeds the dimension limit",
            true,
        ));
    }
    let bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            effect_error(
                BackendErrorKind::TargetAllocation,
                "GPU color effect image size overflowed",
                true,
            )
        })?;
    if bytes > MAX_EFFECT_IMAGE_BYTES {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "GPU color effect image exceeds the individual byte limit",
            true,
        ));
    }
    Ok(bytes)
}

fn operation_kind(kind: ColorEffectKind) -> u32 {
    match kind {
        ColorEffectKind::Brightness => 0,
        ColorEffectKind::Contrast => 1,
        ColorEffectKind::Grayscale => 2,
        ColorEffectKind::HueRotate => 3,
        ColorEffectKind::Invert => 4,
        ColorEffectKind::Opacity => 5,
        ColorEffectKind::Saturate => 6,
        ColorEffectKind::Sepia => 7,
    }
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
    use crate::render::{CanonicalF32, ColorEffect, ForegroundEffectId};
    use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity};

    fn list(functions: Vec<ForegroundEffect>) -> ForegroundEffectList {
        ForegroundEffectList::from_functions(
            ForegroundEffectId::for_node(
                ExperimentalDocumentIdentity { serial: 1 },
                ExperimentalNodeIdentity {
                    slot: 2,
                    generation: 3,
                },
            ),
            functions,
        )
        .expect("valid test list")
    }

    fn color(kind: ColorEffectKind, value: f32) -> ForegroundEffect {
        ForegroundEffect::Color(ColorEffect {
            kind,
            value: CanonicalF32::new(value).expect("finite test value"),
        })
    }

    #[test]
    fn packed_color_operations_are_fixed_bounded_and_deterministic() {
        let source = list(vec![
            color(ColorEffectKind::Brightness, 2.0),
            color(ColorEffectKind::Contrast, 0.5),
            color(ColorEffectKind::Opacity, 0.25),
        ]);
        let first = PackedColorOperations::from_list(&source).expect("pack list");
        let second = PackedColorOperations::from_list(&source).expect("pack list");
        assert_eq!(first, second);
        assert_eq!(first.operation_count(), 3);
        assert_eq!(first.bytes().len(), PACKED_OPERATION_BUFFER_BYTES);
        assert_eq!(
            u32::from_le_bytes(first.bytes()[..4].try_into().unwrap()),
            3
        );
        assert!(
            first.bytes()[4..PACKED_HEADER_BYTES]
                .iter()
                .all(|byte| *byte == 0)
        );
    }

    #[test]
    fn packed_color_operations_preserve_order_and_repetition() {
        let first = PackedColorOperations::from_list(&list(vec![
            color(ColorEffectKind::Brightness, 2.0),
            color(ColorEffectKind::Contrast, 2.0),
            color(ColorEffectKind::Brightness, 2.0),
        ]))
        .expect("pack ordered list");
        let reordered = PackedColorOperations::from_list(&list(vec![
            color(ColorEffectKind::Contrast, 2.0),
            color(ColorEffectKind::Brightness, 2.0),
            color(ColorEffectKind::Brightness, 2.0),
        ]))
        .expect("pack reordered list");
        assert_ne!(first.bytes(), reordered.bytes());
        let first_kind = f32::from_bits(u32::from_le_bytes(
            first.bytes()[PACKED_HEADER_BYTES..PACKED_HEADER_BYTES + 4]
                .try_into()
                .unwrap(),
        ));
        let repeated_kind = f32::from_bits(u32::from_le_bytes(
            first.bytes()[PACKED_HEADER_BYTES + PACKED_OPERATION_BYTES * 2
                ..PACKED_HEADER_BYTES + PACKED_OPERATION_BYTES * 2 + 4]
                .try_into()
                .unwrap(),
        ));
        assert_eq!(first_kind, repeated_kind);
    }

    #[test]
    fn spatial_operations_request_complete_fallback() {
        let source = list(vec![ForegroundEffect::Blur(crate::render::BlurEffect {
            sigma: CanonicalF32::new(1.0).unwrap(),
        })]);
        let error = PackedColorOperations::from_list(&source).unwrap_err();
        assert_eq!(error.kind, BackendErrorKind::FallbackRequired);
    }
}
