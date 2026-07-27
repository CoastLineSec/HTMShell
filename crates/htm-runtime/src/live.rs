use crate::adapter::{elapsed_ms, resolve_resources, validate_document_limits};
use crate::builtin::{
    BindingUpdate, BuiltInElementIndex, BuiltInElementKind, BuiltInElementSummary,
    BuiltInSurfaceKind, ClockDeclaration, DATETIME_ATTRIBUTE, ElementDeclaration,
    ElementInstanceId, PeakBindingKey, PeakMonitorDeclaration, PeakMonitorTarget,
    PeakScopedElementDeclaration, PipeWireControlTarget, RangeControlDeclaration,
    RepeatDeclaration, RepeatedElementDeclaration, STATE_ATTRIBUTE, ShellAction, StateBindingKey,
    StateToken, StateValueKind, ensure_registry_valid,
};
use crate::identity::IdentityRegistry;
use crate::model::{DiagnosticMessage, LogicalRect, ViewportSpec};
#[cfg(feature = "gpu-renderer")]
use crate::render::PreparedRender;
use crate::render::{
    CpuRenderSession, DamageRegion, FramePlan, FrameReason, FrameReasonSet, RenderSurfaceId,
};
use crate::resource::{LocalOnlyResourceProvider, ResourceAudit};
use crate::{
    ExperimentalDocumentIdentity, ExperimentalNodeIdentity, MAX_CLONED_NODES_PER_DOCUMENT,
    MAX_CLONED_NODES_PER_REPEAT, MAX_ITEMS_PER_REPEAT, NumericValue, PipeWireDocumentDemand,
    RepeatItemSnapshot, RepeatSource, RepeatSourceSnapshot, RuntimeError, StateValueFormat,
};
use blitz_dom::node::NodeData;
use blitz_dom::{Document, DocumentConfig, LocalName, QualName, StyleThreading, local_name, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LOGICAL_DIMENSION: u32 = 16_384;
const MAX_PIXEL_BYTES: usize = 256 * 1024 * 1024;
pub const LIVE_SCALE_DENOMINATOR: u32 = 120;
pub const MAX_LIVE_SCALE_NUMERATOR: u32 = 480;
static NEXT_LIVE_DOCUMENT_SERIAL: AtomicU64 = AtomicU64::new(1);

pub type LiveFrameRect = LogicalRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveRenderRequest {
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
    pub scale_numerator: u32,
    pub scale_denominator: u32,
}

impl LiveRenderRequest {
    pub fn new(
        logical_width: u32,
        logical_height: u32,
        scale_numerator: u32,
    ) -> Result<Self, RuntimeError> {
        validate_dimensions(logical_width, logical_height)?;
        if scale_numerator == 0 || scale_numerator > MAX_LIVE_SCALE_NUMERATOR {
            return Err(RuntimeError::LimitExceeded(format!(
                "preferred scale numerator {scale_numerator} is outside 1..={MAX_LIVE_SCALE_NUMERATOR}"
            )));
        }
        let buffer_width = checked_scaled_dimension(logical_width, scale_numerator)?;
        let buffer_height = checked_scaled_dimension(logical_height, scale_numerator)?;
        pixel_len(buffer_width, buffer_height)?;
        Ok(Self {
            logical_width,
            logical_height,
            buffer_width,
            buffer_height,
            scale_numerator,
            scale_denominator: LIVE_SCALE_DENOMINATOR,
        })
    }

    pub fn scale_factor(self) -> f64 {
        f64::from(self.scale_numerator) / f64::from(self.scale_denominator)
    }
}

/// A transient in-process frame for the portable live-presentation experiment.
#[derive(Debug, Clone)]
pub struct LiveFrame {
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
    /// Premultiplied RGBA8 in row-major order.
    pub premultiplied_rgba: Vec<u8>,
    /// Conservative headless estimate. The live host may damage the full surface.
    pub damage_estimate: LogicalRect,
    pub input_regions: Vec<LogicalRect>,
    pub interactive_region: LogicalRect,
    pub generation: u64,
    pub render_ms: f64,
}

/// Opaque feature-gated work item used by the internal live Vello presenter.
///
/// It is not an author-facing frame or renderer-selection API.
#[cfg(feature = "gpu-renderer")]
pub struct LiveGpuPreparedFrame {
    prepared: PreparedRender,
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
    pub damage_estimate: LogicalRect,
    pub input_regions: Vec<LogicalRect>,
    pub interactive_region: LogicalRect,
    generation: u64,
    started: Instant,
}

#[cfg(feature = "gpu-renderer")]
impl LiveGpuPreparedFrame {
    pub(crate) fn plan(&self) -> &FramePlan {
        &self.prepared.plan
    }

    pub(crate) fn prepared(&self) -> &PreparedRender {
        &self.prepared
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveInteractionState {
    pub hovered: bool,
    pub active: bool,
    pub click_count: u64,
}

/// Fixture profile used by the portable live-presentation experiments.
///
/// This is not a stable component or package API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDocumentKind {
    SingleOverlay,
    Panel,
    TransientOverlay,
}

/// Host-visible action emitted by a parse-once live document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveAction {
    SingleOverlayActivate,
    ToggleOverlay,
    CloseOverlay,
    ActivateOverlay,
    ClockEnable(ElementInstanceId),
    ClockDisable(ElementInstanceId),
    ClockToggle(ElementInstanceId),
    PowerProfileSetPowerSaver,
    PowerProfileSetBalanced,
    PowerProfileSetPerformance,
    PipeWireAudio(PipeWireControlRequest),
    PipeWireDefault(PipeWireDefaultControlRequest),
    PipeWirePeak(PipeWirePeakActionRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingActivation {
    id: String,
    action: LiveAction,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PipeWireControlIdentity {
    pub document_generation: ExperimentalDocumentIdentity,
    pub locator: PipeWireControlLocator,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipeWireControlLocator {
    Element(String),
    Repeated {
        repeat_id: String,
        item_key: String,
        local_id: String,
    },
    Contextual {
        repeat_id: String,
        node_item_key: String,
        contextual_id: String,
        channel_item_key: String,
        local_id: String,
    },
    Peak {
        monitor: PipeWirePeakMonitorLocator,
        local_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipeWirePeakMonitorLocator {
    Element(String),
    Repeated {
        repeat_id: String,
        item_key: String,
        local_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PipeWirePeakMonitorIdentity {
    pub document_generation: ExperimentalDocumentIdentity,
    pub locator: PipeWirePeakMonitorLocator,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipeWirePeakTarget {
    NodeItem {
        source_generation: u64,
        item_key: String,
    },
    DefaultSink,
    DefaultSource,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PipeWirePeakDeclarationDemand {
    pub monitor: PipeWirePeakMonitorIdentity,
    pub target: PipeWirePeakTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWirePeakOperation {
    Enable,
    Disable,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWirePeakActionRequest {
    pub control: PipeWireControlIdentity,
    pub monitor: PipeWirePeakMonitorIdentity,
    pub operation: PipeWirePeakOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWirePeakStreamState {
    Starting,
    Ready,
    Failed,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipeWirePeakProjection {
    pub node_item_key: String,
    pub stream_generation: u64,
    pub layout_generation: u64,
    pub state: PipeWirePeakStreamState,
    pub maximum: NumericValue,
    pub channels: crate::ContextualRepeatSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PipeWirePeakProjectionSet {
    pub default_sink_item_key: Option<String>,
    pub default_source_item_key: Option<String>,
    pub monitorable_nodes: BTreeSet<String>,
    pub nodes: BTreeMap<String, PipeWirePeakProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeWireAudioTarget {
    NodeItem {
        source_generation: u64,
        item_key: String,
    },
    ChannelItem {
        source_generation: u64,
        node_item_key: String,
        layout_generation: u64,
        channel_item_key: String,
    },
    DefaultSink,
    DefaultSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWireAudioOperation {
    Mute,
    Unmute,
    ToggleMute,
    SetVolume,
    SetChannelVolume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipeWireDesiredVolume(u64);

impl PipeWireDesiredVolume {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then(|| Self(value.to_bits()))
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireControlRequest {
    pub control: PipeWireControlIdentity,
    pub target: PipeWireAudioTarget,
    pub operation: PipeWireAudioOperation,
    pub volume: Option<PipeWireDesiredVolume>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWireDefaultRole {
    Sink,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeWireDefaultTarget {
    NodeItem {
        source_generation: u64,
        item_key: String,
    },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireDefaultControlRequest {
    pub control: PipeWireControlIdentity,
    pub role: PipeWireDefaultRole,
    pub target: PipeWireDefaultTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWireControlState {
    Idle,
    Pending,
    Failed,
    Unavailable,
}

impl PipeWireControlState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone)]
struct PendingRange {
    control: PipeWireControlIdentity,
    target: PipeWireAudioTarget,
    operation: PipeWireAudioOperation,
    node: ExperimentalNodeIdentity,
    range: RangeControlDeclaration,
    authoritative_value: Option<String>,
    last_desired: Option<PipeWireDesiredVolume>,
}

impl LiveAction {
    fn from_registered(
        action: ShellAction,
        target: Option<ElementInstanceId>,
        pipewire_target: Option<PipeWireControlTarget>,
        control: PipeWireControlIdentity,
    ) -> Result<Self, RuntimeError> {
        Ok(match action {
            ShellAction::OverlayToggle => Self::ToggleOverlay,
            ShellAction::OverlayClose => Self::CloseOverlay,
            ShellAction::OverlayActivate => Self::ActivateOverlay,
            ShellAction::ClockEnable => Self::ClockEnable(target.ok_or_else(|| {
                RuntimeError::InvalidMutationTarget("clock.enable has no target".into())
            })?),
            ShellAction::ClockDisable => Self::ClockDisable(target.ok_or_else(|| {
                RuntimeError::InvalidMutationTarget("clock.disable has no target".into())
            })?),
            ShellAction::ClockToggle => Self::ClockToggle(target.ok_or_else(|| {
                RuntimeError::InvalidMutationTarget("clock.toggle has no target".into())
            })?),
            ShellAction::PowerProfileSetPowerSaver => Self::PowerProfileSetPowerSaver,
            ShellAction::PowerProfileSetBalanced => Self::PowerProfileSetBalanced,
            ShellAction::PowerProfileSetPerformance => Self::PowerProfileSetPerformance,
            ShellAction::PipeWireAudioMute
            | ShellAction::PipeWireAudioUnmute
            | ShellAction::PipeWireAudioToggleMute => {
                Self::PipeWireAudio(pipewire_mute_request(action, pipewire_target, control)?)
            }
            ShellAction::PipeWireAudioSetVolume | ShellAction::PipeWireAudioSetChannelVolume => {
                return Err(RuntimeError::InvalidMutationTarget(
                    "set-volume is emitted by range interaction".into(),
                ));
            }
            ShellAction::PipeWireDefaultsClearPreferredSink => {
                Self::PipeWireDefault(PipeWireDefaultControlRequest {
                    control,
                    role: PipeWireDefaultRole::Sink,
                    target: PipeWireDefaultTarget::Clear,
                })
            }
            ShellAction::PipeWireDefaultsClearPreferredSource => {
                Self::PipeWireDefault(PipeWireDefaultControlRequest {
                    control,
                    role: PipeWireDefaultRole::Source,
                    target: PipeWireDefaultTarget::Clear,
                })
            }
            ShellAction::PipeWireDefaultsSetPreferredSink
            | ShellAction::PipeWireDefaultsSetPreferredSource => {
                return Err(RuntimeError::InvalidMutationTarget(
                    "preferred-default node selection requires a repeated item identity".into(),
                ));
            }
            ShellAction::PipeWirePeaksEnable
            | ShellAction::PipeWirePeaksDisable
            | ShellAction::PipeWirePeaksToggle => {
                return Err(RuntimeError::InvalidMutationTarget(
                    "peak actions require a peak-monitor identity".into(),
                ));
            }
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClockMutation {
    pub changed_text: bool,
    pub changed_datetime: bool,
    pub changed_enabled_state: bool,
}

impl ClockMutation {
    pub const fn changed(self) -> bool {
        self.changed_text || self.changed_datetime || self.changed_enabled_state
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveRuntimeSnapshot {
    pub document_identity: ExperimentalDocumentIdentity,
    pub document_parse_count: u32,
    pub viewport: ViewportSpec,
    pub card_identity: ExperimentalNodeIdentity,
    pub action_identity: ExperimentalNodeIdentity,
    pub card_bounds: LogicalRect,
    pub action_bounds: LogicalRect,
    pub interaction: LiveInteractionState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveRuntimeMeasurements {
    pub package_read_ms: f64,
    pub html_parse_ms: f64,
    pub initial_resolve_ms: f64,
    pub last_resolve_ms: f64,
    pub last_render_ms: f64,
    pub registry_initialization_ms: f64,
    pub declaration_discovery_ms: f64,
    pub registered_element_count: u64,
    pub binding_count: u64,
    pub text_binding_count: u64,
    pub token_binding_count: u64,
    pub value_binding_count: u64,
    pub boolean_binding_count: u64,
    pub action_count: u64,
    pub clock_declaration_count: u64,
    pub repeat_declaration_count: u64,
    pub registry_scan_count: u64,
    pub suppressed_binding_updates: u64,
    pub changed_token_updates: u64,
    pub suppressed_token_updates: u64,
    pub repeat_insertions: u64,
    pub repeat_removals: u64,
    pub repeat_moves: u64,
    pub repeat_property_updates: u64,
    pub repeat_unchanged_items: u64,
    pub repeat_subtree_clones: u64,
    pub repeat_identity_reuses: u64,
    pub repeated_item_count: u64,
    pub cloned_node_count: u64,
    pub contextual_item_count: u64,
    pub channel_source_activations: u64,
    pub channel_source_releases: u64,
    pub channel_insertions: u64,
    pub channel_removals: u64,
    pub channel_moves: u64,
    pub channel_layout_replacements: u64,
    pub channel_value_mutations: u64,
    pub contextual_subtree_clones: u64,
    pub retained_channel_identities: u64,
    pub duplicate_channel_suppressions: u64,
    pub link_insertions: u64,
    pub link_removals: u64,
    pub link_state_mutations: u64,
    pub link_relation_mutations: u64,
    pub link_moves: u64,
    pub group_insertions: u64,
    pub group_removals: u64,
    pub group_member_insertions: u64,
    pub group_member_removals: u64,
    pub representative_changes: u64,
    pub group_state_mutations: u64,
    pub node_tracker_insertions: u64,
    pub node_tracker_removals: u64,
    pub peer_relation_mutations: u64,
    pub retained_link_identities: u64,
    pub retained_group_identities: u64,
    pub retained_tracker_identities: u64,
    pub duplicate_graph_suppressions: u64,
    pub last_reconciliation_ms: f64,
    pub last_state_projection_ms: f64,
    pub last_attribute_mutation_ms: f64,
}

/// Experimental parse-once document session for the portable live host.
///
/// Blitz remains private to this type. This is not the final runtime API.
pub struct LiveDocument {
    document: HtmlDocument,
    identities: IdentityRegistry,
    render_session: CpuRenderSession,
    audit: Arc<ResourceAudit>,
    package_root: PathBuf,
    source: PathBuf,
    viewport: ViewportSpec,
    document_identity: ExperimentalDocumentIdentity,
    builtins: BuiltInElementIndex,
    repeats: BTreeMap<String, LiveRepeat>,
    parse_count: u32,
    started: Instant,
    frame_generation: u64,
    kind: LiveDocumentKind,
    last_pointer: Option<Point<f32>>,
    pressed_action: Option<PendingActivation>,
    pressed_range: Option<PendingRange>,
    pending_action: Option<LiveAction>,
    peak_monitors: BTreeMap<String, LivePeakMonitor>,
    click_count: u64,
    measurements: LiveRuntimeMeasurements,
    diagnostics: Vec<DiagnosticMessage>,
}

#[derive(Debug, Clone)]
struct LiveRepeatedElement {
    declaration: RepeatedElementDeclaration,
    node: ExperimentalNodeIdentity,
}

#[derive(Debug, Clone)]
struct LiveRepeatedItem {
    root: ExperimentalNodeIdentity,
    elements: Vec<LiveRepeatedElement>,
    contextual_repeats: BTreeMap<String, LiveContextualRepeat>,
    peak_monitors: BTreeMap<String, LivePeakMonitor>,
}

#[derive(Debug, Clone)]
struct LivePeakElement {
    declaration: PeakScopedElementDeclaration,
    node: ExperimentalNodeIdentity,
    control_state: PipeWireControlState,
}

#[derive(Debug, Clone)]
struct LivePeakMonitor {
    declaration: PeakMonitorDeclaration,
    identity: PipeWirePeakMonitorIdentity,
    root: ExperimentalNodeIdentity,
    elements: Vec<LivePeakElement>,
    channel_repeats: BTreeMap<String, LiveContextualRepeat>,
    enabled: bool,
    last_status: PipeWirePeakMonitorStatus,
    target_item_key: Option<String>,
    stream_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipeWirePeakMonitorStatus {
    Disabled,
    Suspended,
    Unavailable,
    Starting,
    Ready,
    Failed,
}

impl PipeWirePeakMonitorStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Suspended => "suspended",
            Self::Unavailable => "unavailable",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
struct LiveContextualRepeat {
    declaration: crate::ContextualRepeatDeclaration,
    template_node: ExperimentalNodeIdentity,
    prototype_root: ExperimentalNodeIdentity,
    source_generation: u64,
    items: BTreeMap<String, LiveRepeatedItem>,
    order: Vec<String>,
}

#[derive(Debug, Clone)]
struct LiveRepeat {
    declaration: RepeatDeclaration,
    source_generation: u64,
    items: BTreeMap<String, LiveRepeatedItem>,
    order: Vec<String>,
}

fn instantiate_peak_monitor(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    declaration: PeakMonitorDeclaration,
    root: ExperimentalNodeIdentity,
    identity: PipeWirePeakMonitorIdentity,
) -> Result<LivePeakMonitor, RuntimeError> {
    let root_slot = identities.resolve(document, root)?;
    let slots = subtree_slots(document, root_slot)?;
    if slots.len() != declaration.prototype_nodes {
        return Err(RuntimeError::InvalidMutationTarget(format!(
            "peak monitor `{}` contains {} nodes; expected {}",
            declaration.id,
            slots.len(),
            declaration.prototype_nodes
        )));
    }
    let nodes = slots
        .iter()
        .map(|slot| identities.identity_for_slot(document, *slot))
        .collect::<Result<Vec<_>, _>>()?;
    let elements = declaration
        .descendants
        .iter()
        .map(|descendant| {
            nodes
                .get(descendant.prototype_order)
                .copied()
                .map(|node| LivePeakElement {
                    declaration: descendant.clone(),
                    node,
                    control_state: PipeWireControlState::Idle,
                })
                .ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "peak monitor descendant `{}` has an invalid prototype position",
                        descendant.local_id
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut channel_repeats = BTreeMap::new();
    for channel in &declaration.channel_repeats {
        let template_node = nodes
            .get(channel.template_prototype_order)
            .copied()
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "peak channel repeat `{}` has an invalid template position",
                    channel.id
                ))
            })?;
        let template_slot = identities.resolve(document, template_node)?;
        let prototype_root_slot = template_root_element(document, template_slot)?;
        let prototype_root = identities.identity_for_slot(document, prototype_root_slot)?;
        channel_repeats.insert(
            channel.id.clone(),
            LiveContextualRepeat {
                declaration: crate::ContextualRepeatDeclaration {
                    id: channel.id.clone(),
                    source: crate::ContextualRepeatSource::Channels,
                    template_prototype_order: channel.template_prototype_order,
                    descendants: channel.descendants.clone(),
                    prototype_nodes: channel.prototype_nodes,
                },
                template_node,
                prototype_root,
                source_generation: 0,
                items: BTreeMap::new(),
                order: Vec::new(),
            },
        );
    }
    let enabled = declaration.enabled;
    Ok(LivePeakMonitor {
        declaration,
        identity,
        root,
        elements,
        channel_repeats,
        enabled,
        last_status: if enabled {
            PipeWirePeakMonitorStatus::Suspended
        } else {
            PipeWirePeakMonitorStatus::Disabled
        },
        target_item_key: None,
        stream_generation: 0,
    })
}

fn instantiate_repeated_peak_monitor(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    declaration: PeakMonitorDeclaration,
    clone_identities: &[ExperimentalNodeIdentity],
    identity: PipeWirePeakMonitorIdentity,
) -> Result<LivePeakMonitor, RuntimeError> {
    let root = clone_identities
        .get(declaration.root_prototype_order)
        .copied()
        .ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "repeated peak monitor `{}` has an invalid root position",
                declaration.id
            ))
        })?;
    let elements = declaration
        .descendants
        .iter()
        .map(|descendant| {
            clone_identities
                .get(descendant.prototype_order)
                .copied()
                .map(|node| LivePeakElement {
                    declaration: descendant.clone(),
                    node,
                    control_state: PipeWireControlState::Idle,
                })
                .ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "repeated peak descendant `{}` has an invalid prototype position",
                        descendant.local_id
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut channel_repeats = BTreeMap::new();
    for channel in &declaration.channel_repeats {
        let template_node = clone_identities
            .get(channel.template_prototype_order)
            .copied()
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "repeated peak channel `{}` has an invalid template position",
                    channel.id
                ))
            })?;
        let template_slot = identities.resolve(document, template_node)?;
        let prototype_root_slot = template_root_element(document, template_slot)?;
        let prototype_root = identities.identity_for_slot(document, prototype_root_slot)?;
        channel_repeats.insert(
            channel.id.clone(),
            LiveContextualRepeat {
                declaration: crate::ContextualRepeatDeclaration {
                    id: channel.id.clone(),
                    source: crate::ContextualRepeatSource::Channels,
                    template_prototype_order: channel.template_prototype_order,
                    descendants: channel.descendants.clone(),
                    prototype_nodes: channel.prototype_nodes,
                },
                template_node,
                prototype_root,
                source_generation: 0,
                items: BTreeMap::new(),
                order: Vec::new(),
            },
        );
    }
    let enabled = declaration.enabled;
    Ok(LivePeakMonitor {
        declaration,
        identity,
        root,
        elements,
        channel_repeats,
        enabled,
        last_status: if enabled {
            PipeWirePeakMonitorStatus::Suspended
        } else {
            PipeWirePeakMonitorStatus::Disabled
        },
        target_item_key: None,
        stream_generation: 0,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepeatMutation {
    pub insertions: usize,
    pub removals: usize,
    pub moves: usize,
    pub property_updates: usize,
    pub unchanged_items: usize,
    pub subtree_clones: usize,
    pub identity_reuses: usize,
}

impl RepeatMutation {
    pub const fn changed(self) -> bool {
        self.insertions > 0 || self.removals > 0 || self.moves > 0 || self.property_updates > 0
    }
}

impl LiveDocument {
    pub fn load(
        package: impl AsRef<Path>,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        Self::load_inner(
            package.as_ref(),
            Path::new(LiveDocumentKind::SingleOverlay.source_file()),
            LiveDocumentKind::SingleOverlay,
            logical_width,
            logical_height,
        )
    }

    pub fn load_surface(
        package: impl AsRef<Path>,
        kind: LiveDocumentKind,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        Self::load_inner(
            package.as_ref(),
            Path::new(kind.source_file()),
            kind,
            logical_width,
            logical_height,
        )
    }

    pub fn load_surface_document(
        package: impl AsRef<Path>,
        document: impl AsRef<Path>,
        kind: LiveDocumentKind,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        Self::load_inner(
            package.as_ref(),
            document.as_ref(),
            kind,
            logical_width,
            logical_height,
        )
    }

    fn load_inner(
        package: &Path,
        document: &Path,
        kind: LiveDocumentKind,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        validate_dimensions(logical_width, logical_height)?;
        let read_started = Instant::now();
        let package_root = package
            .canonicalize()
            .map_err(|error| RuntimeError::io("resolve live package directory", package, error))?;
        if !package_root.is_dir() {
            return Err(RuntimeError::InvalidPackage(format!(
                "{} is not a directory",
                package_root.display()
            )));
        }
        if document.is_absolute()
            || document.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(RuntimeError::InvalidPackage(
                "live document must be a package-relative path".into(),
            ));
        }
        let source = package_root
            .join(document)
            .canonicalize()
            .map_err(|error| RuntimeError::io("resolve live document", document, error))?;
        if !source.starts_with(&package_root) {
            return Err(RuntimeError::InvalidPackage(
                "live document resolves outside the package directory".into(),
            ));
        }
        let metadata = source
            .metadata()
            .map_err(|error| RuntimeError::io("inspect live document", &source, error))?;
        if metadata.len() > MAX_HTML_BYTES {
            return Err(RuntimeError::LimitExceeded(format!(
                "index.html is {} bytes; limit is {MAX_HTML_BYTES}",
                metadata.len()
            )));
        }
        let html = std::fs::read_to_string(&source)
            .map_err(|error| RuntimeError::io("read live document as UTF-8", &source, error))?;
        let package_read_ms = elapsed_ms(read_started);

        let viewport = ViewportSpec {
            logical_width,
            logical_height,
            scale_factor: 1.0,
            color_space: "sRGB",
            dynamic_range: "SDR",
        };
        let audit = Arc::new(ResourceAudit::default());
        let provider = Arc::new(LocalOnlyResourceProvider::new(
            package_root.clone(),
            Arc::clone(&audit),
        ));
        let parse_started = Instant::now();
        let mut document = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                viewport: Some(blitz_viewport(viewport)),
                base_url: Some(LocalOnlyResourceProvider::virtual_document_url().to_owned()),
                net_provider: Some(provider),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        validate_document_limits(&document)?;
        let html_parse_ms = elapsed_ms(parse_started);

        for selector in kind.required_selectors() {
            required_selector(&document, selector)?;
        }

        let mut diagnostics = Vec::new();
        let resolve_started = Instant::now();
        resolve_resources(&mut document, &audit, 0.0, &mut diagnostics);
        let initial_resolve_ms = elapsed_ms(resolve_started);
        let identities = IdentityRegistry::from_document(&document);
        let document_identity = ExperimentalDocumentIdentity {
            serial: NEXT_LIVE_DOCUMENT_SERIAL.fetch_add(1, Ordering::Relaxed),
        };
        let registry_started = Instant::now();
        ensure_registry_valid()?;
        let registry_initialization_ms = elapsed_ms(registry_started);
        let discovery_started = Instant::now();
        let builtins = BuiltInElementIndex::discover(
            &document,
            &identities,
            document_identity,
            kind.builtin_surface_kind(),
            &source.display().to_string(),
        )?;
        let declaration_discovery_ms = elapsed_ms(discovery_started);
        let builtin_summary = builtins.summary();
        let peak_monitors = builtins
            .peak_monitor_declarations()
            .into_iter()
            .map(|declaration| {
                let id = declaration.id.clone();
                let root = builtins.indexed_node(&id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "peak monitor `#{id}` disappeared during discovery"
                    ))
                })?;
                let identity = PipeWirePeakMonitorIdentity {
                    document_generation: document_identity,
                    locator: PipeWirePeakMonitorLocator::Element(id.clone()),
                };
                instantiate_peak_monitor(&document, &identities, declaration, root, identity)
                    .map(|monitor| (id, monitor))
            })
            .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?;
        let repeats = builtins
            .repeat_declarations()
            .into_iter()
            .map(|declaration| {
                (
                    declaration.id.html_id.clone(),
                    LiveRepeat {
                        declaration,
                        source_generation: 0,
                        items: BTreeMap::new(),
                        order: Vec::new(),
                    },
                )
            })
            .collect();

        let mut live = Self {
            document,
            identities,
            render_session: CpuRenderSession::default(),
            audit,
            package_root,
            source,
            viewport,
            document_identity,
            builtins,
            repeats,
            parse_count: 1,
            started: Instant::now(),
            frame_generation: 0,
            kind,
            last_pointer: None,
            pressed_action: None,
            pressed_range: None,
            pending_action: None,
            peak_monitors,
            click_count: 0,
            measurements: LiveRuntimeMeasurements {
                package_read_ms,
                html_parse_ms,
                initial_resolve_ms,
                last_resolve_ms: initial_resolve_ms,
                last_render_ms: 0.0,
                registry_initialization_ms,
                declaration_discovery_ms,
                registered_element_count: builtin_summary.registered_elements as u64,
                binding_count: builtin_summary.bindings as u64,
                text_binding_count: builtin_summary.text_bindings as u64,
                token_binding_count: builtin_summary.token_bindings as u64,
                value_binding_count: builtin_summary.value_bindings as u64,
                boolean_binding_count: builtin_summary.boolean_bindings as u64,
                action_count: builtin_summary.actions as u64,
                clock_declaration_count: builtin_summary.clock_declarations as u64,
                repeat_declaration_count: builtin_summary.repeat_declarations as u64,
                registry_scan_count: u64::from(builtin_summary.discovery_scans),
                suppressed_binding_updates: 0,
                changed_token_updates: 0,
                suppressed_token_updates: 0,
                last_state_projection_ms: 0.0,
                last_attribute_mutation_ms: 0.0,
                ..LiveRuntimeMeasurements::default()
            },
            diagnostics,
        };
        live.initialize_peak_monitor_states()?;
        live.apply_bound_tokens(&[
            (StateBindingKey::OverlayStatus, StateToken::Closed),
            (StateBindingKey::SurfaceScaleProfile, StateToken::Scale1),
            (StateBindingKey::BatteryStatus, StateToken::Unavailable),
            (StateBindingKey::BatteryWarning, StateToken::Unknown),
        ])?;
        let unknown_values: Vec<_> = StateBindingKey::ALL
            .into_iter()
            .filter(|key| key.supports(StateValueKind::Value))
            .map(|key| (key, NumericValue::Unknown))
            .collect();
        live.apply_bound_values(&unknown_values)?;
        live.apply_bound_booleans(&[
            (StateBindingKey::PowerProfileAvailability, None),
            (StateBindingKey::PowerProfilePerformanceAvailable, None),
        ])?;
        Ok(live)
    }

    pub fn set_viewport(
        &mut self,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<bool, RuntimeError> {
        validate_dimensions(logical_width, logical_height)?;
        if self.viewport.logical_width == logical_width
            && self.viewport.logical_height == logical_height
        {
            return Ok(false);
        }
        self.viewport.logical_width = logical_width;
        self.viewport.logical_height = logical_height;
        self.document.set_viewport(blitz_viewport(self.viewport));
        self.resolve();
        Ok(true)
    }

    pub fn pointer_move(&mut self, x: f64, y: f64) -> Result<bool, RuntimeError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(RuntimeError::InvalidMutationTarget(
                "pointer coordinates must be finite".into(),
            ));
        }
        if x < 0.0 || y < 0.0 {
            return Ok(self.pointer_leave());
        }
        let point = checked_point(x, y)?;
        self.last_pointer = Some(point);
        if self.pressed_range.is_some() {
            return self.update_pressed_range(point.x);
        }
        let changed = self.document.set_hover_to(point.x, point.y);
        if changed {
            self.resolve();
        }
        Ok(changed)
    }

    pub fn pointer_leave(&mut self) -> bool {
        let mut changed = self.document.clear_hover();
        if let Some(range) = self.pressed_range.take()
            && self
                .apply_attribute_to_node(range.node, "value", range.authoritative_value.as_deref())
                .unwrap_or(false)
        {
            changed = true;
        }
        if self.pressed_action.is_some() {
            let point = self.last_pointer.unwrap_or_default();
            self.document
                .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
            self.pressed_action = None;
            changed = true;
        }
        self.last_pointer = None;
        if changed {
            self.resolve();
        }
        changed
    }

    pub fn pointer_primary(&mut self, pressed: bool) -> Result<bool, RuntimeError> {
        let Some(point) = self.last_pointer else {
            return Ok(false);
        };
        match pressed {
            true if self.pressed_action.is_none() && self.pressed_range.is_none() => {
                if let Some(range) = self.range_at(point.x, point.y)? {
                    self.document
                        .handle_ui_event(UiEvent::PointerDown(pointer_event(
                            point.x, point.y, true,
                        )));
                    self.pressed_range = Some(range);
                    self.update_pressed_range(point.x)?;
                    self.resolve();
                    return Ok(true);
                }
                let Some(action) = self.action_at(point.x, point.y)? else {
                    return Ok(false);
                };
                self.document
                    .handle_ui_event(UiEvent::PointerDown(pointer_event(point.x, point.y, true)));
                self.pressed_action = Some(action);
                self.resolve();
                Ok(true)
            }
            false if self.pressed_action.is_some() => {
                let pressed_action = self.pressed_action.take().expect("checked above");
                self.document
                    .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
                let released_action = self.action_at(point.x, point.y)?;
                if released_action
                    .as_ref()
                    .is_some_and(|released| released.id == pressed_action.id)
                {
                    self.click_count = self.click_count.saturating_add(1);
                    let action = pressed_action.action.clone();
                    if action == LiveAction::SingleOverlayActivate {
                        self.apply_click_mutation()?;
                    }
                    self.pending_action = Some(action);
                }
                self.resolve();
                Ok(true)
            }
            false if self.pressed_range.is_some() => {
                self.update_pressed_range(point.x)?;
                self.document
                    .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
                self.pressed_range = None;
                self.resolve();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn render(&mut self) -> Result<LiveFrame, RuntimeError> {
        let request = LiveRenderRequest::new(
            self.viewport.logical_width,
            self.viewport.logical_height,
            LIVE_SCALE_DENOMINATOR,
        )?;
        self.render_for(request)
    }

    pub fn render_for(&mut self, request: LiveRenderRequest) -> Result<LiveFrame, RuntimeError> {
        self.render_internal(
            request,
            RenderSurfaceId {
                instance: self.document_identity.serial,
                generation: 1,
            },
            true,
        )?
        .ok_or_else(|| {
            RuntimeError::InvalidPackage(
                "forced retained rendering did not produce a frame plan".into(),
            )
        })
    }

    /// Renders only when the retained visual state or target context changed.
    ///
    /// The live host supplies its generation-safe surface identity. A duplicate
    /// logical publication returns `None` and therefore cannot schedule a frame.
    pub fn render_pending_for(
        &mut self,
        request: LiveRenderRequest,
        surface_instance: u64,
        surface_generation: u64,
    ) -> Result<Option<LiveFrame>, RuntimeError> {
        self.render_internal(
            request,
            RenderSurfaceId {
                instance: surface_instance,
                generation: surface_generation,
            },
            false,
        )
    }

    /// Builds one immutable live frame without CPU rasterization.
    ///
    /// The returned work remains provisional until `accept_gpu_frame` or
    /// `render_gpu_frame_on_cpu` consumes it.
    #[cfg(feature = "gpu-renderer")]
    pub fn prepare_gpu_pending_for(
        &mut self,
        request: LiveRenderRequest,
        surface_instance: u64,
        surface_generation: u64,
    ) -> Result<Option<LiveGpuPreparedFrame>, RuntimeError> {
        self.validate_render_request(request)?;
        let started = Instant::now();
        let Some(prepared) = self.render_session.prepare_document(
            &mut self.document,
            &self.identities,
            self.document_identity,
            self.viewport,
            RenderSurfaceId {
                instance: surface_instance,
                generation: surface_generation,
            },
            request.buffer_width,
            request.buffer_height,
            request.scale_numerator,
            request.scale_denominator,
            FrameReasonSet::new(),
            false,
        )?
        else {
            return Ok(None);
        };
        let (damage_estimate, input_regions, interactive_region) =
            self.live_frame_geometry(&prepared.plan)?;
        Ok(Some(LiveGpuPreparedFrame {
            prepared,
            logical_width: request.logical_width,
            logical_height: request.logical_height,
            buffer_width: request.buffer_width,
            buffer_height: request.buffer_height,
            damage_estimate,
            input_regions,
            interactive_region,
            generation: self.frame_generation.saturating_add(1),
            started,
        }))
    }

    /// Accepts a directly presented GPU frame and advances retained identity.
    #[cfg(feature = "gpu-renderer")]
    pub fn accept_gpu_frame(&mut self, frame: LiveGpuPreparedFrame) -> f64 {
        self.render_session.accept_prepared(frame.prepared());
        self.frame_generation = frame.generation;
        let render_ms = elapsed_ms(frame.started);
        self.measurements.last_render_ms = render_ms;
        render_ms
    }

    /// Renders the same provisional frame through the authoritative CPU path.
    #[cfg(feature = "gpu-renderer")]
    pub fn render_gpu_frame_on_cpu(
        &mut self,
        frame: LiveGpuPreparedFrame,
    ) -> Result<LiveFrame, RuntimeError> {
        let rendered = self.render_session.render_prepared_cpu(frame.prepared())?;
        let expected = pixel_len(frame.buffer_width, frame.buffer_height)?;
        if rendered.pixels.len() != expected {
            return Err(RuntimeError::InvalidPackage(format!(
                "CPU fallback returned {} bytes; expected {expected}",
                rendered.pixels.len()
            )));
        }
        self.render_session.accept_prepared(frame.prepared());
        self.frame_generation = frame.generation;
        let render_ms = elapsed_ms(frame.started);
        self.measurements.last_render_ms = render_ms;
        Ok(LiveFrame {
            logical_width: frame.logical_width,
            logical_height: frame.logical_height,
            buffer_width: frame.buffer_width,
            buffer_height: frame.buffer_height,
            premultiplied_rgba: rendered.pixels,
            damage_estimate: frame.damage_estimate,
            input_regions: frame.input_regions,
            interactive_region: frame.interactive_region,
            generation: frame.generation,
            render_ms,
        })
    }

    #[cfg(feature = "gpu-renderer")]
    pub fn reject_gpu_frame(&mut self, frame: LiveGpuPreparedFrame, recoverable: bool) {
        let _ = frame;
        self.render_session.reject_prepared(recoverable);
    }

    /// Requests one full retained repaint after presenter-only state changes.
    #[cfg(feature = "gpu-renderer")]
    pub fn request_gpu_full_repaint(&mut self) {
        self.render_session.reject_prepared(true);
    }

    fn render_internal(
        &mut self,
        request: LiveRenderRequest,
        surface: RenderSurfaceId,
        force: bool,
    ) -> Result<Option<LiveFrame>, RuntimeError> {
        self.validate_render_request(request)?;
        let render_started = Instant::now();
        let mut reasons = FrameReasonSet::new();
        if force {
            reasons.insert(FrameReason::ExplicitInvalidation);
        }
        let Some(frame) = self.render_session.render_document(
            &mut self.document,
            &self.identities,
            self.document_identity,
            self.viewport,
            surface,
            request.buffer_width,
            request.buffer_height,
            request.scale_numerator,
            request.scale_denominator,
            reasons,
            force,
        )?
        else {
            return Ok(None);
        };
        let premultiplied_rgba = frame.pixels;
        let expected = pixel_len(request.buffer_width, request.buffer_height)?;
        if premultiplied_rgba.len() != expected {
            return Err(RuntimeError::InvalidPackage(format!(
                "renderer returned {} bytes; expected {expected}",
                premultiplied_rgba.len()
            )));
        }
        let render_ms = elapsed_ms(render_started);
        self.measurements.last_render_ms = render_ms;
        self.frame_generation = self.frame_generation.saturating_add(1);
        let (damage_estimate, input_regions, interactive_region) =
            self.live_frame_geometry(&frame.plan)?;
        debug_assert!(frame.full_raster);
        Ok(Some(LiveFrame {
            logical_width: request.logical_width,
            logical_height: request.logical_height,
            buffer_width: request.buffer_width,
            buffer_height: request.buffer_height,
            premultiplied_rgba,
            damage_estimate,
            input_regions,
            interactive_region,
            generation: self.frame_generation,
            render_ms,
        }))
    }

    fn validate_render_request(&self, request: LiveRenderRequest) -> Result<(), RuntimeError> {
        if request.logical_width != self.viewport.logical_width
            || request.logical_height != self.viewport.logical_height
        {
            return Err(RuntimeError::InvalidPackage(format!(
                "render request logical size {}x{} does not match live viewport {}x{}",
                request.logical_width,
                request.logical_height,
                self.viewport.logical_width,
                self.viewport.logical_height
            )));
        }
        let checked = LiveRenderRequest::new(
            request.logical_width,
            request.logical_height,
            request.scale_numerator,
        )?;
        if checked != request {
            return Err(RuntimeError::InvalidPackage(
                "render request contains inconsistent physical dimensions".into(),
            ));
        }
        Ok(())
    }

    fn live_frame_geometry(
        &self,
        plan: &FramePlan,
    ) -> Result<(LogicalRect, Vec<LogicalRect>, LogicalRect), RuntimeError> {
        let card = self.bounds_for(self.kind.region_selector())?;
        let action = self.bounds_for(self.kind.primary_action_selector())?;
        validate_rect(&card)?;
        validate_rect(&action)?;
        let damage_estimate = match &plan.damage {
            DamageRegion::Empty => LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            DamageRegion::Full => LogicalRect {
                x: 0.0,
                y: 0.0,
                width: plan.logical_width as f32,
                height: plan.logical_height as f32,
            },
            DamageRegion::Rects(rects) => rects
                .iter()
                .cloned()
                .reduce(|left, right| {
                    let x1 = left.x.min(right.x);
                    let y1 = left.y.min(right.y);
                    let x2 = (left.x + left.width).max(right.x + right.width);
                    let y2 = (left.y + left.height).max(right.y + right.height);
                    LogicalRect {
                        x: x1,
                        y: y1,
                        width: x2 - x1,
                        height: y2 - y1,
                    }
                })
                .unwrap_or(LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }),
        };
        Ok((damage_estimate, vec![card], action))
    }

    pub fn snapshot(&self) -> Result<LiveRuntimeSnapshot, RuntimeError> {
        let card_slot = required_selector(&self.document, self.kind.region_selector())?;
        let action_slot = required_selector(&self.document, self.kind.primary_action_selector())?;
        let card = self
            .document
            .get_node(card_slot)
            .ok_or_else(|| RuntimeError::InvalidMutationTarget("live region disappeared".into()))?;
        let action = self.document.get_node(action_slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("primary live action disappeared".into())
        })?;
        Ok(LiveRuntimeSnapshot {
            document_identity: self.document_identity,
            document_parse_count: self.parse_count,
            viewport: self.viewport,
            card_identity: self
                .identities
                .identity_for_slot(&self.document, card_slot)?,
            action_identity: self
                .identities
                .identity_for_slot(&self.document, action_slot)?,
            card_bounds: node_bounds(card),
            action_bounds: node_bounds(action),
            interaction: LiveInteractionState {
                hovered: action.is_hovered(),
                active: action.is_active(),
                click_count: self.click_count,
            },
        })
    }

    pub fn measurements(&self) -> LiveRuntimeMeasurements {
        self.measurements
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    pub fn resource_request_count(&self) -> usize {
        self.audit.request_count()
    }

    pub fn kind(&self) -> LiveDocumentKind {
        self.kind
    }

    pub fn take_action(&mut self) -> Option<LiveAction> {
        self.pending_action.take()
    }

    pub fn apply_pipewire_control_state(
        &mut self,
        identity: &PipeWireControlIdentity,
        state: PipeWireControlState,
    ) -> Result<bool, RuntimeError> {
        if identity.document_generation != self.document_identity {
            return Err(RuntimeError::InvalidMutationTarget(
                "PipeWire control belongs to a stale document generation".into(),
            ));
        }
        let node = match &identity.locator {
            PipeWireControlLocator::Element(html_id) => {
                let declaration = self.builtins.element(html_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "PipeWire control `#{html_id}` disappeared"
                    ))
                })?;
                if declaration
                    .action
                    .is_none_or(|action| !action.as_str().starts_with("pipewire."))
                {
                    return Err(RuntimeError::InvalidMutationTarget(format!(
                        "`#{html_id}` is not a PipeWire control"
                    )));
                }
                self.builtins.indexed_node(html_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "PipeWire control `#{html_id}` disappeared"
                    ))
                })?
            }
            PipeWireControlLocator::Repeated {
                repeat_id,
                item_key,
                local_id,
            } => {
                let repeat = self.repeats.get(repeat_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "PipeWire repeat `#{repeat_id}` disappeared"
                    ))
                })?;
                let item = repeat.items.get(item_key).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "PipeWire node item `{item_key}` disappeared"
                    ))
                })?;
                item.elements
                    .iter()
                    .find(|element| element.declaration.local_id == *local_id)
                    .filter(|element| element.declaration.action.is_some())
                    .map(|element| element.node)
                    .ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "PipeWire control `{local_id}` disappeared"
                        ))
                    })?
            }
            PipeWireControlLocator::Contextual {
                repeat_id,
                node_item_key,
                contextual_id,
                channel_item_key,
                local_id,
            } => {
                let repeat = self.repeats.get(repeat_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "PipeWire repeat `#{repeat_id}` disappeared"
                    ))
                })?;
                let node = repeat.items.get(node_item_key).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "PipeWire node item `{node_item_key}` disappeared"
                    ))
                })?;
                let contextual = node.contextual_repeats.get(contextual_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "contextual repeat `{contextual_id}` disappeared"
                    ))
                })?;
                let channel = contextual.items.get(channel_item_key).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "channel item `{channel_item_key}` disappeared"
                    ))
                })?;
                channel
                    .elements
                    .iter()
                    .find(|element| element.declaration.local_id == *local_id)
                    .filter(|element| element.declaration.action.is_some())
                    .map(|element| element.node)
                    .ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "channel control `{local_id}` disappeared"
                        ))
                    })?
            }
            PipeWireControlLocator::Peak { monitor, local_id } => {
                let peak = match monitor {
                    PipeWirePeakMonitorLocator::Element(id) => self.peak_monitors.get(id),
                    PipeWirePeakMonitorLocator::Repeated {
                        repeat_id,
                        item_key,
                        local_id,
                    } => self
                        .repeats
                        .get(repeat_id)
                        .and_then(|repeat| repeat.items.get(item_key))
                        .and_then(|item| item.peak_monitors.get(local_id)),
                }
                .ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget("peak monitor disappeared".into())
                })?;
                peak.elements
                    .iter()
                    .find(|element| element.declaration.local_id == *local_id)
                    .filter(|element| element.declaration.action.is_some())
                    .map(|element| element.node)
                    .ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "peak control `{local_id}` disappeared"
                        ))
                    })?
            }
        };
        let changed = self.apply_attribute_to_node(node, STATE_ATTRIBUTE, Some(state.as_str()))?;
        if changed {
            self.resolve();
        }
        Ok(changed)
    }

    pub fn apply_pipewire_peak_action(
        &mut self,
        request: &PipeWirePeakActionRequest,
    ) -> Result<bool, RuntimeError> {
        if request.control.document_generation != self.document_identity
            || request.monitor.document_generation != self.document_identity
        {
            return Err(RuntimeError::InvalidMutationTarget(
                "peak action belongs to a stale document generation".into(),
            ));
        }
        let enabled_count = self.enabled_peak_monitor_count();
        let mut monitor = self.take_peak_monitor(&request.monitor)?;
        let desired = match request.operation {
            PipeWirePeakOperation::Enable => true,
            PipeWirePeakOperation::Disable => false,
            PipeWirePeakOperation::Toggle => !monitor.enabled,
        };
        let enabled_changed = monitor.enabled != desired;
        let control_index = monitor
            .elements
            .iter()
            .position(|element| {
                matches!(
                    &request.control.locator,
                    PipeWireControlLocator::Peak { local_id, .. }
                        if element.declaration.local_id == *local_id
                )
            })
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget("peak action control disappeared".into())
            })?;
        let control_node = monitor.elements[control_index].node;
        if desired
            && !monitor.enabled
            && enabled_count >= crate::MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT
        {
            monitor.elements[control_index].control_state = PipeWireControlState::Failed;
            let changed = self.apply_attribute_to_node(
                control_node,
                STATE_ATTRIBUTE,
                Some(PipeWireControlState::Failed.as_str()),
            )?;
            self.put_peak_monitor(monitor)?;
            if changed {
                self.resolve();
            }
            return Ok(changed);
        }
        monitor.enabled = desired;
        let state = if desired {
            PipeWireControlState::Pending
        } else {
            PipeWireControlState::Idle
        };
        monitor.elements[control_index].control_state = state;
        let state_changed =
            self.apply_attribute_to_node(control_node, STATE_ATTRIBUTE, Some(state.as_str()))?;
        let root_state = if desired {
            PipeWirePeakMonitorStatus::Starting
        } else {
            PipeWirePeakMonitorStatus::Disabled
        };
        let monitor_changed = self.update_peak_monitor(&mut monitor, root_state, None, desired)?;
        self.put_peak_monitor(monitor)?;
        let changed = enabled_changed || state_changed || monitor_changed > 0;
        if changed {
            self.resolve();
        }
        Ok(changed)
    }

    pub fn built_in_summary(&self) -> BuiltInElementSummary {
        self.builtins.summary()
    }

    pub fn built_in_declarations(&self) -> Vec<ElementDeclaration> {
        self.builtins.declarations()
    }

    pub fn clock_declarations(&self) -> Vec<ClockDeclaration> {
        self.builtins.clock_declarations()
    }

    pub fn set_clock_enabled(
        &mut self,
        identity: &ElementInstanceId,
        enabled: bool,
    ) -> Result<bool, RuntimeError> {
        self.validate_element_identity(identity)?;
        self.builtins.set_clock_enabled(identity, enabled)
    }

    pub fn clock_enabled(&self, identity: &ElementInstanceId) -> Result<bool, RuntimeError> {
        self.validate_element_identity(identity)?;
        self.builtins
            .clock_declaration(identity)
            .map(|clock| clock.enabled)
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "target `#{}` is not `clock-text`",
                    identity.html_id
                ))
            })
    }

    pub fn apply_clock_output(
        &mut self,
        identity: &ElementInstanceId,
        text: &str,
        datetime: &str,
        enabled: bool,
    ) -> Result<ClockMutation, RuntimeError> {
        self.validate_element_identity(identity)?;
        let declaration = self.builtins.clock_declaration(identity).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "target `#{}` is not `clock-text`",
                identity.html_id
            ))
        })?;
        if declaration.enabled != enabled {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "clock target `#{}` enabled state is stale",
                identity.html_id
            )));
        }
        let state = if enabled { "enabled" } else { "disabled" };
        let mutation = ClockMutation {
            changed_text: self.element_text(&identity.html_id)? != text,
            changed_datetime: self
                .registered_attribute(&identity.html_id, DATETIME_ATTRIBUTE)?
                .as_deref()
                != Some(datetime),
            changed_enabled_state: self
                .registered_attribute(&identity.html_id, STATE_ATTRIBUTE)?
                .as_deref()
                != Some(state),
        };
        if mutation.changed_text {
            self.set_registered_text(&identity.html_id, text)?;
        }
        let attribute_started = Instant::now();
        if mutation.changed_datetime {
            self.set_registered_attribute(&identity.html_id, DATETIME_ATTRIBUTE, datetime)?;
        }
        if mutation.changed_enabled_state {
            self.set_registered_attribute(&identity.html_id, STATE_ATTRIBUTE, state)?;
        }
        self.measurements.last_attribute_mutation_ms = elapsed_ms(attribute_started);
        if mutation.changed() {
            self.resolve();
        }
        Ok(mutation)
    }

    pub fn element_datetime(&self, html_id: &str) -> Result<String, RuntimeError> {
        self.registered_attribute(html_id, DATETIME_ATTRIBUTE)?
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "registered clock element `#{html_id}` has no runtime datetime"
                ))
            })
    }

    pub fn has_built_in_elements(&self) -> bool {
        !self.builtins.is_empty()
    }

    pub fn binding_target_count(&self, key: StateBindingKey) -> usize {
        self.text_binding_target_count(key)
            + self.token_binding_target_count(key)
            + self.value_binding_target_count(key)
            + self.boolean_binding_target_count(key)
    }

    pub fn text_binding_target_count(&self, key: StateBindingKey) -> usize {
        self.builtins
            .binding_targets(key, StateValueKind::Text)
            .len()
    }

    pub fn token_binding_target_count(&self, key: StateBindingKey) -> usize {
        self.builtins
            .binding_targets(key, StateValueKind::Token)
            .len()
    }

    pub fn value_binding_target_count(&self, key: StateBindingKey) -> usize {
        self.builtins
            .binding_targets(key, StateValueKind::Value)
            .len()
    }

    pub fn boolean_binding_target_count(&self, key: StateBindingKey) -> usize {
        self.builtins
            .binding_targets(key, StateValueKind::Boolean)
            .len()
    }

    pub fn repeat_source_target_count(&self, source: RepeatSource) -> usize {
        self.repeats
            .values()
            .filter(|repeat| repeat.declaration.source == source)
            .count()
    }

    pub fn pipewire_peak_demands(&self, mapped: bool) -> Vec<PipeWirePeakDeclarationDemand> {
        if !mapped {
            return Vec::new();
        }
        let mut demands = Vec::new();
        for monitor in self
            .peak_monitors
            .values()
            .filter(|monitor| monitor.enabled)
        {
            demands.push(PipeWirePeakDeclarationDemand {
                monitor: monitor.identity.clone(),
                target: match monitor.declaration.target {
                    PeakMonitorTarget::DefaultSink => PipeWirePeakTarget::DefaultSink,
                    PeakMonitorTarget::DefaultSource => PipeWirePeakTarget::DefaultSource,
                    PeakMonitorTarget::CurrentItem => continue,
                },
            });
        }
        for (repeat_id, repeat) in &self.repeats {
            if repeat.declaration.source != RepeatSource::PipeWireNodes {
                continue;
            }
            for item_key in &repeat.order {
                let Some(item) = repeat.items.get(item_key) else {
                    continue;
                };
                for monitor in item
                    .peak_monitors
                    .values()
                    .filter(|monitor| monitor.enabled)
                {
                    demands.push(PipeWirePeakDeclarationDemand {
                        monitor: monitor.identity.clone(),
                        target: PipeWirePeakTarget::NodeItem {
                            source_generation: repeat.source_generation,
                            item_key: item_key.clone(),
                        },
                    });
                }
            }
            let _ = repeat_id;
        }
        demands
    }

    pub fn apply_pipewire_peak_projections(
        &mut self,
        projections: &PipeWirePeakProjectionSet,
        mapped: bool,
    ) -> Result<usize, RuntimeError> {
        let identities = self.peak_monitor_identities();
        let enabled_count = self.enabled_peak_monitor_count();
        let mut changed = 0usize;
        for identity in identities {
            let mut monitor = self.take_peak_monitor(&identity)?;
            let item_key = match (&identity.locator, monitor.declaration.target) {
                (_, PeakMonitorTarget::DefaultSink) => projections.default_sink_item_key.as_deref(),
                (_, PeakMonitorTarget::DefaultSource) => {
                    projections.default_source_item_key.as_deref()
                }
                (
                    PipeWirePeakMonitorLocator::Repeated { item_key, .. },
                    PeakMonitorTarget::CurrentItem,
                ) => Some(item_key.as_str()),
                _ => None,
            };
            let projection = item_key.and_then(|key| projections.nodes.get(key));
            let monitorable = item_key
                .is_some_and(|key| projections.monitorable_nodes.contains(key))
                && (monitor.enabled
                    || enabled_count < crate::MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT);
            if monitor.target_item_key.as_deref() != item_key {
                monitor.target_item_key = item_key.map(str::to_owned);
                monitor.stream_generation = 0;
                for element in &mut monitor.elements {
                    if element.declaration.action.is_some() {
                        element.control_state = if monitorable {
                            PipeWireControlState::Idle
                        } else {
                            PipeWireControlState::Unavailable
                        };
                    }
                }
            }
            if let Some(projection) = projection {
                monitor.stream_generation = projection.stream_generation;
            }
            let status = if !monitor.enabled {
                PipeWirePeakMonitorStatus::Disabled
            } else if !mapped {
                PipeWirePeakMonitorStatus::Suspended
            } else if !monitorable {
                PipeWirePeakMonitorStatus::Unavailable
            } else {
                match projection.map(|projection| projection.state) {
                    Some(PipeWirePeakStreamState::Starting) => PipeWirePeakMonitorStatus::Starting,
                    Some(PipeWirePeakStreamState::Ready) => PipeWirePeakMonitorStatus::Ready,
                    Some(PipeWirePeakStreamState::Failed) => PipeWirePeakMonitorStatus::Failed,
                    Some(PipeWirePeakStreamState::Unavailable) | None => {
                        PipeWirePeakMonitorStatus::Unavailable
                    }
                }
            };
            changed = changed.saturating_add(self.update_peak_monitor(
                &mut monitor,
                status,
                projection,
                monitorable,
            )?);
            self.put_peak_monitor(monitor)?;
        }
        if changed > 0 {
            self.resolve();
        }
        Ok(changed)
    }

    pub fn pipewire_demand(&self) -> PipeWireDocumentDemand {
        let mut demand = PipeWireDocumentDemand::default();
        for key in StateBindingKey::ALL {
            if !key.as_str().starts_with("pipewire.") || self.binding_target_count(key) == 0 {
                continue;
            }
            demand.service = true;
            if key == StateBindingKey::PipeWireNodeCount {
                demand.nodes = true;
            }
            if key == StateBindingKey::PipeWireLinkCount {
                demand.link_collection = true;
            }
            if key == StateBindingKey::PipeWireLinkGroupCount {
                demand.link_collection = true;
                demand.link_group_collection = true;
            }
            if key.as_str().starts_with("pipewire.default_")
                || key.as_str().starts_with("pipewire.configured_")
            {
                demand.nodes = true;
                demand.defaults = true;
            }
            if matches!(
                key,
                StateBindingKey::PipeWireDefaultSinkAudioStatus
                    | StateBindingKey::PipeWireDefaultSinkVolume
                    | StateBindingKey::PipeWireDefaultSinkMuteState
                    | StateBindingKey::PipeWireDefaultSinkCanSetVolume
                    | StateBindingKey::PipeWireDefaultSinkCanSetMute
                    | StateBindingKey::PipeWireDefaultSourceAudioStatus
                    | StateBindingKey::PipeWireDefaultSourceVolume
                    | StateBindingKey::PipeWireDefaultSourceMuteState
                    | StateBindingKey::PipeWireDefaultSourceCanSetVolume
                    | StateBindingKey::PipeWireDefaultSourceCanSetMute
                    | StateBindingKey::PipeWireConfiguredSinkAudioStatus
                    | StateBindingKey::PipeWireConfiguredSinkVolume
                    | StateBindingKey::PipeWireConfiguredSinkMuteState
                    | StateBindingKey::PipeWireConfiguredSinkCanSetVolume
                    | StateBindingKey::PipeWireConfiguredSinkCanSetMute
                    | StateBindingKey::PipeWireConfiguredSourceAudioStatus
                    | StateBindingKey::PipeWireConfiguredSourceVolume
                    | StateBindingKey::PipeWireConfiguredSourceMuteState
                    | StateBindingKey::PipeWireConfiguredSourceCanSetVolume
                    | StateBindingKey::PipeWireConfiguredSourceCanSetMute
            ) {
                demand.audio_state = true;
            }
        }
        for declaration in self.builtins.declarations() {
            if declaration
                .action
                .is_some_and(|action| action.as_str().starts_with("pipewire.audio."))
            {
                demand.service = true;
                demand.nodes = true;
                demand.defaults = true;
                demand.audio_state = true;
                demand.audio_writes = true;
            }
            match declaration.action {
                Some(ShellAction::PipeWireDefaultsClearPreferredSink) => {
                    demand.service = true;
                    demand.nodes = true;
                    demand.defaults = true;
                    demand.configured_default_writes = true;
                    demand.preferred_sink_writes = true;
                }
                Some(ShellAction::PipeWireDefaultsClearPreferredSource) => {
                    demand.service = true;
                    demand.nodes = true;
                    demand.defaults = true;
                    demand.configured_default_writes = true;
                    demand.preferred_source_writes = true;
                }
                _ => {}
            }
        }
        for repeat in self
            .repeats
            .values()
            .filter(|repeat| repeat.declaration.source == RepeatSource::PipeWireNodes)
        {
            demand.service = true;
            demand.nodes = true;
            for descendant in &repeat.declaration.descendants {
                if matches!(
                    descendant.binding,
                    Some(
                        crate::ItemBindingKey::Ready
                            | crate::ItemBindingKey::NodeState
                            | crate::ItemBindingKey::Direction
                            | crate::ItemBindingKey::Property
                    )
                ) {
                    demand.node_details = true;
                }
                if matches!(
                    descendant.binding,
                    Some(
                        crate::ItemBindingKey::DefaultRole
                            | crate::ItemBindingKey::ConfiguredRole
                            | crate::ItemBindingKey::CanSetPreferredSink
                            | crate::ItemBindingKey::CanSetPreferredSource
                    )
                ) {
                    demand.defaults = true;
                    demand.node_details = true;
                }
                if matches!(
                    descendant.binding,
                    Some(
                        crate::ItemBindingKey::AudioStatus
                            | crate::ItemBindingKey::Volume
                            | crate::ItemBindingKey::MuteState
                            | crate::ItemBindingKey::CanSetVolume
                            | crate::ItemBindingKey::CanSetMute
                            | crate::ItemBindingKey::ChannelCount
                            | crate::ItemBindingKey::ChannelStatus
                    )
                ) {
                    demand.audio_state = true;
                }
                if matches!(
                    descendant.binding,
                    Some(
                        crate::ItemBindingKey::ChannelCount | crate::ItemBindingKey::ChannelStatus
                    )
                ) {
                    demand.channel_projection = true;
                }
                if matches!(
                    descendant.binding,
                    Some(
                        crate::ItemBindingKey::LinkGroupCount
                            | crate::ItemBindingKey::LinkGroupStatus
                    )
                ) {
                    demand.link_collection = true;
                    demand.link_group_collection = true;
                    demand.node_link_tracking = true;
                }
                match descendant.action {
                    Some(ShellAction::PipeWireDefaultsSetPreferredSink) => {
                        demand.defaults = true;
                        demand.node_details = true;
                        demand.configured_default_writes = true;
                        demand.preferred_sink_writes = true;
                    }
                    Some(ShellAction::PipeWireDefaultsSetPreferredSource) => {
                        demand.defaults = true;
                        demand.node_details = true;
                        demand.configured_default_writes = true;
                        demand.preferred_source_writes = true;
                    }
                    Some(_) => {
                        demand.audio_state = true;
                        demand.audio_writes = true;
                    }
                    None => {}
                }
                if let Some(property_key) = &descendant.property_key {
                    demand.property_keys.insert(property_key.clone());
                }
            }
            for contextual in &repeat.declaration.contextual_repeats {
                match contextual.source {
                    crate::ContextualRepeatSource::Channels => {
                        demand.audio_state = true;
                        demand.channel_projection = true;
                    }
                    crate::ContextualRepeatSource::NodeLinkGroups => {
                        demand.link_collection = true;
                        demand.link_group_collection = true;
                        demand.node_link_tracking = true;
                    }
                    crate::ContextualRepeatSource::GroupLinks => {}
                }
                if contextual.descendants.iter().any(|descendant| {
                    descendant
                        .binding
                        .is_some_and(crate::ItemBindingKey::is_relation)
                }) {
                    demand.relation_projection = true;
                    demand.node_details = true;
                }
                if contextual.source == crate::ContextualRepeatSource::Channels
                    && contextual
                        .descendants
                        .iter()
                        .any(|descendant| descendant.range.is_some())
                {
                    demand.audio_writes = true;
                    demand.channel_writes = true;
                }
            }
        }
        for repeat in self.repeats.values().filter(|repeat| {
            matches!(
                repeat.declaration.source,
                RepeatSource::PipeWireLinks | RepeatSource::PipeWireLinkGroups
            )
        }) {
            demand.service = true;
            demand.nodes = true;
            demand.link_collection = true;
            demand.link_details = true;
            if repeat.declaration.source == RepeatSource::PipeWireLinkGroups {
                demand.link_group_collection = true;
            }
            if repeat.declaration.descendants.iter().any(|descendant| {
                descendant
                    .binding
                    .is_some_and(crate::ItemBindingKey::is_relation)
            }) {
                demand.relation_projection = true;
                demand.node_details = true;
            }
            for contextual in &repeat.declaration.contextual_repeats {
                if contextual.source == crate::ContextualRepeatSource::GroupLinks {
                    demand.group_members = true;
                    demand.link_group_collection = true;
                }
                if contextual.descendants.iter().any(|descendant| {
                    descendant
                        .binding
                        .is_some_and(crate::ItemBindingKey::is_relation)
                }) {
                    demand.relation_projection = true;
                    demand.node_details = true;
                }
            }
        }
        let top_monitors = self.peak_monitors.values().collect::<Vec<_>>();
        let repeated_monitors = self
            .repeats
            .values()
            .flat_map(|repeat| repeat.items.values())
            .flat_map(|item| item.peak_monitors.values())
            .collect::<Vec<_>>();
        if !top_monitors.is_empty() || !repeated_monitors.is_empty() {
            demand.service = true;
            demand.nodes = true;
            demand.node_details = true;
            demand.defaults = top_monitors.iter().any(|monitor| {
                matches!(
                    monitor.declaration.target,
                    PeakMonitorTarget::DefaultSink | PeakMonitorTarget::DefaultSource
                )
            });
            demand.peak_monitor_declarations = true;
            for monitor in top_monitors.into_iter().chain(repeated_monitors) {
                demand.peak_maximum_projection |= monitor
                    .declaration
                    .descendants
                    .iter()
                    .any(|element| element.binding == Some(PeakBindingKey::Maximum));
                demand.peak_channel_projection |= !monitor.declaration.channel_repeats.is_empty()
                    || monitor
                        .declaration
                        .descendants
                        .iter()
                        .any(|element| element.binding == Some(PeakBindingKey::ChannelCount));
            }
        }
        demand
    }

    fn initialize_peak_monitor_states(&mut self) -> Result<(), RuntimeError> {
        let identities = self.peak_monitor_identities();
        for identity in identities {
            let mut monitor = self.take_peak_monitor(&identity)?;
            let status = if monitor.enabled {
                PipeWirePeakMonitorStatus::Suspended
            } else {
                PipeWirePeakMonitorStatus::Disabled
            };
            self.update_peak_monitor(&mut monitor, status, None, false)?;
            self.put_peak_monitor(monitor)?;
        }
        Ok(())
    }

    fn peak_monitor_identities(&self) -> Vec<PipeWirePeakMonitorIdentity> {
        self.peak_monitors
            .values()
            .map(|monitor| monitor.identity.clone())
            .chain(
                self.repeats
                    .values()
                    .flat_map(|repeat| repeat.items.values())
                    .flat_map(|item| item.peak_monitors.values())
                    .map(|monitor| monitor.identity.clone()),
            )
            .collect()
    }

    fn enabled_peak_monitor_count(&self) -> usize {
        self.peak_monitors
            .values()
            .filter(|monitor| monitor.enabled)
            .count()
            .saturating_add(
                self.repeats
                    .values()
                    .flat_map(|repeat| repeat.items.values())
                    .flat_map(|item| item.peak_monitors.values())
                    .filter(|monitor| monitor.enabled)
                    .count(),
            )
    }

    fn take_peak_monitor(
        &mut self,
        identity: &PipeWirePeakMonitorIdentity,
    ) -> Result<LivePeakMonitor, RuntimeError> {
        if identity.document_generation != self.document_identity {
            return Err(RuntimeError::InvalidMutationTarget(
                "peak monitor belongs to a stale document generation".into(),
            ));
        }
        match &identity.locator {
            PipeWirePeakMonitorLocator::Element(id) => {
                self.peak_monitors.remove(id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!("peak monitor `#{id}` disappeared"))
                })
            }
            PipeWirePeakMonitorLocator::Repeated {
                repeat_id,
                item_key,
                local_id,
            } => self
                .repeats
                .get_mut(repeat_id)
                .and_then(|repeat| repeat.items.get_mut(item_key))
                .and_then(|item| item.peak_monitors.remove(local_id))
                .ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "repeated peak monitor `{local_id}` disappeared"
                    ))
                }),
        }
    }

    fn put_peak_monitor(&mut self, monitor: LivePeakMonitor) -> Result<(), RuntimeError> {
        match &monitor.identity.locator {
            PipeWirePeakMonitorLocator::Element(id) => {
                self.peak_monitors.insert(id.clone(), monitor);
            }
            PipeWirePeakMonitorLocator::Repeated {
                repeat_id,
                item_key,
                local_id,
            } => {
                let item = self
                    .repeats
                    .get_mut(repeat_id)
                    .and_then(|repeat| repeat.items.get_mut(item_key))
                    .ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(
                            "repeated peak monitor parent disappeared".into(),
                        )
                    })?;
                item.peak_monitors.insert(local_id.clone(), monitor);
            }
        }
        Ok(())
    }

    fn update_peak_monitor(
        &mut self,
        monitor: &mut LivePeakMonitor,
        status: PipeWirePeakMonitorStatus,
        projection: Option<&PipeWirePeakProjection>,
        monitorable: bool,
    ) -> Result<usize, RuntimeError> {
        let mut changed = 0usize;
        if self.apply_attribute_to_node(monitor.root, STATE_ATTRIBUTE, Some(status.as_str()))? {
            changed = changed.saturating_add(1);
        }
        monitor.last_status = status;
        let active = monitor.enabled
            && matches!(
                projection.map(|projection| projection.state),
                Some(PipeWirePeakStreamState::Starting | PipeWirePeakStreamState::Ready)
            );
        let ready = status == PipeWirePeakMonitorStatus::Ready;
        let maximum = if ready {
            projection
                .map(|projection| projection.maximum)
                .unwrap_or(NumericValue::Unknown)
        } else {
            NumericValue::Unknown
        };
        let channel_count = if ready {
            projection
                .map(|projection| projection.channels.items.len() as i64)
                .unwrap_or(0)
        } else {
            0
        };
        for element in &mut monitor.elements {
            if let Some(binding) = element.declaration.binding {
                let element_changed = match element.declaration.kind {
                    BuiltInElementKind::StateText => {
                        let value = match binding {
                            PeakBindingKey::Status => peak_status_text(status),
                            PeakBindingKey::Enabled => bool_text(monitor.enabled),
                            PeakBindingKey::Active => bool_text(active),
                            PeakBindingKey::CanEnable => bool_text(!monitor.enabled && monitorable),
                            PeakBindingKey::CanDisable => bool_text(monitor.enabled),
                            PeakBindingKey::Maximum | PeakBindingKey::ChannelCount => {
                                return Err(RuntimeError::InvalidMutationTarget(
                                    "numeric peak binding reached a text element".into(),
                                ));
                            }
                        };
                        self.apply_text_to_node(element.node, &value)?
                    }
                    BuiltInElementKind::StateToken => {
                        let value = match binding {
                            PeakBindingKey::Status => status.as_str(),
                            PeakBindingKey::Enabled => {
                                if monitor.enabled {
                                    "true"
                                } else {
                                    "false"
                                }
                            }
                            PeakBindingKey::Active => {
                                if active {
                                    "true"
                                } else {
                                    "false"
                                }
                            }
                            PeakBindingKey::CanEnable => {
                                if !monitor.enabled && monitorable {
                                    "true"
                                } else {
                                    "false"
                                }
                            }
                            PeakBindingKey::CanDisable => {
                                if monitor.enabled {
                                    "true"
                                } else {
                                    "false"
                                }
                            }
                            PeakBindingKey::Maximum | PeakBindingKey::ChannelCount => {
                                return Err(RuntimeError::InvalidMutationTarget(
                                    "numeric peak binding reached a token element".into(),
                                ));
                            }
                        };
                        self.apply_attribute_to_node(element.node, STATE_ATTRIBUTE, Some(value))?
                    }
                    BuiltInElementKind::StateValue => {
                        let value = match binding {
                            PeakBindingKey::Maximum => maximum,
                            PeakBindingKey::ChannelCount => NumericValue::Integer(channel_count),
                            _ => {
                                return Err(RuntimeError::InvalidMutationTarget(
                                    "text peak binding reached a value element".into(),
                                ));
                            }
                        };
                        let format = element
                            .declaration
                            .value_format
                            .unwrap_or(StateValueFormat::Raw);
                        let formatted = if binding == PeakBindingKey::Maximum {
                            value.format_volume(format)
                        } else {
                            value.format(format)
                        }
                        .map_err(|error| {
                            RuntimeError::InvalidMutationTarget(format!(
                                "peak value formatting failed: {error}"
                            ))
                        })?;
                        let text_changed =
                            self.apply_text_to_node(element.node, &formatted.display)?;
                        let value_changed = self.apply_attribute_to_node(
                            element.node,
                            "value",
                            formatted.value.as_deref(),
                        )?;
                        text_changed || value_changed
                    }
                    _ => false,
                };
                changed = changed.saturating_add(usize::from(element_changed));
            }
            if let Some(action) = element.declaration.action {
                let effective_disabled = element.declaration.disabled
                    || match action {
                        ShellAction::PipeWirePeaksEnable => monitor.enabled || !monitorable,
                        ShellAction::PipeWirePeaksDisable => !monitor.enabled,
                        ShellAction::PipeWirePeaksToggle => !monitor.enabled && !monitorable,
                        _ => true,
                    };
                let disabled_changed = self.apply_attribute_to_node(
                    element.node,
                    "disabled",
                    effective_disabled.then_some(""),
                )?;
                element.control_state = match status {
                    PipeWirePeakMonitorStatus::Disabled
                    | PipeWirePeakMonitorStatus::Suspended
                    | PipeWirePeakMonitorStatus::Ready => PipeWireControlState::Idle,
                    PipeWirePeakMonitorStatus::Unavailable => PipeWireControlState::Unavailable,
                    PipeWirePeakMonitorStatus::Failed
                        if element.control_state == PipeWireControlState::Pending =>
                    {
                        PipeWireControlState::Failed
                    }
                    PipeWirePeakMonitorStatus::Starting | PipeWirePeakMonitorStatus::Failed => {
                        element.control_state
                    }
                };
                let state_changed = self.apply_attribute_to_node(
                    element.node,
                    STATE_ATTRIBUTE,
                    Some(element.control_state.as_str()),
                )?;
                changed = changed.saturating_add(usize::from(disabled_changed || state_changed));
            }
        }
        let empty = crate::ContextualRepeatSnapshot {
            source_generation: projection
                .map(|projection| projection.layout_generation)
                .unwrap_or_else(|| {
                    monitor
                        .channel_repeats
                        .values()
                        .map(|repeat| repeat.source_generation)
                        .max()
                        .unwrap_or(0)
                }),
            items: Vec::new(),
        };
        let channels = if ready {
            projection.map(|projection| &projection.channels)
        } else {
            None
        };
        for repeat in monitor.channel_repeats.values_mut() {
            changed = changed.saturating_add(
                self.reconcile_contextual_repeat(repeat, channels.unwrap_or(&empty))?,
            );
        }
        Ok(changed)
    }

    pub fn element_identity(&self, html_id: &str) -> Result<ElementInstanceId, RuntimeError> {
        self.builtins
            .element(html_id)
            .map(|declaration| declaration.id.clone())
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "registered element `#{html_id}` does not exist"
                ))
            })
    }

    pub fn validate_element_identity(
        &self,
        identity: &ElementInstanceId,
    ) -> Result<(), RuntimeError> {
        if identity.document_generation != self.document_identity {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{}` belongs to a stale document generation",
                identity.html_id
            )));
        }
        let current = self.element_identity(&identity.html_id)?;
        if current != *identity {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{}` is stale",
                identity.html_id
            )));
        }
        Ok(())
    }

    pub fn element_text(&self, html_id: &str) -> Result<String, RuntimeError> {
        let node = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` does not exist"
            ))
        })?;
        let slot = self.identities.resolve(&self.document, node)?;
        self.document
            .get_node(slot)
            .map(|node| node.text_content())
            .ok_or(RuntimeError::StaleIdentity {
                slot: node.slot,
                generation: node.generation,
            })
    }

    pub fn element_state_token(&self, html_id: &str) -> Result<String, RuntimeError> {
        let node = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` does not exist"
            ))
        })?;
        let slot = self.identities.resolve(&self.document, node)?;
        self.document
            .get_node(slot)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(LocalName::from(STATE_ATTRIBUTE)))
            .map(str::to_owned)
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "registered token element `#{html_id}` has no runtime state"
                ))
            })
    }

    fn registered_attribute(
        &self,
        html_id: &str,
        attribute: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let node = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` does not exist"
            ))
        })?;
        let slot = self.identities.resolve(&self.document, node)?;
        Ok(self
            .document
            .get_node(slot)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(LocalName::from(attribute)))
            .map(str::to_owned))
    }

    pub fn element_bounds(&self, html_id: &str) -> Result<LogicalRect, RuntimeError> {
        let node = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` does not exist"
            ))
        })?;
        let slot = self.identities.resolve(&self.document, node)?;
        let bounds =
            self.document
                .get_node(slot)
                .map(node_bounds)
                .ok_or(RuntimeError::StaleIdentity {
                    slot: node.slot,
                    generation: node.generation,
                })?;
        validate_rect(&bounds)?;
        Ok(bounds)
    }

    pub fn apply_bound_text(
        &mut self,
        values: &[(StateBindingKey, String)],
    ) -> Result<BindingUpdate, RuntimeError> {
        self.apply_bound_state(values, &[])
    }

    pub fn apply_bound_tokens(
        &mut self,
        values: &[(StateBindingKey, StateToken)],
    ) -> Result<BindingUpdate, RuntimeError> {
        self.apply_bound_state(&[], values)
    }

    pub fn apply_bound_state(
        &mut self,
        text_values: &[(StateBindingKey, String)],
        token_values: &[(StateBindingKey, StateToken)],
    ) -> Result<BindingUpdate, RuntimeError> {
        let projection_started = Instant::now();
        let mut seen = std::collections::BTreeSet::new();
        let mut pending_text = Vec::new();
        let mut pending_tokens = Vec::new();
        let mut changed_keys = std::collections::BTreeSet::new();
        let mut update = BindingUpdate::default();
        for (key, value) in text_values {
            if !key.supports(StateValueKind::Text) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "binding `{}` does not support text presentation",
                    key.as_str()
                )));
            }
            if !seen.insert((*key, StateValueKind::Text)) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "text binding `{}` was supplied more than once",
                    key.as_str()
                )));
            }
            let targets = self.builtins.binding_targets(*key, StateValueKind::Text);
            if targets.is_empty() {
                continue;
            }
            if self
                .builtins
                .binding_is_unchanged(*key, StateValueKind::Text, value)
            {
                update.suppressed_keys = update.suppressed_keys.saturating_add(1);
                continue;
            }
            let targets = targets.to_vec();
            for html_id in &targets {
                let identity = self.builtins.indexed_node(html_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "binding target `#{html_id}` disappeared"
                    ))
                })?;
                self.identities.resolve(&self.document, identity)?;
            }
            pending_text.push((*key, value.clone(), targets));
        }
        for (key, token) in token_values {
            if !key.supports(StateValueKind::Token) || !token.valid_for(*key) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "token `{}` is invalid for binding `{}`",
                    token.as_str(),
                    key.as_str()
                )));
            }
            if !seen.insert((*key, StateValueKind::Token)) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "token binding `{}` was supplied more than once",
                    key.as_str()
                )));
            }
            let targets = self.builtins.binding_targets(*key, StateValueKind::Token);
            if targets.is_empty() {
                continue;
            }
            let value = token.as_str();
            if self
                .builtins
                .binding_is_unchanged(*key, StateValueKind::Token, value)
            {
                update.suppressed_keys = update.suppressed_keys.saturating_add(1);
                self.measurements.suppressed_token_updates =
                    self.measurements.suppressed_token_updates.saturating_add(1);
                continue;
            }
            let targets = targets.to_vec();
            for html_id in &targets {
                let identity = self.builtins.indexed_node(html_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "token binding target `#{html_id}` disappeared"
                    ))
                })?;
                self.identities.resolve(&self.document, identity)?;
            }
            pending_tokens.push((*key, *token, targets));
        }
        for (key, value, targets) in pending_text {
            for html_id in &targets {
                self.set_registered_text(html_id, &value)?;
                update.changed_elements = update.changed_elements.saturating_add(1);
                update.changed_text_elements = update.changed_text_elements.saturating_add(1);
            }
            self.builtins
                .record_binding(key, StateValueKind::Text, value);
            changed_keys.insert(key);
        }
        let attribute_started = Instant::now();
        for (key, token, targets) in pending_tokens {
            for html_id in &targets {
                self.set_registered_token(html_id, token)?;
                update.changed_elements = update.changed_elements.saturating_add(1);
                update.changed_token_elements = update.changed_token_elements.saturating_add(1);
            }
            self.builtins
                .record_binding(key, StateValueKind::Token, token.as_str().to_owned());
            changed_keys.insert(key);
        }
        update.changed_keys = changed_keys.len();
        self.measurements.last_attribute_mutation_ms = elapsed_ms(attribute_started);
        self.measurements.changed_token_updates = self
            .measurements
            .changed_token_updates
            .saturating_add(update.changed_token_elements as u64);
        self.measurements.suppressed_binding_updates = self
            .measurements
            .suppressed_binding_updates
            .saturating_add(update.suppressed_keys as u64);
        if update.changed_elements > 0 {
            self.resolve();
        }
        self.measurements.last_state_projection_ms = elapsed_ms(projection_started);
        Ok(update)
    }

    pub fn apply_bound_values(
        &mut self,
        values: &[(StateBindingKey, NumericValue)],
    ) -> Result<BindingUpdate, RuntimeError> {
        let started = Instant::now();
        let mut seen = BTreeSet::new();
        let mut update = BindingUpdate::default();
        for (key, value) in values {
            if !key.supports(StateValueKind::Value) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "binding `{}` does not support numeric presentation",
                    key.as_str()
                )));
            }
            if !seen.insert(*key) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "numeric binding `{}` was supplied more than once",
                    key.as_str()
                )));
            }
            let targets = self
                .builtins
                .binding_targets(*key, StateValueKind::Value)
                .to_vec();
            for html_id in targets {
                let declaration = self.builtins.element(&html_id).cloned().ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "numeric binding target `#{html_id}` disappeared"
                    ))
                })?;
                let format = declaration.value_format.ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "numeric binding target `#{html_id}` has no format"
                    ))
                })?;
                let formatted = if is_pipewire_volume_key(*key) {
                    value.format_volume(format)
                } else {
                    value.format(format)
                }
                .map_err(|error| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "numeric binding `{}` could not be formatted: {error}",
                        key.as_str()
                    ))
                })?;
                let node = self.builtins.indexed_node(&html_id).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "numeric binding target `#{html_id}` disappeared"
                    ))
                })?;
                let changed = if declaration.kind == BuiltInElementKind::RangeControl {
                    let range = declaration.range.ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "range control `#{html_id}` has no validated bounds"
                        ))
                    })?;
                    let visual = value
                        .as_f64()
                        .map(|value| value.clamp(range.minimum.get(), range.maximum.get()));
                    let visual = visual
                        .map(|value| NumericValue::Decimal(value).format(StateValueFormat::Raw))
                        .transpose()
                        .map_err(|error| {
                            RuntimeError::InvalidMutationTarget(format!(
                                "range control `#{html_id}` value could not be formatted: {error}"
                            ))
                        })?
                        .and_then(|value| value.value);
                    self.record_range_authoritative(node, visual.clone());
                    if self.node_attribute(node, STATE_ATTRIBUTE)?.as_deref() == Some("pending") {
                        false
                    } else {
                        self.apply_attribute_to_node(node, "value", visual.as_deref())?
                    }
                } else {
                    self.apply_value_to_node(node, &formatted.display, formatted.value.as_deref())?
                };
                if changed {
                    update.changed_elements = update.changed_elements.saturating_add(1);
                    update.changed_value_elements = update.changed_value_elements.saturating_add(1);
                }
            }
        }
        if update.changed_elements > 0 {
            self.resolve();
        }
        self.measurements.last_attribute_mutation_ms = elapsed_ms(started);
        Ok(update)
    }

    pub fn apply_bound_booleans(
        &mut self,
        values: &[(StateBindingKey, Option<bool>)],
    ) -> Result<BindingUpdate, RuntimeError> {
        let started = Instant::now();
        let mut seen = BTreeSet::new();
        let mut update = BindingUpdate::default();
        for (key, value) in values {
            if !key.supports(StateValueKind::Boolean) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "binding `{}` does not support Boolean presentation",
                    key.as_str()
                )));
            }
            if !seen.insert(*key) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "Boolean binding `{}` was supplied more than once",
                    key.as_str()
                )));
            }
            let targets = self
                .builtins
                .binding_targets(*key, StateValueKind::Boolean)
                .to_vec();
            for html_id in targets {
                let declaration = self.builtins.element(&html_id).cloned().ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "Boolean binding target `#{html_id}` disappeared"
                    ))
                })?;
                if declaration.kind == BuiltInElementKind::RangeControl
                    || declaration
                        .action
                        .is_some_and(|action| action.as_str().starts_with("pipewire."))
                {
                    let node = self.builtins.indexed_node(&html_id).ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "control `#{html_id}` disappeared"
                        ))
                    })?;
                    if self.apply_control_availability(
                        node,
                        declaration.disabled,
                        *value == Some(true),
                    )? {
                        update.changed_elements = update.changed_elements.saturating_add(1);
                        update.changed_boolean_elements =
                            update.changed_boolean_elements.saturating_add(1);
                    }
                    continue;
                }
                let disabled = declaration.disabled || *value != Some(true);
                let current = self.registered_attribute(&html_id, "disabled")?.is_some();
                if current == disabled {
                    continue;
                }
                if disabled {
                    self.set_registered_attribute(&html_id, "disabled", "")?;
                } else {
                    self.clear_registered_attribute(&html_id, "disabled")?;
                }
                update.changed_elements = update.changed_elements.saturating_add(1);
                update.changed_boolean_elements = update.changed_boolean_elements.saturating_add(1);
            }
        }
        if update.changed_elements > 0 {
            self.resolve();
        }
        self.measurements.last_attribute_mutation_ms = elapsed_ms(started);
        Ok(update)
    }

    pub fn apply_repeat_source(
        &mut self,
        snapshot: &RepeatSourceSnapshot,
    ) -> Result<RepeatMutation, RuntimeError> {
        let started = Instant::now();
        if snapshot.items.len() > MAX_ITEMS_PER_REPEAT {
            return Err(RuntimeError::LimitExceeded(format!(
                "repeat source `{}` has {} items; limit is {MAX_ITEMS_PER_REPEAT}",
                snapshot.source.as_str(),
                snapshot.items.len()
            )));
        }
        let mut keys = BTreeSet::new();
        for item in &snapshot.items {
            if item.key.is_empty() || !keys.insert(item.key.clone()) {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "repeat source `{}` contains an empty or duplicate item key",
                    snapshot.source.as_str()
                )));
            }
        }
        let repeat_ids: Vec<_> = self
            .repeats
            .iter()
            .filter(|(_, repeat)| repeat.declaration.source == snapshot.source)
            .map(|(id, _)| id.clone())
            .collect();
        let mut projected_document_nodes = self.total_repeated_nodes();
        for repeat_id in &repeat_ids {
            let repeat = &self.repeats[repeat_id];
            let current = Self::repeat_node_count(repeat)?;
            let desired = Self::desired_repeat_node_count(&repeat.declaration, snapshot)?;
            if desired > MAX_CLONED_NODES_PER_REPEAT {
                return Err(RuntimeError::LimitExceeded(format!(
                    "repeat `#{repeat_id}` would clone {desired} nodes; the per-repeat limit is {MAX_CLONED_NODES_PER_REPEAT}"
                )));
            }
            projected_document_nodes = projected_document_nodes
                .checked_sub(current)
                .and_then(|nodes| nodes.checked_add(desired))
                .ok_or_else(|| {
                    RuntimeError::LimitExceeded("projected repeat node count overflow".into())
                })?;
        }
        if projected_document_nodes > MAX_CLONED_NODES_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "repeat source `{}` would exceed the document node limit of {MAX_CLONED_NODES_PER_DOCUMENT}",
                snapshot.source.as_str()
            )));
        }
        let mut total = RepeatMutation::default();
        for repeat_id in repeat_ids {
            let update = self.reconcile_repeat(&repeat_id, snapshot)?;
            total.insertions = total.insertions.saturating_add(update.insertions);
            total.removals = total.removals.saturating_add(update.removals);
            total.moves = total.moves.saturating_add(update.moves);
            total.property_updates = total
                .property_updates
                .saturating_add(update.property_updates);
            total.unchanged_items = total.unchanged_items.saturating_add(update.unchanged_items);
            total.subtree_clones = total.subtree_clones.saturating_add(update.subtree_clones);
            total.identity_reuses = total.identity_reuses.saturating_add(update.identity_reuses);
        }
        if total.changed() {
            self.resolve();
        }
        self.measurements.repeat_insertions = self
            .measurements
            .repeat_insertions
            .saturating_add(total.insertions as u64);
        self.measurements.repeat_removals = self
            .measurements
            .repeat_removals
            .saturating_add(total.removals as u64);
        self.measurements.repeat_moves = self
            .measurements
            .repeat_moves
            .saturating_add(total.moves as u64);
        self.measurements.repeat_property_updates = self
            .measurements
            .repeat_property_updates
            .saturating_add(total.property_updates as u64);
        self.measurements.repeat_unchanged_items = self
            .measurements
            .repeat_unchanged_items
            .saturating_add(total.unchanged_items as u64);
        self.measurements.repeat_subtree_clones = self
            .measurements
            .repeat_subtree_clones
            .saturating_add(total.subtree_clones as u64);
        self.measurements.repeat_identity_reuses = self
            .measurements
            .repeat_identity_reuses
            .saturating_add(total.identity_reuses as u64);
        match snapshot.source {
            RepeatSource::PipeWireLinks => {
                self.measurements.link_insertions = self
                    .measurements
                    .link_insertions
                    .saturating_add(total.insertions as u64);
                self.measurements.link_removals = self
                    .measurements
                    .link_removals
                    .saturating_add(total.removals as u64);
                self.measurements.link_moves = self
                    .measurements
                    .link_moves
                    .saturating_add(total.moves as u64);
                self.measurements.link_state_mutations = self
                    .measurements
                    .link_state_mutations
                    .saturating_add(total.property_updates as u64);
                self.measurements.link_relation_mutations = self
                    .measurements
                    .link_relation_mutations
                    .saturating_add(total.property_updates as u64);
                self.measurements.retained_link_identities = self
                    .measurements
                    .retained_link_identities
                    .saturating_add(total.identity_reuses as u64);
                self.measurements.duplicate_graph_suppressions = self
                    .measurements
                    .duplicate_graph_suppressions
                    .saturating_add(total.unchanged_items as u64);
            }
            RepeatSource::PipeWireLinkGroups => {
                self.measurements.group_insertions = self
                    .measurements
                    .group_insertions
                    .saturating_add(total.insertions as u64);
                self.measurements.group_removals = self
                    .measurements
                    .group_removals
                    .saturating_add(total.removals as u64);
                self.measurements.group_state_mutations = self
                    .measurements
                    .group_state_mutations
                    .saturating_add(total.property_updates as u64);
                self.measurements.retained_group_identities = self
                    .measurements
                    .retained_group_identities
                    .saturating_add(total.identity_reuses as u64);
                self.measurements.duplicate_graph_suppressions = self
                    .measurements
                    .duplicate_graph_suppressions
                    .saturating_add(total.unchanged_items as u64);
            }
            _ => {}
        }
        self.measurements.repeated_item_count = self
            .repeats
            .values()
            .map(|repeat| repeat.items.len() as u64)
            .sum();
        self.measurements.contextual_item_count = self
            .repeats
            .values()
            .flat_map(|repeat| repeat.items.values())
            .flat_map(|item| item.contextual_repeats.values())
            .map(|repeat| repeat.items.len() as u64)
            .sum();
        self.measurements.cloned_node_count = self.total_repeated_nodes() as u64;
        self.measurements.last_reconciliation_ms = elapsed_ms(started);
        Ok(total)
    }

    pub fn update_panel_state(
        &mut self,
        overlay_open: bool,
        last_action: &str,
    ) -> Result<bool, RuntimeError> {
        if self.kind != LiveDocumentKind::Panel {
            return Err(RuntimeError::InvalidMutationTarget(
                "panel state can only update a panel document".into(),
            ));
        }
        if self.has_built_in_elements() {
            return Ok(false);
        }
        let text = if overlay_open {
            format!("Overlay open · {last_action}")
        } else {
            format!("Overlay closed · {last_action}")
        };
        self.set_text("#panel-status", &text)?;
        self.set_class(
            "#panel-root",
            if overlay_open { "panel open" } else { "panel" },
        )?;
        self.resolve();
        Ok(true)
    }

    pub fn update_overlay_state(
        &mut self,
        activation_count: u64,
        last_action: &str,
    ) -> Result<bool, RuntimeError> {
        if self.kind != LiveDocumentKind::TransientOverlay {
            return Err(RuntimeError::InvalidMutationTarget(
                "overlay state can only update a transient-overlay document".into(),
            ));
        }
        if self.has_built_in_elements() {
            return Ok(false);
        }
        self.set_text(
            "#overlay-status",
            &format!("Activated {activation_count} time(s) · {last_action}"),
        )?;
        self.set_class(
            "#overlay-card",
            if activation_count == 0 {
                "overlay-card"
            } else {
                "overlay-card activated"
            },
        )?;
        self.resolve();
        Ok(true)
    }

    pub fn set_instance_context(
        &mut self,
        template_id: &str,
        output_label: &str,
    ) -> Result<bool, RuntimeError> {
        if self.kind == LiveDocumentKind::SingleOverlay {
            return Err(RuntimeError::InvalidMutationTarget(
                "instance context is only available to multi-surface fixtures".into(),
            ));
        }
        if self.has_built_in_elements() {
            return Ok(false);
        }
        let mut changed = false;
        for (selector, value) in [
            ("#surface-id-label", template_id),
            ("#output-label", output_label),
        ] {
            if self
                .document
                .query_selector(selector)
                .map_err(|error| RuntimeError::InvalidMutationTarget(format!("{error:?}")))?
                .is_some()
            {
                self.set_text(selector, value)?;
                changed = true;
            }
        }
        if changed {
            self.resolve();
        }
        Ok(changed)
    }

    fn apply_click_mutation(&mut self) -> Result<(), RuntimeError> {
        self.set_text(
            "#status-label",
            &format!("Activated {} time(s)", self.click_count),
        )?;
        self.set_class("#shell-card", "shell-card activated")?;
        Ok(())
    }

    fn set_text(&mut self, selector: &str, value: &str) -> Result<(), RuntimeError> {
        let parent = required_selector(&self.document, selector)?;
        let text = self
            .document
            .get_node(parent)
            .and_then(|node| {
                node.children.iter().copied().find(|child| {
                    self.document
                        .get_node(*child)
                        .is_some_and(|node| matches!(node.data, NodeData::Text(_)))
                })
            })
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "{selector} does not contain a text node"
                ))
            })?;
        self.document.mutate().set_node_text(text, value);
        Ok(())
    }

    fn set_class(&mut self, selector: &str, value: &str) -> Result<(), RuntimeError> {
        let node = required_selector(&self.document, selector)?;
        self.document.mutate().set_attribute(
            node,
            QualName {
                prefix: None,
                ns: ns!(),
                local: local_name!("class"),
            },
            value,
        );
        Ok(())
    }

    fn reconcile_repeat(
        &mut self,
        repeat_id: &str,
        snapshot: &RepeatSourceSnapshot,
    ) -> Result<RepeatMutation, RuntimeError> {
        let mut repeat = self.repeats.remove(repeat_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "repeat declaration `#{repeat_id}` disappeared"
            ))
        })?;
        let result = self.reconcile_repeat_inner(&mut repeat, snapshot);
        self.repeats.insert(repeat_id.to_owned(), repeat);
        result
    }

    fn reconcile_repeat_inner(
        &mut self,
        repeat: &mut LiveRepeat,
        snapshot: &RepeatSourceSnapshot,
    ) -> Result<RepeatMutation, RuntimeError> {
        if repeat.declaration.source != snapshot.source {
            return Err(RuntimeError::InvalidMutationTarget(
                "repeat source does not match its declaration".into(),
            ));
        }
        if snapshot.source_generation < repeat.source_generation {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "stale `{}` source generation {} follows {}",
                snapshot.source.as_str(),
                snapshot.source_generation,
                repeat.source_generation
            )));
        }
        let desired_repeat_nodes = Self::desired_repeat_node_count(&repeat.declaration, snapshot)?;
        if desired_repeat_nodes > MAX_CLONED_NODES_PER_REPEAT {
            return Err(RuntimeError::LimitExceeded(format!(
                "repeat clone would exceed the per-repeat node limit of {MAX_CLONED_NODES_PER_REPEAT}"
            )));
        }
        let desired_document_nodes = self
            .total_repeated_nodes()
            .checked_add(desired_repeat_nodes)
            .ok_or_else(|| RuntimeError::LimitExceeded("repeated node count overflow".into()))?;
        if desired_document_nodes > MAX_CLONED_NODES_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "repeat clone would exceed the document node limit of {MAX_CLONED_NODES_PER_DOCUMENT}"
            )));
        }
        if repeat.declaration.source == RepeatSource::PipeWireNodes {
            let source_replacement = repeat.source_generation != 0
                && repeat.source_generation != snapshot.source_generation;
            let authored_enabled = repeat
                .declaration
                .peak_monitors
                .iter()
                .filter(|monitor| monitor.enabled)
                .count();
            let repeated_enabled = snapshot.items.iter().try_fold(0usize, |count, item| {
                let item_enabled = if source_replacement {
                    authored_enabled
                } else {
                    repeat
                        .items
                        .get(&item.key)
                        .map_or(authored_enabled, |item| {
                            item.peak_monitors
                                .values()
                                .filter(|monitor| monitor.enabled)
                                .count()
                        })
                };
                count.checked_add(item_enabled).ok_or_else(|| {
                    RuntimeError::LimitExceeded("enabled peak monitor count overflow".into())
                })
            })?;
            let enabled = self
                .enabled_peak_monitor_count()
                .checked_add(repeated_enabled)
                .ok_or_else(|| {
                    RuntimeError::LimitExceeded("enabled peak monitor count overflow".into())
                })?;
            if enabled > crate::MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT {
                return Err(RuntimeError::LimitExceeded(format!(
                    "repeat projection would exceed the document limit of {} enabled peak monitors",
                    crate::MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT
                )));
            }
        }
        let mut update = RepeatMutation::default();
        if repeat.source_generation != 0 && repeat.source_generation != snapshot.source_generation {
            let old_keys = repeat.order.clone();
            for key in old_keys {
                self.remove_repeat_item(repeat, &key)?;
                update.removals = update.removals.saturating_add(1);
            }
        }
        repeat.source_generation = snapshot.source_generation;
        let desired_keys: BTreeSet<_> = snapshot
            .items
            .iter()
            .map(|item| item.key.as_str())
            .collect();
        let removed: Vec<_> = repeat
            .order
            .iter()
            .filter(|key| !desired_keys.contains(key.as_str()))
            .cloned()
            .collect();
        for key in removed {
            self.remove_repeat_item(repeat, &key)?;
            update.removals = update.removals.saturating_add(1);
        }

        for item in &snapshot.items {
            if repeat.items.contains_key(&item.key) {
                update.identity_reuses = update.identity_reuses.saturating_add(1);
                let changed = self.update_repeat_item(repeat, item)?;
                if changed == 0 {
                    update.unchanged_items = update.unchanged_items.saturating_add(1);
                } else {
                    update.property_updates = update.property_updates.saturating_add(changed);
                }
            } else {
                self.insert_repeat_item(repeat, item)?;
                update.insertions = update.insertions.saturating_add(1);
                update.subtree_clones = update.subtree_clones.saturating_add(1);
            }
        }
        let desired_order: Vec<_> = snapshot.items.iter().map(|item| item.key.clone()).collect();
        if repeat.order != desired_order {
            update.moves = desired_order
                .iter()
                .enumerate()
                .filter(|(index, key)| repeat.order.get(*index) != Some(*key))
                .count();
            let template = self
                .identities
                .resolve(&self.document, repeat.declaration.template_node)?;
            for key in &desired_order {
                let root = repeat.items.get(key).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "repeat item `{key}` disappeared during reorder"
                    ))
                })?;
                let slot = self.identities.resolve(&self.document, root.root)?;
                self.document
                    .mutate()
                    .insert_nodes_before(template, &[slot]);
            }
            repeat.order = desired_order;
        }
        Ok(update)
    }

    fn total_repeated_nodes(&self) -> usize {
        self.repeats
            .values()
            .map(|repeat| Self::repeat_node_count(repeat).unwrap_or(usize::MAX))
            .fold(0usize, usize::saturating_add)
    }

    fn repeat_node_count(repeat: &LiveRepeat) -> Result<usize, RuntimeError> {
        let outer = repeat
            .items
            .len()
            .checked_mul(repeat.declaration.prototype_nodes)
            .ok_or_else(|| RuntimeError::LimitExceeded("repeat node count overflow".into()))?;
        repeat
            .items
            .values()
            .flat_map(|item| item.contextual_repeats.values())
            .try_fold(outer, |nodes, nested| {
                nested
                    .items
                    .len()
                    .checked_mul(nested.declaration.prototype_nodes)
                    .and_then(|nested_nodes| nodes.checked_add(nested_nodes))
                    .ok_or_else(|| {
                        RuntimeError::LimitExceeded("contextual repeat node count overflow".into())
                    })
            })
    }

    fn desired_repeat_node_count(
        declaration: &RepeatDeclaration,
        snapshot: &RepeatSourceSnapshot,
    ) -> Result<usize, RuntimeError> {
        let outer = snapshot
            .items
            .len()
            .checked_mul(declaration.prototype_nodes)
            .ok_or_else(|| RuntimeError::LimitExceeded("repeat node count overflow".into()))?;
        snapshot.items.iter().try_fold(outer, |nodes, item| {
            declaration
                .contextual_repeats
                .iter()
                .try_fold(nodes, |nodes, nested| {
                    let item_count = item
                        .contextual(nested.source)
                        .map_or(0, |snapshot| snapshot.items.len());
                    if item_count > nested.source.item_limit() {
                        return Err(RuntimeError::LimitExceeded(format!(
                            "`{}` for item `{}` has {item_count} items; limit is {}",
                            nested.source.as_str(),
                            item.key,
                            nested.source.item_limit()
                        )));
                    }
                    item_count
                        .checked_mul(nested.prototype_nodes)
                        .and_then(|nested_nodes| nodes.checked_add(nested_nodes))
                        .ok_or_else(|| {
                            RuntimeError::LimitExceeded(
                                "contextual repeat node count overflow".into(),
                            )
                        })
                })
        })
    }

    fn insert_repeat_item(
        &mut self,
        repeat: &mut LiveRepeat,
        item: &RepeatItemSnapshot,
    ) -> Result<(), RuntimeError> {
        let prototype = self
            .identities
            .resolve(&self.document, repeat.declaration.root_node)?;
        let clone = self.document.mutate().deep_clone_node(prototype);
        let clone_slots = subtree_slots(&self.document, clone)?;
        if clone_slots.len() != repeat.declaration.prototype_nodes {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "repeat `#{}` cloned {} nodes; expected {}",
                repeat.declaration.id.html_id,
                clone_slots.len(),
                repeat.declaration.prototype_nodes
            )));
        }
        let mut clone_identities = Vec::with_capacity(clone_slots.len());
        for slot in &clone_slots {
            clone_identities.push(self.identities.activate_created(&self.document, *slot)?);
        }
        let template = self
            .identities
            .resolve(&self.document, repeat.declaration.template_node)?;
        self.document
            .mutate()
            .insert_nodes_before(template, &[clone]);
        let elements = repeat
            .declaration
            .descendants
            .iter()
            .map(|declaration| {
                let node = clone_identities
                    .get(declaration.prototype_order)
                    .copied()
                    .ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "repeat local id `{}` has an invalid prototype position",
                            declaration.local_id
                        ))
                    })?;
                Ok(LiveRepeatedElement {
                    declaration: declaration.clone(),
                    node,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let mut contextual_repeats = BTreeMap::new();
        for declaration in &repeat.declaration.contextual_repeats {
            let template_node = clone_identities
                .get(declaration.template_prototype_order)
                .copied()
                .ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "contextual repeat `{}` has an invalid prototype position",
                        declaration.id
                    ))
                })?;
            let template_slot = self.identities.resolve(&self.document, template_node)?;
            let prototype_root_slot = template_root_element(&self.document, template_slot)?;
            let prototype_root = self
                .identities
                .identity_for_slot(&self.document, prototype_root_slot)?;
            contextual_repeats.insert(
                declaration.id.clone(),
                LiveContextualRepeat {
                    declaration: declaration.clone(),
                    template_node,
                    prototype_root,
                    source_generation: 0,
                    items: BTreeMap::new(),
                    order: Vec::new(),
                },
            );
        }
        let mut peak_monitors = BTreeMap::new();
        for declaration in &repeat.declaration.peak_monitors {
            let identity = PipeWirePeakMonitorIdentity {
                document_generation: self.document_identity,
                locator: PipeWirePeakMonitorLocator::Repeated {
                    repeat_id: repeat.declaration.id.html_id.clone(),
                    item_key: item.key.clone(),
                    local_id: declaration.id.clone(),
                },
            };
            let mut monitor = instantiate_repeated_peak_monitor(
                &self.document,
                &self.identities,
                declaration.clone(),
                &clone_identities,
                identity,
            )?;
            let status = if monitor.enabled {
                PipeWirePeakMonitorStatus::Suspended
            } else {
                PipeWirePeakMonitorStatus::Disabled
            };
            self.update_peak_monitor(&mut monitor, status, None, false)?;
            peak_monitors.insert(declaration.id.clone(), monitor);
        }
        repeat.items.insert(
            item.key.clone(),
            LiveRepeatedItem {
                root: clone_identities[0],
                elements,
                contextual_repeats,
                peak_monitors,
            },
        );
        repeat.order.push(item.key.clone());
        self.update_repeat_item(repeat, item)?;
        Ok(())
    }

    fn remove_repeat_item(
        &mut self,
        repeat: &mut LiveRepeat,
        key: &str,
    ) -> Result<(), RuntimeError> {
        let Some(item) = repeat.items.remove(key) else {
            return Ok(());
        };
        let slots = self.identities.subtree_slots(&self.document, item.root)?;
        if self
            .pressed_range
            .as_ref()
            .is_some_and(|range| slots.contains(&range.node.slot))
        {
            self.pressed_range = None;
            self.pending_action = None;
        }
        let root = self.identities.resolve(&self.document, item.root)?;
        self.document.mutate().remove_and_drop_node(root);
        self.identities.retire_removed(&self.document, &slots)?;
        repeat.order.retain(|current| current != key);
        Ok(())
    }

    fn update_repeat_item(
        &mut self,
        repeat: &mut LiveRepeat,
        item: &RepeatItemSnapshot,
    ) -> Result<usize, RuntimeError> {
        let live = repeat.items.get_mut(&item.key).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("repeat item `{}` disappeared", item.key))
        })?;
        let elements = live.elements.clone();
        let mut changed = self.update_repeated_elements(&elements, item)?;
        if !live.contextual_repeats.is_empty() {
            for contextual in live.contextual_repeats.values_mut() {
                let empty;
                let snapshot =
                    if let Some(snapshot) = item.contextual(contextual.declaration.source) {
                        snapshot
                    } else {
                        empty = crate::ContextualRepeatSnapshot {
                            source_generation: contextual.source_generation,
                            items: Vec::new(),
                        };
                        &empty
                    };
                changed =
                    changed.saturating_add(self.reconcile_contextual_repeat(contextual, snapshot)?);
            }
        }
        Ok(changed)
    }

    fn update_repeated_elements(
        &mut self,
        elements: &[LiveRepeatedElement],
        item: &RepeatItemSnapshot,
    ) -> Result<usize, RuntimeError> {
        let mut changed = 0usize;
        for element in elements {
            let element_changed = match element.declaration.kind {
                BuiltInElementKind::StateText => {
                    let binding = element.declaration.binding.ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(
                            "repeated text binding disappeared".into(),
                        )
                    })?;
                    let value = if binding == crate::ItemBindingKey::Property {
                        element
                            .declaration
                            .property_key
                            .as_ref()
                            .and_then(|key| item.properties.get(key))
                            .map(String::as_str)
                            .unwrap_or("—")
                    } else {
                        item.text.get(&binding).map(String::as_str).unwrap_or("—")
                    };
                    self.apply_text_to_node(element.node, value)?
                }
                BuiltInElementKind::StateToken => {
                    let binding = element.declaration.binding.ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(
                            "repeated token binding disappeared".into(),
                        )
                    })?;
                    let token = if binding == crate::ItemBindingKey::Property {
                        if element
                            .declaration
                            .property_key
                            .as_ref()
                            .is_some_and(|key| item.properties.contains_key(key))
                        {
                            StateToken::Available.as_str()
                        } else {
                            StateToken::Unavailable.as_str()
                        }
                    } else {
                        item.tokens
                            .get(&binding)
                            .map(String::as_str)
                            .unwrap_or(StateToken::Unknown.as_str())
                    };
                    self.apply_attribute_to_node(element.node, STATE_ATTRIBUTE, Some(token))?
                }
                BuiltInElementKind::StateValue => {
                    let binding = element.declaration.binding.ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(
                            "repeated value binding disappeared".into(),
                        )
                    })?;
                    let value = item
                        .values
                        .get(&binding)
                        .copied()
                        .unwrap_or(NumericValue::Unknown);
                    let format = element.declaration.value_format.ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "repeat value `{}` has no numeric format",
                            element.declaration.local_id
                        ))
                    })?;
                    let formatted = if matches!(
                        binding,
                        crate::ItemBindingKey::Volume | crate::ItemBindingKey::Peak
                    ) {
                        value.format_volume(format)
                    } else {
                        value.format(format)
                    }
                    .map_err(|error| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "repeat value `{}` could not be formatted: {error}",
                            element.declaration.local_id
                        ))
                    })?;
                    self.apply_value_to_node(
                        element.node,
                        &formatted.display,
                        formatted.value.as_deref(),
                    )?
                }
                BuiltInElementKind::ActionButton => {
                    let enabled = element
                        .declaration
                        .enabled_binding
                        .and_then(|binding| item.tokens.get(&binding))
                        .is_some_and(|token| token == StateToken::True.as_str());
                    self.apply_control_availability(
                        element.node,
                        element.declaration.disabled,
                        enabled,
                    )?
                }
                BuiltInElementKind::RangeControl => {
                    let enabled = element
                        .declaration
                        .enabled_binding
                        .and_then(|binding| item.tokens.get(&binding))
                        .is_some_and(|token| token == StateToken::True.as_str());
                    let mut changed = self.apply_control_availability(
                        element.node,
                        element.declaration.disabled,
                        enabled,
                    )?;
                    let value = item
                        .values
                        .get(&crate::ItemBindingKey::Volume)
                        .and_then(|value| value.as_f64());
                    let range = element.declaration.range.ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(
                            "repeated range control has no validated bounds".into(),
                        )
                    })?;
                    let value = value
                        .map(|value| {
                            NumericValue::Decimal(
                                value.clamp(range.minimum.get(), range.maximum.get()),
                            )
                            .format(StateValueFormat::Raw)
                        })
                        .transpose()
                        .map_err(|error| {
                            RuntimeError::InvalidMutationTarget(format!(
                                "repeated range value could not be formatted: {error}"
                            ))
                        })?
                        .and_then(|value| value.value);
                    self.record_range_authoritative(element.node, value.clone());
                    if self
                        .node_attribute(element.node, STATE_ATTRIBUTE)?
                        .as_deref()
                        != Some("pending")
                    {
                        changed |=
                            self.apply_attribute_to_node(element.node, "value", value.as_deref())?;
                    }
                    changed
                }
                _ => {
                    return Err(RuntimeError::InvalidMutationTarget(
                        "repeat contains a forbidden live element kind".into(),
                    ));
                }
            };
            if element_changed
                && element.declaration.binding
                    == Some(crate::ItemBindingKey::RepresentativeLinkRawId)
            {
                self.measurements.representative_changes =
                    self.measurements.representative_changes.saturating_add(1);
            }
            changed = changed.saturating_add(usize::from(element_changed));
        }
        Ok(changed)
    }

    fn reconcile_contextual_repeat(
        &mut self,
        repeat: &mut LiveContextualRepeat,
        snapshot: &crate::ContextualRepeatSnapshot,
    ) -> Result<usize, RuntimeError> {
        let was_empty = repeat.items.is_empty();
        if snapshot.items.len() > repeat.declaration.source.item_limit() {
            return Err(RuntimeError::LimitExceeded(format!(
                "`{}` contains {} items; limit is {}",
                repeat.declaration.source.as_str(),
                snapshot.items.len(),
                repeat.declaration.source.item_limit()
            )));
        }
        let mut keys = BTreeSet::new();
        if snapshot
            .items
            .iter()
            .any(|item| item.key.is_empty() || !keys.insert(item.key.clone()))
        {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "`{}` contains an empty or duplicate key",
                repeat.declaration.source.as_str()
            )));
        }
        if snapshot.source_generation < repeat.source_generation {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "stale `{}` source generation",
                repeat.declaration.source.as_str()
            )));
        }
        let mut changed = 0usize;
        if repeat.source_generation != 0 && repeat.source_generation != snapshot.source_generation {
            if repeat.declaration.source == crate::ContextualRepeatSource::Channels {
                self.measurements.channel_layout_replacements = self
                    .measurements
                    .channel_layout_replacements
                    .saturating_add(1);
            }
            for key in repeat.order.clone() {
                self.remove_contextual_item(repeat, &key)?;
                changed = changed.saturating_add(1);
                match repeat.declaration.source {
                    crate::ContextualRepeatSource::Channels => {
                        self.measurements.channel_removals =
                            self.measurements.channel_removals.saturating_add(1);
                    }
                    crate::ContextualRepeatSource::GroupLinks => {
                        self.measurements.group_member_removals =
                            self.measurements.group_member_removals.saturating_add(1);
                    }
                    crate::ContextualRepeatSource::NodeLinkGroups => {
                        self.measurements.node_tracker_removals =
                            self.measurements.node_tracker_removals.saturating_add(1);
                    }
                }
            }
        }
        repeat.source_generation = snapshot.source_generation;
        let desired = snapshot
            .items
            .iter()
            .map(|item| item.key.as_str())
            .collect::<BTreeSet<_>>();
        for key in repeat
            .order
            .iter()
            .filter(|key| !desired.contains(key.as_str()))
            .cloned()
            .collect::<Vec<_>>()
        {
            self.remove_contextual_item(repeat, &key)?;
            changed = changed.saturating_add(1);
            match repeat.declaration.source {
                crate::ContextualRepeatSource::Channels => {
                    self.measurements.channel_removals =
                        self.measurements.channel_removals.saturating_add(1);
                }
                crate::ContextualRepeatSource::GroupLinks => {
                    self.measurements.group_member_removals =
                        self.measurements.group_member_removals.saturating_add(1);
                }
                crate::ContextualRepeatSource::NodeLinkGroups => {
                    self.measurements.node_tracker_removals =
                        self.measurements.node_tracker_removals.saturating_add(1);
                }
            }
        }
        for item in &snapshot.items {
            if let Some(live) = repeat.items.get(&item.key) {
                match repeat.declaration.source {
                    crate::ContextualRepeatSource::Channels => {
                        self.measurements.retained_channel_identities = self
                            .measurements
                            .retained_channel_identities
                            .saturating_add(1);
                    }
                    crate::ContextualRepeatSource::GroupLinks => {
                        self.measurements.retained_link_identities =
                            self.measurements.retained_link_identities.saturating_add(1);
                    }
                    crate::ContextualRepeatSource::NodeLinkGroups => {
                        self.measurements.retained_tracker_identities = self
                            .measurements
                            .retained_tracker_identities
                            .saturating_add(1);
                    }
                }
                let elements = live.elements.clone();
                let mutations = self.update_repeated_elements(&elements, item)?;
                if mutations == 0 {
                    match repeat.declaration.source {
                        crate::ContextualRepeatSource::Channels => {
                            self.measurements.duplicate_channel_suppressions = self
                                .measurements
                                .duplicate_channel_suppressions
                                .saturating_add(1);
                        }
                        _ => {
                            self.measurements.duplicate_graph_suppressions = self
                                .measurements
                                .duplicate_graph_suppressions
                                .saturating_add(1);
                        }
                    }
                } else {
                    match repeat.declaration.source {
                        crate::ContextualRepeatSource::Channels => {
                            self.measurements.channel_value_mutations = self
                                .measurements
                                .channel_value_mutations
                                .saturating_add(mutations as u64);
                        }
                        crate::ContextualRepeatSource::GroupLinks => {
                            self.measurements.link_state_mutations = self
                                .measurements
                                .link_state_mutations
                                .saturating_add(mutations as u64);
                            self.measurements.link_relation_mutations = self
                                .measurements
                                .link_relation_mutations
                                .saturating_add(mutations as u64);
                        }
                        crate::ContextualRepeatSource::NodeLinkGroups => {
                            self.measurements.group_state_mutations = self
                                .measurements
                                .group_state_mutations
                                .saturating_add(mutations as u64);
                            self.measurements.peer_relation_mutations = self
                                .measurements
                                .peer_relation_mutations
                                .saturating_add(mutations as u64);
                        }
                    }
                }
                changed = changed.saturating_add(mutations);
            } else {
                self.insert_contextual_item(repeat, item)?;
                changed = changed.saturating_add(1);
                match repeat.declaration.source {
                    crate::ContextualRepeatSource::Channels => {
                        self.measurements.channel_insertions =
                            self.measurements.channel_insertions.saturating_add(1);
                    }
                    crate::ContextualRepeatSource::GroupLinks => {
                        self.measurements.group_member_insertions =
                            self.measurements.group_member_insertions.saturating_add(1);
                    }
                    crate::ContextualRepeatSource::NodeLinkGroups => {
                        self.measurements.node_tracker_insertions =
                            self.measurements.node_tracker_insertions.saturating_add(1);
                    }
                }
                self.measurements.contextual_subtree_clones = self
                    .measurements
                    .contextual_subtree_clones
                    .saturating_add(1);
            }
        }
        let desired_order = snapshot
            .items
            .iter()
            .map(|item| item.key.clone())
            .collect::<Vec<_>>();
        if repeat.order != desired_order {
            let moves = desired_order
                .iter()
                .enumerate()
                .filter(|(index, key)| repeat.order.get(*index) != Some(*key))
                .count() as u64;
            match repeat.declaration.source {
                crate::ContextualRepeatSource::Channels => {
                    self.measurements.channel_moves =
                        self.measurements.channel_moves.saturating_add(moves);
                }
                crate::ContextualRepeatSource::GroupLinks => {
                    self.measurements.link_moves =
                        self.measurements.link_moves.saturating_add(moves);
                }
                crate::ContextualRepeatSource::NodeLinkGroups => {}
            }
            let marker = self
                .identities
                .resolve(&self.document, repeat.template_node)?;
            for key in &desired_order {
                let root = repeat.items.get(key).ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "channel item `{key}` disappeared during reorder"
                    ))
                })?;
                let slot = self.identities.resolve(&self.document, root.root)?;
                self.document.mutate().insert_nodes_before(marker, &[slot]);
            }
            repeat.order = desired_order;
            changed = changed.saturating_add(1);
        }
        let is_empty = repeat.items.is_empty();
        if repeat.declaration.source == crate::ContextualRepeatSource::Channels
            && was_empty
            && !is_empty
        {
            self.measurements.channel_source_activations = self
                .measurements
                .channel_source_activations
                .saturating_add(1);
        } else if repeat.declaration.source == crate::ContextualRepeatSource::Channels
            && !was_empty
            && is_empty
        {
            self.measurements.channel_source_releases =
                self.measurements.channel_source_releases.saturating_add(1);
        }
        Ok(changed)
    }

    fn insert_contextual_item(
        &mut self,
        repeat: &mut LiveContextualRepeat,
        item: &RepeatItemSnapshot,
    ) -> Result<(), RuntimeError> {
        let prototype = self
            .identities
            .resolve(&self.document, repeat.prototype_root)?;
        let clone = self.document.mutate().deep_clone_node(prototype);
        let clone_slots = subtree_slots(&self.document, clone)?;
        if clone_slots.len() != repeat.declaration.prototype_nodes {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "contextual repeat `{}` cloned {} nodes; expected {}",
                repeat.declaration.id,
                clone_slots.len(),
                repeat.declaration.prototype_nodes
            )));
        }
        let mut identities = Vec::with_capacity(clone_slots.len());
        for slot in &clone_slots {
            identities.push(self.identities.activate_created(&self.document, *slot)?);
        }
        let marker = self
            .identities
            .resolve(&self.document, repeat.template_node)?;
        self.document.mutate().insert_nodes_before(marker, &[clone]);
        let elements = repeat
            .declaration
            .descendants
            .iter()
            .map(|declaration| {
                let node = identities
                    .get(declaration.prototype_order)
                    .copied()
                    .ok_or_else(|| {
                        RuntimeError::InvalidMutationTarget(format!(
                            "channel local id `{}` has an invalid prototype position",
                            declaration.local_id
                        ))
                    })?;
                Ok(LiveRepeatedElement {
                    declaration: declaration.clone(),
                    node,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        repeat.items.insert(
            item.key.clone(),
            LiveRepeatedItem {
                root: identities[0],
                elements: elements.clone(),
                contextual_repeats: BTreeMap::new(),
                peak_monitors: BTreeMap::new(),
            },
        );
        repeat.order.push(item.key.clone());
        self.update_repeated_elements(&elements, item)?;
        Ok(())
    }

    fn remove_contextual_item(
        &mut self,
        repeat: &mut LiveContextualRepeat,
        key: &str,
    ) -> Result<(), RuntimeError> {
        let Some(item) = repeat.items.remove(key) else {
            return Ok(());
        };
        let slots = self.identities.subtree_slots(&self.document, item.root)?;
        if self
            .pressed_range
            .as_ref()
            .is_some_and(|range| slots.contains(&range.node.slot))
        {
            self.pressed_range = None;
            self.pending_action = None;
        }
        let root = self.identities.resolve(&self.document, item.root)?;
        self.document.mutate().remove_and_drop_node(root);
        self.identities.retire_removed(&self.document, &slots)?;
        repeat.order.retain(|current| current != key);
        Ok(())
    }

    fn apply_control_availability(
        &mut self,
        identity: ExperimentalNodeIdentity,
        author_disabled: bool,
        enabled: bool,
    ) -> Result<bool, RuntimeError> {
        let disabled = author_disabled || !enabled;
        let mut changed =
            self.apply_attribute_to_node(identity, "disabled", disabled.then_some(""))?;
        if disabled
            && self
                .pressed_range
                .as_ref()
                .is_some_and(|range| range.node == identity)
        {
            let range = self.pressed_range.take().expect("checked above");
            if self.pending_action.as_ref().is_some_and(|action| {
                matches!(
                    action,
                    LiveAction::PipeWireAudio(request) if request.control == range.control
                )
            }) {
                self.pending_action = None;
            }
            changed |= self.apply_attribute_to_node(
                identity,
                "value",
                range.authoritative_value.as_deref(),
            )?;
        }
        let slot = self.identities.resolve(&self.document, identity)?;
        let current = self
            .document
            .get_node(slot)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(LocalName::from(STATE_ATTRIBUTE)));
        let desired = if enabled {
            match current {
                Some("pending" | "failed") => None,
                Some("idle") => None,
                _ => Some("idle"),
            }
        } else {
            Some("unavailable")
        };
        if let Some(desired) = desired {
            changed |= self.apply_attribute_to_node(identity, STATE_ATTRIBUTE, Some(desired))?;
        }
        Ok(changed)
    }

    fn apply_value_to_node(
        &mut self,
        identity: ExperimentalNodeIdentity,
        display: &str,
        value: Option<&str>,
    ) -> Result<bool, RuntimeError> {
        let text_changed = self.apply_text_to_node(identity, display)?;
        let value_changed = self.apply_attribute_to_node(identity, "value", value)?;
        Ok(text_changed || value_changed)
    }

    fn apply_text_to_node(
        &mut self,
        identity: ExperimentalNodeIdentity,
        value: &str,
    ) -> Result<bool, RuntimeError> {
        let parent = self.identities.resolve(&self.document, identity)?;
        let current = self
            .document
            .get_node(parent)
            .ok_or(RuntimeError::StaleIdentity {
                slot: identity.slot,
                generation: identity.generation,
            })?
            .text_content();
        if current == value {
            return Ok(false);
        }
        let children = self
            .document
            .get_node(parent)
            .expect("resolved above")
            .children
            .clone();
        let text_nodes: Vec<_> = children
            .into_iter()
            .filter(|child| {
                self.document
                    .get_node(*child)
                    .is_some_and(|node| matches!(node.data, NodeData::Text(_)))
            })
            .collect();
        if let Some((first, remaining)) = text_nodes.split_first() {
            self.document.mutate().set_node_text(*first, value);
            for text in remaining {
                self.document.mutate().set_node_text(*text, "");
            }
        } else {
            let text = self.document.mutate().create_text_node(value);
            self.document.mutate().append_children(parent, &[text]);
            self.identities.activate_created(&self.document, text)?;
        }
        Ok(true)
    }

    fn apply_attribute_to_node(
        &mut self,
        identity: ExperimentalNodeIdentity,
        attribute: &str,
        value: Option<&str>,
    ) -> Result<bool, RuntimeError> {
        let node = self.identities.resolve(&self.document, identity)?;
        let current = self
            .document
            .get_node(node)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(LocalName::from(attribute)));
        if current == value {
            return Ok(false);
        }
        let name = QualName {
            prefix: None,
            ns: ns!(),
            local: LocalName::from(attribute),
        };
        if let Some(value) = value {
            self.document.mutate().set_attribute(node, name, value);
        } else {
            self.document.mutate().clear_attribute(node, name);
        }
        Ok(true)
    }

    fn set_registered_text(&mut self, html_id: &str, value: &str) -> Result<(), RuntimeError> {
        let identity = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` disappeared"
            ))
        })?;
        let parent = self.identities.resolve(&self.document, identity)?;
        let children = self
            .document
            .get_node(parent)
            .ok_or(RuntimeError::StaleIdentity {
                slot: identity.slot,
                generation: identity.generation,
            })?
            .children
            .clone();
        let text_nodes: Vec<_> = children
            .into_iter()
            .filter(|child| {
                self.document
                    .get_node(*child)
                    .is_some_and(|node| matches!(node.data, NodeData::Text(_)))
            })
            .collect();
        if let Some((first, remaining)) = text_nodes.split_first() {
            self.document.mutate().set_node_text(*first, value);
            for text in remaining {
                self.document.mutate().set_node_text(*text, "");
            }
        } else {
            let text = self.document.mutate().create_text_node(value);
            self.document.mutate().append_children(parent, &[text]);
            self.identities.activate_created(&self.document, text)?;
        }
        Ok(())
    }

    fn set_registered_token(
        &mut self,
        html_id: &str,
        token: StateToken,
    ) -> Result<(), RuntimeError> {
        self.set_registered_attribute(html_id, STATE_ATTRIBUTE, token.as_str())
    }

    fn set_registered_attribute(
        &mut self,
        html_id: &str,
        attribute: &str,
        value: &str,
    ) -> Result<(), RuntimeError> {
        let identity = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` disappeared"
            ))
        })?;
        let node = self.identities.resolve(&self.document, identity)?;
        self.document.mutate().set_attribute(
            node,
            QualName {
                prefix: None,
                ns: ns!(),
                local: LocalName::from(attribute),
            },
            value,
        );
        Ok(())
    }

    fn clear_registered_attribute(
        &mut self,
        html_id: &str,
        attribute: &str,
    ) -> Result<(), RuntimeError> {
        let identity = self.builtins.indexed_node(html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "registered element `#{html_id}` disappeared"
            ))
        })?;
        let node = self.identities.resolve(&self.document, identity)?;
        self.document.mutate().clear_attribute(
            node,
            QualName {
                prefix: None,
                ns: ns!(),
                local: LocalName::from(attribute),
            },
        );
        Ok(())
    }

    fn action_at(&self, x: f32, y: f32) -> Result<Option<PendingActivation>, RuntimeError> {
        if !self.builtins.is_empty() {
            for monitor in self.peak_monitors.values().chain(
                self.repeats
                    .values()
                    .flat_map(|repeat| repeat.items.values())
                    .flat_map(|item| item.peak_monitors.values()),
            ) {
                for element in monitor.elements.iter().rev() {
                    let Some(action) = element.declaration.action else {
                        continue;
                    };
                    if self.node_is_disabled(element.node)? {
                        continue;
                    }
                    let bounds = self.bounds_for_identity(element.node)?;
                    if !contains(&bounds, x, y) {
                        continue;
                    }
                    let operation = match action {
                        ShellAction::PipeWirePeaksEnable => PipeWirePeakOperation::Enable,
                        ShellAction::PipeWirePeaksDisable => PipeWirePeakOperation::Disable,
                        ShellAction::PipeWirePeaksToggle => PipeWirePeakOperation::Toggle,
                        _ => continue,
                    };
                    let control = PipeWireControlIdentity {
                        document_generation: self.document_identity,
                        locator: PipeWireControlLocator::Peak {
                            monitor: monitor.identity.locator.clone(),
                            local_id: element.declaration.local_id.clone(),
                        },
                    };
                    return Ok(Some(PendingActivation {
                        id: format!(
                            "peak:{:?}:{}",
                            monitor.identity.locator, element.declaration.local_id
                        ),
                        action: LiveAction::PipeWirePeak(PipeWirePeakActionRequest {
                            control,
                            monitor: monitor.identity.clone(),
                            operation,
                        }),
                    }));
                }
            }
            for (repeat_id, repeat) in &self.repeats {
                if repeat.declaration.source != RepeatSource::PipeWireNodes {
                    continue;
                }
                for item_key in repeat.order.iter().rev() {
                    let item = &repeat.items[item_key];
                    for element in item.elements.iter().rev() {
                        let Some(action) = element.declaration.action else {
                            continue;
                        };
                        if element.declaration.kind != BuiltInElementKind::ActionButton
                            || self.node_is_disabled(element.node)?
                        {
                            continue;
                        }
                        let bounds = self.bounds_for_identity(element.node)?;
                        if contains(&bounds, x, y) {
                            let control = PipeWireControlIdentity {
                                document_generation: self.document_identity,
                                locator: PipeWireControlLocator::Repeated {
                                    repeat_id: repeat_id.clone(),
                                    item_key: item_key.clone(),
                                    local_id: element.declaration.local_id.clone(),
                                },
                            };
                            let emitted = match action {
                                ShellAction::PipeWireDefaultsSetPreferredSink => {
                                    LiveAction::PipeWireDefault(PipeWireDefaultControlRequest {
                                        control,
                                        role: PipeWireDefaultRole::Sink,
                                        target: PipeWireDefaultTarget::NodeItem {
                                            source_generation: repeat.source_generation,
                                            item_key: item_key.clone(),
                                        },
                                    })
                                }
                                ShellAction::PipeWireDefaultsSetPreferredSource => {
                                    LiveAction::PipeWireDefault(PipeWireDefaultControlRequest {
                                        control,
                                        role: PipeWireDefaultRole::Source,
                                        target: PipeWireDefaultTarget::NodeItem {
                                            source_generation: repeat.source_generation,
                                            item_key: item_key.clone(),
                                        },
                                    })
                                }
                                _ => LiveAction::PipeWireAudio(PipeWireControlRequest {
                                    control,
                                    target: PipeWireAudioTarget::NodeItem {
                                        source_generation: repeat.source_generation,
                                        item_key: item_key.clone(),
                                    },
                                    operation: pipewire_mute_operation(action)?,
                                    volume: None,
                                }),
                            };
                            return Ok(Some(PendingActivation {
                                id: format!(
                                    "{repeat_id}:{}:{}",
                                    item_key, element.declaration.local_id
                                ),
                                action: emitted,
                            }));
                        }
                    }
                }
            }
            for html_id in self.builtins.action_candidates() {
                let Some(target) =
                    self.builtins
                        .action_target(html_id, &self.document, &self.identities)?
                else {
                    continue;
                };
                let slot = self.identities.resolve(&self.document, target.node)?;
                let bounds = self.document.get_node(slot).map(node_bounds).ok_or(
                    RuntimeError::StaleIdentity {
                        slot: target.node.slot,
                        generation: target.node.generation,
                    },
                )?;
                validate_rect(&bounds)?;
                if contains(&bounds, x, y) {
                    let control = PipeWireControlIdentity {
                        document_generation: self.document_identity,
                        locator: PipeWireControlLocator::Element(target.id.html_id.clone()),
                    };
                    return Ok(Some(PendingActivation {
                        id: target.id.html_id.clone(),
                        action: LiveAction::from_registered(
                            target.action,
                            target.target,
                            target.pipewire_target,
                            control,
                        )?,
                    }));
                }
            }
            return Ok(None);
        }
        for (selector, action) in self.kind.actions() {
            if contains(&self.bounds_for(selector)?, x, y) {
                return Ok(Some(PendingActivation {
                    id: (*selector).to_owned(),
                    action: action.clone(),
                }));
            }
        }
        Ok(None)
    }

    fn range_at(&self, x: f32, y: f32) -> Result<Option<PendingRange>, RuntimeError> {
        for (repeat_id, repeat) in &self.repeats {
            if repeat.declaration.source != RepeatSource::PipeWireNodes {
                continue;
            }
            for item_key in repeat.order.iter().rev() {
                let item = &repeat.items[item_key];
                for (contextual_id, contextual) in &item.contextual_repeats {
                    for channel_item_key in contextual.order.iter().rev() {
                        let channel = &contextual.items[channel_item_key];
                        for element in channel.elements.iter().rev() {
                            let Some(range) = element.declaration.range else {
                                continue;
                            };
                            if self.node_is_disabled(element.node)? {
                                continue;
                            }
                            let bounds = self.bounds_for_identity(element.node)?;
                            if contains(&bounds, x, y) {
                                return Ok(Some(PendingRange {
                                    control: PipeWireControlIdentity {
                                        document_generation: self.document_identity,
                                        locator: PipeWireControlLocator::Contextual {
                                            repeat_id: repeat_id.clone(),
                                            node_item_key: item_key.clone(),
                                            contextual_id: contextual_id.clone(),
                                            channel_item_key: channel_item_key.clone(),
                                            local_id: element.declaration.local_id.clone(),
                                        },
                                    },
                                    target: PipeWireAudioTarget::ChannelItem {
                                        source_generation: repeat.source_generation,
                                        node_item_key: item_key.clone(),
                                        layout_generation: contextual.source_generation,
                                        channel_item_key: channel_item_key.clone(),
                                    },
                                    operation: PipeWireAudioOperation::SetChannelVolume,
                                    node: element.node,
                                    range,
                                    authoritative_value: self
                                        .node_attribute(element.node, "value")?,
                                    last_desired: None,
                                }));
                            }
                        }
                    }
                }
                for element in item.elements.iter().rev() {
                    let Some(range) = element.declaration.range else {
                        continue;
                    };
                    if self.node_is_disabled(element.node)? {
                        continue;
                    }
                    let bounds = self.bounds_for_identity(element.node)?;
                    if contains(&bounds, x, y) {
                        return Ok(Some(PendingRange {
                            control: PipeWireControlIdentity {
                                document_generation: self.document_identity,
                                locator: PipeWireControlLocator::Repeated {
                                    repeat_id: repeat_id.clone(),
                                    item_key: item_key.clone(),
                                    local_id: element.declaration.local_id.clone(),
                                },
                            },
                            target: PipeWireAudioTarget::NodeItem {
                                source_generation: repeat.source_generation,
                                item_key: item_key.clone(),
                            },
                            operation: PipeWireAudioOperation::SetVolume,
                            node: element.node,
                            range,
                            authoritative_value: self.node_attribute(element.node, "value")?,
                            last_desired: None,
                        }));
                    }
                }
            }
        }
        for declaration in self.builtins.declarations() {
            let Some(range) = declaration.range else {
                continue;
            };
            let node = self
                .builtins
                .indexed_node(&declaration.id.html_id)
                .ok_or_else(|| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "range control `#{}` disappeared",
                        declaration.id.html_id
                    ))
                })?;
            if self.node_is_disabled(node)? {
                continue;
            }
            let bounds = self.bounds_for_identity(node)?;
            if contains(&bounds, x, y) {
                return Ok(Some(PendingRange {
                    control: PipeWireControlIdentity {
                        document_generation: self.document_identity,
                        locator: PipeWireControlLocator::Element(declaration.id.html_id.clone()),
                    },
                    target: pipewire_audio_target(range.target)?,
                    operation: PipeWireAudioOperation::SetVolume,
                    node,
                    range,
                    authoritative_value: self.node_attribute(node, "value")?,
                    last_desired: None,
                }));
            }
        }
        Ok(None)
    }

    fn update_pressed_range(&mut self, x: f32) -> Result<bool, RuntimeError> {
        let Some(mut pending) = self.pressed_range.take() else {
            return Ok(false);
        };
        let bounds = self.bounds_for_identity(pending.node)?;
        let span = f64::from(bounds.width.max(1.0));
        let position = ((f64::from(x) - f64::from(bounds.x)) / span).clamp(0.0, 1.0);
        let minimum = pending.range.minimum.get();
        let maximum = pending.range.maximum.get();
        let step = pending.range.step.get();
        let raw = minimum + position * (maximum - minimum);
        let steps = ((raw - minimum) / step).round();
        let desired = (minimum + steps * step).clamp(minimum, maximum);
        let desired = PipeWireDesiredVolume::new(desired).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("range produced an invalid volume".into())
        })?;
        let changed = pending.last_desired != Some(desired);
        if changed {
            let value = NumericValue::Decimal(desired.get())
                .format(StateValueFormat::Raw)
                .map_err(|error| {
                    RuntimeError::InvalidMutationTarget(format!(
                        "range value could not be formatted: {error}"
                    ))
                })?
                .value;
            self.apply_attribute_to_node(pending.node, "value", value.as_deref())?;
            self.apply_attribute_to_node(
                pending.node,
                STATE_ATTRIBUTE,
                Some(PipeWireControlState::Pending.as_str()),
            )?;
            self.pending_action = Some(LiveAction::PipeWireAudio(PipeWireControlRequest {
                control: pending.control.clone(),
                target: pending.target.clone(),
                operation: pending.operation,
                volume: Some(desired),
            }));
            pending.last_desired = Some(desired);
            self.resolve();
        }
        self.pressed_range = Some(pending);
        Ok(changed)
    }

    fn record_range_authoritative(
        &mut self,
        node: ExperimentalNodeIdentity,
        value: Option<String>,
    ) {
        if let Some(pending) = self
            .pressed_range
            .as_mut()
            .filter(|pending| pending.node == node)
        {
            pending.authoritative_value = value;
        }
    }

    fn bounds_for_identity(
        &self,
        identity: ExperimentalNodeIdentity,
    ) -> Result<LogicalRect, RuntimeError> {
        let slot = self.identities.resolve(&self.document, identity)?;
        let bounds =
            self.document
                .get_node(slot)
                .map(node_bounds)
                .ok_or(RuntimeError::StaleIdentity {
                    slot: identity.slot,
                    generation: identity.generation,
                })?;
        validate_rect(&bounds)?;
        Ok(bounds)
    }

    fn node_attribute(
        &self,
        identity: ExperimentalNodeIdentity,
        attribute: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let slot = self.identities.resolve(&self.document, identity)?;
        Ok(self
            .document
            .get_node(slot)
            .and_then(|node| node.element_data())
            .and_then(|element| element.attr(LocalName::from(attribute)))
            .map(str::to_owned))
    }

    fn node_is_disabled(&self, identity: ExperimentalNodeIdentity) -> Result<bool, RuntimeError> {
        Ok(self.node_attribute(identity, "disabled")?.is_some())
    }

    fn bounds_for(&self, selector: &str) -> Result<LogicalRect, RuntimeError> {
        let slot = required_selector(&self.document, selector)?;
        let node = self.document.get_node(slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("{selector} disappeared"))
        })?;
        let bounds = node_bounds(node);
        validate_rect(&bounds)?;
        Ok(bounds)
    }

    fn resolve(&mut self) {
        let started = Instant::now();
        self.document.resolve(self.started.elapsed().as_secs_f64());
        self.measurements.last_resolve_ms = elapsed_ms(started);
    }
}

