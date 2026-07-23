//! Experimental HTMShell adapter for the Gate A headless feasibility spike.
//!
//! Blitz types stay inside private modules. The exported values describe only this
//! experiment and are not a proposed HTMShell runtime API.

mod adapter;
mod builtin;
mod error;
mod identity;
mod incremental;
mod live;
mod model;
mod mutation;
mod resource;
mod scene;
mod stylesheet;

pub use adapter::{run_package, run_package_with_options};
pub use builtin::{
    BindingUpdate, BuiltInElementKind, BuiltInElementSummary, ElementDeclaration,
    ElementInstanceId, ShellAction, StateBindingKey, StateBindingScope, built_in_registry_names,
};
pub use error::RuntimeError;
pub use incremental::{
    DamageEstimate, ExperimentalDocumentIdentity, ExperimentalNodeIdentity, ExperimentalSceneDiff,
    ExperimentalSceneSnapshot, IncrementalExperimentRun, InvalidationEvidence, MutationArtifact,
    MutationPhase, MutationPhaseMeasurement, ScaleBaseline, SceneDiffSummary, SceneNodeChange,
    SceneNodeSnapshot, SlotReuseEvidence, StylesheetReloadAttempt,
};
pub use live::{
    LIVE_SCALE_DENOMINATOR, LiveAction, LiveDocument, LiveDocumentKind, LiveFrame, LiveFrameRect,
    LiveInteractionState, LiveRenderRequest, LiveRuntimeMeasurements, LiveRuntimeSnapshot,
    MAX_LIVE_SCALE_NUMERATOR,
};
pub use model::{
    Artifact, DiagnosticNode, DiagnosticReport, ExperimentOptions, ExperimentRun,
    InteractionEvidence, Phase, ResourceRecord, RunMeasurements, ViewportSpec,
};
pub use mutation::run_incremental_experiment;

pub const BLITZ_REVISION: &str = "389e3762fc0ac19f6de7c0cec7201d0c8bde393a";
pub const DIAGNOSTIC_SCHEMA_VERSION: &str = "htmshell.experimental-diagnostic.v1";
pub const INCREMENTAL_SNAPSHOT_SCHEMA_VERSION: &str = "htmshell.experimental-scene-snapshot.v1";
pub const INCREMENTAL_DIFF_SCHEMA_VERSION: &str = "htmshell.experimental-scene-diff.v1";
