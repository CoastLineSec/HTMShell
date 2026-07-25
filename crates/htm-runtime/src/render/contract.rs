use super::{
    DamageRegion, FramePlan, RenderSurfaceId, SceneResourceId, SceneResourceVersion, SceneRevision,
};
use std::fmt;

pub const MAX_BACKEND_ERROR_MESSAGE_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PixelFormat {
    PremultipliedRgba8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTarget {
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendErrorKind {
    InvalidPlan,
    TargetAllocation,
    ResourcePreparation,
    Render,
    BackendReset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    pub kind: BackendErrorKind,
    pub message: String,
    pub recoverable: bool,
}

impl BackendError {
    pub(crate) fn new(
        kind: BackendErrorKind,
        message: impl Into<String>,
        recoverable: bool,
    ) -> Self {
        let mut message = message.into();
        if message.len() > MAX_BACKEND_ERROR_MESSAGE_BYTES {
            let mut boundary = MAX_BACKEND_ERROR_MESSAGE_BYTES;
            while !message.is_char_boundary(boundary) {
                boundary -= 1;
            }
            message.truncate(boundary);
        }
        Self {
            kind,
            message,
            recoverable,
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for BackendError {}

#[derive(Debug, Clone)]
pub struct RenderResult {
    pub scene_revision: SceneRevision,
    pub pixels: Vec<u8>,
    pub applied_damage: DamageRegion,
    pub full_raster: bool,
    pub prepared_resources: Vec<(SceneResourceId, SceneResourceVersion)>,
}

/// Internal renderer boundary.
///
/// Scheduling and presentation remain outside this contract. Implementations
/// receive immutable frame plans and backend-private prepared state.
pub trait Renderer {
    type Prepared;

    fn create_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError>;

    fn resize_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError>;

    fn prepare(&mut self, plan: &FramePlan, prepared: Self::Prepared) -> Result<(), BackendError>;

    fn render(
        &mut self,
        plan: &FramePlan,
        target: RenderTarget,
    ) -> Result<RenderResult, BackendError>;

    fn readback(&mut self, result: RenderResult) -> Result<Vec<u8>, BackendError>;

    fn release_resources(
        &mut self,
        live: &[(SceneResourceId, SceneResourceVersion)],
    ) -> Result<(), BackendError>;

    fn reset(&mut self) -> Result<(), BackendError>;

    fn release_target(&mut self, surface: RenderSurfaceId);

    fn shutdown(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_errors_are_typed_and_message_bounded() {
        let error = BackendError::new(BackendErrorKind::Render, "é".repeat(2_000), true);
        assert_eq!(error.kind, BackendErrorKind::Render);
        assert!(error.recoverable);
        assert!(error.message.len() <= MAX_BACKEND_ERROR_MESSAGE_BYTES);
        assert!(error.message.is_char_boundary(error.message.len()));
    }
}