const fn is_pipewire_volume_key(key: StateBindingKey) -> bool {
    matches!(
        key,
        StateBindingKey::PipeWireDefaultSinkVolume
            | StateBindingKey::PipeWireDefaultSourceVolume
            | StateBindingKey::PipeWireConfiguredSinkVolume
            | StateBindingKey::PipeWireConfiguredSourceVolume
    )
}

fn pipewire_audio_target(
    target: PipeWireControlTarget,
) -> Result<PipeWireAudioTarget, RuntimeError> {
    match target {
        PipeWireControlTarget::DefaultSink => Ok(PipeWireAudioTarget::DefaultSink),
        PipeWireControlTarget::DefaultSource => Ok(PipeWireAudioTarget::DefaultSource),
        PipeWireControlTarget::CurrentItem => Err(RuntimeError::InvalidMutationTarget(
            "current-item target requires a repeat identity".into(),
        )),
    }
}

fn pipewire_mute_operation(action: ShellAction) -> Result<PipeWireAudioOperation, RuntimeError> {
    match action {
        ShellAction::PipeWireAudioMute => Ok(PipeWireAudioOperation::Mute),
        ShellAction::PipeWireAudioUnmute => Ok(PipeWireAudioOperation::Unmute),
        ShellAction::PipeWireAudioToggleMute => Ok(PipeWireAudioOperation::ToggleMute),
        ShellAction::PipeWireAudioSetChannelVolume => Err(RuntimeError::InvalidMutationTarget(
            "set-channel-volume is emitted by contextual range interaction".into(),
        )),
        _ => Err(RuntimeError::InvalidMutationTarget(format!(
            "action `{}` is not a PipeWire mute operation",
            action.as_str()
        ))),
    }
}

