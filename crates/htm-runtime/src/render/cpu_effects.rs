use super::cpu_blur::{CpuBlurAlgorithm, CpuBlurScratch, apply_cpu_blur};
use super::cpu_shadow::{CpuDropShadowResult, CpuShadowScratch, apply_cpu_drop_shadow};
use super::{
    BackendError, BackendErrorKind, ForegroundEffect, ForegroundEffectList, MAX_EFFECT_IMAGE_BYTES,
    MAX_EFFECT_LAYER_DIMENSION, MAX_EFFECT_SURFACE_BYTES, RetainedScene, SceneEffect,
};
use crate::model::LogicalRect;
use anyrender::recording::{ClipCommand, LayerCommand, RenderCommand};
use anyrender::{ImageRenderer, PaintScene, Scene};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use kurbo::{Affine, Rect, Shape};
use peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat};

#[derive(Clone)]
pub(super) struct CpuEffectPlan {
    execution: CpuEffectExecution,
    source_bounds: Option<LogicalRect>,
    filtered_bounds: Option<LogicalRect>,
    element_transform: Option<Affine>,
}

#[derive(Clone)]
enum CpuEffectExecution {
    Ready(ForegroundEffectList),
    Identity,
    Deferred,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct CpuEffectStatistics {
    pub layer_creations: u64,
    pub layer_reuses: u64,
    pub identity_fast_paths: u64,
    pub deferred_layers: u64,
    pub filtered_pixels: u64,
    pub blur_stages: u64,
    pub gaussian_blur_stages: u64,
    pub three_box_blur_stages: u64,
    pub blur_passes: u64,
    pub blur_pixels: u64,
    pub blur_scratch_reuses: u64,
    pub blur_scratch_replacements: u64,
    pub blur_scratch_bytes: usize,
    pub drop_shadow_stages: u64,
    pub shadow_identity_fast_paths: u64,
    pub shadow_mask_pixels: u64,
    pub shadow_blur_stages: u64,
    pub shadow_composite_pixels: u64,
    pub shadow_mask_reuses: u64,
    pub shadow_mask_replacements: u64,
    pub shadow_scratch_bytes: usize,
    pub allocated_image_bytes: usize,
}

#[derive(Default)]
pub(super) struct CpuEffectScratch {
    renderer: Option<(u32, u32, VelloCpuImageRenderer)>,
    blur: CpuBlurScratch,
    shadow: CpuShadowScratch,
}

#[derive(Clone, Copy)]
struct PhysicalEffectBounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    width: u32,
    height: u32,
}

#[derive(Clone)]
enum RecordedNode {
    Leaf(RenderCommand),
    Group {
        push: RenderCommand,
        children: Vec<RecordedNode>,
    },
}

pub(super) fn collect_effect_plans(scene: &RetainedScene) -> Vec<CpuEffectPlan> {
    let mut nodes: Vec<_> = scene
        .nodes
        .iter()
        .filter_map(|node| {
            let element_transform = node.effects.iter().find_map(|effect| match effect {
                SceneEffect::Transform { coefficients } => Some(Affine::new(*coefficients)),
                _ => None,
            });
            node.effects.iter().find_map(|effect| match effect {
                SceneEffect::ForegroundFilter {
                    list,
                    source_graphic_bounds,
                    filtered_bounds,
                    ..
                } => {
                    let execution = if list.is_visual_identity() {
                        CpuEffectExecution::Identity
                    } else if list.functions.iter().all(|effect| {
                        matches!(
                            effect,
                            ForegroundEffect::Color(_)
                                | ForegroundEffect::Blur(_)
                                | ForegroundEffect::DropShadow(_)
                        )
                    }) {
                        CpuEffectExecution::Ready(list.clone())
                    } else {
                        CpuEffectExecution::Deferred
                    };
                    Some((
                        (node.paint_order, node.tree_order, node.id),
                        CpuEffectPlan {
                            execution,
                            source_bounds: Some(source_graphic_bounds.clone()),
                            filtered_bounds: Some(filtered_bounds.clone()),
                            element_transform,
                        },
                    ))
                }
                SceneEffect::RejectedForegroundFilter { .. } => Some((
                    (node.paint_order, node.tree_order, node.id),
                    CpuEffectPlan {
                        execution: CpuEffectExecution::Deferred,
                        source_bounds: None,
                        filtered_bounds: None,
                        element_transform,
                    },
                )),
                _ => None,
            })
        })
        .collect();
    nodes.sort_by_key(|(order, _)| *order);
    nodes.into_iter().map(|(_, plan)| plan).collect()
}

