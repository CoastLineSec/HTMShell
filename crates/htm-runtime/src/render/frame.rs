use super::{DamageRegion, PixelFormat, RetainedScene, SceneDelta, SceneRevision};
use crate::ExperimentalDocumentIdentity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const FRAME_PLAN_SCHEMA_VERSION: &str = "htmshell.internal-frame-plan.v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderSurfaceId {
    pub instance: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FrameReason {
    InitialPresentation,
    DocumentMutation,
    LayoutChange,
    ResourceChange,
    SurfaceResize,
    ScaleChange,
    MappedTransition,
    RendererRecovery,
    ExplicitInvalidation,
}

pub type FrameReasonSet = BTreeSet<FrameReason>;

/// Immutable logical work description for one surface revision.
///
/// Presentation objects and backend-private resources deliberately do not
/// cross this boundary.
#[derive(Debug, Clone)]
pub struct FramePlan {
    pub surface: RenderSurfaceId,
    pub document: ExperimentalDocumentIdentity,
    pub scene_revision: SceneRevision,
    pub prior_scene_revision: Option<SceneRevision>,
    pub logical_width: u32,
    pub logical_height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_numerator: u32,
    pub scale_denominator: u32,
    pub pixel_format: PixelFormat,
    pub clear: bool,
    pub scene: Arc<RetainedScene>,
    pub delta: SceneDelta,
    pub damage: DamageRegion,
    pub reasons: FrameReasonSet,
    pub full_repaint: bool,
    pub presentation_eligible: bool,
}

impl FramePlan {
    pub fn deterministic_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        #[derive(Serialize)]
        struct Diagnostic<'a> {
            schema_version: &'static str,
            surface: RenderSurfaceId,
            document: ExperimentalDocumentIdentity,
            scene_revision: SceneRevision,
            prior_scene_revision: Option<SceneRevision>,
            logical_size: [u32; 2],
            physical_size: [u32; 2],
            scale: [u32; 2],
            pixel_format: &'static str,
            clear: bool,
            scene: &'a RetainedScene,
            delta: &'a SceneDelta,
            damage: &'a DamageRegion,
            reasons: &'a FrameReasonSet,
            full_repaint: bool,
            presentation_eligible: bool,
        }

        serde_json::to_vec_pretty(&Diagnostic {
            schema_version: FRAME_PLAN_SCHEMA_VERSION,
            surface: self.surface,
            document: self.document,
            scene_revision: self.scene_revision,
            prior_scene_revision: self.prior_scene_revision,
            logical_size: [self.logical_width, self.logical_height],
            physical_size: [self.physical_width, self.physical_height],
            scale: [self.scale_numerator, self.scale_denominator],
            pixel_format: match self.pixel_format {
                PixelFormat::PremultipliedRgba8 => "premultiplied_rgba8",
            },
            clear: self.clear,
            scene: &self.scene,
            delta: &self.delta,
            damage: &self.damage,
            reasons: &self.reasons,
            full_repaint: self.full_repaint,
            presentation_eligible: self.presentation_eligible,
        })
    }
}