fn pipewire_mute_request(
    action: ShellAction,
    target: Option<PipeWireControlTarget>,
    control: PipeWireControlIdentity,
) -> Result<PipeWireControlRequest, RuntimeError> {
    Ok(PipeWireControlRequest {
        control,
        target: pipewire_audio_target(target.ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("PipeWire mute action has no target".into())
        })?)?,
        operation: pipewire_mute_operation(action)?,
        volume: None,
    })
}

fn bool_text(value: bool) -> String {
    if value { "true" } else { "false" }.into()
}

fn peak_status_text(status: PipeWirePeakMonitorStatus) -> String {
    match status {
        PipeWirePeakMonitorStatus::Disabled => "Disabled",
        PipeWirePeakMonitorStatus::Suspended => "Suspended",
        PipeWirePeakMonitorStatus::Unavailable => "Unavailable",
        PipeWirePeakMonitorStatus::Starting => "Starting",
        PipeWirePeakMonitorStatus::Ready => "Ready",
        PipeWirePeakMonitorStatus::Failed => "Failed",
    }
    .into()
}

fn checked_scaled_dimension(logical: u32, numerator: u32) -> Result<u32, RuntimeError> {
    let product = u64::from(logical)
        .checked_mul(u64::from(numerator))
        .ok_or_else(|| RuntimeError::LimitExceeded("scaled dimension overflow".into()))?;
    let scaled = product
        .checked_add(u64::from(LIVE_SCALE_DENOMINATOR - 1))
        .ok_or_else(|| RuntimeError::LimitExceeded("scaled ceiling overflow".into()))?
        / u64::from(LIVE_SCALE_DENOMINATOR);
    u32::try_from(scaled)
        .map_err(|_| RuntimeError::LimitExceeded("scaled dimension exceeds u32".into()))
}

