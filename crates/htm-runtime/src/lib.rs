//! Experimental HTMShell adapter for the Gate A headless feasibility spike.
//!
//! Blitz types stay inside private modules. The exported values describe only this
//! experiment and are not a proposed HTMShell runtime API.

mod adapter;
mod error;
mod model;
mod resource;

pub use adapter::{run_package, run_package_with_options};
pub use error::RuntimeError;
pub use model::{
    Artifact, DiagnosticNode, DiagnosticReport, ExperimentOptions, ExperimentRun,
    InteractionEvidence, Phase, ResourceRecord, RunMeasurements, ViewportSpec,
};

pub const BLITZ_REVISION: &str = "389e3762fc0ac19f6de7c0cec7201d0c8bde393a";
pub const DIAGNOSTIC_SCHEMA_VERSION: &str = "htmshell.experimental-diagnostic.v1";
