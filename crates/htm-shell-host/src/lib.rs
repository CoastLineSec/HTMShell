//! Experimental compositor-neutral layer-shell presentation for HTMShell.
//!
//! This crate is intentionally narrow. It presents one parse-once HTMShell
//! document through standard Wayland objects and wlr layer shell.

mod buffer;
mod clock;
mod error;
mod lifecycle;
mod manifest;
mod output;
mod pixel;
mod power;
mod scale;
mod scheduler;
mod wayland;

#[cfg(test)]
mod session_model;

pub use clock::ClockServiceSummary;
pub use error::ShellHostError;
pub use manifest::{
    ManifestMeasurements, OutputScope, OverlayTemplate, PanelEdge, PanelTemplate, ShellManifest,
    SurfaceKind, SurfacePreset, SurfaceTemplate, ValidatedManifest,
};
pub use output::{OutputCatalog, OutputEligibility, OutputKey, OutputRecord};
pub use pixel::{Argb8888Layout, convert_premultiplied_rgba_to_argb8888};
pub use power::{
    BatteryAvailability, BatteryChargeState, BatteryServiceSummary, BatterySnapshot,
    BatteryWarning, PerformanceDegradationReason, PowerProfile, PowerProfileHold,
    PowerProfilesSnapshot, PowerServiceSummary, PowerSnapshot, UPowerDeviceSnapshot,
    UPowerDeviceState, UPowerDeviceType,
};
pub use scale::{PresentationProfile, SurfaceScaleState};
pub use scheduler::{FrameScheduler, ScheduleDecision};
pub use wayland::{
    LiveHostOptions, LiveHostSummary, ManifestHostOptions, ManifestHostSummary,
    ManifestOutputHostSummary, ManifestSurfaceHostSummary, MultiSurfaceHostOptions,
    MultiSurfaceHostSummary, SurfaceHostSummary, run_live_overlay, run_manifest_shell,
    run_multi_surface_shell,
};