pub(super) fn execute_cpu_effects(
    recording: &Scene,
    plans: &[CpuEffectPlan],
    target_width: u32,
    target_height: u32,
    scale: f64,
    scratch: &mut CpuEffectScratch,
) -> Result<(Scene, CpuEffectStatistics), BackendError> {
    let mut scaled = Scene::with_tolerance(recording.tolerance);
    scaled.append_scene(recording.clone(), Affine::scale(scale));
    let mut nodes = parse_commands(&scaled.commands)?;
    include_outset_box_shadows(&mut nodes);

    let mut plan_index = 0usize;
    let mut statistics = CpuEffectStatistics::default();
    let mut allocated_bytes = 0usize;
    transform_nodes(
        &mut nodes,
        plans,
        &mut plan_index,
        target_width,
        target_height,
        scale,
        scratch,
        &mut allocated_bytes,
        &mut statistics,
    )?;
    if plan_index != plans.len() {
        return Err(effect_error(
            BackendErrorKind::ResourcePreparation,
            "CPU effect plan count does not match the recorded foreground-filter layers",
            false,
        ));
    }

    let mut output = Scene::with_tolerance(recording.tolerance);
    flatten_nodes(&nodes, &mut output.commands);
    statistics.blur_scratch_bytes = scratch.blur.allocated_bytes();
    statistics.shadow_scratch_bytes = scratch.shadow.allocated_bytes();
    statistics.allocated_image_bytes = checked_surface_bytes(
        checked_surface_bytes(allocated_bytes, statistics.blur_scratch_bytes)?,
        statistics.shadow_scratch_bytes,
    )?;
    Ok((output, statistics))
}

fn parse_commands(commands: &[RenderCommand]) -> Result<Vec<RecordedNode>, BackendError> {
    fn parse(
        commands: &[RenderCommand],
        cursor: &mut usize,
        nested: bool,
    ) -> Result<Vec<RecordedNode>, BackendError> {
        let mut nodes = Vec::new();
        while *cursor < commands.len() {
            match &commands[*cursor] {
                RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) => {
                    let push = commands[*cursor].clone();
                    *cursor += 1;
                    let children = parse(commands, cursor, true)?;
                    nodes.push(RecordedNode::Group { push, children });
                }
                RenderCommand::PopLayer => {
                    if !nested {
                        return Err(effect_error(
                            BackendErrorKind::CommandEncoding,
                            "CPU recording contains an unmatched layer pop",
                            false,
                        ));
                    }
                    *cursor += 1;
                    return Ok(nodes);
                }
                command => {
                    nodes.push(RecordedNode::Leaf(command.clone()));
                    *cursor += 1;
                }
            }
        }
        if nested {
            return Err(effect_error(
                BackendErrorKind::CommandEncoding,
                "CPU recording contains an unterminated layer",
                false,
            ));
        }
        Ok(nodes)
    }

    let mut cursor = 0;
    parse(commands, &mut cursor, false)
}

fn flatten_nodes(nodes: &[RecordedNode], commands: &mut Vec<RenderCommand>) {
    for node in nodes {
        match node {
            RecordedNode::Leaf(command) => commands.push(command.clone()),
            RecordedNode::Group { push, children } => {
                commands.push(push.clone());
                flatten_nodes(children, commands);
                commands.push(RenderCommand::PopLayer);
            }
        }
    }
}

fn include_outset_box_shadows(nodes: &mut Vec<RecordedNode>) {
    for node in nodes.iter_mut() {
        if let RecordedNode::Group { children, .. } = node {
            include_outset_box_shadows(children);
        }
    }

    let mut index = 1usize;
    while index < nodes.len() {
        if nodes[index - 1].is_box_shadow_only() && nodes[index].has_direct_filter_owner() {
            let shadow = nodes.remove(index - 1);
            nodes[index - 1]
                .direct_filter_owner_mut()
                .expect("filter owner was checked")
                .insert(0, shadow);
            index = index.saturating_sub(1).max(1);
        } else {
            index += 1;
        }
    }
}

