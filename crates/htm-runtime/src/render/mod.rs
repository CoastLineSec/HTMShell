mod contract;
mod cpu;
mod damage;
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
