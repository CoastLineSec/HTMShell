use super::cpu_effects::{
    CpuEffectPlan, CpuEffectScratch, CpuEffectStatistics, collect_effect_plans,
    execute_color_effects,
};
use super::{
    BackendError, BackendErrorKind, DamageRegion, FramePlan, FrameReason, FrameReasonSet,
    PixelFormat, RenderResult, RenderSurfaceId, RenderTarget, Renderer, RetainedScene,
    SceneChangeKind, SceneRevision, build_retained_scene, logical_damage_to_physical,
};
use crate::identity::IdentityRegistry;
use crate::model::ViewportSpec;
use crate::{ExperimentalDocumentIdentity, RuntimeError};
use anyrender::{PaintScene, Scene, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_html::HtmlDocument;
use blitz_paint::paint_scene;
use kurbo::Affine;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct CpuPreparedScene {
    pub(crate) revision: SceneRevision,
    pub(crate) recording: Scene,
}

#[derive(Default)]
pub(super) struct CpuReferenceRenderer {
    prepared: BTreeMap<SceneRevision, CpuPreparedScene>,
    targets: BTreeMap<RenderSurfaceId, RenderTarget>,
    effect_plans: BTreeMap<SceneRevision, Vec<CpuEffectPlan>>,
    effect_scratch: CpuEffectScratch,
    last_effect_statistics: CpuEffectStatistics,
}

impl Renderer for CpuReferenceRenderer {
    type Prepared = CpuPreparedScene;

    fn create_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError> {
        if target.width == 0 || target.height == 0 {
            return Err(BackendError::new(
                BackendErrorKind::TargetAllocation,
                "CPU target dimensions must be nonzero",
                true,
            ));
        }
        self.targets.insert(surface, target);
        Ok(())
    }

    fn resize_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError> {
        if !self.targets.contains_key(&surface) {
            return Err(BackendError::new(
                BackendErrorKind::TargetAllocation,
                "cannot resize a missing CPU target",
                true,
            ));
        }
        self.create_target(surface, target)
    }

    fn prepare(&mut self, plan: &FramePlan, prepared: Self::Prepared) -> Result<(), BackendError> {
        if prepared.revision != plan.scene_revision {
            return Err(BackendError::new(
                BackendErrorKind::ResourcePreparation,
                "CPU recording revision does not match the frame plan",
                true,
            ));
        }
        self.effect_plans
            .insert(plan.scene_revision, collect_effect_plans(&plan.scene));
        self.prepared.insert(prepared.revision, prepared);
        Ok(())
    }

    fn render(
        &mut self,
        plan: &FramePlan,
        target: RenderTarget,
    ) -> Result<RenderResult, BackendError> {
        if !plan.presentation_eligible
            || target.width != plan.physical_width
            || target.height != plan.physical_height
            || target.pixel_format != plan.pixel_format
            || self.targets.get(&plan.surface) != Some(&target)
        {
            return Err(BackendError::new(
                BackendErrorKind::InvalidPlan,
                "CPU render target does not match the frame plan",
                false,
            ));
        }
        let prepared = self.prepared.get(&plan.scene_revision).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::ResourcePreparation,
                "CPU recording is unavailable for the requested scene revision",
                true,
            )
        })?;
        let scale = f64::from(plan.scale_numerator) / f64::from(plan.scale_denominator);
        let plans = self.effect_plans.get(&plan.scene_revision).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::ResourcePreparation,
                "CPU effect plans are unavailable for the requested scene revision",
                true,
            )
        })?;
        let (recording, statistics) = execute_color_effects(
            &prepared.recording,
            plans,
            target.width,
            target.height,
            scale,
            &mut self.effect_scratch,
        )?;
        self.last_effect_statistics = statistics;
        let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
            |target| target.append_scene(recording, Affine::IDENTITY),
            target.width,
            target.height,
        );
        Ok(RenderResult {
            scene_revision: plan.scene_revision,
            pixels,
            applied_damage: plan.damage.clone(),
            full_raster: true,
            prepared_resources: plan.scene.live_resources(),
        })
    }

    fn readback(&mut self, result: RenderResult) -> Result<Vec<u8>, BackendError> {
        Ok(result.pixels)
    }

    fn release_resources(
        &mut self,
        _live: &[(super::SceneResourceId, super::SceneResourceVersion)],
    ) -> Result<(), BackendError> {
        if self.prepared.len() > 2 {
            let newest = self.prepared.keys().next_back().copied();
            self.prepared
                .retain(|revision, _| Some(*revision) == newest);
            self.effect_plans
                .retain(|revision, _| Some(*revision) == newest);
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), BackendError> {
        self.prepared.clear();
        self.effect_plans.clear();
        self.effect_scratch = CpuEffectScratch::default();
        self.last_effect_statistics = CpuEffectStatistics::default();
        self.targets.clear();
        Ok(())
    }

    fn release_target(&mut self, surface: RenderSurfaceId) {
        self.targets.remove(&surface);
    }

    fn shutdown(&mut self) {
        self.prepared.clear();
        self.effect_plans.clear();
        self.effect_scratch = CpuEffectScratch::default();
        self.last_effect_statistics = CpuEffectStatistics::default();
        self.targets.clear();
    }
}