impl LiveDocumentKind {
    fn builtin_surface_kind(self) -> BuiltInSurfaceKind {
        match self {
            Self::SingleOverlay => BuiltInSurfaceKind::SingleOverlay,
            Self::Panel => BuiltInSurfaceKind::Panel,
            Self::TransientOverlay => BuiltInSurfaceKind::Overlay,
        }
    }
    fn source_file(self) -> &'static str {
        match self {
            Self::SingleOverlay => "index.html",
            Self::Panel => "panel.html",
            Self::TransientOverlay => "overlay.html",
        }
    }

    fn region_selector(self) -> &'static str {
        match self {
            Self::SingleOverlay => "#shell-card",
            Self::Panel => "#panel-root",
            Self::TransientOverlay => "#overlay-card",
        }
    }

    fn primary_action_selector(self) -> &'static str {
        match self {
            Self::SingleOverlay => "#primary-action",
            Self::Panel => "#overlay-toggle",
            Self::TransientOverlay => "#overlay-action",
        }
    }

    fn required_selectors(self) -> &'static [&'static str] {
        match self {
            Self::SingleOverlay => &["#shell-card", "#primary-action", "#status-label"],
            Self::Panel => &["#panel-root", "#overlay-toggle"],
            Self::TransientOverlay => &[
                "#overlay-card",
                "#overlay-close",
                "#overlay-action",
                "#overlay-status",
            ],
        }
    }

    fn actions(self) -> &'static [(&'static str, LiveAction)] {
        match self {
            Self::SingleOverlay => &[("#primary-action", LiveAction::SingleOverlayActivate)],
            Self::Panel => &[("#overlay-toggle", LiveAction::ToggleOverlay)],
            Self::TransientOverlay => &[
                ("#overlay-close", LiveAction::CloseOverlay),
                ("#overlay-action", LiveAction::ActivateOverlay),
            ],
        }
    }
}

