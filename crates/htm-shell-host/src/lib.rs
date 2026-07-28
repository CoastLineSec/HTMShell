//! Layer-shell presentation for HTMShell.
//!
//! This crate is intentionally narrow. It presents one parse-once HTMShell
//! document through standard Wayland objects and wlr layer shell.

mod buffer;
mod clock;
mod error;
mod lifecycle;
mod manifest;
mod output;
mod pipewire;
mod pixel;
mod power;
#[cfg(feature = "gpu-renderer")]
mod presenter;
mod scale;
mod scheduler;
mod wayland;

#[cfg(test)]
mod session_model;

pub use clock::ClockServiceSummary;
pub use error::ShellHostError;
pub use htm_runtime::{
    MAX_CANDIDATE_BYTES, MAX_DEPENDENCY_DEPTH, MAX_DIRECT_DEPENDENCIES, MAX_PACKAGE_ALIAS_BYTES,
    MAX_PACKAGE_ID_BYTES, MAX_PACKAGE_MANIFEST_BYTES, MAX_PACKAGE_PATH_BYTES,
    MAX_PACKAGES_PER_GRAPH, PackageAlias, PackageErrorKind, PackageId, PackageKind,
    PackageLoadError, PackageSchemaSource, PackageSnapshot, PackageSnapshotGeneration,
    PackageSnapshotLoader, PackageVersion,
};
pub use manifest::{
    ManifestMeasurements, OutputScope, OverlayTemplate, PanelEdge, PanelTemplate, ShellManifest,
    SurfaceKind, SurfacePreset, SurfaceTemplate, ValidatedManifest,
};
pub use output::{OutputCatalog, OutputEligibility, OutputKey, OutputRecord};
pub use pipewire::{
    PipeWireAudioChannelPosition, PipeWireNodeDirection, PipeWireNodeType,
    run_pipewire_graph_diagnostic_json,
};
pub use pixel::{Argb8888Layout, convert_premultiplied_rgba_to_argb8888};
pub use power::{
    BatteryAvailability, BatteryChargeState, BatteryServiceSummary, BatterySnapshot,
    BatteryWarning, PerformanceDegradationReason, PowerProfile, PowerProfileHold,
    PowerProfilesSnapshot, PowerServiceSummary, PowerSnapshot, UPowerDeviceSnapshot,
    UPowerDeviceState, UPowerDeviceType,
};
pub use scale::{PresentationProfile, SurfaceScaleState};
pub use scheduler::{FrameScheduler, ScheduleDecision};
#[cfg(feature = "gpu-renderer")]
pub use wayland::GpuSurfaceHostSummary;
pub use wayland::{
    LiveHostOptions, LiveHostSummary, ManifestHostOptions, ManifestHostSummary,
    ManifestOutputHostSummary, ManifestSurfaceHostSummary, MultiSurfaceHostOptions,
    MultiSurfaceHostSummary, SurfaceHostSummary, run_live_overlay, run_manifest_shell,
    run_multi_surface_shell,
};
