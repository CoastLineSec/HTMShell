use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ViewportSpec {
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale_factor: f32,
    pub color_space: &'static str,
    pub dynamic_range: &'static str,
}

impl Default for ViewportSpec {
    fn default() -> Self {
        Self {
            logical_width: 1440,
            logical_height: 900,
            scale_factor: 1.0,
            color_space: "sRGB",
            dynamic_range: "SDR",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExperimentOptions {
    pub viewport: ViewportSpec,
    pub render_png: bool,
    pub run_interaction: bool,
    pub output_directory: Option<PathBuf>,
}

impl Default for ExperimentOptions {
    fn default() -> Self {
        Self {
            viewport: ViewportSpec::default(),
            render_png: true,
            run_interaction: true,
            output_directory: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Initial,
    Hover,
    Active,
}

impl Phase {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Hover => "hover",
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverflowDiagnostic {
    pub x: String,
    pub y: String,
    pub establishes_clip: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CornerRadii {
    pub top_left: [f32; 2],
    pub top_right: [f32; 2],
    pub bottom_right: [f32; 2],
    pub bottom_left: [f32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextDiagnostic {
    pub content: String,
    pub measured_bounds: LogicalRect,
    pub line_count: usize,
    pub right_to_left: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontRecord {
    pub family: String,
    pub subfamily: Option<String>,
    pub postscript_name: Option<String>,
    pub face_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageDiagnostic {
    pub source: String,
    pub decoded_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticNode {
    pub experiment_node_id: usize,
    pub parent_node_id: Option<usize>,
    pub node_kind: String,
    pub tag: Option<String>,
    pub html_id: Option<String>,
    pub classes: Vec<String>,
    pub logical_bounds: LogicalRect,
    pub display: String,
    pub position: String,
    pub visibility: String,
    pub visible: bool,
    pub overflow: OverflowDiagnostic,
    pub background_srgba: Option<[f32; 4]>,
    pub border_radii: Option<CornerRadii>,
    pub text: Option<TextDiagnostic>,
    pub image: Option<ImageDiagnostic>,
    pub hovered: bool,
    pub active: bool,
    pub retained_paint_order: Option<usize>,
    pub children: Vec<DiagnosticNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceRecord {
    pub url: String,
    pub resource_kind: String,
    pub decision: String,
    pub detail: String,
    pub byte_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiagnosticMessage {
    pub level: String,
    pub code: String,
    pub message: String,
    pub node_id: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InteractionEvidence {
    pub phase: Phase,
    pub target_node_id: usize,
    pub state_changed: bool,
    pub target_hovered: bool,
    pub target_active: bool,
    pub node_identity_retained: bool,
    pub dom_tree_rebuilt: bool,
    pub dirty_descendant_flags_before_resolve: usize,
    pub damaged_nodes_before_resolve: usize,
    pub observably_changed_style_nodes: usize,
    pub observably_changed_layout_nodes: usize,
    pub observably_changed_paint_signature_nodes: usize,
    pub exact_nodes_restyled: Option<usize>,
    pub exact_layout_nodes_recomputed: Option<usize>,
    pub exact_paint_nodes_regenerated: Option<usize>,
    pub animation_running_after_state_change: bool,
    pub full_anyrender_scene_rebuilt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticReport {
    pub schema_version: &'static str,
    pub phase: Phase,
    pub viewport: ViewportSpec,
    pub renderer: String,
    pub blitz_revision: &'static str,
    pub document_source: String,
    pub node_count: usize,
    pub retained_scene_order_kind: String,
    pub retained_scene_order: Vec<usize>,
    pub fonts: Vec<FontRecord>,
    pub resources: Vec<ResourceRecord>,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub unsupported_features: Vec<String>,
    pub interaction: Option<InteractionEvidence>,
    pub tree: DiagnosticNode,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub phase: Phase,
    pub report: DiagnosticReport,
    pub diagnostic_json: Vec<u8>,
    pub png: Option<Vec<u8>>,
    pub diagnostic_path: Option<PathBuf>,
    pub png_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct RunMeasurements {
    pub package_read_ms: f64,
    pub html_parse_ms: f64,
    pub initial_resolve_ms: f64,
    pub initial_paint_ms: f64,
    pub hover_resolve_ms: Option<f64>,
    pub hover_paint_ms: Option<f64>,
    pub active_resolve_ms: Option<f64>,
    pub active_paint_ms: Option<f64>,
    pub artifact_write_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Clone)]
pub struct ExperimentRun {
    pub artifacts: Vec<Artifact>,
    pub measurements: RunMeasurements,
    pub package_root: PathBuf,
}