fn blitz_viewport(viewport: ViewportSpec) -> Viewport {
    Viewport::new(
        viewport.logical_width,
        viewport.logical_height,
        viewport.scale_factor,
        ColorScheme::Dark,
    )
}

fn required_selector(document: &HtmlDocument, selector: &str) -> Result<usize, RuntimeError> {
    document
        .query_selector(selector)
        .map_err(|error| RuntimeError::InvalidMutationTarget(format!("{error:?}")))?
        .ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("selector `{selector}` did not match"))
        })
}

fn subtree_slots(document: &HtmlDocument, root: usize) -> Result<Vec<usize>, RuntimeError> {
    let mut slots = Vec::new();
    let mut stack = vec![root];
    while let Some(slot) = stack.pop() {
        let node = document.get_node(slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("repeat subtree node {slot} disappeared"))
        })?;
        slots.push(slot);
        stack.extend(node.children.iter().rev().copied());
    }
    Ok(slots)
}

fn template_root_element(document: &HtmlDocument, template: usize) -> Result<usize, RuntimeError> {
    let node = document.get_node(template).ok_or_else(|| {
        RuntimeError::InvalidMutationTarget("contextual template disappeared".into())
    })?;
    let roots = node
        .children
        .iter()
        .copied()
        .filter(|slot| {
            document
                .get_node(*slot)
                .is_some_and(|node| matches!(node.data, NodeData::Element(_)))
        })
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(RuntimeError::InvalidMutationTarget(
            "contextual template no longer has exactly one root".into(),
        ));
    }
    Ok(roots[0])
}

fn node_bounds(node: &blitz_dom::Node) -> LogicalRect {
    let absolute = node.absolute_position(0.0, 0.0);
    LogicalRect {
        x: absolute.x,
        y: absolute.y,
        width: node.final_layout.size.width,
        height: node.final_layout.size.height,
    }
}

fn validate_rect(rect: &LogicalRect) -> Result<(), RuntimeError> {
    if [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f32::is_finite)
        && rect.width >= 0.0
        && rect.height >= 0.0
    {
        Ok(())
    } else {
        Err(RuntimeError::InvalidPackage(
            "live layout contains nonfinite or negative geometry".into(),
        ))
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), RuntimeError> {
    if width == 0 || height == 0 {
        return Err(RuntimeError::InvalidPackage(
            "live viewport dimensions must be nonzero".into(),
        ));
    }
    if width > MAX_LOGICAL_DIMENSION || height > MAX_LOGICAL_DIMENSION {
        return Err(RuntimeError::LimitExceeded(format!(
            "live viewport {width}x{height} exceeds {MAX_LOGICAL_DIMENSION}"
        )));
    }
    pixel_len(width, height)?;
    Ok(())
}

fn pixel_len(width: u32, height: u32) -> Result<usize, RuntimeError> {
    let len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| RuntimeError::LimitExceeded("live pixel dimensions overflow".into()))?;
    if len > MAX_PIXEL_BYTES {
        return Err(RuntimeError::LimitExceeded(format!(
            "live frame requires {len} bytes; limit is {MAX_PIXEL_BYTES}"
        )));
    }
    Ok(len)
}

fn checked_point(x: f64, y: f64) -> Result<Point<f32>, RuntimeError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(RuntimeError::InvalidMutationTarget(
            "pointer coordinates must be finite".into(),
        ));
    }
    Ok(Point {
        x: x as f32,
        y: y as f32,
    })
}