impl RecordedNode {
    fn is_box_shadow_only(&self) -> bool {
        match self {
            Self::Leaf(RenderCommand::BoxShadow(_)) => true,
            Self::Leaf(_) => false,
            Self::Group { children, .. } => {
                !children.is_empty() && children.iter().all(Self::is_box_shadow_only)
            }
        }
    }

    fn has_direct_filter_owner(&self) -> bool {
        match self {
            Self::Group { push, .. } if command_has_foreground_filter(push) => true,
            Self::Group { children, .. } if children.len() == 1 => {
                children[0].has_direct_filter_owner()
            }
            _ => false,
        }
    }

    fn direct_filter_owner_mut(&mut self) -> Option<&mut Vec<RecordedNode>> {
        let Self::Group { push, children } = self else {
            return None;
        };
        if command_has_foreground_filter(push) {
            return Some(children);
        }
        if children.len() == 1 {
            return children[0].direct_filter_owner_mut();
        }
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn transform_nodes(
    nodes: &mut Vec<RecordedNode>,
    plans: &[CpuEffectPlan],
    plan_index: &mut usize,
    target_width: u32,
    target_height: u32,
    scale: f64,
    scratch: &mut CpuEffectScratch,
    allocated_bytes: &mut usize,
    statistics: &mut CpuEffectStatistics,
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
                    "recorded foreground-filter layer has no retained effect plan",
                    false,
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
            target_width,
            target_height,
            scale,
            scratch,
            allocated_bytes,
            statistics,
        )?;

        let Some(plan) = own_plan else {
            continue;
        };
        match plan.execution {
            CpuEffectExecution::Identity => {
                remove_identity_filter(push, target_width, target_height);
                statistics.identity_fast_paths = statistics.identity_fast_paths.saturating_add(1);
            }
            CpuEffectExecution::Deferred => {
                statistics.deferred_layers = statistics.deferred_layers.saturating_add(1);
            }
            CpuEffectExecution::Ready(list) => {
                let (to_filter_space, from_filter_space) =
                    filter_space_transforms(push, plan.element_transform)?;
                let source_bounds = plan.source_bounds.as_ref().ok_or_else(|| {
                    effect_error(
                        BackendErrorKind::ResourcePreparation,
                        "executable CPU effect plan has no SourceGraphic bounds",
                        false,
                    )
                })?;
                if source_bounds.width <= 0.0 || source_bounds.height <= 0.0 {
                    continue;
                }
                let bounds = physical_bounds(
                    plan.filtered_bounds.as_ref().ok_or_else(|| {
                        effect_error(
                            BackendErrorKind::ResourcePreparation,
                            "executable CPU effect plan has no filtered bounds",
                            false,
                        )
                    })?,
                    target_width,
                    target_height,
                    scale,
                    to_filter_space,
                )?;
                let Some(bounds) = bounds else {
                    continue;
                };
                let width = bounds.width;
                let height = bounds.height;
                let image_bytes = checked_image_bytes(width, height)?;
                let next_total = checked_surface_bytes(*allocated_bytes, image_bytes)?;
                let has_active_rgba_blur = list.functions.iter().any(|effect| {
                    matches!(effect, ForegroundEffect::Blur(blur) if blur.sigma.get() > 0.0)
                });
                let has_active_shadow = list.functions.iter().any(|effect| {
                    matches!(effect, ForegroundEffect::DropShadow(shadow) if shadow.color.alpha.get() > 0.0)
                });
                let has_active_shadow_blur = list.functions.iter().any(|effect| {
                    matches!(
                        effect,
                        ForegroundEffect::DropShadow(shadow)
                            if shadow.color.alpha.get() > 0.0 && shadow.sigma.get() > 0.0
                    )
                });
                let mask_bytes = image_bytes / 4;
                let blur_work_bytes = if has_active_rgba_blur {
                    image_bytes
                } else if has_active_shadow_blur {
                    mask_bytes
                } else {
                    0
                };
                let shadow_work_bytes = if has_active_shadow { mask_bytes } else { 0 };
                checked_surface_bytes(
                    checked_surface_bytes(next_total, blur_work_bytes)?,
                    shadow_work_bytes,
                )?;
                if checked_surface_bytes(next_total, scratch.blur.allocated_bytes()).is_err() {
                    scratch.blur = CpuBlurScratch::default();
                }
                if checked_surface_bytes(next_total, scratch.shadow.allocated_bytes()).is_err() {
                    scratch.shadow = CpuShadowScratch::default();
                }

                let mut recorded_source = Scene::new();
                flatten_nodes(children, &mut recorded_source.commands);
                let mut source = Scene::new();
                source.append_scene(recorded_source, to_filter_space);
                let mut pixels =
                    scratch.render(source, bounds.x0, bounds.y0, width, height, statistics)?;
                pixels = apply_ordered_effects(
                    pixels, &list, width, height, scale, scratch, next_total, statistics,
                )?;
                statistics.filtered_pixels = statistics
                    .filtered_pixels
                    .saturating_add(u64::from(width) * u64::from(height));
                statistics.layer_creations = statistics.layer_creations.saturating_add(1);
                *allocated_bytes = next_total;

                let image = ImageBrush::new(ImageData {
                    data: Blob::from(pixels),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::AlphaPremultiplied,
                    width,
                    height,
                });
                let mut replacement = Scene::new();
                replacement.draw_image(
                    image.as_ref(),
                    from_filter_space * Affine::translate((bounds.x0, bounds.y0)),
                );
                *children = parse_commands(&replacement.commands)?;
                if let RenderCommand::PushLayer(layer) = push {
                    layer.filter = None;
                    layer.transform = from_filter_space;
                    layer.clip = Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1).to_path(0.1);
                }
            }
        }
    }
    Ok(())
}

