mod contract;
mod cpu;
mod damage;
mod effects;
mod frame;
#[cfg(feature = "gpu-renderer")]
#[allow(dead_code)]
mod gpu;
mod scene;

pub(crate) use cpu::{CpuRenderSession, PreparedRender};
#[cfg(feature = "gpu-renderer")]
pub use gpu::{
    LiveGpuBackendInfo, LiveGpuConfiguration, LiveGpuError, LiveGpuErrorKind, LiveGpuPresenter,
    LiveGpuStatistics, LiveWaylandHandle, PendingLiveGpuFrame,
};
pub(crate) use scene::build_retained_scene;

pub use contract::{
    BackendError, BackendErrorKind, PixelFormat, RenderResult, RenderTarget, Renderer,
};
pub use damage::{DamageRegion, MAX_DAMAGE_RECTS, PhysicalDamageRect, logical_damage_to_physical};
pub use effects::{
    BlurEffect, CanonicalF32, ColorEffect, ColorEffectKind, ColorMatrix, ColorMatrixRun,
    DropShadowEffect, EffectColor, FOREGROUND_EFFECT_COMPOSITION_ORDER, ForegroundEffect,
    ForegroundEffectAlphaModel, ForegroundEffectBackendCoverage, ForegroundEffectColorSpace,
    ForegroundEffectCompositionStage, ForegroundEffectCoverage, ForegroundEffectId,
    ForegroundEffectLayerMetadata, ForegroundEffectList, ForegroundEffectRejection,
    ForegroundEffectRole, ForegroundEffectVersion, MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE,
    MAX_EFFECT_IMAGE_BYTES, MAX_EFFECT_LAYER_DIMENSION, MAX_EFFECT_PIPELINE_VARIANTS,
    MAX_EFFECT_SURFACE_BYTES, MAX_FILTER_DECLARATIONS_PER_DOCUMENT, MAX_FILTER_NESTING_DEPTH,
    MAX_FOREGROUND_BLUR_SIGMA, MAX_FOREGROUND_EFFECT_EXPANSION, MAX_FOREGROUND_EFFECT_FACTOR,
    MAX_FOREGROUND_EFFECT_FUNCTIONS, MAX_FOREGROUND_EFFECT_SERIALIZED_BYTES,
    MAX_FOREGROUND_SHADOW_OFFSET, MAX_HUE_ROTATION_TURNS,
};
pub use frame::{
    FRAME_PLAN_SCHEMA_VERSION, FramePlan, FrameReason, FrameReasonSet, RenderSurfaceId,
};
pub use scene::{
    MAX_RETAINED_RESOURCES, MAX_SCENE_CHILDREN, MAX_SCENE_DELTA_ENTRIES, MAX_SCENE_DEPTH,
    MAX_SCENE_NODES, ResourceChange, ResourceKind, ResourceLifecycle, ResourceOwner, RetainedScene,
    SceneBounds, SceneChangeKind, SceneDelta, SceneEffect, SceneNode, SceneNodeChange, SceneNodeId,
    SceneNodeKind, SceneResource, SceneResourceId, SceneResourceKey, SceneResourceVersion,
    SceneRevision, SceneSubpart,
};

pub(crate) fn stable_hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

pub(crate) fn stable_hash_parts(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for part in parts {
        for byte in part.as_bytes().iter().chain(std::iter::once(&0xff)) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}
