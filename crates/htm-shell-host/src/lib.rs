//! Experimental compositor-neutral layer-shell presentation for HTMShell.
//!
//! This crate is intentionally narrow. It presents one parse-once HTMShell
//! document through standard Wayland objects and wlr layer shell.

mod buffer;
mod error;
mod lifecycle;
mod pixel;
mod scheduler;
mod wayland;

pub use error::ShellHostError;
pub use pixel::{Argb8888Layout, convert_premultiplied_rgba_to_argb8888};
pub use scheduler::{FrameScheduler, ScheduleDecision};
pub use wayland::{
    LiveHostOptions, LiveHostSummary, MultiSurfaceHostOptions, MultiSurfaceHostSummary,
    SurfaceHostSummary, run_live_overlay, run_multi_surface_shell,
};