fn contains(rect: &LogicalRect, x: f32, y: f32) -> bool {
    x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

fn pointer_event(x: f32, y: f32, pressed: bool) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            client_x: x,
            client_y: y,
            screen_x: x,
            screen_y: y,
        },
        button: MouseEventButton::Main,
        buttons: if pressed {
            MouseEventButtons::Primary
        } else {
            MouseEventButtons::None
        },
        mods: Default::default(),
        details: PointerDetails {
            pressure: if pressed { 0.5 } else { 0.0 },
            ..Default::default()
        },
        element: Point { x, y },
        active_pointers: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/live-overlay")
    }

    fn multi_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/two-surface-shell")
    }

    fn manifest_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/multi-output-shell")
    }

    fn built_in_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/built-in-panel")
    }

    fn clock_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/clock-panel")
    }

    fn static_panel_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/static-panel")
    }

    fn battery_panel_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/battery-panel")
    }

    fn formatted_clock_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/formatted-clock")
    }

    fn power_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/power")
    }

    fn audio_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/audio-inspector")
    }

    fn element_attribute(live: &LiveDocument, html_id: &str, name: &str) -> Option<String> {
        let identity = live.builtins.indexed_node(html_id)?;
        let slot = live.identities.resolve(&live.document, identity).ok()?;
        live.document
            .get_node(slot)?
            .element_data()?
            .attr(LocalName::from(name))
            .map(str::to_owned)
    }

    fn selector_bounds(live: &LiveDocument, selector: &str) -> LogicalRect {
        let slot = required_selector(&live.document, selector).unwrap();
        node_bounds(live.document.get_node(slot).expect("selector node exists"))
    }

    fn binding_values(
        output: &str,
        scale: &str,
        surface: &str,
        status: &str,
        count: &str,
        action: &str,
    ) -> Vec<(StateBindingKey, String)> {
        vec![
            (StateBindingKey::OutputLabel, output.into()),
            (StateBindingKey::OutputScale, scale.into()),
            (StateBindingKey::SurfaceTemplateId, surface.into()),
            (StateBindingKey::OverlayStatus, status.into()),
            (StateBindingKey::OverlayActivationCount, count.into()),
            (StateBindingKey::ShellLastAction, action.into()),
        ]
    }

    fn click_action(live: &mut LiveDocument, bounds: &LogicalRect) -> LiveAction {
        let x = f64::from(bounds.x + bounds.width / 2.0);
        let y = f64::from(bounds.y + bounds.height / 2.0);
        assert!(live.pointer_move(x, y).unwrap());
        assert!(live.pointer_primary(true).unwrap());
        assert!(live.pointer_primary(false).unwrap());
        live.take_action().expect("action emitted")
    }

    fn alpha_bounds(frame: &LiveFrame) -> Option<(u32, u32, u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for (index, pixel) in frame.premultiplied_rgba.chunks_exact(4).enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let x = index as u32 % frame.buffer_width;
            let y = index as u32 / frame.buffer_width;
            bounds = Some(match bounds {
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                None => (x, y, x, y),
            });
        }
        bounds
    }

    fn device_item(key: &str, model: &str, percentage: f64) -> RepeatItemSnapshot {
        RepeatItemSnapshot {
            key: key.into(),
            text: BTreeMap::from([(crate::ItemBindingKey::Model, model.into())]),
            tokens: BTreeMap::from([
                (
                    crate::ItemBindingKey::State,
                    StateToken::Discharging.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::Type,
                    StateToken::Battery.as_str().into(),
                ),
            ]),
            values: BTreeMap::from([
                (
                    crate::ItemBindingKey::Percentage,
                    NumericValue::Decimal(percentage),
                ),
                (crate::ItemBindingKey::Energy, NumericValue::Decimal(42.0)),
            ]),
            properties: BTreeMap::new(),
            channels: None,
            links: None,
            link_groups: None,
        }
    }

    #[test]
    fn standard_template_reconciles_keyed_items_without_reparse_or_rescan() {
        let mut overlay = LiveDocument::load_surface_document(
            power_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            800,
            600,
        )
        .unwrap();
        assert_eq!(overlay.built_in_summary().repeat_declarations, 2);
        let parse_count = overlay.snapshot().unwrap().document_parse_count;
        let scans = overlay.measurements().registry_scan_count;
        let first = RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: 7,
            items: vec![
                device_item("7:/device/a", "Battery A", 41.0),
                device_item("7:/device/b", "Battery B", 72.0),
            ],
        };
        let inserted = overlay.apply_repeat_source(&first).unwrap();
        assert_eq!(inserted.insertions, 2);
        assert_eq!(inserted.subtree_clones, 2);
        let identities: BTreeMap<_, _> = overlay.repeats["device-row"]
            .items
            .iter()
            .map(|(key, item)| (key.clone(), item.root))
            .collect();
        let request = LiveRenderRequest::new(800, 600, LIVE_SCALE_DENOMINATOR).unwrap();
        overlay
            .render_pending_for(request, 91, 1)
            .unwrap()
            .expect("first retained repeat frame");
        let scene_ids: BTreeMap<_, _> = identities
            .iter()
            .map(|(key, identity)| {
                (
                    key.clone(),
                    overlay
                        .render_session
                        .current_scene()
                        .unwrap()
                        .nodes
                        .iter()
                        .find(|node| node.id.dom == Some(*identity))
                        .unwrap()
                        .id,
                )
            })
            .collect();

        let second = RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: 7,
            items: vec![
                device_item("7:/device/b", "Battery B updated", 73.0),
                device_item("7:/device/a", "Battery A", 41.0),
            ],
        };
        let updated = overlay.apply_repeat_source(&second).unwrap();
        assert!(updated.moves > 0);
        assert!(updated.property_updates > 0);
        assert_eq!(updated.identity_reuses, 2);
        for (key, identity) in identities {
            assert_eq!(overlay.repeats["device-row"].items[&key].root, identity);
        }
        overlay
            .render_pending_for(request, 91, 1)
            .unwrap()
            .expect("keyed move retained frame");
        for (key, id) in scene_ids {
            let root = overlay.repeats["device-row"].items[&key].root;
            assert_eq!(
                overlay
                    .render_session
                    .current_scene()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|node| node.id.dom == Some(root))
                    .unwrap()
                    .id,
                id
            );
        }
        let third = RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: 7,
            items: vec![device_item("7:/device/b", "Battery B updated", 73.0)],
        };
        let removed = overlay.apply_repeat_source(&third).unwrap();
        assert_eq!(removed.removals, 1);
        let stale = RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: 6,
            items: Vec::new(),
        };
        assert!(overlay.apply_repeat_source(&stale).is_err());
        let duplicate = RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: 7,
            items: vec![
                device_item("duplicate", "One", 1.0),
                device_item("duplicate", "Two", 2.0),
            ],
        };
        assert!(overlay.apply_repeat_source(&duplicate).is_err());
        assert_eq!(
            overlay.snapshot().unwrap().document_parse_count,
            parse_count
        );
        assert_eq!(overlay.measurements().registry_scan_count, scans);
        assert!(overlay.render().is_ok());
    }

    #[test]
    fn pipewire_nodes_reconcile_with_exact_properties_and_stable_identity() {
        let mut overlay = LiveDocument::load_surface_document(
            audio_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1100,
            800,
        )
        .unwrap();
        let demand = overlay.pipewire_demand();
        assert!(demand.service);
        assert!(demand.nodes);
        assert!(demand.node_details);
        assert!(demand.defaults);
        assert!(demand.audio_state);
        assert!(demand.audio_writes);
        assert!(demand.channel_projection);
        assert!(demand.channel_writes);
        assert!(demand.configured_default_writes);
        assert!(demand.preferred_sink_writes);
        assert!(demand.preferred_source_writes);
        assert_eq!(
            demand.property_keys,
            BTreeSet::from(["application.name".into(), "media.title".into()])
        );
        let parse_count = overlay.snapshot().unwrap().document_parse_count;
        let scans = overlay.measurements().registry_scan_count;
        let mut item = RepeatItemSnapshot {
            key: "7:42".into(),
            text: BTreeMap::from([
                (crate::ItemBindingKey::Name, "node.name".into()),
                (
                    crate::ItemBindingKey::Description,
                    "Node description".into(),
                ),
                (crate::ItemBindingKey::MediaClass, "Audio/Sink".into()),
                (crate::ItemBindingKey::NodeType, "Audio sink".into()),
                (crate::ItemBindingKey::NodeState, "Running".into()),
                (crate::ItemBindingKey::Direction, "Sink".into()),
            ]),
            tokens: BTreeMap::from([
                (
                    crate::ItemBindingKey::NodeType,
                    StateToken::AudioSink.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::NodeState,
                    StateToken::Running.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::DefaultRole,
                    StateToken::DefaultSink.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::ConfiguredRole,
                    StateToken::ConfiguredSink.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::IsAudio,
                    StateToken::True.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::IsVideo,
                    StateToken::False.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::IsStream,
                    StateToken::False.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::AudioStatus,
                    StateToken::Ready.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::MuteState,
                    StateToken::Unmuted.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::CanSetVolume,
                    StateToken::True.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::CanSetMute,
                    StateToken::True.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::CanSetPreferredSink,
                    StateToken::True.as_str().into(),
                ),
                (
                    crate::ItemBindingKey::CanSetPreferredSource,
                    StateToken::False.as_str().into(),
                ),
            ]),
            values: BTreeMap::from([
                (crate::ItemBindingKey::RawId, NumericValue::Integer(42)),
                (crate::ItemBindingKey::Volume, NumericValue::Decimal(0.75)),
            ]),
            properties: BTreeMap::from([("application.name".into(), "Player".into())]),
            channels: Some(crate::ContextualRepeatSnapshot {
                source_generation: 1,
                items: vec![
                    RepeatItemSnapshot {
                        key: "1:0:3".into(),
                        text: BTreeMap::from([
                            (crate::ItemBindingKey::PositionName, "Front Left".into()),
                            (crate::ItemBindingKey::Position, "front-left".into()),
                            (crate::ItemBindingKey::Status, "Ready".into()),
                        ]),
                        tokens: BTreeMap::from([
                            (crate::ItemBindingKey::Position, "front-left".into()),
                            (crate::ItemBindingKey::Status, "ready".into()),
                            (crate::ItemBindingKey::CanSetVolume, "true".into()),
                        ]),
                        values: BTreeMap::from([
                            (crate::ItemBindingKey::Index, NumericValue::Integer(0)),
                            (crate::ItemBindingKey::Volume, NumericValue::Decimal(0.8)),
                        ]),
                        properties: BTreeMap::new(),
                        channels: None,
                        links: None,
                        link_groups: None,
                    },
                    RepeatItemSnapshot {
                        key: "1:1:4".into(),
                        text: BTreeMap::from([
                            (crate::ItemBindingKey::PositionName, "Front Right".into()),
                            (crate::ItemBindingKey::Position, "front-right".into()),
                            (crate::ItemBindingKey::Status, "Ready".into()),
                        ]),
                        tokens: BTreeMap::from([
                            (crate::ItemBindingKey::Position, "front-right".into()),
                            (crate::ItemBindingKey::Status, "ready".into()),
                            (crate::ItemBindingKey::CanSetVolume, "true".into()),
                        ]),
                        values: BTreeMap::from([
                            (crate::ItemBindingKey::Index, NumericValue::Integer(1)),
                            (crate::ItemBindingKey::Volume, NumericValue::Decimal(0.6)),
                        ]),
                        properties: BTreeMap::new(),
                        channels: None,
                        links: None,
                        link_groups: None,
                    },
                ],
            }),
            links: None,
            link_groups: None,
        };
        let first = RepeatSourceSnapshot {
            source: RepeatSource::PipeWireNodes,
            source_generation: 7,
            items: vec![item.clone()],
        };
        assert_eq!(overlay.apply_repeat_source(&first).unwrap().insertions, 2);
        let identity = overlay.repeats["node-card"].items["7:42"].root;
        let contextual =
            &overlay.repeats["node-card"].items["7:42"].contextual_repeats["channels-0"];
        assert_eq!(contextual.order, ["1:0:3", "1:1:4"]);
        let left_identity = contextual.items["1:0:3"].root;
        let left_range = contextual.items["1:0:3"]
            .elements
            .iter()
            .find(|element| element.declaration.local_id == "channel-volume-control")
            .unwrap()
            .node;
        let left_bounds = overlay.bounds_for_identity(left_range).unwrap();
        let pending = overlay
            .range_at(
                left_bounds.x + left_bounds.width * 0.5,
                left_bounds.y + left_bounds.height * 0.5,
            )
            .unwrap()
            .expect("channel range is interactive");
        assert_eq!(pending.operation, PipeWireAudioOperation::SetChannelVolume);
        assert_eq!(
            pending.target,
            PipeWireAudioTarget::ChannelItem {
                source_generation: 7,
                node_item_key: "7:42".into(),
                layout_generation: 1,
                channel_item_key: "1:0:3".into(),
            }
        );
        let repeated = &overlay.repeats["node-card"].items["7:42"].elements;
        let mute_node = repeated
            .iter()
            .find(|element| element.declaration.local_id == "mute-control")
            .unwrap()
            .node;
        let range_node = repeated
            .iter()
            .find(|element| element.declaration.local_id == "volume-control")
            .unwrap()
            .node;
        let prefer_output_node = repeated
            .iter()
            .find(|element| element.declaration.local_id == "prefer-output")
            .unwrap()
            .node;
        let mute_bounds = overlay.bounds_for_identity(mute_node).unwrap();
        let mute_action = click_action(&mut overlay, &mute_bounds);
        let LiveAction::PipeWireAudio(mute_request) = mute_action else {
            panic!("item-local mute action was not emitted");
        };
        assert_eq!(mute_request.operation, PipeWireAudioOperation::ToggleMute);
        assert_eq!(
            mute_request.target,
            PipeWireAudioTarget::NodeItem {
                source_generation: 7,
                item_key: "7:42".into(),
            }
        );
        let prefer_bounds = overlay.bounds_for_identity(prefer_output_node).unwrap();
        let LiveAction::PipeWireDefault(preferred) = click_action(&mut overlay, &prefer_bounds)
        else {
            panic!("item-local preferred sink action was not emitted");
        };
        assert_eq!(preferred.role, PipeWireDefaultRole::Sink);
        assert_eq!(
            preferred.target,
            PipeWireDefaultTarget::NodeItem {
                source_generation: 7,
                item_key: "7:42".into(),
            }
        );
        assert!(
            overlay
                .apply_pipewire_control_state(&mute_request.control, PipeWireControlState::Pending,)
                .unwrap()
        );
        assert_eq!(
            overlay.node_attribute(mute_node, STATE_ATTRIBUTE).unwrap(),
            Some("pending".into())
        );

        let range_bounds = overlay.bounds_for_identity(range_node).unwrap();
        let range_x = f64::from(range_bounds.x + range_bounds.width * 0.5);
        let range_y = f64::from(range_bounds.y + range_bounds.height * 0.5);
        assert!(overlay.pointer_move(range_x, range_y).unwrap());
        assert!(overlay.pointer_primary(true).unwrap());
        let LiveAction::PipeWireAudio(volume_request) =
            overlay.take_action().expect("range emits set-volume")
        else {
            panic!("range did not emit a PipeWire audio request");
        };
        assert_eq!(volume_request.operation, PipeWireAudioOperation::SetVolume);
        assert_eq!(volume_request.volume.unwrap().get(), 0.5);
        assert!(overlay.pointer_primary(false).unwrap());
        overlay
            .apply_pipewire_control_state(&volume_request.control, PipeWireControlState::Failed)
            .unwrap();

        item.properties
            .insert("application.name".into(), "Player updated".into());
        item.properties
            .insert("media.title".into(), "Current track".into());
        item.values
            .insert(crate::ItemBindingKey::Volume, NumericValue::Decimal(0.9));
        item.channels.as_mut().unwrap().items[0]
            .values
            .insert(crate::ItemBindingKey::Volume, NumericValue::Decimal(0.85));
        let second = RepeatSourceSnapshot {
            source: RepeatSource::PipeWireNodes,
            source_generation: 7,
            items: vec![item],
        };
        let update = overlay.apply_repeat_source(&second).unwrap();
        assert!(update.property_updates > 0);
        assert_eq!(overlay.repeats["node-card"].items["7:42"].root, identity);
        assert_eq!(
            overlay.repeats["node-card"].items["7:42"].contextual_repeats["channels-0"]
                .items["1:0:3"]
                .root,
            left_identity
        );
        assert_eq!(
            overlay.node_attribute(range_node, "value").unwrap(),
            Some("0.9".into())
        );
        assert_eq!(
            overlay.snapshot().unwrap().document_parse_count,
            parse_count
        );
        assert_eq!(overlay.measurements().registry_scan_count, scans);

        let mut replacement_item = second.items[0].clone();
        let channels = replacement_item.channels.as_mut().unwrap();
        channels.source_generation = 2;
        channels.items.reverse();
        for (index, channel) in channels.items.iter_mut().enumerate() {
            let raw = if index == 0 { 4 } else { 3 };
            channel.key = format!("2:{index}:{raw}");
            channel.values.insert(
                crate::ItemBindingKey::Index,
                NumericValue::Integer(index as i64),
            );
        }
        let replacement = RepeatSourceSnapshot {
            source: RepeatSource::PipeWireNodes,
            source_generation: 7,
            items: vec![replacement_item],
        };
        assert!(
            overlay
                .apply_repeat_source(&replacement)
                .unwrap()
                .property_updates
                > 0
        );
        let live_item = &overlay.repeats["node-card"].items["7:42"];
        assert_eq!(live_item.root, identity);
        let channels = &live_item.contextual_repeats["channels-0"];
        assert_eq!(channels.source_generation, 2);
        assert!(!channels.items.contains_key("1:0:3"));
        assert_ne!(channels.items["2:1:3"].root, left_identity);
        assert!(
            overlay
                .apply_pipewire_control_state(&pending.control, PipeWireControlState::Idle)
                .is_err()
        );
        let measurements = overlay.measurements();
        assert_eq!(measurements.contextual_item_count, 2);
        assert_eq!(measurements.channel_source_activations, 1);
        assert_eq!(measurements.channel_layout_replacements, 1);
        assert_eq!(measurements.channel_insertions, 4);
        assert_eq!(measurements.channel_removals, 2);
        assert!(measurements.retained_channel_identities >= 2);
    }

    #[test]
    fn pipewire_graph_reconciles_links_groups_members_and_node_trackers_incrementally() {
        let mut overlay = LiveDocument::load_surface_document(
            audio_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1100,
            800,
        )
        .unwrap();
        let demand = overlay.pipewire_demand();
        assert!(demand.nodes);
        assert!(demand.link_collection);
        assert!(demand.link_details);
        assert!(demand.link_group_collection);
        assert!(demand.group_members);
        assert!(demand.node_link_tracking);
        assert!(demand.relation_projection);

        let link = RepeatItemSnapshot {
            key: "7:20".into(),
            text: BTreeMap::from([
                (crate::ItemBindingKey::Ready, "Ready".into()),
                (crate::ItemBindingKey::State, "Active".into()),
                (crate::ItemBindingKey::SourceDescription, "Player".into()),
                (crate::ItemBindingKey::TargetDescription, "Speakers".into()),
                (crate::ItemBindingKey::IsMonitor, "false".into()),
            ]),
            tokens: BTreeMap::from([
                (crate::ItemBindingKey::Ready, "ready".into()),
                (crate::ItemBindingKey::State, "active".into()),
                (crate::ItemBindingKey::IsMonitor, "false".into()),
            ]),
            values: BTreeMap::from([
                (crate::ItemBindingKey::RawId, NumericValue::Integer(20)),
                (
                    crate::ItemBindingKey::SourcePortId,
                    NumericValue::Integer(30),
                ),
                (
                    crate::ItemBindingKey::TargetPortId,
                    NumericValue::Integer(40),
                ),
            ]),
            properties: BTreeMap::new(),
            channels: None,
            links: None,
            link_groups: None,
        };
        let group = RepeatItemSnapshot {
            key: "7:1:2".into(),
            text: BTreeMap::from([
                (crate::ItemBindingKey::State, "Active".into()),
                (crate::ItemBindingKey::SourceDescription, "Player".into()),
                (crate::ItemBindingKey::TargetDescription, "Speakers".into()),
                (crate::ItemBindingKey::SourceStatus, "Available".into()),
                (crate::ItemBindingKey::TargetStatus, "Available".into()),
                (crate::ItemBindingKey::IsMonitor, "false".into()),
                (
                    crate::ItemBindingKey::ConnectionDirection,
                    "Outgoing".into(),
                ),
                (crate::ItemBindingKey::PeerDescription, "Speakers".into()),
                (crate::ItemBindingKey::PeerStatus, "Available".into()),
            ]),
            tokens: BTreeMap::from([
                (crate::ItemBindingKey::State, "active".into()),
                (crate::ItemBindingKey::SourceStatus, "available".into()),
                (crate::ItemBindingKey::TargetStatus, "available".into()),
                (crate::ItemBindingKey::IsMonitor, "false".into()),
                (
                    crate::ItemBindingKey::ConnectionDirection,
                    "outgoing".into(),
                ),
                (crate::ItemBindingKey::PeerStatus, "available".into()),
            ]),
            values: BTreeMap::from([(
                crate::ItemBindingKey::MemberCount,
                NumericValue::Integer(1),
            )]),
            properties: BTreeMap::new(),
            channels: None,
            links: Some(crate::ContextualRepeatSnapshot {
                source_generation: 7,
                items: vec![link.clone()],
            }),
            link_groups: None,
        };
        let node = RepeatItemSnapshot {
            key: "7:1".into(),
            text: BTreeMap::from([(crate::ItemBindingKey::LinkGroupStatus, "Ready".into())]),
            tokens: BTreeMap::from([(crate::ItemBindingKey::LinkGroupStatus, "ready".into())]),
            values: BTreeMap::from([(
                crate::ItemBindingKey::LinkGroupCount,
                NumericValue::Integer(1),
            )]),
            properties: BTreeMap::new(),
            channels: None,
            links: None,
            link_groups: Some(crate::ContextualRepeatSnapshot {
                source_generation: 7,
                items: vec![group.clone()],
            }),
        };

        overlay
            .apply_repeat_source(&RepeatSourceSnapshot {
                source: RepeatSource::PipeWireNodes,
                source_generation: 7,
                items: vec![node.clone()],
            })
            .unwrap();
        overlay
            .apply_repeat_source(&RepeatSourceSnapshot {
                source: RepeatSource::PipeWireLinks,
                source_generation: 7,
                items: vec![link.clone()],
            })
            .unwrap();
        overlay
            .apply_repeat_source(&RepeatSourceSnapshot {
                source: RepeatSource::PipeWireLinkGroups,
                source_generation: 7,
                items: vec![group.clone()],
            })
            .unwrap();

        let link_root = overlay.repeats["link-row"].items["7:20"].root;
        let group_root = overlay.repeats["link-group-card"].items["7:1:2"].root;
        let member_root = overlay.repeats["link-group-card"].items["7:1:2"]
            .contextual_repeats["links-0"]
            .items["7:20"]
            .root;
        let tracker_root =
            overlay.repeats["node-route-card"].items["7:1"].contextual_repeats["link_groups-0"]
                .items["7:1:2"]
                .root;

        let mut updated_link = link;
        updated_link
            .text
            .insert(crate::ItemBindingKey::State, "Paused".into());
        updated_link
            .tokens
            .insert(crate::ItemBindingKey::State, "paused".into());
        let mut updated_group = group;
        updated_group
            .text
            .insert(crate::ItemBindingKey::State, "Paused".into());
        updated_group
            .tokens
            .insert(crate::ItemBindingKey::State, "paused".into());
        updated_group.links.as_mut().unwrap().items[0] = updated_link.clone();
        let mut updated_node = node;
        let tracked = &mut updated_node.link_groups.as_mut().unwrap().items[0];
        tracked
            .text
            .insert(crate::ItemBindingKey::State, "Paused".into());
        tracked
            .tokens
            .insert(crate::ItemBindingKey::State, "paused".into());
        tracked.text.insert(
            crate::ItemBindingKey::PeerDescription,
            "New speakers".into(),
        );

        overlay
            .apply_repeat_source(&RepeatSourceSnapshot {
                source: RepeatSource::PipeWireLinks,
                source_generation: 7,
                items: vec![updated_link],
            })
            .unwrap();
        overlay
            .apply_repeat_source(&RepeatSourceSnapshot {
                source: RepeatSource::PipeWireLinkGroups,
                source_generation: 7,
                items: vec![updated_group],
            })
            .unwrap();
        overlay
            .apply_repeat_source(&RepeatSourceSnapshot {
                source: RepeatSource::PipeWireNodes,
                source_generation: 7,
                items: vec![updated_node],
            })
            .unwrap();

        assert_eq!(overlay.repeats["link-row"].items["7:20"].root, link_root);
        assert_eq!(
            overlay.repeats["link-group-card"].items["7:1:2"].root,
            group_root
        );
        assert_eq!(
            overlay.repeats["link-group-card"].items["7:1:2"].contextual_repeats["links-0"]
                .items["7:20"]
                .root,
            member_root
        );
        assert_eq!(
            overlay.repeats["node-route-card"].items["7:1"].contextual_repeats["link_groups-0"]
                .items["7:1:2"]
                .root,
            tracker_root
        );
    }

    #[test]
    fn graph_only_relations_request_nodes_without_audio_or_write_demand() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "htmshell-pipewire-graph-demand-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("graph.html"),
            r#"
            <main id="overlay-card">
              <button id="overlay-close"
                      data-htm-element="action-button"
                      data-htm-action="overlay.close">Close</button>
              <button id="overlay-action"
                      data-htm-element="action-button"
                      data-htm-action="overlay.activate">Refresh</button>
              <span id="overlay-status"
                    data-htm-element="state-text"
                    data-htm-bind="overlay.status"></span>
              <template id="links" data-htm-element="repeat" data-htm-source="pipewire.links">
                <div>
                  <span data-htm-element="state-text"
                        data-htm-local-id="source"
                        data-htm-bind="item.source.description"></span>
                </div>
              </template>
            </main>
            "#,
        )
        .unwrap();
        let live = LiveDocument::load_surface_document(
            &root,
            "graph.html",
            LiveDocumentKind::TransientOverlay,
            640,
            480,
        )
        .unwrap();
        let demand = live.pipewire_demand();
        let _ = std::fs::remove_dir_all(root);

        assert!(demand.service);
        assert!(demand.nodes);
        assert!(demand.node_details);
        assert!(demand.link_collection);
        assert!(demand.link_details);
        assert!(demand.relation_projection);
        assert!(!demand.audio_state);
        assert!(!demand.audio_writes);
        assert!(!demand.channel_projection);
        assert!(!demand.channel_writes);
    }

    #[test]
    fn default_pipewire_controls_use_typed_targets_and_authoritative_values() {
        let mut overlay = LiveDocument::load_surface_document(
            audio_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1100,
            800,
        )
        .unwrap();
        overlay
            .apply_bound_values(&[
                (
                    StateBindingKey::PipeWireDefaultSinkVolume,
                    NumericValue::Decimal(0.72),
                ),
                (
                    StateBindingKey::PipeWireDefaultSourceVolume,
                    NumericValue::Decimal(0.31),
                ),
            ])
            .unwrap();
        overlay
            .apply_bound_booleans(&[
                (StateBindingKey::PipeWireDefaultSinkCanSetVolume, Some(true)),
                (StateBindingKey::PipeWireDefaultSinkCanSetMute, Some(true)),
                (
                    StateBindingKey::PipeWireDefaultSourceCanSetVolume,
                    Some(true),
                ),
                (StateBindingKey::PipeWireDefaultSourceCanSetMute, Some(true)),
                (StateBindingKey::PipeWireConfiguredSinkCanClear, Some(true)),
                (
                    StateBindingKey::PipeWireConfiguredSourceCanClear,
                    Some(true),
                ),
            ])
            .unwrap();

        let clear_bounds = selector_bounds(&overlay, "#clear-configured-output");
        let LiveAction::PipeWireDefault(clear) = click_action(&mut overlay, &clear_bounds) else {
            panic!("configured sink clear did not emit a PipeWire default action");
        };
        assert_eq!(clear.role, PipeWireDefaultRole::Sink);
        assert_eq!(clear.target, PipeWireDefaultTarget::Clear);

        let mute_bounds = selector_bounds(&overlay, "#default-output-toggle");
        let LiveAction::PipeWireAudio(mute) = click_action(&mut overlay, &mute_bounds) else {
            panic!("default mute control did not emit a PipeWire action");
        };
        assert_eq!(mute.target, PipeWireAudioTarget::DefaultSink);
        assert_eq!(mute.operation, PipeWireAudioOperation::ToggleMute);

        let range_bounds = selector_bounds(&overlay, "#default-input-range");
        let x = f64::from(range_bounds.x + range_bounds.width);
        let y = f64::from(range_bounds.y + range_bounds.height * 0.5);
        assert!(overlay.pointer_move(x - 0.5, y).unwrap());
        assert!(overlay.pointer_primary(true).unwrap());
        let LiveAction::PipeWireAudio(volume) = overlay
            .take_action()
            .expect("default range emits an action")
        else {
            panic!("default range did not emit a PipeWire action");
        };
        assert_eq!(volume.target, PipeWireAudioTarget::DefaultSource);
        assert_eq!(volume.operation, PipeWireAudioOperation::SetVolume);
        assert_eq!(volume.volume.unwrap().get(), 1.0);
        assert!(overlay.pointer_primary(false).unwrap());

        overlay
            .apply_pipewire_control_state(&volume.control, PipeWireControlState::Failed)
            .unwrap();
        overlay
            .apply_bound_values(&[(
                StateBindingKey::PipeWireDefaultSourceVolume,
                NumericValue::Decimal(0.31),
            )])
            .unwrap();
        assert_eq!(
            overlay
                .registered_attribute("default-input-range", "value")
                .unwrap(),
            Some("0.31".into())
        );

        overlay
            .apply_bound_booleans(&[(StateBindingKey::PipeWireDefaultSinkCanSetMute, Some(false))])
            .unwrap();
        assert!(
            overlay
                .registered_attribute("default-output-toggle", "disabled")
                .unwrap()
                .is_some()
        );

        let output_range = selector_bounds(&overlay, "#default-output-range");
        let output_x = f64::from(output_range.x + output_range.width * 0.8);
        let output_y = f64::from(output_range.y + output_range.height * 0.5);
        assert!(overlay.pointer_move(output_x, output_y).unwrap());
        assert!(overlay.pointer_primary(true).unwrap());
        assert!(overlay.take_action().is_some());
        overlay
            .apply_bound_booleans(&[(
                StateBindingKey::PipeWireDefaultSinkCanSetVolume,
                Some(false),
            )])
            .unwrap();
        assert!(overlay.take_action().is_none());
        assert!(!overlay.pointer_primary(false).unwrap());
        assert_eq!(
            overlay
                .registered_attribute("default-output-range", "value")
                .unwrap(),
            Some("0.72".into())
        );
        assert_eq!(
            overlay
                .registered_attribute("default-output-range", STATE_ATTRIBUTE)
                .unwrap(),
            Some("unavailable".into())
        );
    }

    #[test]
    fn explicit_peak_monitors_share_demand_and_reconcile_latest_channels() {
        let mut overlay = LiveDocument::load_surface_document(
            audio_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1100,
            1200,
        )
        .unwrap();
        let demand = overlay.pipewire_demand();
        assert!(demand.peak_monitor_declarations);
        assert!(demand.peak_maximum_projection);
        assert!(demand.peak_channel_projection);
        assert_eq!(overlay.pipewire_peak_demands(false).len(), 0);
        let mapped = overlay.pipewire_peak_demands(true);
        assert_eq!(mapped.len(), 2);
        assert!(
            mapped
                .iter()
                .all(|demand| demand.target == PipeWirePeakTarget::DefaultSink)
        );

        let channel = RepeatItemSnapshot {
            key: "4:0:3".into(),
            text: BTreeMap::from([
                (crate::ItemBindingKey::Position, "front-left".into()),
                (crate::ItemBindingKey::PositionName, "Front left".into()),
                (crate::ItemBindingKey::Status, "Ready".into()),
                (crate::ItemBindingKey::IsAuxiliary, "false".into()),
                (crate::ItemBindingKey::IsCustom, "false".into()),
            ]),
            tokens: BTreeMap::from([
                (crate::ItemBindingKey::Position, "front-left".into()),
                (crate::ItemBindingKey::Status, "ready".into()),
                (crate::ItemBindingKey::IsAuxiliary, "false".into()),
                (crate::ItemBindingKey::IsCustom, "false".into()),
            ]),
            values: BTreeMap::from([
                (crate::ItemBindingKey::Index, NumericValue::Integer(0)),
                (crate::ItemBindingKey::Peak, NumericValue::Decimal(0.75)),
            ]),
            properties: BTreeMap::new(),
            channels: None,
            links: None,
            link_groups: None,
        };
        let ready = PipeWirePeakProjection {
            node_item_key: "1:42".into(),
            stream_generation: 3,
            layout_generation: 4,
            state: PipeWirePeakStreamState::Ready,
            maximum: NumericValue::Decimal(0.75),
            channels: crate::ContextualRepeatSnapshot {
                source_generation: 4,
                items: vec![channel],
            },
        };
        let mut projections = PipeWirePeakProjectionSet {
            default_sink_item_key: Some("1:42".into()),
            default_source_item_key: Some("1:43".into()),
            monitorable_nodes: BTreeSet::from(["1:42".into(), "1:43".into()]),
            nodes: BTreeMap::from([("1:42".into(), ready)]),
        };
        assert!(
            overlay
                .apply_pipewire_peak_projections(&projections, true)
                .unwrap()
                > 0
        );
        assert_eq!(
            overlay
                .registered_attribute("default-output-peaks", STATE_ATTRIBUTE)
                .unwrap(),
            Some("ready".into())
        );
        assert_eq!(
            overlay
                .document
                .get_node(
                    required_selector(&overlay.document, "#default-output-peak-maximum").unwrap()
                )
                .and_then(|node| node.element_data())
                .and_then(|element| element.attr(local_name!("value"))),
            Some("0.75")
        );
        let channel_root =
            overlay.peak_monitors["default-output-peaks"].channel_repeats["peak-channels-0"].items
                ["4:0:3"]
                .root;

        let mut updated = projections.nodes["1:42"].clone();
        updated.maximum = NumericValue::Decimal(0.5);
        updated.channels.items[0]
            .values
            .insert(crate::ItemBindingKey::Peak, NumericValue::Decimal(0.5));
        projections.nodes.insert("1:42".into(), updated);
        overlay
            .apply_pipewire_peak_projections(&projections, true)
            .unwrap();
        assert_eq!(
            overlay.peak_monitors["default-output-peaks"].channel_repeats["peak-channels-0"].items
                ["4:0:3"]
                .root,
            channel_root
        );

        let enable_bounds = selector_bounds(&overlay, "#default-input-peak-enable");
        let LiveAction::PipeWirePeak(request) = click_action(&mut overlay, &enable_bounds) else {
            panic!("peak action emitted the wrong typed request");
        };
        assert_eq!(request.operation, PipeWirePeakOperation::Enable);
        assert!(overlay.apply_pipewire_peak_action(&request).unwrap());
        assert_eq!(
            overlay.peak_monitors["default-input-peaks"]
                .elements
                .iter()
                .find(|element| element.declaration.local_id == "default-input-peak-enable")
                .unwrap()
                .control_state,
            PipeWireControlState::Pending
        );
        assert_eq!(
            overlay.peak_monitors["default-input-peaks"]
                .elements
                .iter()
                .find(|element| element.declaration.local_id == "default-input-peak-disable")
                .unwrap()
                .control_state,
            PipeWireControlState::Idle
        );
        assert_eq!(overlay.pipewire_peak_demands(true).len(), 3);
        assert!(
            overlay
                .pipewire_peak_demands(true)
                .iter()
                .any(|demand| demand.target == PipeWirePeakTarget::DefaultSource)
        );

        let mut source = projections.nodes["1:42"].clone();
        source.node_item_key = "1:43".into();
        source.stream_generation = 5;
        source.state = PipeWirePeakStreamState::Failed;
        source.channels.items.clear();
        source.maximum = NumericValue::Unknown;
        projections.nodes.insert("1:43".into(), source.clone());
        overlay
            .apply_pipewire_peak_projections(&projections, true)
            .unwrap();
        assert_eq!(
            overlay.peak_monitors["default-input-peaks"]
                .elements
                .iter()
                .find(|element| element.declaration.local_id == "default-input-peak-enable")
                .unwrap()
                .control_state,
            PipeWireControlState::Failed
        );
        assert_eq!(
            overlay.peak_monitors["default-input-peaks"]
                .elements
                .iter()
                .find(|element| element.declaration.local_id == "default-input-peak-disable")
                .unwrap()
                .control_state,
            PipeWireControlState::Idle
        );
        source.state = PipeWirePeakStreamState::Ready;
        projections.nodes.insert("1:43".into(), source);
        overlay
            .apply_pipewire_peak_projections(&projections, true)
            .unwrap();
        assert_eq!(
            overlay.peak_monitors["default-input-peaks"]
                .elements
                .iter()
                .find(|element| element.declaration.local_id == "default-input-peak-enable")
                .unwrap()
                .control_state,
            PipeWireControlState::Idle
        );

        assert!(
            overlay
                .apply_pipewire_peak_projections(&projections, false)
                .unwrap()
                > 0
        );
        assert_eq!(
            overlay
                .registered_attribute("default-input-peaks", STATE_ATTRIBUTE)
                .unwrap(),
            Some("suspended".into())
        );
        assert!(overlay.pipewire_peak_demands(false).is_empty());
    }

    #[test]
    fn peak_enable_action_contains_the_per_document_active_limit() {
        let mut overlay = LiveDocument::load_surface_document(
            audio_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1100,
            1200,
        )
        .unwrap();
        let prototype = overlay.peak_monitors["shared-output-peaks"].clone();
        for index in 0..30 {
            let id = format!("modeled-enabled-peak-{index}");
            let mut monitor = prototype.clone();
            monitor.identity.locator = PipeWirePeakMonitorLocator::Element(id.clone());
            overlay.peak_monitors.insert(id, monitor);
        }
        assert_eq!(
            overlay.enabled_peak_monitor_count(),
            crate::MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT
        );

        let monitor = &overlay.peak_monitors["default-input-peaks"];
        let request = PipeWirePeakActionRequest {
            control: PipeWireControlIdentity {
                document_generation: overlay.document_identity,
                locator: PipeWireControlLocator::Peak {
                    monitor: monitor.identity.locator.clone(),
                    local_id: "default-input-peak-enable".into(),
                },
            },
            monitor: monitor.identity.clone(),
            operation: PipeWirePeakOperation::Enable,
        };
        assert!(overlay.apply_pipewire_peak_action(&request).unwrap());
        let monitor = &overlay.peak_monitors["default-input-peaks"];
        assert!(!monitor.enabled);
        assert_eq!(
            monitor
                .elements
                .iter()
                .find(|element| element.declaration.local_id == "default-input-peak-enable")
                .unwrap()
                .control_state,
            PipeWireControlState::Failed
        );
    }

    #[test]
    fn one_thousand_range_motion_events_retain_only_the_latest_intent() {
        let mut overlay = LiveDocument::load_surface_document(
            audio_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1100,
            800,
        )
        .unwrap();
        overlay
            .apply_bound_values(&[(
                StateBindingKey::PipeWireDefaultSinkVolume,
                NumericValue::Decimal(0.5),
            )])
            .unwrap();
        overlay
            .apply_bound_booleans(&[(StateBindingKey::PipeWireDefaultSinkCanSetVolume, Some(true))])
            .unwrap();
        let bounds = selector_bounds(&overlay, "#default-output-range");
        let y = f64::from(bounds.y + bounds.height * 0.5);
        assert!(overlay.pointer_move(f64::from(bounds.x + 1.0), y).unwrap());
        assert!(overlay.pointer_primary(true).unwrap());
        for index in 0..1_000 {
            let position = (index % 101) as f32 / 100.0;
            let x = bounds.x + position * bounds.width.max(1.0);
            overlay.pointer_move(f64::from(x), y).unwrap();
        }
        let LiveAction::PipeWireAudio(latest) = overlay
            .take_action()
            .expect("latest range intent is retained")
        else {
            panic!("range emitted the wrong action");
        };
        assert_eq!(latest.operation, PipeWireAudioOperation::SetVolume);
        assert!(latest.volume.unwrap().get() <= 1.0);
        assert!(overlay.take_action().is_none());
        overlay
            .apply_bound_values(&[(
                StateBindingKey::PipeWireDefaultSinkVolume,
                NumericValue::Decimal(0.27),
            )])
            .unwrap();
        assert!(overlay.pointer_leave());
        assert_eq!(
            overlay
                .registered_attribute("default-output-range", "value")
                .unwrap(),
            Some("0.27".into())
        );
        assert!(!overlay.pointer_primary(false).unwrap());
    }

    #[test]
    fn five_hundred_collection_changes_and_duplicates_remain_bounded() {
        let mut overlay = LiveDocument::load_surface_document(
            power_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            800,
            600,
        )
        .unwrap();
        let parse_count = overlay.snapshot().unwrap().document_parse_count;
        let scans = overlay.measurements().registry_scan_count;
        let mut last = RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: 1,
            items: Vec::new(),
        };
        let mut changed_passes = 0usize;
        for index in 0..500 {
            let first = device_item("1:/device/a", "A", index as f64 % 100.0);
            let second = device_item("1:/device/b", "B", 50.0);
            last.items = if index % 3 == 0 {
                vec![first]
            } else if index % 2 == 0 {
                vec![second, first]
            } else {
                vec![first, second]
            };
            let update = overlay.apply_repeat_source(&last).unwrap();
            changed_passes += usize::from(update.changed());
            assert!(overlay.repeats["device-row"].items.len() <= 2);
        }
        assert_eq!(changed_passes, 500);
        for _ in 0..500 {
            assert!(!overlay.apply_repeat_source(&last).unwrap().changed());
        }
        assert_eq!(
            overlay.snapshot().unwrap().document_parse_count,
            parse_count
        );
        let measurements = overlay.measurements();
        assert_eq!(measurements.registry_scan_count, scans);
        assert!(measurements.repeat_insertions > 0);
        assert!(measurements.repeat_removals > 0);
        assert!(measurements.repeat_moves > 0);
        assert!(measurements.repeat_property_updates > 0);
        assert!(measurements.repeat_unchanged_items >= 500);
        assert!(measurements.repeat_subtree_clones > 0);
        assert!(measurements.repeat_identity_reuses > 0);
        assert!(measurements.repeated_item_count <= 2);
        assert!(measurements.cloned_node_count <= MAX_CLONED_NODES_PER_REPEAT as u64);
    }

    #[test]
    fn state_value_and_dynamic_action_disable_mutate_incrementally() {
        let mut panel = LiveDocument::load_surface_document(
            power_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        assert!(element_attribute(&panel, "performance", "disabled").is_some());
        let disabled_bounds = panel.element_bounds("performance").unwrap();
        let x = f64::from(disabled_bounds.x + disabled_bounds.width / 2.0);
        let y = f64::from(disabled_bounds.y + disabled_bounds.height / 2.0);
        panel.pointer_move(x, y).unwrap();
        assert!(!panel.pointer_primary(true).unwrap());
        assert!(!panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), None);
        let enabled = panel
            .apply_bound_booleans(&[
                (StateBindingKey::PowerProfileAvailability, Some(true)),
                (
                    StateBindingKey::PowerProfilePerformanceAvailable,
                    Some(true),
                ),
            ])
            .unwrap();
        assert_eq!(enabled.changed_boolean_elements, 3);
        assert!(element_attribute(&panel, "performance", "disabled").is_none());
        assert_eq!(
            click_action(&mut panel, &disabled_bounds),
            LiveAction::PowerProfileSetPerformance
        );

        let mut overlay = LiveDocument::load_surface_document(
            power_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            800,
            600,
        )
        .unwrap();
        let energy_identity = overlay.element_identity("battery-energy").unwrap();
        let value = overlay
            .apply_bound_values(&[(StateBindingKey::BatteryEnergy, NumericValue::Decimal(42.25))])
            .unwrap();
        assert_eq!(value.changed_elements, 1);
        assert_eq!(overlay.element_text("battery-energy").unwrap(), "42.2 Wh");
        assert_eq!(
            element_attribute(&overlay, "battery-energy", "value").as_deref(),
            Some("42.25")
        );
        let unknown = overlay
            .apply_bound_values(&[(StateBindingKey::BatteryEnergy, NumericValue::Unknown)])
            .unwrap();
        assert_eq!(unknown.changed_elements, 1);
        assert_eq!(element_attribute(&overlay, "battery-energy", "value"), None);
        assert_eq!(
            overlay.element_identity("battery-energy").unwrap(),
            energy_identity
        );
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(overlay.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(overlay.measurements().registry_scan_count, 1);
    }

    #[test]
    fn live_document_parses_once_across_resize_and_interaction() {
        let mut live = LiveDocument::load(fixture(), 800, 600).unwrap();
        let initial = live.snapshot().unwrap();
        assert_eq!(initial.document_parse_count, 1);

        assert!(live.set_viewport(1024, 768).unwrap());
        let resized = live.snapshot().unwrap();
        assert_eq!(resized.document_parse_count, 1);
        assert_eq!(initial.document_identity, resized.document_identity);
        assert_eq!(initial.card_identity, resized.card_identity);
        assert_eq!(initial.action_identity, resized.action_identity);

        let point = (
            resized.action_bounds.x + resized.action_bounds.width / 2.0,
            resized.action_bounds.y + resized.action_bounds.height / 2.0,
        );
        assert!(
            live.pointer_move(f64::from(point.0), f64::from(point.1))
                .unwrap()
        );
        assert!(live.snapshot().unwrap().interaction.hovered);
        assert!(live.pointer_primary(true).unwrap());
        assert!(live.snapshot().unwrap().interaction.active);
        assert!(live.pointer_primary(false).unwrap());
        let clicked = live.snapshot().unwrap();
        assert!(!clicked.interaction.active);
        assert_eq!(clicked.interaction.click_count, 1);
        assert_eq!(clicked.document_parse_count, 1);
        assert_eq!(clicked.action_identity, initial.action_identity);
    }

    #[test]
    fn identical_pointer_motion_does_not_dirty_the_document() {
        let mut live = LiveDocument::load(fixture(), 800, 600).unwrap();
        let action = live.snapshot().unwrap().action_bounds;
        let x = f64::from(action.x + action.width / 2.0);
        let y = f64::from(action.y + action.height / 2.0);
        assert!(live.pointer_move(x, y).unwrap());
        assert!(!live.pointer_move(x, y).unwrap());
        assert!(live.pointer_leave());
        assert!(!live.snapshot().unwrap().interaction.hovered);
        assert!(!live.pointer_leave());
    }

    #[test]
    fn live_cpu_color_filters_use_the_reference_compositor_and_return_to_idle() {
        let mut live = LiveDocument::load(fixture(), 640, 480).unwrap();
        let before = live.render().unwrap();
        let bounds = live.snapshot().unwrap().card_bounds;
        let x = (bounds.x + 12.0).floor() as usize;
        let y = (bounds.y + 12.0).floor() as usize;
        let offset = (y * 640 + x) * 4;
        let source: [u8; 4] = before.premultiplied_rgba[offset..offset + 4]
            .try_into()
            .unwrap();
        assert!(source[3] > 0);

        let card = live
            .document
            .query_selector("#shell-card")
            .unwrap()
            .unwrap();
        live.document.mutate().set_attribute(
            card,
            QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: LocalName::from("style"),
            },
            "filter:invert(1)",
        );
        live.resolve();
        let filtered = live.render().unwrap();
        let actual: [u8; 4] = filtered.premultiplied_rgba[offset..offset + 4]
            .try_into()
            .unwrap();
        assert_eq!(
            actual,
            [
                source[3] - source[0],
                source[3] - source[1],
                source[3] - source[2],
                source[3],
            ]
        );
        assert!(
            live.render_pending_for(
                LiveRenderRequest::new(640, 480, LIVE_SCALE_DENOMINATOR).unwrap(),
                live.document_identity.serial,
                1,
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn live_cpu_blur_uses_the_reference_compositor_and_returns_to_idle() {
        let mut live = LiveDocument::load(fixture(), 640, 480).unwrap();
        let before = live.render().unwrap();
        let bounds = live.snapshot().unwrap().card_bounds;
        let outside_x = (bounds.x - 2.0).floor() as usize;
        let x = (bounds.x + 1.0).floor() as usize;
        let y = (bounds.y + bounds.height / 2.0).floor() as usize;
        let offset = (y * 640 + x) * 4;
        let outside_offset = (y * 640 + outside_x) * 4;
        assert_eq!(
            &before.premultiplied_rgba[outside_offset..outside_offset + 4],
            &[0, 0, 0, 0]
        );
        let source: [u8; 4] = before.premultiplied_rgba[offset..offset + 4]
            .try_into()
            .unwrap();

        let card = live
            .document
            .query_selector("#shell-card")
            .unwrap()
            .unwrap();
        live.document.mutate().set_attribute(
            card,
            QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: LocalName::from("style"),
            },
            "filter:blur(2px)",
        );
        live.resolve();
        let filtered = live.render().unwrap();
        let fringe: [u8; 4] = filtered.premultiplied_rgba[offset..offset + 4]
            .try_into()
            .unwrap();
        assert_ne!(fringe, source);
        assert!(fringe[3] > 0);
        assert!(fringe[0] <= fringe[3]);
        assert!(fringe[1] <= fringe[3]);
        assert!(fringe[2] <= fringe[3]);
        assert_eq!(
            &filtered.premultiplied_rgba[outside_offset..outside_offset + 4],
            &[0, 0, 0, 0]
        );
        assert!(
            live.render_pending_for(
                LiveRenderRequest::new(640, 480, LIVE_SCALE_DENOMINATOR).unwrap(),
                live.document_identity.serial,
                1,
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn rendered_frame_is_premultiplied_rgba_and_has_input_region() {
        let mut live = LiveDocument::load(fixture(), 640, 480).unwrap();
        let frame = live.render().unwrap();
        assert_eq!(frame.premultiplied_rgba.len(), 640 * 480 * 4);
        assert_eq!(frame.input_regions.len(), 1);
        assert!(frame.input_regions[0].width > 0.0);
        assert!(frame.interactive_region.width > 0.0);
        for pixel in frame.premultiplied_rgba.chunks_exact(4) {
            assert!(pixel[0] <= pixel[3]);
            assert!(pixel[1] <= pixel[3]);
            assert!(pixel[2] <= pixel[3]);
        }
        assert!(
            live.render_pending_for(
                LiveRenderRequest::new(640, 480, LIVE_SCALE_DENOMINATOR).unwrap(),
                live.document_identity.serial,
                1,
            )
            .unwrap()
            .is_none()
        );
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn live_gpu_preparation_is_provisional_and_cpu_fallback_is_complete() {
        let mut reference = LiveDocument::load(fixture(), 640, 480).unwrap();
        let reference_frame = reference.render().unwrap();
        let bounds = reference.snapshot().unwrap().card_bounds;
        let x = (bounds.x + 12.0).floor() as usize;
        let y = (bounds.y + 12.0).floor() as usize;
        let offset = (y * 640 + x) * 4;
        let source: [u8; 4] = reference_frame.premultiplied_rgba[offset..offset + 4]
            .try_into()
            .unwrap();

        let mut live = LiveDocument::load(fixture(), 640, 480).unwrap();
        let card = live
            .document
            .query_selector("#shell-card")
            .unwrap()
            .unwrap();
        live.document.mutate().set_attribute(
            card,
            QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: LocalName::from("style"),
            },
            "filter:invert(1)",
        );
        live.resolve();
        let request = LiveRenderRequest::new(640, 480, LIVE_SCALE_DENOMINATOR).unwrap();
        let prepared = live
            .prepare_gpu_pending_for(request, 51, 7)
            .unwrap()
            .expect("initial GPU plan");
        assert_eq!(prepared.generation(), 1);
        assert_eq!(live.frame_generation, 0);
        assert!(matches!(prepared.plan().damage, DamageRegion::Full));
        live.reject_gpu_frame(prepared, true);
        assert_eq!(live.frame_generation, 0);

        let replacement = live
            .prepare_gpu_pending_for(request, 51, 7)
            .unwrap()
            .expect("recovery plan");
        assert!(
            replacement
                .plan()
                .reasons
                .contains(&FrameReason::RendererRecovery)
        );
        let frame = live.render_gpu_frame_on_cpu(replacement).unwrap();
        assert_eq!(frame.generation, 1);
        assert_eq!(frame.premultiplied_rgba.len(), 640 * 480 * 4);
        assert_eq!(
            &frame.premultiplied_rgba[offset..offset + 4],
            &[
                source[3] - source[0],
                source[3] - source[1],
                source[3] - source[2],
                source[3],
            ]
        );
        assert_eq!(live.frame_generation, 1);
        assert!(
            live.prepare_gpu_pending_for(request, 51, 7)
                .unwrap()
                .is_none()
        );
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn live_gpu_blur_fallback_matches_the_complete_cpu_reference_frame() {
        fn set_blur(live: &mut LiveDocument) {
            let card = live
                .document
                .query_selector("#shell-card")
                .unwrap()
                .unwrap();
            live.document.mutate().set_attribute(
                card,
                QualName {
                    prefix: None,
                    ns: blitz_dom::Namespace::from(""),
                    local: LocalName::from("style"),
                },
                "filter:brightness(1.2) blur(2px)",
            );
            live.resolve();
        }

        let mut reference = LiveDocument::load(fixture(), 640, 480).unwrap();
        set_blur(&mut reference);
        let expected = reference.render().unwrap();

        let mut live = LiveDocument::load(fixture(), 640, 480).unwrap();
        set_blur(&mut live);
        let request = LiveRenderRequest::new(640, 480, LIVE_SCALE_DENOMINATOR).unwrap();
        let prepared = live
            .prepare_gpu_pending_for(request, 61, 13)
            .unwrap()
            .expect("blur fallback plan");
        let actual = live.render_gpu_frame_on_cpu(prepared).unwrap();
        assert_eq!(actual.premultiplied_rgba, expected.premultiplied_rgba);
        assert_eq!(actual.generation, 1);
        assert!(
            live.prepare_gpu_pending_for(request, 61, 13)
                .unwrap()
                .is_none()
        );
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn accepted_live_gpu_plan_advances_retained_state_without_cpu_pixels() {
        let mut live = LiveDocument::load(fixture(), 320, 240).unwrap();
        let request = LiveRenderRequest::new(320, 240, 150).unwrap();
        let prepared = live
            .prepare_gpu_pending_for(request, 52, 9)
            .unwrap()
            .expect("initial GPU plan");
        assert_eq!((prepared.buffer_width, prepared.buffer_height), (400, 300));
        live.accept_gpu_frame(prepared);
        assert_eq!(live.frame_generation, 1);
        assert!(
            live.prepare_gpu_pending_for(request, 52, 9)
                .unwrap()
                .is_none()
        );
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn cpu_fallback_creates_its_target_after_a_successful_gpu_frame() {
        let mut live = LiveDocument::load(fixture(), 320, 240).unwrap();
        let request = LiveRenderRequest::new(320, 240, LIVE_SCALE_DENOMINATOR).unwrap();
        let initial = live
            .prepare_gpu_pending_for(request, 53, 11)
            .unwrap()
            .expect("initial GPU plan");
        live.accept_gpu_frame(initial);

        let action = live.snapshot().unwrap().action_bounds;
        assert!(
            live.pointer_move(
                f64::from(action.x + action.width / 2.0),
                f64::from(action.y + action.height / 2.0),
            )
            .unwrap()
        );
        let changed = live
            .prepare_gpu_pending_for(request, 53, 11)
            .unwrap()
            .expect("visual mutation after GPU presentation");
        let fallback = live.render_gpu_frame_on_cpu(changed).unwrap();
        assert_eq!(fallback.generation, 2);
        assert_eq!(fallback.premultiplied_rgba.len(), 320 * 240 * 4);
        assert_eq!(live.frame_generation, 2);
    }

    #[test]
    fn invalid_dimensions_are_rejected() {
        assert!(LiveDocument::load(fixture(), 0, 600).is_err());
        assert!(LiveDocument::load(fixture(), u32::MAX, 600).is_err());
    }

    #[test]
    fn checked_fractional_dimensions_use_coverage_preserving_ceiling() {
        for (logical, numerator, expected) in [
            ((100, 50), 120, (100, 50)),
            ((100, 50), 150, (125, 63)),
            ((100, 50), 180, (150, 75)),
            ((101, 51), 150, (127, 64)),
            ((1, 1), 210, (2, 2)),
            ((101, 51), 240, (202, 102)),
        ] {
            let request = LiveRenderRequest::new(logical.0, logical.1, numerator).unwrap();
            assert_eq!((request.buffer_width, request.buffer_height), expected);
            assert_eq!(request.scale_denominator, 120);
        }
        assert!(LiveRenderRequest::new(8192, 8192, 120).is_ok());
        assert!(LiveRenderRequest::new(8193, 8193, 120).is_err());
        assert!(LiveRenderRequest::new(100, 50, 0).is_err());
        assert!(LiveRenderRequest::new(100, 50, MAX_LIVE_SCALE_NUMERATOR + 1).is_err());
    }

    #[test]
    fn fractional_render_changes_pixels_not_logical_layout_or_identity() {
        let mut live = LiveDocument::load(fixture(), 801, 601).unwrap();
        let before = live.snapshot().unwrap();
        for numerator in [120, 150, 180, 210, 240] {
            let request = LiveRenderRequest::new(801, 601, numerator).unwrap();
            let frame = live.render_for(request).unwrap();
            let after = live.snapshot().unwrap();
            assert_eq!((frame.logical_width, frame.logical_height), (801, 601));
            assert_eq!(
                (frame.buffer_width, frame.buffer_height),
                (request.buffer_width, request.buffer_height)
            );
            assert_eq!(before.viewport, after.viewport);
            assert_eq!(before.card_bounds, after.card_bounds);
            assert_eq!(before.action_bounds, after.action_bounds);
            assert_eq!(before.document_identity, after.document_identity);
            assert_eq!(before.card_identity, after.card_identity);
            assert_eq!(after.document_parse_count, 1);
            assert_eq!(
                frame.premultiplied_rgba.len(),
                request.buffer_width as usize * request.buffer_height as usize * 4
            );

            let (x0, y0, x1, y1) = alpha_bounds(&frame).expect("card paints nontransparent pixels");
            let scale = numerator as f32 / LIVE_SCALE_DENOMINATOR as f32;
            let expected_x0 = (before.card_bounds.x * scale).floor() as i32;
            let expected_y0 = (before.card_bounds.y * scale).floor() as i32;
            let expected_x1 =
                ((before.card_bounds.x + before.card_bounds.width) * scale).ceil() as i32;
            let expected_y1 =
                ((before.card_bounds.y + before.card_bounds.height) * scale).ceil() as i32;
            assert!((x0 as i32 - expected_x0).abs() <= 2);
            assert!((y0 as i32 - expected_y0).abs() <= 2);
            assert!((x1 as i32 + 1 - expected_x1).abs() <= 2);
            assert!((y1 as i32 + 1 - expected_y1).abs() <= 2);
        }

        let boundary_x = before.action_bounds.x + before.action_bounds.width - 0.25;
        let boundary_y = before.action_bounds.y + before.action_bounds.height - 0.25;
        assert!(
            live.pointer_move(f64::from(boundary_x), f64::from(boundary_y))
                .unwrap()
        );
        assert!(live.snapshot().unwrap().interaction.hovered);
        live.render_for(LiveRenderRequest::new(801, 601, 210).unwrap())
            .unwrap();
        let after_scaled_hit = live.snapshot().unwrap();
        assert!(after_scaled_hit.interaction.hovered);
        assert_eq!(after_scaled_hit.action_bounds, before.action_bounds);
        assert_eq!(after_scaled_hit.document_parse_count, 1);
    }

    #[test]
    fn invalid_package_path_is_contained_as_an_error() {
        assert!(LiveDocument::load(fixture().join("does-not-exist"), 800, 600).is_err());
    }

    #[test]
    fn panel_document_parses_once_and_emits_toggle_action() {
        let mut panel =
            LiveDocument::load_surface(multi_fixture(), LiveDocumentKind::Panel, 1280, 52).unwrap();
        let initial = panel.snapshot().unwrap();
        assert_eq!(initial.document_parse_count, 1);
        assert_eq!(
            click_action(&mut panel, &initial.action_bounds),
            LiveAction::ToggleOverlay
        );
        panel.update_panel_state(true, "Opened from panel").unwrap();
        panel.set_viewport(1440, 52).unwrap();
        let changed = panel.snapshot().unwrap();
        assert_eq!(changed.document_parse_count, 1);
        assert_eq!(initial.document_identity, changed.document_identity);
        assert_eq!(initial.card_identity, changed.card_identity);
        assert_eq!(initial.action_identity, changed.action_identity);
    }

    #[test]
    fn transient_overlay_emits_independent_actions_without_reparse() {
        let mut overlay = LiveDocument::load_surface(
            multi_fixture(),
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        )
        .unwrap();
        let initial = overlay.snapshot().unwrap();
        assert_eq!(
            click_action(&mut overlay, &initial.action_bounds),
            LiveAction::ActivateOverlay
        );
        overlay
            .update_overlay_state(1, "Overlay state updated")
            .unwrap();
        let close = overlay.bounds_for("#overlay-close").unwrap();
        assert_eq!(click_action(&mut overlay, &close), LiveAction::CloseOverlay);
        let changed = overlay.snapshot().unwrap();
        assert_eq!(changed.document_parse_count, 1);
        assert_eq!(initial.document_identity, changed.document_identity);
        assert_eq!(initial.card_identity, changed.card_identity);
        assert_eq!(initial.action_identity, changed.action_identity);
    }

    #[test]
    fn surface_state_updates_are_profile_scoped() {
        let mut panel =
            LiveDocument::load_surface(multi_fixture(), LiveDocumentKind::Panel, 800, 52).unwrap();
        let mut overlay = LiveDocument::load_surface(
            multi_fixture(),
            LiveDocumentKind::TransientOverlay,
            800,
            600,
        )
        .unwrap();
        assert!(panel.update_overlay_state(1, "invalid").is_err());
        assert!(overlay.update_panel_state(true, "invalid").is_err());
        let panel_snapshot = panel.snapshot().unwrap();
        let overlay_snapshot = overlay.snapshot().unwrap();
        assert_eq!(panel_snapshot.document_parse_count, 1);
        assert_eq!(overlay_snapshot.document_parse_count, 1);
        assert_ne!(
            panel_snapshot.document_identity,
            overlay_snapshot.document_identity
        );
    }

    #[test]
    fn custom_surface_documents_are_independent_and_contextualized() {
        let mut first = LiveDocument::load_surface_document(
            manifest_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            52,
        )
        .unwrap();
        let mut second = LiveDocument::load_surface_document(
            manifest_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            52,
        )
        .unwrap();
        first.set_instance_context("panel", "output-a").unwrap();
        second.set_instance_context("panel", "output-b").unwrap();
        first.update_panel_state(true, "output A opened").unwrap();
        let first_snapshot = first.snapshot().unwrap();
        let second_snapshot = second.snapshot().unwrap();
        assert_eq!(first_snapshot.document_parse_count, 1);
        assert_eq!(second_snapshot.document_parse_count, 1);
        assert_ne!(
            first_snapshot.document_identity,
            second_snapshot.document_identity
        );
        assert_eq!(second_snapshot.interaction.click_count, 0);
    }

    #[test]
    fn custom_document_path_cannot_escape_package() {
        assert!(
            LiveDocument::load_surface_document(
                manifest_fixture(),
                "../two-surface-shell/panel.html",
                LiveDocumentKind::Panel,
                1280,
                52,
            )
            .is_err()
        );
    }

    #[test]
    fn built_in_panel_discovers_once_binds_text_and_dispatches_typed_action() {
        let mut panel = LiveDocument::load_surface_document(
            built_in_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        assert_eq!(
            panel.built_in_summary(),
            BuiltInElementSummary {
                registered_elements: 7,
                bindings: 5,
                text_bindings: 5,
                token_bindings: 0,
                actions: 2,
                clock_declarations: 0,
                value_bindings: 0,
                boolean_bindings: 0,
                repeat_declarations: 0,
                discovery_scans: 1,
            }
        );
        let identity = panel.element_identity("panel-status").unwrap();
        let initial_document = panel.snapshot().unwrap().document_identity;
        let update = panel
            .apply_bound_text(&binding_values(
                "Output: A",
                "Scale: 1.50×",
                "Surface: panel",
                "Overlay: closed",
                "Activations: 0",
                "Last action: Ready",
            ))
            .unwrap();
        assert_eq!(update.changed_keys, 4);
        assert_eq!(update.changed_elements, 5);
        assert_eq!(
            panel.element_text("panel-status").unwrap(),
            "Overlay: closed"
        );
        assert_eq!(panel.element_identity("panel-status").unwrap(), identity);
        let duplicate = panel
            .apply_bound_text(&binding_values(
                "Output: A",
                "Scale: 1.50×",
                "Surface: panel",
                "Overlay: closed",
                "Activations: 0",
                "Last action: Ready",
            ))
            .unwrap();
        assert_eq!(duplicate.changed_elements, 0);
        assert_eq!(duplicate.suppressed_keys, 4);

        let bounds = panel.element_bounds("overlay-toggle").unwrap();
        assert_eq!(click_action(&mut panel, &bounds), LiveAction::ToggleOverlay);
        panel.set_viewport(1440, 58).unwrap();
        panel
            .render_for(LiveRenderRequest::new(1440, 58, 180).unwrap())
            .unwrap();
        assert_eq!(
            panel.snapshot().unwrap().document_identity,
            initial_document
        );
        assert_eq!(panel.element_identity("panel-status").unwrap(), identity);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn built_in_action_click_requires_same_enabled_button() {
        let mut panel = LiveDocument::load_surface_document(
            built_in_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let toggle = panel.element_bounds("overlay-toggle").unwrap();
        let x = f64::from(toggle.x + toggle.width / 2.0);
        let y = f64::from(toggle.y + toggle.height / 2.0);
        assert!(panel.pointer_move(x, y).unwrap());
        assert!(panel.pointer_primary(true).unwrap());
        assert!(panel.pointer_move(1.0, 1.0).unwrap());
        assert!(panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), None);

        assert!(panel.pointer_move(1.0, 1.0).is_ok());
        assert!(!panel.pointer_primary(true).unwrap());
        assert!(panel.pointer_move(x, y).unwrap());
        assert!(!panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), None);

        assert!(!panel.pointer_move(x, y).unwrap());
        assert!(panel.pointer_primary(true).unwrap());
        assert!(panel.pointer_leave());
        assert_eq!(panel.take_action(), None);

        let disabled = panel.element_bounds("disabled-action").unwrap();
        panel
            .pointer_move(
                f64::from(disabled.x + disabled.width / 2.0),
                f64::from(disabled.y + disabled.height / 2.0),
            )
            .unwrap();
        assert!(!panel.pointer_primary(true).unwrap());
        assert!(!panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), None);
    }

    #[test]
    fn finite_negative_pointer_motion_is_a_contained_leave() {
        let mut panel = LiveDocument::load_surface_document(
            built_in_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let toggle = panel.element_bounds("overlay-toggle").unwrap();
        let x = f64::from(toggle.x + toggle.width / 2.0);
        let y = f64::from(toggle.y + toggle.height / 2.0);

        assert!(panel.pointer_move(x, y).unwrap());
        assert!(panel.pointer_primary(true).unwrap());
        assert!(panel.pointer_move(-0.25, y).unwrap());
        assert!(!panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), None);
        assert!(panel.pointer_move(-1.0, -1.0).is_ok());
        assert!(panel.pointer_move(f64::NAN, y).is_err());
    }

    #[test]
    fn built_in_overlay_actions_and_state_are_generation_scoped() {
        let mut first = LiveDocument::load_surface_document(
            built_in_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        )
        .unwrap();
        let second = LiveDocument::load_surface_document(
            built_in_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        )
        .unwrap();
        let stale_for_second = first.element_identity("overlay-action").unwrap();
        assert!(second.validate_element_identity(&stale_for_second).is_err());

        first
            .apply_bound_text(&binding_values(
                "Output: A",
                "Scale: 1.25×",
                "Surface: overlay",
                "Overlay: open",
                "Activations: 0",
                "Last action: Ready",
            ))
            .unwrap();
        let action_identity = first.element_identity("overlay-action").unwrap();
        let action = first.element_bounds("overlay-action").unwrap();
        assert_eq!(
            click_action(&mut first, &action),
            LiveAction::ActivateOverlay
        );
        first
            .apply_bound_text(&[(
                StateBindingKey::OverlayActivationCount,
                "Activations: 1".into(),
            )])
            .unwrap();
        assert_eq!(
            first.element_text("overlay-count").unwrap(),
            "Activations: 1"
        );
        assert_eq!(
            first.element_identity("overlay-action").unwrap(),
            action_identity
        );
        let close = first.element_bounds("overlay-close").unwrap();
        assert_eq!(click_action(&mut first, &close), LiveAction::CloseOverlay);
    }

    #[test]
    fn typed_visual_state_is_incremental_and_preserves_author_markup() {
        let mut panel = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let summary = panel.built_in_summary();
        assert_eq!(summary.registered_elements, 6);
        assert_eq!(summary.bindings, 5);
        assert_eq!(summary.text_bindings, 3);
        assert_eq!(summary.token_bindings, 2);
        assert_eq!(summary.actions, 1);
        assert_eq!(summary.discovery_scans, 1);
        assert!(panel.resource_request_count() >= 3);
        assert!(
            panel
                .diagnostics()
                .iter()
                .all(|diagnostic| !diagnostic.code.starts_with("resource."))
        );
        assert_eq!(
            panel.element_state_token("scale-profile").unwrap(),
            "scale-1"
        );
        assert_eq!(
            panel.element_state_token("overlay-toggle-state").unwrap(),
            "closed"
        );
        assert_eq!(
            element_attribute(&panel, "scale-profile", "class").as_deref(),
            Some("profile-pill")
        );
        assert_eq!(
            element_attribute(&panel, "scale-profile", "data-role").as_deref(),
            Some("presentation-profile")
        );
        let profile_identity = panel.element_identity("scale-profile").unwrap();
        let button_identity = panel.element_identity("overlay-toggle").unwrap();
        let parse_count = panel.snapshot().unwrap().document_parse_count;

        let update = panel
            .apply_bound_state(
                &[(StateBindingKey::OutputScale, "Scale: 1.50×".to_owned())],
                &[
                    (StateBindingKey::OverlayStatus, StateToken::Open),
                    (StateBindingKey::SurfaceScaleProfile, StateToken::Fractional),
                ],
            )
            .unwrap();
        assert_eq!(update.changed_keys, 3);
        assert_eq!(update.changed_elements, 3);
        assert_eq!(update.changed_text_elements, 1);
        assert_eq!(update.changed_token_elements, 2);
        assert_eq!(panel.element_text("scale-label").unwrap(), "Scale: 1.50×");
        assert_eq!(
            panel.element_state_token("overlay-toggle-state").unwrap(),
            "open"
        );
        assert_eq!(
            panel.element_state_token("scale-profile").unwrap(),
            "fractional"
        );
        assert_eq!(
            element_attribute(&panel, "scale-profile", "class").as_deref(),
            Some("profile-pill")
        );
        assert_eq!(
            element_attribute(&panel, "scale-profile", "data-role").as_deref(),
            Some("presentation-profile")
        );
        assert_eq!(
            panel.element_identity("scale-profile").unwrap(),
            profile_identity
        );
        assert_eq!(
            panel.element_identity("overlay-toggle").unwrap(),
            button_identity
        );
        assert_eq!(panel.snapshot().unwrap().document_parse_count, parse_count);
        assert_eq!(panel.measurements().registry_scan_count, 1);

        let duplicate = panel
            .apply_bound_state(
                &[(StateBindingKey::OutputScale, "Scale: 1.50×".to_owned())],
                &[
                    (StateBindingKey::OverlayStatus, StateToken::Open),
                    (StateBindingKey::SurfaceScaleProfile, StateToken::Fractional),
                ],
            )
            .unwrap();
        assert_eq!(duplicate.changed_elements, 0);
        assert_eq!(duplicate.suppressed_keys, 3);
        assert_eq!(panel.measurements().suppressed_token_updates, 2);
    }

    #[test]
    fn token_change_drives_css_without_reparse_or_rescan() {
        let mut panel = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let identity = panel.element_identity("overlay-toggle-state").unwrap();
        let before = panel.render().unwrap();
        let update = panel
            .apply_bound_tokens(&[(StateBindingKey::OverlayStatus, StateToken::Open)])
            .unwrap();
        assert_eq!(update.changed_token_elements, 1);
        let after = panel.render().unwrap();
        assert_ne!(before.premultiplied_rgba, after.premultiplied_rgba);
        assert_eq!(
            panel.element_identity("overlay-toggle-state").unwrap(),
            identity
        );
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn one_state_key_coalesces_text_and_token_projections() {
        let mut overlay = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        )
        .unwrap();
        let text_identity = overlay.element_identity("overlay-status").unwrap();
        let token_identity = overlay.element_identity("overlay-state-token").unwrap();
        let update = overlay
            .apply_bound_state(
                &[(StateBindingKey::OverlayStatus, "Overlay: open".to_owned())],
                &[(StateBindingKey::OverlayStatus, StateToken::Open)],
            )
            .unwrap();
        assert_eq!(update.changed_keys, 1);
        assert_eq!(update.changed_elements, 2);
        assert_eq!(update.changed_text_elements, 1);
        assert_eq!(update.changed_token_elements, 1);
        assert_eq!(
            overlay.element_text("overlay-status").unwrap(),
            "Overlay: open"
        );
        assert_eq!(
            overlay.element_state_token("overlay-state-token").unwrap(),
            "open"
        );
        assert_eq!(
            overlay.element_identity("overlay-status").unwrap(),
            text_identity
        );
        assert_eq!(
            overlay.element_identity("overlay-state-token").unwrap(),
            token_identity
        );
        assert_eq!(overlay.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(overlay.measurements().registry_scan_count, 1);
    }

    #[test]
    fn invalid_typed_token_is_contained_before_mutation() {
        let mut panel = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        assert!(
            panel
                .apply_bound_tokens(&[(StateBindingKey::OverlayStatus, StateToken::Scale1,)])
                .is_err()
        );
        assert_eq!(
            panel.element_state_token("overlay-toggle-state").unwrap(),
            "closed"
        );
    }

    #[test]
    fn nested_action_descendant_and_token_update_preserve_click() {
        for selector in ["#overlay-toggle img", "#overlay-toggle > span"] {
            let mut panel = LiveDocument::load_surface_document(
                static_panel_fixture(),
                "panel.html",
                LiveDocumentKind::Panel,
                1280,
                62,
            )
            .unwrap();
            let bounds = selector_bounds(&panel, selector);
            assert_eq!(click_action(&mut panel, &bounds), LiveAction::ToggleOverlay);
        }

        let mut panel = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let token = panel.element_bounds("overlay-toggle-state").unwrap();
        let x = f64::from(token.x + token.width / 2.0);
        let y = f64::from(token.y + token.height / 2.0);
        assert!(panel.pointer_move(x, y).unwrap());
        assert!(panel.pointer_primary(true).unwrap());
        panel
            .apply_bound_tokens(&[(StateBindingKey::OverlayStatus, StateToken::Open)])
            .unwrap();
        assert!(panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), Some(LiveAction::ToggleOverlay));
    }

    #[test]
    fn token_identity_survives_viewport_and_fractional_render_changes() {
        let mut panel = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let identity = panel.element_identity("scale-profile").unwrap();
        panel.set_viewport(1440, 62).unwrap();
        panel
            .render_for(LiveRenderRequest::new(1440, 62, 180).unwrap())
            .unwrap();
        panel
            .apply_bound_tokens(&[(StateBindingKey::SurfaceScaleProfile, StateToken::Fractional)])
            .unwrap();
        assert_eq!(panel.element_identity("scale-profile").unwrap(), identity);
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn token_state_is_isolated_across_output_document_generations() {
        let mut panel_a = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let panel_b = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let a_identity = panel_a.element_identity("overlay-toggle-state").unwrap();
        let b_identity = panel_b.element_identity("overlay-toggle-state").unwrap();
        assert_ne!(a_identity, b_identity);

        panel_a
            .apply_bound_tokens(&[
                (StateBindingKey::OverlayStatus, StateToken::Open),
                (StateBindingKey::SurfaceScaleProfile, StateToken::Fractional),
            ])
            .unwrap();
        assert_eq!(
            panel_a.element_state_token("overlay-toggle-state").unwrap(),
            "open"
        );
        assert_eq!(
            panel_b.element_state_token("overlay-toggle-state").unwrap(),
            "closed"
        );
        assert_eq!(
            panel_a.element_state_token("scale-profile").unwrap(),
            "fractional"
        );
        assert_eq!(
            panel_b.element_state_token("scale-profile").unwrap(),
            "scale-1"
        );

        drop(panel_a);
        let mut stale_identity = a_identity;
        for _ in 0..25 {
            let replacement_a = LiveDocument::load_surface_document(
                static_panel_fixture(),
                "panel.html",
                LiveDocumentKind::Panel,
                1280,
                62,
            )
            .unwrap();
            assert!(
                replacement_a
                    .validate_element_identity(&stale_identity)
                    .is_err()
            );
            assert_eq!(
                replacement_a
                    .element_state_token("overlay-toggle-state")
                    .unwrap(),
                "closed"
            );
            stale_identity = replacement_a
                .element_identity("overlay-toggle-state")
                .unwrap();
        }
        assert_eq!(
            panel_b.element_state_token("overlay-toggle-state").unwrap(),
            "closed"
        );
    }

    #[test]
    fn repeated_token_changes_and_suppressions_remain_parse_once() {
        let mut panel = LiveDocument::load_surface_document(
            static_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            62,
        )
        .unwrap();
        let identity = panel.element_identity("overlay-toggle-state").unwrap();
        let initial_changed = panel.measurements().changed_token_updates;
        let initial_suppressed = panel.measurements().suppressed_token_updates;
        for index in 0..100 {
            let token = if index % 2 == 0 {
                StateToken::Open
            } else {
                StateToken::Closed
            };
            let changed = panel
                .apply_bound_tokens(&[(StateBindingKey::OverlayStatus, token)])
                .unwrap();
            assert_eq!(changed.changed_token_elements, 1);
            let suppressed = panel
                .apply_bound_tokens(&[(StateBindingKey::OverlayStatus, token)])
                .unwrap();
            assert_eq!(suppressed.changed_elements, 0);
            assert_eq!(suppressed.suppressed_keys, 1);
        }
        assert_eq!(
            panel.element_identity("overlay-toggle-state").unwrap(),
            identity
        );
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
        assert_eq!(
            panel.measurements().changed_token_updates,
            initial_changed + 100
        );
        assert_eq!(
            panel.measurements().suppressed_token_updates,
            initial_suppressed + 100
        );
    }

    #[test]
    fn duplicate_binding_input_is_rejected_before_mutation() {
        let mut panel = LiveDocument::load_surface_document(
            built_in_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let before = panel.element_text("panel-status").unwrap();
        assert!(
            panel
                .apply_bound_text(&[
                    (StateBindingKey::OverlayStatus, "first".into()),
                    (StateBindingKey::OverlayStatus, "second".into()),
                ])
                .is_err()
        );
        assert_eq!(panel.element_text("panel-status").unwrap(), before);
    }

    #[test]
    fn one_binding_updates_multiple_elements_without_rescan() {
        let mut panel = LiveDocument::load_surface_document(
            built_in_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let update = panel
            .apply_bound_text(&[(StateBindingKey::ShellLastAction, "Last action: test".into())])
            .unwrap();
        assert_eq!(update.changed_keys, 1);
        assert_eq!(update.changed_elements, 2);
        assert_eq!(
            panel.element_text("panel-last-action").unwrap(),
            "Last action: test"
        );
        assert_eq!(
            panel.element_text("panel-last-action-copy").unwrap(),
            "Last action: test"
        );
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn stale_binding_target_is_contained_before_mutation() {
        let mut panel = LiveDocument::load_surface_document(
            built_in_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let identity = panel.builtins.indexed_node("panel-status").unwrap();
        let slots = panel
            .identities
            .subtree_slots(&panel.document, identity)
            .unwrap();
        assert!(
            panel
                .document
                .mutate()
                .remove_and_drop_node(identity.slot)
                .is_some()
        );
        panel
            .identities
            .retire_removed(&panel.document, &slots)
            .unwrap();
        assert!(
            panel
                .apply_bound_text(&[(StateBindingKey::OverlayStatus, "Overlay: open".into(),)])
                .is_err()
        );
    }

    #[test]
    fn repeated_valid_clicks_dispatch_once_each_without_reparse() {
        let mut overlay = LiveDocument::load_surface_document(
            built_in_fixture(),
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        )
        .unwrap();
        let document = overlay.snapshot().unwrap().document_identity;
        let action = overlay.element_bounds("overlay-action").unwrap();
        let x = f64::from(action.x + action.width / 2.0);
        let y = f64::from(action.y + action.height / 2.0);
        for _ in 0..50 {
            overlay.pointer_move(x, y).unwrap();
            assert!(overlay.pointer_primary(true).unwrap());
            assert!(overlay.pointer_primary(false).unwrap());
            assert_eq!(overlay.take_action(), Some(LiveAction::ActivateOverlay));
            overlay.pointer_leave();
        }
        let snapshot = overlay.snapshot().unwrap();
        assert_eq!(snapshot.document_identity, document);
        assert_eq!(snapshot.document_parse_count, 1);
        assert_eq!(snapshot.interaction.click_count, 50);
        assert_eq!(overlay.measurements().registry_scan_count, 1);
    }

    #[test]
    fn clock_binding_updates_incrementally_without_rescan_or_identity_change() {
        let mut panel = LiveDocument::load_surface_document(
            clock_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let document = panel.snapshot().unwrap().document_identity;
        let element = panel.element_identity("clock").unwrap();
        assert_eq!(panel.binding_target_count(StateBindingKey::ClockTime), 1);
        let first = panel
            .apply_bound_text(&[(StateBindingKey::ClockTime, "09:07".into())])
            .unwrap();
        assert_eq!(first.changed_elements, 1);
        assert_eq!(panel.element_text("clock").unwrap(), "09:07");
        let duplicate = panel
            .apply_bound_text(&[(StateBindingKey::ClockTime, "09:07".into())])
            .unwrap();
        assert_eq!(duplicate.changed_elements, 0);
        assert_eq!(duplicate.suppressed_keys, 1);
        panel.set_viewport(1600, 58).unwrap();
        panel
            .render_for(LiveRenderRequest::new(1600, 58, 180).unwrap())
            .unwrap();
        panel
            .apply_bound_text(&[(StateBindingKey::ClockTime, "09:08".into())])
            .unwrap();
        assert_eq!(panel.snapshot().unwrap().document_identity, document);
        assert_eq!(panel.element_identity("clock").unwrap(), element);
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn clock_update_does_not_dispatch_or_cancel_a_valid_pending_button() {
        let mut panel = LiveDocument::load_surface_document(
            clock_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        )
        .unwrap();
        let toggle = panel.element_bounds("overlay-toggle").unwrap();
        let x = f64::from(toggle.x + toggle.width / 2.0);
        let y = f64::from(toggle.y + toggle.height / 2.0);
        assert!(panel.pointer_move(x, y).unwrap());
        assert!(panel.pointer_primary(true).unwrap());
        assert_eq!(panel.take_action(), None);
        panel
            .apply_bound_text(&[(StateBindingKey::ClockTime, "17:42".into())])
            .unwrap();
        assert_eq!(panel.take_action(), None);
        assert!(panel.pointer_primary(false).unwrap());
        assert_eq!(panel.take_action(), Some(LiveAction::ToggleOverlay));
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn clock_text_mutates_semantic_output_without_reparse_or_identity_loss() {
        let mut panel = LiveDocument::load_surface_document(
            formatted_clock_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1440,
            88,
        )
        .unwrap();
        assert_eq!(panel.clock_declarations().len(), 6);
        assert_eq!(panel.measurements().clock_declaration_count, 6);
        let document = panel.snapshot().unwrap().document_identity;
        let identity = panel.element_identity("local-time").unwrap();
        let first = panel
            .apply_clock_output(&identity, "09:07", "2026-07-23T09:07:00-04:00", true)
            .unwrap();
        assert!(first.changed_text);
        assert!(first.changed_datetime);
        assert!(first.changed_enabled_state);
        assert_eq!(panel.element_text("local-time").unwrap(), "09:07");
        assert_eq!(
            panel.element_datetime("local-time").unwrap(),
            "2026-07-23T09:07:00-04:00"
        );
        assert_eq!(
            element_attribute(&panel, "local-time", STATE_ATTRIBUTE).as_deref(),
            Some("enabled")
        );
        assert_eq!(
            element_attribute(&panel, "local-time", "class").as_deref(),
            Some("primary-clock")
        );
        assert_eq!(
            element_attribute(&panel, "local-time", "data-role").as_deref(),
            Some("local-clock")
        );
        assert_eq!(
            element_attribute(&panel, "local-date", "data-htm-format").as_deref(),
            Some("%A, %B %-d, %Y")
        );
        let duplicate = panel
            .apply_clock_output(&identity, "09:07", "2026-07-23T09:07:00-04:00", true)
            .unwrap();
        assert!(!duplicate.changed());
        panel.set_viewport(1600, 88).unwrap();
        panel
            .render_for(LiveRenderRequest::new(1600, 88, 180).unwrap())
            .unwrap();
        assert_eq!(panel.snapshot().unwrap().document_identity, document);
        assert_eq!(panel.element_identity("local-time").unwrap(), identity);
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn clock_buttons_emit_exact_generation_safe_targets() {
        let mut panel = LiveDocument::load_surface_document(
            formatted_clock_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1440,
            88,
        )
        .unwrap();
        let target = panel.element_identity("paused-clock").unwrap();
        for (button, expected) in [
            ("clock-enable", LiveAction::ClockEnable(target.clone())),
            ("clock-disable", LiveAction::ClockDisable(target.clone())),
            ("clock-toggle", LiveAction::ClockToggle(target.clone())),
        ] {
            let bounds = panel.element_bounds(button).unwrap();
            assert_eq!(click_action(&mut panel, &bounds), expected);
        }
        let stale = ElementInstanceId {
            document_generation: ExperimentalDocumentIdentity {
                serial: target.document_generation.serial.saturating_add(1),
            },
            html_id: target.html_id.clone(),
        };
        assert!(panel.clock_enabled(&stale).is_err());
        assert!(panel.set_clock_enabled(&target, true).unwrap());
        assert!(!panel.set_clock_enabled(&target, true).unwrap());
        assert!(panel.set_clock_enabled(&target, false).unwrap());
        panel
            .apply_clock_output(&target, "10:00:10", "2026-07-23T10:00:10-04:00", false)
            .unwrap();
        let toggle = panel.element_bounds("clock-toggle").unwrap();
        let x = f64::from(toggle.x + toggle.width / 2.0);
        let y = f64::from(toggle.y + toggle.height / 2.0);
        panel.pointer_move(x, y).unwrap();
        assert!(panel.pointer_primary(true).unwrap());
        panel
            .apply_clock_output(&target, "10:00:11", "2026-07-23T10:00:11-04:00", false)
            .unwrap();
        assert!(panel.pointer_primary(false).unwrap());
        assert_eq!(
            panel.take_action(),
            Some(LiveAction::ClockToggle(target.clone()))
        );
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }

    #[test]
    fn battery_text_and_tokens_update_incrementally_without_identity_loss() {
        let mut panel = LiveDocument::load_surface_document(
            battery_panel_fixture(),
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            64,
        )
        .unwrap();
        let document = panel.snapshot().unwrap().document_identity;
        let percentage = panel.element_identity("battery-percentage").unwrap();
        let status = panel.element_identity("battery-state").unwrap();
        let warning = panel.element_identity("battery-warning").unwrap();
        assert_eq!(
            panel.binding_target_count(StateBindingKey::BatteryPercentage),
            1
        );
        assert_eq!(
            panel.binding_target_count(StateBindingKey::BatteryStatus),
            2
        );
        assert_eq!(
            panel.binding_target_count(StateBindingKey::BatteryWarning),
            1
        );

        let changed = panel
            .apply_bound_state(
                &[
                    (StateBindingKey::BatteryPercentage, "78%".into()),
                    (StateBindingKey::BatteryStatus, "Charging".into()),
                ],
                &[
                    (StateBindingKey::BatteryStatus, StateToken::Charging),
                    (StateBindingKey::BatteryWarning, StateToken::None),
                ],
            )
            .unwrap();
        assert_eq!(changed.changed_elements, 4);
        assert_eq!(panel.element_text("battery-percentage").unwrap(), "78%");
        assert_eq!(
            element_attribute(&panel, "battery-state", "data-htm-state").as_deref(),
            Some("charging")
        );
        assert_eq!(
            element_attribute(&panel, "battery-warning", "data-htm-state").as_deref(),
            Some("none")
        );

        let duplicate = panel
            .apply_bound_state(
                &[
                    (StateBindingKey::BatteryPercentage, "78%".into()),
                    (StateBindingKey::BatteryStatus, "Charging".into()),
                ],
                &[
                    (StateBindingKey::BatteryStatus, StateToken::Charging),
                    (StateBindingKey::BatteryWarning, StateToken::None),
                ],
            )
            .unwrap();
        assert_eq!(duplicate.changed_elements, 0);
        assert_eq!(panel.snapshot().unwrap().document_identity, document);
        assert_eq!(
            panel.element_identity("battery-percentage").unwrap(),
            percentage
        );
        assert_eq!(panel.element_identity("battery-state").unwrap(), status);
        assert_eq!(panel.element_identity("battery-warning").unwrap(), warning);
        assert_eq!(panel.snapshot().unwrap().document_parse_count, 1);
        assert_eq!(panel.measurements().registry_scan_count, 1);
    }
}
