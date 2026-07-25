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
        let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
            |target| target.append_scene(prepared.recording.clone(), Affine::scale(scale)),
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
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), BackendError> {
        self.prepared.clear();
        self.targets.clear();
        Ok(())
    }

    fn release_target(&mut self, surface: RenderSurfaceId) {
        self.targets.remove(&surface);
    }

    fn shutdown(&mut self) {
        self.prepared.clear();
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
