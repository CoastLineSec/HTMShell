use crate::model::{CornerRadii, ImageDiagnostic, LogicalRect, OverflowDiagnostic, ViewportSpec};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MutationPhase {
    Initial,
    TextMutation,
    ClassMutation,
    ListAppend,
    ListRemoval,
    SlotReuse,
    StylesheetReplacement,
    MissingStylesheetRejected,
    MalformedStylesheetRejected,
}

impl MutationPhase {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::TextMutation => "text-mutation",
            Self::ClassMutation => "class-mutation",
            Self::ListAppend => "list-append",
            Self::ListRemoval => "list-removal",
            Self::SlotReuse => "slot-reuse",
            Self::StylesheetReplacement => "stylesheet-replacement",
            Self::MissingStylesheetRejected => "missing-stylesheet-rejected",
            Self::MalformedStylesheetRejected => "malformed-stylesheet-rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExperimentalDocumentIdentity {
    pub serial: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExperimentalNodeIdentity {
    pub slot: usize,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeMetadataSnapshot {
    pub html_id: Option<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipSnapshot {
    pub overflow: OverflowDiagnostic,
    pub establishes_stacking_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractionStateSnapshot {
    pub hovered: bool,
    pub active: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneTextSnapshot {
    pub content: String,
    pub stable_hash: String,
    pub measured_bounds: Option<LogicalRect>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNodeSnapshot {
    pub identity: ExperimentalNodeIdentity,
    pub parent_identity: Option<ExperimentalNodeIdentity>,
    pub tree_order: usize,
    pub paint_order: Option<usize>,
    pub node_type: String,
    pub tag: Option<String>,
    pub metadata: NodeMetadataSnapshot,
    pub logical_bounds: LogicalRect,
    pub visibility: String,
    pub visible: bool,
    pub display: String,
    pub position: String,
    pub opacity: f32,
    pub background_srgba: Option<[f32; 4]>,
    pub border_radii: Option<CornerRadii>,
    pub border_signature: String,
    pub transform_signature: Option<String>,
    pub style_paint_signature: String,
    pub text: Option<SceneTextSnapshot>,
    pub resource: Option<ImageDiagnostic>,
    pub clip: ClipSnapshot,
    pub interaction: InteractionStateSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExperimentalSceneSnapshot {
    pub schema_version: &'static str,
    pub phase: MutationPhase,
    pub document_identity: ExperimentalDocumentIdentity,
    pub document_parse_count: u32,
    pub blitz_document_instance_retained: bool,
    pub viewport: ViewportSpec,
    pub node_count: usize,
    pub nodes: Vec<SceneNodeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldChange<T> {
    pub old: T,
    pub new: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNodeChange {
    pub identity: ExperimentalNodeIdentity,
    pub geometry: Option<FieldChange<LogicalRect>>,
    pub style_or_paint: Option<FieldChange<String>>,
    pub metadata: Option<FieldChange<NodeMetadataSnapshot>>,
    pub text: Option<FieldChange<Option<SceneTextSnapshot>>>,
    pub resource: Option<FieldChange<Option<ImageDiagnostic>>>,
    pub parent: Option<FieldChange<Option<ExperimentalNodeIdentity>>>,
    pub tree_order: Option<FieldChange<usize>>,
    pub paint_order: Option<FieldChange<Option<usize>>>,
    pub clip: Option<FieldChange<ClipSnapshot>>,
    pub transform: Option<FieldChange<Option<String>>>,
    pub interaction: Option<FieldChange<InteractionStateSnapshot>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DamageEstimate {
    pub label: &'static str,
    pub changed_node_bounds: Vec<LogicalRect>,
    pub total_bounds: Option<LogicalRect>,
    pub excluded_expansion: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SceneDiffSummary {
    pub created: usize,
    pub removed: usize,
    pub retained_unchanged: usize,
    pub changed: usize,
    pub geometry_changes: usize,
    pub style_or_paint_changes: usize,
    pub text_changes: usize,
    pub resource_changes: usize,
    pub parent_changes: usize,
    pub order_changes: usize,
    pub clip_changes: usize,
    pub interaction_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExperimentalSceneDiff {
    pub schema_version: &'static str,
    pub from_phase: MutationPhase,
    pub to_phase: MutationPhase,
    pub is_empty: bool,
    pub summary: SceneDiffSummary,
    pub created_nodes: Vec<SceneNodeSnapshot>,
    pub removed_nodes: Vec<SceneNodeSnapshot>,
    pub retained_unchanged: Vec<ExperimentalNodeIdentity>,
    pub changed_nodes: Vec<SceneNodeChange>,
    pub damage_estimate: DamageEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvalidationEvidence {
    pub dirty_descendant_flags_before_resolve: usize,
    pub damaged_nodes_before_resolve: usize,
    pub style_snapshots_before_resolve: usize,
    pub animation_running_after_resolve: bool,
    pub exact_nodes_restyled: Option<usize>,
    pub exact_layout_nodes_recomputed: Option<usize>,
    pub exact_paint_commands_regenerated: Option<usize>,
    pub exact_paint_nodes_retained: Option<usize>,
    pub full_anyrender_scene_rebuilt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MutationPhaseMeasurement {
    pub phase: MutationPhase,
    pub operation_ms: f64,
    pub resolve_ms: Option<f64>,
    pub snapshot_ms: f64,
    pub diff_ms: Option<f64>,
    pub paint_ms: Option<f64>,
    pub png_encode_ms: Option<f64>,
    pub snapshot_json_bytes: usize,
    pub diff_json_bytes: Option<usize>,
    pub invalidation: InvalidationEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StylesheetReloadAttempt {
    pub phase: MutationPhase,
    pub candidate: String,
    pub accepted: bool,
    pub diagnostic: Option<String>,
    pub accepted_snapshot_preserved: bool,
    pub document_identity_preserved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotReuseEvidence {
    pub cycles: usize,
    pub initially_removed: Vec<ExperimentalNodeIdentity>,
    pub final_created: Vec<ExperimentalNodeIdentity>,
    pub reused_slots: Vec<usize>,
    pub maximum_generation: u64,
    pub stale_lookups_rejected: usize,
    pub retained_sibling_identities_preserved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScaleBaseline {
    pub requested_nodes: usize,
    pub exact_initial_nodes: usize,
    pub exact_final_nodes: usize,
    pub document_parse_count: u32,
    pub retained_nodes: usize,
    pub created_nodes: usize,
    pub removed_nodes: usize,
    pub changed_nodes: usize,
    pub style_or_paint_changes: usize,
    pub geometry_changes: usize,
    pub parse_ms: f64,
    pub initial_resolve_ms: f64,
    pub mutation_ms: f64,
    pub mutation_resolve_ms: f64,
    pub initial_snapshot_ms: f64,
    pub final_snapshot_ms: f64,
    pub diff_ms: f64,
    pub full_paint_ms: f64,
    pub initial_snapshot_json_bytes: usize,
    pub final_snapshot_json_bytes: usize,
    pub diff_json_bytes: usize,
    pub process_rss_kib: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MutationArtifact {
    pub phase: MutationPhase,
    pub snapshot: ExperimentalSceneSnapshot,
    pub diff_from_previous: Option<ExperimentalSceneDiff>,
    pub snapshot_json: Vec<u8>,
    pub diff_json: Option<Vec<u8>>,
    pub png: Option<Vec<u8>>,
    pub snapshot_path: Option<PathBuf>,
    pub diff_path: Option<PathBuf>,
    pub png_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct IncrementalExperimentRun {
    pub document_parse_count: u32,
    pub document_identity_preserved: bool,
    pub artifacts: Vec<MutationArtifact>,
    pub phase_measurements: Vec<MutationPhaseMeasurement>,
    pub stylesheet_attempts: Vec<StylesheetReloadAttempt>,
    pub slot_reuse: SlotReuseEvidence,
    pub scale_baselines: Vec<ScaleBaseline>,
    pub total_ms: f64,
    pub package_root: PathBuf,
}