impl CpuEffectScratch {
    fn render(
        &mut self,
        scene: Scene,
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        statistics: &mut CpuEffectStatistics,
    ) -> Result<Vec<u8>, BackendError> {
        let byte_len = checked_image_bytes(width, height)?;
        let renderer = match &mut self.renderer {
            Some((old_width, old_height, renderer))
                if *old_width == width && *old_height == height =>
            {
                statistics.layer_reuses = statistics.layer_reuses.saturating_add(1);
                renderer.reset();
                renderer
            }
            Some((old_width, old_height, renderer)) => {
                *old_width = width;
                *old_height = height;
                renderer.resize(width, height);
                renderer.reset();
                renderer
            }
            slot @ None => {
                let _ = slot.insert((width, height, VelloCpuImageRenderer::new(width, height)));
                &mut slot.as_mut().expect("renderer inserted").2
            }
        };
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(byte_len).map_err(|_| {
            effect_error(
                BackendErrorKind::TargetAllocation,
                "CPU effect-layer allocation failed",
                true,
            )
        })?;
        pixels.resize(byte_len, 0);
        renderer.render(
            |target| target.append_scene(scene, Affine::translate((-x, -y))),
            &mut pixels,
        );
        Ok(pixels)
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_ordered_effects(
    mut pixels: Vec<u8>,
    list: &ForegroundEffectList,
    width: u32,
    height: u32,
    scale: f64,
    scratch: &mut CpuEffectScratch,
    committed_surface_bytes: usize,
    statistics: &mut CpuEffectStatistics,
) -> Result<Vec<u8>, BackendError> {
    let mut index = 0usize;
    while index < list.functions.len() {
        match &list.functions[index] {
            ForegroundEffect::Color(_) => {
                let start = index;
                while matches!(list.functions.get(index), Some(ForegroundEffect::Color(_))) {
                    index += 1;
                }
                apply_ordered_color_matrices(&mut pixels, &list.functions[start..index])?;
            }
            ForegroundEffect::Blur(blur) => {
                index += 1;
                let physical_sigma = f64::from(blur.sigma.get()) * scale;
                if physical_sigma == 0.0 {
                    continue;
                }
                let result = apply_cpu_blur(
                    pixels,
                    width,
                    height,
                    physical_sigma,
                    &mut scratch.blur,
                    checked_surface_bytes(
                        committed_surface_bytes,
                        scratch.shadow.allocated_bytes(),
                    )?,
                )?;
                pixels = result.pixels;
                statistics.blur_stages = statistics.blur_stages.saturating_add(1);
                statistics.blur_passes = statistics
                    .blur_passes
                    .saturating_add(u64::from(result.pass_count));
                statistics.blur_pixels = statistics
                    .blur_pixels
                    .saturating_add(u64::from(width) * u64::from(height));
                match result.algorithm {
                    CpuBlurAlgorithm::DirectGaussian => {
                        statistics.gaussian_blur_stages =
                            statistics.gaussian_blur_stages.saturating_add(1);
                    }
                    CpuBlurAlgorithm::ThreeBox => {
                        statistics.three_box_blur_stages =
                            statistics.three_box_blur_stages.saturating_add(1);
                    }
                }
                if result.scratch_reused {
                    statistics.blur_scratch_reuses =
                        statistics.blur_scratch_reuses.saturating_add(1);
                } else {
                    statistics.blur_scratch_replacements =
                        statistics.blur_scratch_replacements.saturating_add(1);
                }
            }
            ForegroundEffect::DropShadow(shadow) => {
                index += 1;
                let result = apply_cpu_drop_shadow(
                    pixels,
                    width,
                    height,
                    *shadow,
                    scale,
                    &mut scratch.blur,
                    &mut scratch.shadow,
                    committed_surface_bytes,
                )?;
                record_shadow_statistics(&result, width, height, statistics);
                pixels = result.pixels;
            }
        }
    }
    Ok(pixels)
}

fn record_shadow_statistics(
    result: &CpuDropShadowResult,
    width: u32,
    height: u32,
    statistics: &mut CpuEffectStatistics,
) {
    statistics.drop_shadow_stages = statistics.drop_shadow_stages.saturating_add(1);
    if result.identity_fast_path {
        statistics.shadow_identity_fast_paths =
            statistics.shadow_identity_fast_paths.saturating_add(1);
        return;
    }
    let pixels = u64::from(width) * u64::from(height);
    statistics.shadow_mask_pixels = statistics.shadow_mask_pixels.saturating_add(pixels);
    statistics.shadow_composite_pixels = statistics.shadow_composite_pixels.saturating_add(pixels);
    if result.blur_algorithm.is_some() {
        statistics.shadow_blur_stages = statistics.shadow_blur_stages.saturating_add(1);
        statistics.blur_passes = statistics
            .blur_passes
            .saturating_add(u64::from(result.blur_pass_count));
        statistics.blur_pixels = statistics.blur_pixels.saturating_add(pixels);
        match result.blur_algorithm {
            Some(CpuBlurAlgorithm::DirectGaussian) => {
                statistics.gaussian_blur_stages = statistics.gaussian_blur_stages.saturating_add(1);
            }
            Some(CpuBlurAlgorithm::ThreeBox) => {
                statistics.three_box_blur_stages =
                    statistics.three_box_blur_stages.saturating_add(1);
            }
            None => {}
        }
        if result.blur_scratch_reused {
            statistics.blur_scratch_reuses = statistics.blur_scratch_reuses.saturating_add(1);
        } else {
            statistics.blur_scratch_replacements =
                statistics.blur_scratch_replacements.saturating_add(1);
        }
    }
    if result.mask_scratch_reused {
        statistics.shadow_mask_reuses = statistics.shadow_mask_reuses.saturating_add(1);
    } else {
        statistics.shadow_mask_replacements = statistics.shadow_mask_replacements.saturating_add(1);
    }
}

fn apply_ordered_color_matrices(
    pixels: &mut [u8],
    effects: &[ForegroundEffect],
) -> Result<(), BackendError> {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = f32::from(pixel[3]) / 255.0;
        let mut straight = if pixel[3] == 0 {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [
                (f32::from(pixel[0]) / f32::from(pixel[3])).clamp(0.0, 1.0),
                (f32::from(pixel[1]) / f32::from(pixel[3])).clamp(0.0, 1.0),
                (f32::from(pixel[2]) / f32::from(pixel[3])).clamp(0.0, 1.0),
                alpha,
            ]
        };
        for effect in effects {
            let ForegroundEffect::Color(_) = effect else {
                return Err(effect_error(
                    BackendErrorKind::UnsupportedCapability,
                    "spatial foreground effects cannot enter the CPU color compositor",
                    false,
                ));
            };
            let matrix = effect.color_matrix().map_err(|_| {
                effect_error(
                    BackendErrorKind::ResourcePreparation,
                    "normalized foreground color matrix is invalid",
                    false,
                )
            })?;
            straight = matrix
                .expect("color effects always derive a matrix")
                .transform(straight);
            if straight.iter().any(|channel| !channel.is_finite()) {
                return Err(effect_error(
                    BackendErrorKind::ResourcePreparation,
                    "foreground color filtering produced a nonfinite channel",
                    false,
                ));
            }
            straight = straight.map(|channel| channel.clamp(0.0, 1.0));
        }
        store_premultiplied(pixel, straight);
    }
    Ok(())
}

fn store_premultiplied(pixel: &mut [u8], straight: [f32; 4]) {
    let alpha = quantize(straight[3]);
    pixel[3] = alpha;
    for channel in 0..3 {
        pixel[channel] = quantize(straight[channel] * straight[3]).min(alpha);
    }
}

fn quantize(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn filter_space_transforms(
    command: &RenderCommand,
    element_transform: Option<Affine>,
) -> Result<(Affine, Affine), BackendError> {
    let Some(element_transform) = element_transform else {
        return Ok((Affine::IDENTITY, Affine::IDENTITY));
    };
    let RenderCommand::PushLayer(layer) = command else {
        return Err(effect_error(
            BackendErrorKind::CommandEncoding,
            "foreground filter is not represented by a recorded effect layer",
            false,
        ));
    };
    if element_transform.determinant().abs() <= f64::EPSILON
        || layer.transform.determinant().abs() <= f64::EPSILON
    {
        return Ok((Affine::IDENTITY, Affine::IDENTITY));
    }
    let base_transform = layer.transform * element_transform.inverse();
    let to_filter_space = base_transform * layer.transform.inverse();
    let from_filter_space = to_filter_space.inverse();
    if to_filter_space
        .as_coeffs()
        .into_iter()
        .chain(from_filter_space.as_coeffs())
        .any(|coefficient| !coefficient.is_finite())
    {
        return Err(effect_error(
            BackendErrorKind::InvalidPlan,
            "foreground filter transform is nonfinite",
            false,
        ));
    }
    Ok((to_filter_space, from_filter_space))
}

fn physical_bounds(
    bounds: &LogicalRect,
    target_width: u32,
    target_height: u32,
    scale: f64,
    to_filter_space: Affine,
) -> Result<Option<PhysicalEffectBounds>, BackendError> {
    let values = [
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.width),
        f64::from(bounds.height),
        scale,
    ];
    if values.into_iter().any(|value| !value.is_finite()) || scale <= 0.0 {
        return Err(effect_error(
            BackendErrorKind::InvalidPlan,
            "CPU effect bounds or scale are nonfinite",
            false,
        ));
    }
    let presented = Rect::new(
        f64::from(bounds.x) * scale,
        f64::from(bounds.y) * scale,
        (f64::from(bounds.x) + f64::from(bounds.width)) * scale,
        (f64::from(bounds.y) + f64::from(bounds.height)) * scale,
    );
    let visible = presented.intersect(Rect::new(
        0.0,
        0.0,
        f64::from(target_width),
        f64::from(target_height),
    ));
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return Ok(None);
    }
    // The final target clips the filtered result. The effect layer itself must retain
    // the complete SourceGraphic because blur and offsets can move off-target source
    // samples back into the visible result.
    let mapped = to_filter_space.transform_rect_bbox(presented);
    let x0 = mapped.x0.floor();
    let y0 = mapped.y0.floor();
    let x1 = mapped.x1.ceil();
    let y1 = mapped.y1.ceil();
    if [x0, y0, x1, y1]
        .into_iter()
        .any(|coordinate| !coordinate.is_finite())
        || x1 <= x0
        || y1 <= y0
        || x0 < f64::from(i32::MIN)
        || y0 < f64::from(i32::MIN)
        || x1 > f64::from(i32::MAX)
        || y1 > f64::from(i32::MAX)
    {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "CPU effect layer has invalid filter-space bounds",
            true,
        ));
    }
    let width = x1 - x0;
    let height = y1 - y0;
    if width > f64::from(u32::MAX) || height > f64::from(u32::MAX) {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "CPU effect layer dimensions overflow",
            true,
        ));
    }
    Ok(Some(PhysicalEffectBounds {
        x0,
        y0,
        x1,
        y1,
        width: width as u32,
        height: height as u32,
    }))
}

