//! Native HTML and CSS runtime for HTMShell.

mod adapter;
mod builtin;
mod clock;
mod collection;
mod component;
mod component_resource;
mod component_style;
mod component_svg;
mod error;
mod identity;
mod incremental;
mod live;
mod model;
mod mutation;
mod package;
// R1 intentionally defines recovery and backend-neutral states that the CPU
// reference backend does not exercise in every production path.
#[allow(dead_code, unused_imports)]
mod render;
mod resource;
mod scene;
mod style_owner;
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
pub use component::{
    ComponentCatalog, ComponentDefinition, ComponentDefinitionId, ComponentDefinitionKey,
    ComponentDescendantProvenance, ComponentExport, ComponentFallbackNodeProvenance,
    ComponentInputConsumerKind, ComponentInputConsumerRecord, ComponentInputDeclaration,
    ComponentInputName, ComponentInputProvenance, ComponentInputType, ComponentInputValue,
    ComponentInputVersion, ComponentInstanceId, ComponentInstanceRecord, ComponentName,
    ComponentReference, ComponentSlotDeclaration, ComponentSlotDefinition,
    ComponentSlotDefinitionId, ComponentSlotName, ComponentSlotProjectionId,
    ComponentSlotProjectionOutcome, ComponentSlotProjectionRecord, ComponentSlotProjectionVersion,
    ComponentValidationTotals, MAX_COMPONENT_EXPANDED_NODES, MAX_COMPONENT_EXPORTS_PER_GRAPH,
    MAX_COMPONENT_EXPORTS_PER_PACKAGE, MAX_COMPONENT_INPUT_ATTRIBUTES,
    MAX_COMPONENT_INPUT_LITERAL_BYTES, MAX_COMPONENT_INPUT_NAME_BYTES,
    MAX_COMPONENT_INPUT_STRING_BYTES, MAX_COMPONENT_INPUTS, MAX_COMPONENT_INSTANCES_PER_DOCUMENT,
    MAX_COMPONENT_NAME_BYTES, MAX_COMPONENT_NESTING_DEPTH, MAX_COMPONENT_REFERENCES_PER_DOCUMENT,
    MAX_COMPONENT_SLOT_NAME_BYTES, MAX_COMPONENT_SLOTS, MAX_COMPONENT_SOURCE_BYTES,
    MAX_COMPONENT_SOURCE_NODES, PreparedDocument, PreparedDocumentStats, ProjectedNodeProvenance,
    ResolvedComponentInput, ResolvedComponentInputs, SlotProjectionSource,
};
pub use component_resource::{
    ComponentRasterFormat, ComponentRasterSource, ComponentResourceAssociation,
    ComponentResourceCatalog, ComponentResourceDeclaration, ComponentResourceKind,
    ComponentResourceName, ComponentResourcePath, ComponentResourceSemanticVersion,
    ComponentResourceSource, ComponentResourceSourceId, ComponentResourceUsage,
    ComponentResourceUsageId, ComponentResourceValidationTotals,
    MAX_COMPONENT_RASTER_DECODED_BYTES, MAX_COMPONENT_RASTER_HEIGHT, MAX_COMPONENT_RASTER_PIXELS,
    MAX_COMPONENT_RASTER_SOURCE_BYTES, MAX_COMPONENT_RASTER_WIDTH,
    MAX_COMPONENT_RESOURCE_ASSOCIATIONS_PER_PACKAGE, MAX_COMPONENT_RESOURCE_DECLARATIONS,
    MAX_COMPONENT_RESOURCE_NAME_BYTES, MAX_COMPONENT_RESOURCE_PATH_BYTES,
    MAX_COMPONENT_RESOURCE_PATH_COMPONENTS, MAX_COMPONENT_RESOURCE_SNAPSHOT_DECODED_BYTES,
    MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE, MAX_COMPONENT_SVG_DEPTH, MAX_COMPONENT_SVG_HEIGHT,
    MAX_COMPONENT_SVG_NODES, MAX_COMPONENT_SVG_PATH_SEGMENTS, MAX_COMPONENT_SVG_PIXELS,
    MAX_COMPONENT_SVG_SOURCE_BYTES, MAX_COMPONENT_SVG_WIDTH,
};
pub use component_style::{
    ComponentStyleCatalog, ComponentStyleValidationTotals, ComponentStylesheetAssociation,
    ComponentStylesheetPath, ComponentStylesheetSemanticVersion, ComponentStylesheetSource,
    MAX_COMPONENT_STYLESHEET_BYTES, MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE,
    MAX_COMPONENT_STYLESHEET_PATH_BYTES, MAX_COMPONENT_STYLESHEETS,
};
pub use component_svg::{
    ComponentSvgResolverStatistics, ComponentSvgSource, ComponentSvgStatistics, ComponentSvgViewBox,
};
pub use error::RuntimeError;
pub use incremental::{
    DamageEstimate, ExperimentalDocumentIdentity, ExperimentalNodeIdentity, ExperimentalSceneDiff,
    ExperimentalSceneSnapshot, IncrementalExperimentRun, InvalidationEvidence, MutationArtifact,
    MutationPhase, MutationPhaseMeasurement, ScaleBaseline, SceneDiffSummary, SceneNodeChange,
    SceneNodeSnapshot, SlotReuseEvidence, StylesheetReloadAttempt,
};
#[cfg(feature = "gpu-renderer")]
pub use live::LiveGpuPreparedFrame;
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
pub use package::{
    MAX_CANDIDATE_BYTES, MAX_DEPENDENCY_DEPTH, MAX_DIRECT_DEPENDENCIES, MAX_PACKAGE_ALIAS_BYTES,
    MAX_PACKAGE_ID_BYTES, MAX_PACKAGE_MANIFEST_BYTES, MAX_PACKAGE_PATH_BYTES,
    MAX_PACKAGE_VERSION_BYTES, MAX_PACKAGES_PER_GRAPH, ManifestMeasurements, OutputScope,
    OverlayTemplate, PackageAlias, PackageDependency, PackageEntryDocument, PackageErrorKind,
    PackageId, PackageKind, PackageLoadError, PackageNodeIdentity, PackageSchemaSource,
    PackageSnapshot, PackageSnapshotCandidate, PackageSnapshotGeneration, PackageSnapshotLoader,
    PackageVersion, PanelEdge, PanelTemplate, ResolvedPackage, ResolvedPackageDependency,
    ShellManifest, SurfaceKind, SurfacePreset, SurfaceTemplate, ValidatedManifest,
};
#[cfg(feature = "gpu-renderer")]
pub use render::{
    LiveGpuBackendInfo, LiveGpuConfiguration, LiveGpuError, LiveGpuErrorKind, LiveGpuPresenter,
    LiveGpuStatistics, LiveWaylandHandle, PendingLiveGpuFrame, RenderSurfaceId,
};

pub const BLITZ_REVISION: &str = "74b51b07ac0562b8de7a52bc6c1ba4511706af93";
pub const DIAGNOSTIC_SCHEMA_VERSION: &str = "htmshell.experimental-diagnostic.v1";
pub const INCREMENTAL_SNAPSHOT_SCHEMA_VERSION: &str = "htmshell.experimental-scene-snapshot.v1";
pub const INCREMENTAL_DIFF_SCHEMA_VERSION: &str = "htmshell.experimental-scene-diff.v1";