impl CpuReferenceRenderer {
    fn has_target(&self, surface: RenderSurfaceId) -> bool {
        self.targets.contains_key(&surface)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CpuFrame {
    pub plan: Arc<FramePlan>,
    pub pixels: Vec<u8>,
    pub full_raster: bool,
}

#[derive(Clone)]
pub(crate) struct PreparedRender {
    pub(crate) plan: Arc<FramePlan>,
    pub(crate) prepared: CpuPreparedScene,
    scene: Arc<RetainedScene>,
    surface: RenderSurfaceId,
    size: [u32; 4],
    scale: [u32; 2],
}

#[derive(Default)]
pub(crate) struct CpuRenderSession {
    next_revision: u64,
    current_scene: Option<Arc<RetainedScene>>,
    current_surface: Option<RenderSurfaceId>,
    current_size: Option<[u32; 4]>,
    current_scale: Option<[u32; 2]>,
    renderer: CpuReferenceRenderer,
    recovery_pending: bool,
}

impl CpuRenderSession {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_document(
        &mut self,
        document: &mut HtmlDocument,
        identities: &IdentityRegistry,
        document_identity: ExperimentalDocumentIdentity,
        viewport: ViewportSpec,
        surface: RenderSurfaceId,
        physical_width: u32,
        physical_height: u32,
        scale_numerator: u32,
        scale_denominator: u32,
        reasons: FrameReasonSet,
        force: bool,
    ) -> Result<Option<CpuFrame>, RuntimeError> {
        let Some(prepared) = self.prepare_document(
            document,
            identities,
            document_identity,
            viewport,
            surface,
            physical_width,
            physical_height,
            scale_numerator,
            scale_denominator,
            reasons,
            force,
        )?
        else {
            return Ok(None);
        };
        let frame = self.render_prepared_cpu(&prepared)?;
        self.accept_prepared(&prepared);
        Ok(Some(frame))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_document(
        &mut self,
        document: &mut HtmlDocument,
        identities: &IdentityRegistry,
        document_identity: ExperimentalDocumentIdentity,
        viewport: ViewportSpec,
        surface: RenderSurfaceId,
        physical_width: u32,
        physical_height: u32,
        scale_numerator: u32,
        scale_denominator: u32,
        mut reasons: FrameReasonSet,
        force: bool,
    ) -> Result<Option<PreparedRender>, RuntimeError> {
        if scale_numerator == 0 || scale_denominator == 0 {
            return Err(RuntimeError::InvalidPackage(
                "renderer scale numerator and denominator must be nonzero".into(),
            ));
        }
        self.next_revision = self.next_revision.checked_add(1).ok_or_else(|| {
            RuntimeError::LimitExceeded("retained scene revision exhausted".into())
        })?;
        let candidate = build_retained_scene(
            document,
            identities,
            document_identity,
            SceneRevision(self.next_revision),
            viewport,
        )?;
        let previous = self.current_scene.as_deref();
        let delta = super::scene::diff_retained_scenes(previous, &candidate);
        let surface_changed = self.current_surface.is_some_and(|prior| prior != surface);
        let size = [
            viewport.logical_width,
            viewport.logical_height,
            physical_width,
            physical_height,
        ];
        let size_changed = self.current_size.is_some_and(|prior| prior != size);
        let scale = [scale_numerator, scale_denominator];
        let scale_changed = self.current_scale.is_some_and(|prior| prior != scale);
        if previous.is_none() {
            reasons.insert(FrameReason::InitialPresentation);
        }
        if surface_changed {
            reasons.insert(FrameReason::MappedTransition);
        }
        if size_changed {
            reasons.insert(FrameReason::SurfaceResize);
        }
        if scale_changed {
            reasons.insert(FrameReason::ScaleChange);
        }
        if !delta.resource_changes.is_empty() {
            reasons.insert(FrameReason::ResourceChange);
        }
        if delta
            .changes
            .iter()
            .any(|change| change.kinds.contains(&SceneChangeKind::Geometry))
        {
            reasons.insert(FrameReason::LayoutChange);
        }
        if !delta.is_empty() && reasons.is_empty() {
            reasons.insert(FrameReason::DocumentMutation);
        }
        if self.recovery_pending {
            reasons.insert(FrameReason::RendererRecovery);
        }
        if delta.is_empty()
            && !surface_changed
            && !size_changed
            && !scale_changed
            && !self.recovery_pending
            && !force
        {
            return Ok(None);
        }

        let damage = if previous.is_none()
            || surface_changed
            || size_changed
            || scale_changed
            || self.recovery_pending
        {
            DamageRegion::Full
        } else {
            DamageRegion::from_delta(previous, &candidate, &delta)
        };
        let scene = Arc::new(candidate);
        let plan = Arc::new(FramePlan {
            surface,
            document: document_identity,
            scene_revision: scene.revision,
            prior_scene_revision: previous.map(|scene| scene.revision),
            logical_width: viewport.logical_width,
            logical_height: viewport.logical_height,
            physical_width,
            physical_height,
            scale_numerator,
            scale_denominator,
            pixel_format: PixelFormat::PremultipliedRgba8,
            clear: true,
            scene: Arc::clone(&scene),
            delta,
            full_repaint: matches!(damage, DamageRegion::Full),
            damage,
            reasons,
            presentation_eligible: true,
        });
        let prepared = prepare_scene(document, plan.scene_revision, viewport)?;
        Ok(Some(PreparedRender {
            plan,
            prepared,
            scene,
            surface,
            size,
            scale,
        }))
    }

    pub(crate) fn render_prepared_cpu(
        &mut self,
        prepared: &PreparedRender,
    ) -> Result<CpuFrame, RuntimeError> {
        let plan = &prepared.plan;
        let target = RenderTarget {
            width: plan.physical_width,
            height: plan.physical_height,
            pixel_format: PixelFormat::PremultipliedRgba8,
        };
        let surface_changed = self
            .current_surface
            .is_some_and(|prior| prior != prepared.surface);
        let size_changed = self
            .current_size
            .is_some_and(|prior| prior != prepared.size);
        let scale_changed = self
            .current_scale
            .is_some_and(|prior| prior != prepared.scale);
        if surface_changed {
            if let Some(previous_surface) = self.current_surface {
                self.renderer.release_target(previous_surface);
            }
            self.renderer
                .create_target(prepared.surface, target)
                .map_err(runtime_backend_error)?;
        } else if !self.renderer.has_target(prepared.surface) {
            // Retained state may have advanced through a successful GPU
            // presentation before this presenter ever needed a CPU target.
            // Backend targets are intentionally independent from neutral
            // scene identity and must be created on the first CPU fallback.
            self.renderer
                .create_target(prepared.surface, target)
                .map_err(runtime_backend_error)?;
        } else if size_changed || scale_changed {
            self.renderer
                .resize_target(prepared.surface, target)
                .map_err(runtime_backend_error)?;
        }
        self.renderer
            .prepare(plan, prepared.prepared.clone())
            .map_err(runtime_backend_error)?;
        let result = self.renderer.render(plan, target).map_err(|error| {
            self.recovery_pending = error.recoverable;
            runtime_backend_error(error)
        })?;
        if result.scene_revision != plan.scene_revision
            || result.applied_damage != plan.damage
            || result.prepared_resources != prepared.scene.live_resources()
        {
            self.recovery_pending = true;
            return Err(RuntimeError::InvalidPackage(
                "renderer returned state that does not match the immutable frame plan".into(),
            ));
        }
        let physical_damage = logical_damage_to_physical(
            &plan.damage,
            plan.logical_width,
            plan.logical_height,
            plan.physical_width,
            plan.physical_height,
            plan.scale_numerator,
            plan.scale_denominator,
        );
        if !matches!(plan.damage, DamageRegion::Empty) && physical_damage.is_empty() {
            self.recovery_pending = true;
            return Err(RuntimeError::InvalidPackage(
                "nonempty logical damage converted to empty physical damage".into(),
            ));
        }
        let full_raster = result.full_raster;
        let pixels = self
            .renderer
            .readback(result)
            .map_err(runtime_backend_error)?;
        self.renderer
            .release_resources(&prepared.scene.live_resources())
            .map_err(runtime_backend_error)?;
        Ok(CpuFrame {
            plan: Arc::clone(plan),
            pixels,
            full_raster,
        })
    }

    pub(crate) fn accept_prepared(&mut self, prepared: &PreparedRender) {
        self.current_scene = Some(Arc::clone(&prepared.scene));
        self.current_surface = Some(prepared.surface);
        self.current_size = Some(prepared.size);
        self.current_scale = Some(prepared.scale);
        self.recovery_pending = false;
    }

    pub(crate) fn reject_prepared(&mut self, recoverable: bool) {
        self.recovery_pending |= recoverable;
    }

    pub(crate) fn reset_backend(&mut self) -> Result<(), RuntimeError> {
        self.renderer.reset().map_err(runtime_backend_error)?;
        self.current_surface = None;
        self.current_size = None;
        self.current_scale = None;
        self.recovery_pending = true;
        Ok(())
    }

    pub(crate) fn current_scene(&self) -> Option<&Arc<RetainedScene>> {
        self.current_scene.as_ref()
    }
}

impl Drop for CpuRenderSession {
    fn drop(&mut self) {
        self.renderer.shutdown();
    }
}

pub(super) fn prepare_scene(
    document: &mut HtmlDocument,
    revision: SceneRevision,
    viewport: ViewportSpec,
) -> Result<CpuPreparedScene, RuntimeError> {
    let mut recording = Scene::new();
    paint_scene(
        &mut recording,
        document,
        1.0,
        viewport.logical_width,
        viewport.logical_height,
        0,
        0,
    );
    Ok(CpuPreparedScene {
        revision,
        recording,
    })
}

fn runtime_backend_error(error: BackendError) -> RuntimeError {
    RuntimeError::InvalidPackage(format!("renderer backend failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityRegistry;
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::HtmlProvider;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use std::time::Instant;

    fn document() -> HtmlDocument {
        let mut document = HtmlDocument::from_html(
            "<!doctype html><html><head><style>body{margin:0}#box{width:40px;height:20px;background:#246}#copy{color:white}</style></head><body><div id=\"box\"><span id=\"copy\">hello</span></div></body></html>",
            DocumentConfig {
                viewport: Some(Viewport::new(100, 60, 1.0, ColorScheme::Dark)),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        document
    }

    fn render(
        session: &mut CpuRenderSession,
        document: &mut HtmlDocument,
        identities: &IdentityRegistry,
        force: bool,
    ) -> Result<Option<CpuFrame>, RuntimeError> {
        session.render_document(
            document,
            identities,
            ExperimentalDocumentIdentity { serial: 7 },
            ViewportSpec {
                logical_width: 100,
                logical_height: 60,
                ..ViewportSpec::default()
            },
            RenderSurfaceId {
                instance: 11,
                generation: 3,
            },
            100,
            60,
            120,
            120,
            FrameReasonSet::new(),
            force,
        )
    }

    fn render_html(html: &str, width: u32, height: u32) -> CpuFrame {
        let mut document = HtmlDocument::from_html(
            html,
            DocumentConfig {
                viewport: Some(Viewport::new(width, height, 1.0, ColorScheme::Dark)),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        let identities = IdentityRegistry::from_document(&document);
        CpuRenderSession::default()
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 19 },
                ViewportSpec {
                    logical_width: width,
                    logical_height: height,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 21,
                    generation: 1,
                },
                width,
                height,
                120,
                120,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap()
    }

    fn pixel(frame: &CpuFrame, width: usize, x: usize, y: usize) -> [u8; 4] {
        let offset = (y * width + x) * 4;
        frame.pixels[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn reference_pixels_match_the_prior_direct_cpu_path() {
        let mut document = document();
        let identities = IdentityRegistry::from_document(&document);
        let expected = render_to_buffer::<VelloCpuImageRenderer, _>(
            |target| paint_scene(target, &mut document, 1.0, 100, 60, 0, 0),
            100,
            60,
        );
        let actual = render(
            &mut CpuRenderSession::default(),
            &mut document,
            &identities,
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(actual.pixels, expected);
        assert!(actual.full_raster);
        assert_eq!(actual.plan.damage, DamageRegion::Full);
    }

    #[test]
    fn cpu_color_filter_renders_the_recorded_source_graphic() {
        let mut document = HtmlDocument::from_html(
            "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#box{width:20px;height:20px;background:rgb(64 128 192 / 50%);filter:brightness(2)}</style></head><body><div id=\"box\"></div></body></html>",
            DocumentConfig {
                viewport: Some(Viewport::new(40, 30, 1.0, ColorScheme::Dark)),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        let identities = IdentityRegistry::from_document(&document);
        let frame = CpuRenderSession::default()
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 9 },
                ViewportSpec {
                    logical_width: 40,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 12,
                    generation: 1,
                },
                40,
                30,
                120,
                120,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap();
        let offset = (10 * 40 + 10) * 4;
        assert_eq!(&frame.pixels[offset..offset + 4], &[64, 128, 128, 128]);
    }

    #[test]
    fn ordered_repeated_nested_and_external_opacity_filters_are_distinct() {
        let prefix = "<!doctype html><html><head><style>html,body{margin:0;background:transparent}";
        let suffix =
            "</style></head><body><div id=\"box\"><div id=\"child\"></div></div></body></html>";
        let forward = render_html(
            &format!(
                "{prefix}#box{{width:20px;height:20px;background:#4080c0;filter:brightness(2) contrast(.5)}}{suffix}"
            ),
            30,
            30,
        );
        let reverse = render_html(
            &format!(
                "{prefix}#box{{width:20px;height:20px;background:#4080c0;filter:contrast(.5) brightness(2)}}{suffix}"
            ),
            30,
            30,
        );
        assert_eq!(pixel(&forward, 30, 10, 10), [128, 191, 191, 255]);
        assert_eq!(pixel(&reverse, 30, 10, 10), [192, 255, 255, 255]);

        let repeated = render_html(
            &format!(
                "{prefix}#box{{width:20px;height:20px;background:#204060;filter:brightness(2) brightness(2)}}{suffix}"
            ),
            30,
            30,
        );
        assert_eq!(pixel(&repeated, 30, 10, 10), [128, 255, 255, 255]);

        let nested = render_html(
            &format!(
                "{prefix}#box{{width:20px;height:20px;filter:invert(1)}}#child{{width:20px;height:20px;background:#800000;filter:brightness(.5)}}{suffix}"
            ),
            30,
            30,
        );
        assert_eq!(pixel(&nested, 30, 10, 10), [191, 255, 255, 255]);

        let opacity = render_html(
            &format!(
                "{prefix}#box{{width:20px;height:20px;background:#ff0000;filter:opacity(.5);opacity:.5}}{suffix}"
            ),
            30,
            30,
        );
        assert_eq!(pixel(&opacity, 30, 10, 10), [64, 0, 0, 64]);
    }

    #[test]
    fn source_graphic_includes_box_shadow_and_excludes_parent_and_sibling_pixels() {
        let filtered = render_html(
            "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#parent{width:30px;height:20px;background:#102030}#box{width:10px;height:10px;background:#ff0000;box-shadow:8px 0 0 rgb(0 0 0 / 50%);filter:invert(1)}#sibling{position:absolute;left:24px;top:0;width:6px;height:6px;background:#204060}</style></head><body><div id=\"parent\"><div id=\"box\"></div><div id=\"sibling\"></div></div></body></html>",
            40,
            30,
        );
        assert_eq!(pixel(&filtered, 40, 5, 5), [0, 255, 255, 255]);
        assert_eq!(pixel(&filtered, 40, 15, 5), [136, 144, 152, 255]);
        assert_eq!(pixel(&filtered, 40, 26, 3), [32, 64, 96, 255]);
        assert_eq!(pixel(&filtered, 40, 20, 15), [16, 32, 48, 255]);
    }

    #[test]
    fn external_clipping_and_transform_position_follow_color_filtering() {
        let frame = render_html(
            "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#clip{position:absolute;left:2px;top:2px;width:10px;height:10px;overflow:hidden;border-radius:2px}#box{width:12px;height:12px;background:#204060;filter:invert(1);transform:translate(4px,3px)}</style></head><body><div id=\"clip\"><div id=\"box\"></div></div></body></html>",
            20,
            20,
        );
        assert_eq!(pixel(&frame, 20, 7, 7), [223, 191, 159, 255]);
        assert_eq!(pixel(&frame, 20, 3, 7), [0, 0, 0, 0]);
        assert_eq!(pixel(&frame, 20, 13, 7), [0, 0, 0, 0]);
        assert_eq!(pixel(&frame, 20, 7, 13), [0, 0, 0, 0]);
    }

    #[test]
    fn mixed_spatial_lists_are_not_partially_executed() {
        let baseline = render_html(
            "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#box{width:20px;height:20px;background:#204060}</style></head><body><div id=\"box\"></div></body></html>",
            30,
            30,
        );
        for filter in [
            "brightness(2) blur(4px)",
            "blur(4px) brightness(2)",
            "contrast(2) drop-shadow(1px 1px 2px black)",
            "drop-shadow(1px 1px 2px black) sepia(1)",
        ] {
            let filtered = render_html(
                &format!(
                    "<!doctype html><html><head><style>html,body{{margin:0;background:transparent}}#box{{width:20px;height:20px;background:#204060;filter:{filter}}}</style></head><body><div id=\"box\"></div></body></html>"
                ),
                30,
                30,
            );
            assert_eq!(filtered.pixels, baseline.pixels);
        }
    }

    #[test]
    fn identity_filters_skip_effect_images_and_scratch_is_reused_then_reset() {
        let html = "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#box{width:20px;height:20px;background:#204060;filter:brightness(1)}</style></head><body><div id=\"box\"></div></body></html>";
        let mut document = HtmlDocument::from_html(
            html,
            DocumentConfig {
                viewport: Some(Viewport::new(30, 30, 1.0, ColorScheme::Dark)),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        let identities = IdentityRegistry::from_document(&document);
        let mut session = CpuRenderSession::default();
        let identity_started = Instant::now();
        let frame = session
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 23 },
                ViewportSpec {
                    logical_width: 30,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 24,
                    generation: 1,
                },
                30,
                30,
                120,
                120,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap();
        let identity_us = identity_started.elapsed().as_micros();
        assert_eq!(pixel(&frame, 30, 10, 10), [32, 64, 96, 255]);
        assert_eq!(
            session.renderer.last_effect_statistics.identity_fast_paths,
            1
        );
        assert_eq!(session.renderer.last_effect_statistics.layer_creations, 0);

        let box_node = document.query_selector("#box").unwrap().unwrap();
        document.mutate().set_attribute(
            box_node,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "background:#402010;filter:invert(1)",
        );
        document.resolve(0.0);
        let allocation_started = Instant::now();
        let changed = session
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 23 },
                ViewportSpec {
                    logical_width: 30,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 24,
                    generation: 1,
                },
                30,
                30,
                120,
                120,
                FrameReasonSet::new(),
                false,
            )
            .unwrap()
            .unwrap();
        let allocation_us = allocation_started.elapsed().as_micros();
        assert_eq!(pixel(&changed, 30, 10, 10), [191, 223, 239, 255]);
        assert_eq!(session.renderer.last_effect_statistics.layer_creations, 1);
        assert_eq!(session.renderer.last_effect_statistics.layer_reuses, 0);

        let reuse_started = Instant::now();
        let repeated = session
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 23 },
                ViewportSpec {
                    logical_width: 30,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 24,
                    generation: 1,
                },
                30,
                30,
                120,
                120,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap();
        let reuse_us = reuse_started.elapsed().as_micros();
        assert_eq!(repeated.pixels, changed.pixels);
        assert_eq!(session.renderer.last_effect_statistics.layer_reuses, 1);
        eprintln!(
            "cpu_color_filter_measurement identity_us={identity_us} allocation_us={allocation_us} reuse_us={reuse_us}"
        );

        session.reset_backend().unwrap();
        let reset = session
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 23 },
                ViewportSpec {
                    logical_width: 30,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 24,
                    generation: 1,
                },
                30,
                30,
                120,
                120,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap();
        assert_eq!(reset.pixels, changed.pixels);
        assert_eq!(session.renderer.last_effect_statistics.layer_reuses, 0);
    }

    #[test]
    fn document_replacement_clears_a_reused_effect_target() {
        fn filtered_document(color: &str) -> HtmlDocument {
            let mut document = HtmlDocument::from_html(
                &format!(
                    "<!doctype html><html><head><style>html,body{{margin:0;background:transparent}}#box{{width:20px;height:20px;background:{color};filter:invert(1)}}</style></head><body><div id=\"box\"></div></body></html>"
                ),
                DocumentConfig {
                    viewport: Some(Viewport::new(30, 30, 1.0, ColorScheme::Dark)),
                    html_parser_provider: Some(Arc::new(HtmlProvider)),
                    style_threading: StyleThreading::Sequential,
                    ..Default::default()
                },
            );
            document.set_incremental_layout(true);
            document.resolve(0.0);
            document
        }

        let mut first_document = filtered_document("#ff0000");
        let first_identities = IdentityRegistry::from_document(&first_document);
        let mut session = CpuRenderSession::default();
        let first = session
            .render_document(
                &mut first_document,
                &first_identities,
                ExperimentalDocumentIdentity { serial: 31 },
                ViewportSpec {
                    logical_width: 30,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 26,
                    generation: 1,
                },
                30,
                30,
                120,
                120,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap();
        assert_eq!(pixel(&first, 30, 10, 10), [0, 255, 255, 255]);

        let mut second_document = filtered_document("#00ff00");
        let second_identities = IdentityRegistry::from_document(&second_document);
        let second = session
            .render_document(
                &mut second_document,
                &second_identities,
                ExperimentalDocumentIdentity { serial: 32 },
                ViewportSpec {
                    logical_width: 30,
                    logical_height: 30,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 26,
                    generation: 1,
                },
                30,
                30,
                120,
                120,
                FrameReasonSet::new(),
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(pixel(&second, 30, 10, 10), [255, 0, 255, 255]);
        assert!(second.plan.delta.full_scene_replacement);
        assert_eq!(session.renderer.last_effect_statistics.layer_reuses, 1);
    }

    #[test]
    fn color_filters_preserve_pixels_at_supported_fractional_scales() {
        let html = "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#box{width:20px;height:20px;background:#204060;filter:invert(1)}</style></head><body><div id=\"box\"></div></body></html>";
        for (numerator, physical) in [(120_u32, 30_u32), (150, 38), (180, 45)] {
            let mut document = HtmlDocument::from_html(
                html,
                DocumentConfig {
                    viewport: Some(Viewport::new(30, 30, 1.0, ColorScheme::Dark)),
                    html_parser_provider: Some(Arc::new(HtmlProvider)),
                    style_threading: StyleThreading::Sequential,
                    ..Default::default()
                },
            );
            document.set_incremental_layout(true);
            document.resolve(0.0);
            let identities = IdentityRegistry::from_document(&document);
            let frame = CpuRenderSession::default()
                .render_document(
                    &mut document,
                    &identities,
                    ExperimentalDocumentIdentity {
                        serial: u64::from(numerator),
                    },
                    ViewportSpec {
                        logical_width: 30,
                        logical_height: 30,
                        ..ViewportSpec::default()
                    },
                    RenderSurfaceId {
                        instance: 25,
                        generation: u64::from(numerator),
                    },
                    physical,
                    physical,
                    numerator,
                    120,
                    FrameReasonSet::new(),
                    true,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                frame.pixels.len(),
                physical as usize * physical as usize * 4
            );
            assert_eq!(
                pixel(&frame, physical as usize, 10, 10),
                [223, 191, 159, 255]
            );
            assert_eq!(frame.plan.damage, DamageRegion::Full);
        }
    }

    #[test]
    fn no_op_is_suppressed_and_text_update_retains_scene_identity() {
        let mut document = document();
        let identities = IdentityRegistry::from_document(&document);
        let mut session = CpuRenderSession::default();
        let initial = render(&mut session, &mut document, &identities, false)
            .unwrap()
            .unwrap();
        assert!(
            render(&mut session, &mut document, &identities, false)
                .unwrap()
                .is_none()
        );

        let copy = document.query_selector("#copy").unwrap().unwrap();
        let text = document.get_node(copy).unwrap().children[0];
        let old_id = initial
            .plan
            .scene
            .nodes
            .iter()
            .find(|node| node.id.dom.is_some_and(|identity| identity.slot == text))
            .unwrap()
            .id;
        document.mutate().set_node_text(text, "updated");
        document.resolve(0.0);
        let updated = render(&mut session, &mut document, &identities, false)
            .unwrap()
            .unwrap();
        let change = updated
            .plan
            .delta
            .changes
            .iter()
            .find(|change| change.id == old_id)
            .expect("retained text change");
        assert!(change.kinds.contains(&super::super::SceneChangeKind::Paint));
        assert_eq!(
            updated.plan.scene.node(old_id).map(|node| node.id),
            Some(old_id)
        );
        assert!(!updated.plan.delta.full_scene_replacement);
    }

    #[test]
    fn scale_surface_and_recovery_changes_force_bounded_full_plans() {
        let mut document = document();
        let identities = IdentityRegistry::from_document(&document);
        let mut session = CpuRenderSession::default();
        render(&mut session, &mut document, &identities, false)
            .unwrap()
            .unwrap();
        let scaled = session
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 7 },
                ViewportSpec {
                    logical_width: 100,
                    logical_height: 60,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 11,
                    generation: 4,
                },
                150,
                90,
                180,
                120,
                FrameReasonSet::new(),
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(scaled.plan.damage, DamageRegion::Full);
        assert!(scaled.plan.reasons.contains(&FrameReason::ScaleChange));
        assert!(scaled.plan.reasons.contains(&FrameReason::MappedTransition));

        session.reset_backend().unwrap();
        let recovered = session
            .render_document(
                &mut document,
                &identities,
                ExperimentalDocumentIdentity { serial: 7 },
                ViewportSpec {
                    logical_width: 100,
                    logical_height: 60,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 11,
                    generation: 4,
                },
                150,
                90,
                180,
                120,
                FrameReasonSet::new(),
                false,
            )
            .unwrap()
            .unwrap();
        assert!(
            recovered
                .plan
                .reasons
                .contains(&FrameReason::RendererRecovery)
        );
        assert_eq!(recovered.plan.damage, DamageRegion::Full);
    }

    #[test]
    fn frame_plan_diagnostics_are_deterministic_and_native_object_free() {
        let mut left_document = document();
        let mut right_document = document();
        let left_identities = IdentityRegistry::from_document(&left_document);
        let right_identities = IdentityRegistry::from_document(&right_document);
        let left = render(
            &mut CpuRenderSession::default(),
            &mut left_document,
            &left_identities,
            false,
        )
        .unwrap()
        .unwrap();
        let right = render(
            &mut CpuRenderSession::default(),
            &mut right_document,
            &right_identities,
            false,
        )
        .unwrap()
        .unwrap();
        let left_json = left.plan.deterministic_json().unwrap();
        assert_eq!(left_json, right.plan.deterministic_json().unwrap());
        let text = std::str::from_utf8(&left_json).unwrap();
        for forbidden in ["wl_surface", "PipeWire", "D-Bus", "0x"] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn retained_renderer_measurement_probe_is_bounded() {
        let mut document = document();
        let identities = IdentityRegistry::from_document(&document);
        let mut session = CpuRenderSession::default();

        let started = Instant::now();
        let initial = render(&mut session, &mut document, &identities, false)
            .unwrap()
            .unwrap();
        let initial_us = started.elapsed().as_micros();

        let started = Instant::now();
        for _ in 0..1_000 {
            assert!(
                render(&mut session, &mut document, &identities, false)
                    .unwrap()
                    .is_none()
            );
        }
        let noop_1000_us = started.elapsed().as_micros();

        let copy = document.query_selector("#copy").unwrap().unwrap();
        let text = document.get_node(copy).unwrap().children[0];
        document.mutate().set_node_text(text, "measurement");
        document.resolve(0.0);
        let started = Instant::now();
        let update = render(&mut session, &mut document, &identities, false)
            .unwrap()
            .unwrap();
        let text_update_us = started.elapsed().as_micros();

        let started = Instant::now();
        let physical = logical_damage_to_physical(&update.plan.damage, 100, 60, 150, 90, 180, 120);
        let damage_us = started.elapsed().as_micros();

        assert!(!initial.plan.scene.nodes.is_empty());
        assert!(!update.plan.delta.changes.is_empty());
        assert!(!physical.is_empty());
        eprintln!(
            "renderer_r1_measurement initial_us={initial_us} noop_1000_us={noop_1000_us} text_update_us={text_update_us} damage_us={damage_us} nodes={} resources={} delta_changes={}",
            initial.plan.scene.nodes.len(),
            initial.plan.scene.resources.len(),
            update.plan.delta.changes.len(),
        );
    }
}
