//! Native HTML and CSS runtime for HTMShell.

mod adapter;
mod builtin;
mod clock;
mod collection;
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
    BindingUpdate, BuiltInElementKind, BuiltInElementSummary, ClockDeclaration,
    ContextualRepeatDeclaration, DEVICE_STATE_TOKENS, DEVICE_TYPE_TOKENS, ElementDeclaration,
    ElementInstanceId, PeakBindingKey, PeakChannelRepeatDeclaration, PeakMonitorDeclaration,
    PeakMonitorTarget, PeakScopedElementDeclaration, RepeatDeclaration, RepeatedElementDeclaration,
    ShellAction, StateBindingKey, StateBindingScope, StateToken, StateValueKind,
    built_in_registry_names,
};
pub use clock::{
    CLOCK_FORMAT_CONVERSIONS, CLOCK_FORMAT_FLAGS, CLOCK_PUBLIC_ATTRIBUTES,
    CLOCK_REJECTED_CONVERSIONS, ClockCadence, ClockDirective, ClockFormat, ClockFormatError,
    ClockFormatFlag, ClockFormatPart, ClockTimeZone, MAX_CLOCK_DECLARATIONS_PER_DOCUMENT,
    MAX_CLOCK_DECLARATIONS_PER_PROCESS, MAX_CLOCK_FORMAT_BYTES, MAX_CLOCK_FORMATS_PER_PROCESS,
    MAX_CLOCK_OUTPUT_BYTES, MAX_CLOCK_ZONE_BYTES, MAX_CLOCK_ZONES_PER_PROCESS,
};
pub use collection::{
    ContextualRepeatSnapshot, ContextualRepeatSource, FormattedValue, ItemBindingKey,
    MAX_CLONED_NODES_PER_DOCUMENT, MAX_CLONED_NODES_PER_REPEAT,
    MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT, MAX_CONTEXTUAL_LINK_GROUP_REPEATS_PER_NODE_TEMPLATE,
    MAX_CONTEXTUAL_LINK_REPEATS_PER_GROUP_TEMPLATE, MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT,
    MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE, MAX_ITEMS_PER_REPEAT,
    MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS, MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT,
    MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM, MAX_PIPEWIRE_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_CHANNEL_BINDINGS_PER_ITEM, MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT,
    MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM, MAX_PIPEWIRE_CHANNELS_PER_NODE,
    MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT, MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM,
    MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT, MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_PIPEWIRE_LINK_GROUPS_PER_PROCESS,
    MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_PIPEWIRE_LINKS_PER_PROCESS,
    MAX_PIPEWIRE_NODES_PER_PROCESS, MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR,
    MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR, MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR, MAX_PIPEWIRE_PEAK_CHANNELS_PER_STREAM,
    MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET, MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT,
    MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM, MAX_PIPEWIRE_PERCEPTUAL_VOLUME,
    MAX_PIPEWIRE_PROPERTY_KEY_BYTES, MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT,
    MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS, MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM,
    MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM, MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT,
    MAX_POWER_PROFILE_HOLDS_PER_PROCESS, MAX_RANGE_CONTROLS_PER_DOCUMENT,
    MAX_RANGE_CONTROLS_PER_ITEM, MAX_RANGE_NUMBER_BYTES, MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE,
    MAX_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_REPEAT_TEMPLATE_DEPTH,
    MAX_UPOWER_DEVICES_PER_PROCESS, NumericValue, PipeWireDocumentDemand, RepeatItemSnapshot,
    RepeatSource, RepeatSourceSnapshot, StateValueFormat, ValueFormatError,
};
pub use error::RuntimeError;
pub use incremental::{
    DamageEstimate, ExperimentalDocumentIdentity, ExperimentalNodeIdentity, ExperimentalSceneDiff,
    ExperimentalSceneSnapshot, IncrementalExperimentRun, InvalidationEvidence, MutationArtifact,
    MutationPhase, MutationPhaseMeasurement, ScaleBaseline, SceneDiffSummary, SceneNodeChange,
    SceneNodeSnapshot, SlotReuseEvidence, StylesheetReloadAttempt,
};
pub use live::{
    ClockMutation, LIVE_SCALE_DENOMINATOR, LiveAction, LiveDocument, LiveDocumentKind, LiveFrame,
    LiveFrameRect, LiveInteractionState, LiveRenderRequest, LiveRuntimeMeasurements,
    LiveRuntimeSnapshot, MAX_LIVE_SCALE_NUMERATOR, PipeWireAudioOperation, PipeWireAudioTarget,
    PipeWireControlIdentity, PipeWireControlLocator, PipeWireControlRequest, PipeWireControlState,
    PipeWireDefaultControlRequest, PipeWireDefaultRole, PipeWireDefaultTarget,
    PipeWireDesiredVolume, PipeWirePeakActionRequest, PipeWirePeakDeclarationDemand,
    PipeWirePeakMonitorIdentity, PipeWirePeakMonitorLocator, PipeWirePeakOperation,
    PipeWirePeakProjection, PipeWirePeakProjectionSet, PipeWirePeakStreamState, PipeWirePeakTarget,
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