fn checked_image_bytes(width: u32, height: u32) -> Result<usize, BackendError> {
    if width == 0
        || height == 0
        || width > MAX_EFFECT_LAYER_DIMENSION
        || height > MAX_EFFECT_LAYER_DIMENSION
    {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "CPU effect layer exceeds the dimension limit",
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
                "CPU effect-layer byte size overflowed",
                true,
            )
        })?;
    if bytes > MAX_EFFECT_IMAGE_BYTES {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "CPU effect layer exceeds the image byte limit",
            true,
        ));
    }
    Ok(bytes)
}

fn checked_surface_bytes(current: usize, additional: usize) -> Result<usize, BackendError> {
    let total = current.checked_add(additional).ok_or_else(|| {
        effect_error(
            BackendErrorKind::TargetAllocation,
            "CPU effect-layer byte accounting overflowed",
            true,
        )
    })?;
    if total > MAX_EFFECT_SURFACE_BYTES {
        return Err(effect_error(
            BackendErrorKind::TargetAllocation,
            "CPU effect layers exceed the per-surface byte limit",
            true,
        ));
    }
    Ok(total)
}

fn remove_identity_filter(push: &mut RenderCommand, width: u32, height: u32) {
    let RenderCommand::PushLayer(layer) = push else {
        return;
    };
    layer.filter = None;
    if layer.alpha == 1.0 && layer.backdrop_filter.is_none() {
        *push = RenderCommand::PushClipLayer(ClipCommand {
            transform: Affine::IDENTITY,
            clip: Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).to_path(0.1),
        });
    }
}

fn command_has_foreground_filter(command: &RenderCommand) -> bool {
    matches!(
        command,
        RenderCommand::PushLayer(LayerCommand {
            filter: Some(_),
            ..
        })
    )
}

fn effect_error(kind: BackendErrorKind, message: &'static str, recoverable: bool) -> BackendError {
    BackendError::new(kind, message, recoverable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{CanonicalF32, ColorEffect, ColorEffectKind, ForegroundEffectId};
    use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity};

    fn list(effects: &[(ColorEffectKind, f32)]) -> ForegroundEffectList {
        ForegroundEffectList::from_functions(
            ForegroundEffectId::for_node(
                ExperimentalDocumentIdentity { serial: 1 },
                ExperimentalNodeIdentity {
                    slot: 2,
                    generation: 3,
                },
            ),
            effects
                .iter()
                .map(|(kind, value)| {
                    ForegroundEffect::Color(ColorEffect {
                        kind: *kind,
                        value: CanonicalF32::new(*value).unwrap(),
                    })
                })
                .collect(),
        )
        .unwrap()
    }

    fn filtered(input: [u8; 4], effects: &[(ColorEffectKind, f32)]) -> [u8; 4] {
        let mut pixels = input;
        apply_ordered_color_matrices(&mut pixels, &list(effects).functions).unwrap();
        pixels
    }

    #[test]
    fn color_math_unpremultiplies_clamps_each_stage_and_premultiplies_once() {
        assert_eq!(
            filtered([64, 32, 16, 128], &[(ColorEffectKind::Brightness, 2.0)]),
            [128, 64, 32, 128]
        );
        assert_eq!(
            filtered([64, 32, 16, 128], &[(ColorEffectKind::Opacity, 0.5)]),
            [32, 16, 8, 64]
        );
        assert_eq!(
            filtered(
                [64, 32, 16, 128],
                &[
                    (ColorEffectKind::Brightness, 2.0),
                    (ColorEffectKind::Invert, 1.0),
                ],
            ),
            [0, 64, 96, 128]
        );
        assert_eq!(
            filtered([9, 8, 7, 0], &[(ColorEffectKind::Invert, 1.0)]),
            [0, 0, 0, 0]
        );
    }

    #[test]
    fn every_color_function_uses_the_normalized_encoded_srgb_matrix() {
        let input = [64, 128, 192, 255];
        let fixtures = [
            (ColorEffectKind::Brightness, 0.5, [32, 64, 96, 255]),
            (ColorEffectKind::Contrast, 0.5, [96, 128, 160, 255]),
            (ColorEffectKind::Grayscale, 1.0, [119, 119, 119, 255]),
            (
                ColorEffectKind::HueRotate,
                std::f32::consts::FRAC_PI_2,
                [192, 92, 174, 255],
            ),
            (ColorEffectKind::Invert, 0.5, [128, 128, 128, 255]),
            (ColorEffectKind::Opacity, 0.5, [32, 64, 96, 128]),
            (ColorEffectKind::Saturate, 0.0, [119, 119, 119, 255]),
            (ColorEffectKind::Sepia, 1.0, [160, 142, 111, 255]),
        ];
        for (kind, value, expected) in fixtures {
            assert_eq!(filtered(input, &[(kind, value)]), expected, "{kind:?}");
        }
    }

    #[test]
    fn alpha_boundaries_remain_finite_canonical_and_premultiplied() {
        for alpha in [0, 1, 64, 128, 254, 255] {
            let input = [alpha / 2, alpha / 3, alpha / 4, alpha];
            for effects in [
                vec![(ColorEffectKind::Brightness, 1.0)],
                vec![(ColorEffectKind::Brightness, 2.0)],
                vec![(ColorEffectKind::Contrast, 2.0)],
                vec![(ColorEffectKind::Invert, 0.75)],
                vec![(ColorEffectKind::Opacity, 0.5)],
                vec![
                    (ColorEffectKind::Brightness, 2.0),
                    (ColorEffectKind::Invert, 0.25),
                    (ColorEffectKind::Opacity, 0.5),
                ],
            ] {
                let output = filtered(input, &effects);
                assert!(output[0] <= output[3]);
                assert!(output[1] <= output[3]);
                assert!(output[2] <= output[3]);
                if output[3] == 0 {
                    assert_eq!(output, [0, 0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn per_stage_clamping_makes_order_and_repetition_observable() {
        let input = [64, 128, 192, 255];
        assert_eq!(
            filtered(
                input,
                &[
                    (ColorEffectKind::Brightness, 2.0),
                    (ColorEffectKind::Contrast, 0.5),
                ],
            ),
            [128, 191, 191, 255]
        );
        assert_eq!(
            filtered(
                input,
                &[
                    (ColorEffectKind::Contrast, 0.5),
                    (ColorEffectKind::Brightness, 2.0),
                ],
            ),
            [192, 255, 255, 255]
        );
        assert_eq!(
            filtered(
                [32, 64, 96, 255],
                &[
                    (ColorEffectKind::Brightness, 2.0),
                    (ColorEffectKind::Brightness, 2.0),
                ],
            ),
            [128, 255, 255, 255]
        );
    }

    #[test]
    fn image_limits_are_checked_before_allocation() {
        assert_eq!(checked_image_bytes(4096, 4096).unwrap(), 64 * 1024 * 1024);
        assert!(checked_image_bytes(4097, 1).is_err());
        assert!(checked_image_bytes(0, 1).is_err());
        assert_eq!(
            checked_surface_bytes(MAX_EFFECT_SURFACE_BYTES - 4, 4).unwrap(),
            MAX_EFFECT_SURFACE_BYTES
        );
        assert!(checked_surface_bytes(MAX_EFFECT_SURFACE_BYTES, 4).is_err());
        assert!(checked_surface_bytes(usize::MAX, 1).is_err());
    }
}
