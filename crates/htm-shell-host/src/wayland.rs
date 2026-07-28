use crate::ShellHostError;
use crate::buffer::{BufferData, BufferPoolStats, ShmBufferPool};
use crate::clock::{ClockService, ClockServiceSummary, ClockUpdate};
use crate::lifecycle::LayerLifecycle;
use crate::manifest::{SurfaceKind as ManifestSurfaceKind, ValidatedManifest};
use crate::output::{OutputCatalog, OutputEligibility, OutputKey};
use crate::pipewire::{PipeWireDemand, PipeWireSnapshot, PipeWireSource, duration_to_timespec};
use crate::power::{
    BatteryServiceSummary, PowerFanoutMetrics, PowerProfile, PowerService, PowerSnapshot,
};
#[cfg(feature = "gpu-renderer")]
use crate::presenter::{PresenterState, SurfacePresenter};
use crate::scale::{PresentationProfile, SurfaceScaleState};
use crate::scheduler::{FrameScheduler, ScheduleDecision};
use htm_runtime::{
    ItemBindingKey, LIVE_SCALE_DENOMINATOR, LiveAction, LiveDocument, LiveDocumentKind,
    MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET, MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS,
    PipeWirePeakTarget, RepeatSource, StateBindingKey, StateToken,
};
#[cfg(feature = "gpu-renderer")]
use htm_runtime::{
    LiveFrame, LiveGpuConfiguration, LiveGpuError, LiveGpuErrorKind, LiveGpuPreparedFrame,
    LiveGpuPresenter, LiveRenderRequest, LiveWaylandHandle, RenderSurfaceId,
};
use rustix::event::{PollFd, PollFlags, poll};
use rustix::fd::BorrowedFd;
use std::path::PathBuf;
#[cfg(feature = "gpu-renderer")]
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Instant;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    backend::WaylandError,
    delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_output, wl_pointer, wl_region, wl_registry,
        wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
    viewporter::client::{wp_viewport, wp_viewporter},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

const SINGLE_OWNER: u64 = 1;
const PANEL_OWNER: u64 = 2;
const OVERLAY_OWNER: u64 = 3;
const PANEL_NAMESPACE: &str = "htmshell-panel";
const OVERLAY_NAMESPACE: &str = "htmshell-overlay";
const SINGLE_NAMESPACE: &str = "htmshell";
const LAYER_SHELL_MAX_VERSION: u32 = 5;
const BTN_LEFT: u32 = 0x110;
const SHUTDOWN_ROUNDTRIPS: usize = 6;
const MAX_SESSION_MAPPED_BYTES: usize = 256 * 1024 * 1024;
const WL_OUTPUT_RELEASE_VERSION: u32 = 3;
const WL_POINTER_RELEASE_VERSION: u32 = 3;
const WL_SEAT_RELEASE_VERSION: u32 = 5;
const WL_SHM_RELEASE_VERSION: u32 = 2;
const FRACTIONAL_SCALE_VERSION: u32 = 1;
const VIEWPORTER_VERSION: u32 = 1;

#[cfg(feature = "gpu-renderer")]
fn internal_gpu_renderer_requested() -> bool {
    internal_gpu_renderer_value(std::env::var("HTMSHELL_INTERNAL_RENDERER").ok().as_deref())
}

#[cfg(feature = "gpu-renderer")]
fn internal_gpu_renderer_value(value: Option<&str>) -> bool {
    value == Some("vello")
}

#[derive(Debug, Clone)]
pub struct LiveHostOptions {
    pub package: PathBuf,
    pub exit_after_frames: Option<u64>,
    pub exit_after_click: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LiveHostSummary {
    pub layer_shell_version: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
    pub output_scale: i32,
    pub viewporter_advertised: bool,
    pub fractional_scale_advertised: bool,
    pub preferred_scale_numerator: u32,
    pub scale_denominator: u32,
    pub fractional_viewport_active: bool,
    pub html_parse_count: u32,
    pub frames_committed: u64,
    pub full_damage_commits: u64,
    pub partial_damage_commits: u64,
    pub frame_callbacks: u64,
    pub buffer_releases: u64,
    pub pointer_enters: u64,
    pub pointer_motions: u64,
    pub pointer_buttons: u64,
    pub click_mutations: u64,
    pub buffers_allocated: u64,
    pub buffer_reallocations: u64,
    pub frames_skipped_busy: u64,
    pub maximum_mapped_bytes: usize,
    pub wayland_connection_us: u64,
    pub first_configure_us: u64,
    pub first_commit_us: u64,
    pub first_frame_callback_us: u64,
    pub package_read_us: u64,
    pub html_parse_us: u64,
    pub initial_resolve_us: u64,
    pub last_resolve_us: u64,
    pub last_render_us: u64,
    pub last_pixel_conversion_us: u64,
    #[cfg(feature = "gpu-renderer")]
    pub gpu: GpuSurfaceHostSummary,
}

#[derive(Debug, Clone)]
pub struct MultiSurfaceHostOptions {
    pub package: PathBuf,
    pub panel_height: u32,
    pub automatic_overlay_cycles: u32,
    pub exit_after_automatic_cycles: bool,
    pub exit_after_overlay_close: bool,
    pub open_overlay_on_start: bool,
    pub exit_after_peak_publications: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ManifestHostOptions {
    pub manifest: ValidatedManifest,
    pub exit_after_initial_frames: bool,
    pub exit_after_output_events: Option<u64>,
    pub exit_after_actions: Option<u64>,
    pub exit_after_scale_changes: Option<u64>,
    pub exit_after_clock_updates: Option<u64>,
    pub exit_after_battery_updates: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SurfaceHostSummary {
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
    pub preferred_scale_numerator: u32,
    pub scale_denominator: u32,
    pub fractional_viewport_active: bool,
    pub preferred_scale_changes: u64,
    pub last_scale_change_to_commit_us: u64,
    pub last_scale_change_to_frame_callback_us: u64,
    pub html_parse_count: u32,
    pub configure_count: u64,
    pub frames_committed: u64,
    pub frame_callbacks: u64,
    pub buffer_releases: u64,
    pub pointer_enters: u64,
    pub pointer_motions: u64,
    pub pointer_buttons: u64,
    pub action_count: u64,
    pub buffer_allocations: u64,
    pub buffer_reallocations: u64,
    pub retired_buffer_peak: usize,
    pub mapped_memory_peak: usize,
    pub busy_buffer_skips: u64,
    pub first_configure_us: u64,
    pub first_commit_us: u64,
    pub first_frame_callback_us: u64,
    pub package_read_us: u64,
    pub html_parse_us: u64,
    pub initial_resolve_us: u64,
    pub last_resolve_us: u64,
    pub last_render_us: u64,
    pub last_pixel_conversion_us: u64,
    pub registry_initialization_us: u64,
    pub declaration_discovery_us: u64,
    pub registered_element_count: u64,
    pub binding_count: u64,
    pub text_binding_count: u64,
    pub token_binding_count: u64,
    pub registered_action_count: u64,
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
    pub last_reconciliation_us: u64,
    pub last_state_projection_us: u64,
    pub last_attribute_mutation_us: u64,
    pub last_pointer_release_to_action_dispatch_us: u64,
    pub last_action_dispatch_to_state_mutation_us: u64,
    pub last_state_mutation_to_commit_us: u64,
    pub last_state_mutation_to_frame_callback_us: u64,
    #[cfg(feature = "gpu-renderer")]
    pub gpu: GpuSurfaceHostSummary,
}

#[cfg(feature = "gpu-renderer")]
#[derive(Debug, Clone, Default)]
pub struct GpuSurfaceHostSummary {
    pub requested: bool,
    pub successful_gpu_frame: bool,
    pub adapter: String,
    pub graphics_api: String,
    pub device_type: String,
    pub driver: String,
    pub device_generation: u64,
    pub presenter_state: String,
    pub surface_format: String,
    pub present_mode: String,
    pub alpha_mode: String,
    pub configuration_generation: u64,
    pub presenter_creations: u64,
    pub presenter_releases: u64,
    pub configurations: u64,
    pub reconfigurations: u64,
    pub frames_planned: u64,
    pub frames_rendered: u64,
    pub frames_submitted: u64,
    pub frames_presented: u64,
    pub surface_acquisitions: u64,
    pub acquisition_failures: u64,
    pub conversion_passes: u64,
    pub full_target_renders: u64,
    pub partial_renders: u64,
    pub logical_damage_rectangles: u64,
    pub physical_damage_rectangles: u64,
    pub physical_damaged_pixels: u64,
    pub selected_tiles: u64,
    pub vello_rasterized_pixels: u64,
    pub backing_updated_pixels: u64,
    pub surface_converted_pixels: u64,
    pub wayland_damage_rectangles: u64,
    pub wayland_damaged_pixels: u64,
    pub full_wayland_damage_frames: u64,
    pub narrow_wayland_damage_frames: u64,
    pub cpu_fallbacks: u64,
    pub shm_frames: u64,
    pub frame_callbacks_requested: u64,
    pub frame_callbacks_completed: u64,
    pub surface_losses: u64,
    pub surface_timeouts: u64,
    pub surface_outdated: u64,
    pub device_losses: u64,
    pub target_recreations: u64,
    pub closed_surface_suppressions: u64,
    pub duplicate_frame_suppressions: u64,
    pub resource_entries: usize,
    pub resource_bytes: u64,
    pub resource_uploads: u64,
    pub cache_hits: u64,
    pub last_error: String,
}

#[cfg(feature = "gpu-renderer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuWaylandDamageMode {
    Buffer,
    SurfaceFull,
}

#[cfg(feature = "gpu-renderer")]
fn queue_gpu_wayland_damage(
    surface: &wl_surface::WlSurface,
    physical_damage: &[[u32; 4]],
    logical_width: u32,
    logical_height: u32,
) -> GpuWaylandDamageMode {
    if select_gpu_wayland_damage_mode(surface.version()) == GpuWaylandDamageMode::Buffer {
        for [x, y, width, height] in physical_damage {
            surface.damage_buffer(
                (*x).min(i32::MAX as u32) as i32,
                (*y).min(i32::MAX as u32) as i32,
                (*width).min(i32::MAX as u32) as i32,
                (*height).min(i32::MAX as u32) as i32,
            );
        }
        GpuWaylandDamageMode::Buffer
    } else {
        surface.damage(
            0,
            0,
            logical_width.min(i32::MAX as u32) as i32,
            logical_height.min(i32::MAX as u32) as i32,
        );
        GpuWaylandDamageMode::SurfaceFull
    }
}

#[cfg(feature = "gpu-renderer")]
fn select_gpu_wayland_damage_mode(surface_version: u32) -> GpuWaylandDamageMode {
    if surface_version >= 4 {
        GpuWaylandDamageMode::Buffer
    } else {
        GpuWaylandDamageMode::SurfaceFull
    }
}

#[derive(Debug, Clone, Default)]
pub struct PipeWirePeakHostSummary {
    pub active_streams: usize,
    pub stream_starts: u64,
    pub stream_stops: u64,
    pub process_callbacks: u64,
    pub callbacks_coalesced: u64,
    pub vectors_published: u64,
    pub duplicate_vectors_suppressed: u64,
}

#[derive(Debug, Clone, Default)]
pub struct MultiSurfaceHostSummary {
    pub layer_shell_version: u32,
    pub output_scale: i32,
    pub viewporter_advertised: bool,
    pub fractional_scale_advertised: bool,
    pub wayland_connection_us: u64,
    pub panel: SurfaceHostSummary,
    pub overlay: SurfaceHostSummary,
    pub overlay_open_count: u64,
    pub overlay_close_count: u64,
    pub overlay_activation_count: u64,
    pub panel_click_to_overlay_frame_us: u64,
    pub overlay_close_to_unmap_us: u64,
    pub combined_mapped_memory_peak: usize,
    pub automatic_cycles_completed: u32,
    pub last_action: String,
    pub pipewire_peaks: PipeWirePeakHostSummary,
}

#[derive(Debug, Clone, Default)]
pub struct ManifestSurfaceHostSummary {
    pub template_id: String,
    pub owner: u64,
    pub instance_generation: u64,
    pub output_key: Option<OutputKey>,
    pub metrics: SurfaceHostSummary,
}

#[derive(Debug, Clone, Default)]
pub struct ManifestOutputHostSummary {
    pub output_key: Option<OutputKey>,
    pub diagnostic_label: String,
    pub overlay_open: bool,
    pub overlay_activation_count: u64,
    pub output_ready_us: u64,
    pub first_panel_frame_us: u64,
    pub panel: Option<ManifestSurfaceHostSummary>,
    pub overlay: Option<ManifestSurfaceHostSummary>,
}

#[derive(Debug, Clone, Default)]
pub struct ManifestHostSummary {
    pub manifest_id: String,
    pub manifest_parse_count: u32,
    pub manifest_parse_us: u64,
    pub manifest_validation_us: u64,
    pub package_snapshot_generation: u64,
    pub package_count: usize,
    pub layer_shell_version: u32,
    pub viewporter_advertised: bool,
    pub fractional_scale_advertised: bool,
    pub output_generations: u64,
    pub output_additions: u64,
    pub output_removals: u64,
    pub unsupported_scale_outputs: u64,
    pub active_outputs: Vec<ManifestOutputHostSummary>,
    pub peak_output_instances: usize,
    pub peak_runtime_documents: usize,
    pub combined_mapped_memory_peak: usize,
    pub aggregate_shm_limit: usize,
    pub stale_callbacks_contained: u64,
    pub stale_releases_contained: u64,
    pub stale_scale_events_contained: u64,
    pub first_output_instance_us: u64,
    pub last_output_teardown_us: u64,
    pub actions: u64,
    pub clock: ClockServiceSummary,
    pub battery: BatteryServiceSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceKind {
    SingleOverlay,
    Panel,
    Overlay,
}

impl SurfaceKind {
    fn owner(self) -> u64 {
        match self {
            Self::SingleOverlay => SINGLE_OWNER,
            Self::Panel => PANEL_OWNER,
            Self::Overlay => OVERLAY_OWNER,
        }
    }

    fn document_kind(self) -> LiveDocumentKind {
        match self {
            Self::SingleOverlay => LiveDocumentKind::SingleOverlay,
            Self::Panel => LiveDocumentKind::Panel,
            Self::Overlay => LiveDocumentKind::TransientOverlay,
        }
    }
}

#[derive(Debug, Clone)]
enum SessionOptions {
    Single(LiveHostOptions),
    Multi(MultiSurfaceHostOptions),
    Manifest(ManifestHostOptions),
}

#[derive(Debug, Clone, Default)]
struct SharedShellState {
    overlay_open: bool,
    overlay_activation_count: u64,
    last_action: String,
}

#[derive(Debug, Clone, Copy)]
struct OutputData {
    key: OutputKey,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceData {
    owner: u64,
}

#[derive(Debug, Clone, Copy)]
struct ScaleData {
    owner: u64,
    surface_generation: u64,
}

#[derive(Debug, Clone, Copy)]
enum CallbackData {
    Frame { owner: u64, generation: u64 },
    OutputReady(OutputKey),
}

#[derive(Debug, Clone, Copy)]
struct LayerData {
    owner: u64,
}

struct BoundOutput {
    key: OutputKey,
    proxy: wl_output::WlOutput,
    advertised_at: Instant,
}

#[derive(Debug, Clone)]
struct OutputShellInstance {
    key: OutputKey,
    diagnostic_label: String,
    panel_owner: u64,
    overlay_owner: u64,
    shared: SharedShellState,
    created_at: Instant,
    output_ready_us: u64,
    first_panel_frame_us: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct RequiredGlobals {
    compositor: bool,
    shm: bool,
    argb8888: bool,
    output: bool,
    seat: bool,
    pointer: bool,
    layer_shell: bool,
}

impl RequiredGlobals {
    fn validate(self, require_output: bool) -> Result<(), ShellHostError> {
        for (present, interface) in [
            (self.compositor, "wl_compositor"),
            (self.shm, "wl_shm"),
            (self.seat, "wl_seat"),
            (self.layer_shell, "zwlr_layer_shell_v1"),
        ] {
            if !present {
                return Err(ShellHostError::MissingGlobal(interface));
            }
        }
        if !self.argb8888 {
            return Err(ShellHostError::UnsupportedShmFormat);
        }
        if !self.pointer {
            return Err(ShellHostError::MissingPointerCapability);
        }
        if require_output && !self.output {
            return Err(ShellHostError::MissingGlobal("wl_output"));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct ConfigureCoalescer {
    latest: Option<(u32, u32)>,
    received: u64,
    presented: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceMapState {
    AwaitingConfigure,
    Unmapped,
    PendingMap,
    Mapped,
    Unmapping,
    Closed,
}

impl SurfaceMapState {
    fn configured(&mut self, wants_mapping: bool) {
        if *self != Self::Closed {
            *self = if wants_mapping {
                Self::PendingMap
            } else {
                Self::Unmapped
            };
        }
    }

    fn request_map(&mut self, configured: bool) {
        if *self != Self::Closed {
            *self = if configured {
                Self::PendingMap
            } else {
                Self::AwaitingConfigure
            };
        }
    }

    fn mapped(&mut self) {
        if *self == Self::PendingMap {
            *self = Self::Mapped;
        }
    }

    fn begin_unmap(&mut self) {
        if matches!(self, Self::PendingMap | Self::Mapped) {
            *self = Self::Unmapping;
        }
    }

    fn finish_unmap(&mut self) {
        if *self == Self::Unmapping {
            *self = Self::Unmapped;
        }
    }

    fn close(&mut self) {
        *self = Self::Closed;
    }
}

impl ConfigureCoalescer {
    fn receive(&mut self, width: u32, height: u32) {
        self.latest = Some((width, height));
        self.received = self.received.saturating_add(1);
    }

    fn latest(&self) -> Option<(u32, u32)> {
        self.latest
    }

    fn mark_presented(&mut self) {
        self.presented = self.presented.saturating_add(1);
    }

    fn invalidate_for_unmap(&mut self) {
        self.latest = None;
    }
}

struct ShellSurfaceState {
    owner: u64,
    output_key: OutputKey,
    template_id: String,
    instance_generation: u64,
    document: Option<PathBuf>,
    package_snapshot: Option<Arc<htm_runtime::PackageSnapshot>>,
    instance_context: Option<(String, String)>,
    kind: SurfaceKind,
    package: PathBuf,
    namespace: String,
    panel_height: u32,
    reserve_space: bool,
    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    viewport: Option<wp_viewport::WpViewport>,
    fractional_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    role_generation: u64,
    scale_state: SurfaceScaleState,
    runtime: Option<LiveDocument>,
    pool: ShmBufferPool,
    scheduler: FrameScheduler,
    lifecycle: LayerLifecycle,
    configures: ConfigureCoalescer,
    desired_mapped: bool,
    mapped: bool,
    map_state: SurfaceMapState,
    summary: SurfaceHostSummary,
    maximum_mapped_bytes: usize,
    last_click_count: u64,
    presentation_failed: bool,
    pending_scale_started: Option<Instant>,
    scaled_commit_started: Option<Instant>,
    pending_binding_mutation_started: Option<Instant>,
    binding_commit_started: Option<Instant>,
    #[cfg(feature = "gpu-renderer")]
    presenter: SurfacePresenter,
    #[cfg(feature = "gpu-renderer")]
    gpu_consecutive_timeouts: u8,
}

#[derive(Debug, Clone, Copy)]
struct PresentedFrame {
    buffer_width: u32,
    buffer_height: u32,
    render_us: u64,
    conversion_us: u64,
}

#[cfg(feature = "gpu-renderer")]
enum GpuFrameAttempt {
    Presented(PresentedFrame),
    CpuFallback(LiveFrame),
    NoFrame,
}

impl ShellSurfaceState {
    fn all_released(&self) -> bool {
        self.pool.all_released()
    }

    fn refresh_pool_summary(&mut self) {
        let BufferPoolStats {
            allocations,
            reallocations,
            releases,
            skipped_no_free_buffer,
            total_mapped_bytes,
            retired_buffers,
            ..
        } = self.pool.stats();
        self.summary.buffer_allocations = allocations;
        self.summary.buffer_reallocations = reallocations;
        self.summary.buffer_releases = releases;
        self.summary.busy_buffer_skips = skipped_no_free_buffer;
        self.summary.retired_buffer_peak = self.summary.retired_buffer_peak.max(retired_buffers);
        self.maximum_mapped_bytes = self.maximum_mapped_bytes.max(total_mapped_bytes);
        self.summary.mapped_memory_peak = self.maximum_mapped_bytes;
    }
}

struct State {
    options: SessionOptions,
    started: Instant,
    running: bool,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shm_argb8888: bool,
    output_catalog: OutputCatalog,
    outputs: Vec<BoundOutput>,
    selected_output: Option<OutputKey>,
    seat: Option<wl_seat::WlSeat>,
    seat_global_name: Option<u32>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_focus: Option<u64>,
    layer_shell: Option<ZwlrLayerShellV1>,
    viewporter: Option<wp_viewporter::WpViewporter>,
    fractional_scale_manager: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    layer_shell_version: u32,
    surfaces: Vec<ShellSurfaceState>,
    shared: SharedShellState,
    output_instances: Vec<OutputShellInstance>,
    next_surface_owner: u64,
    next_instance_generation: u64,
    initial_discovery_complete: bool,
    output_additions: u64,
    output_removals: u64,
    unsupported_scale_outputs: u64,
    peak_output_instances: usize,
    peak_runtime_documents: usize,
    stale_callbacks_contained: u64,
    stale_releases_contained: u64,
    stale_scale_events_contained: u64,
    first_output_instance_us: u64,
    last_output_teardown_us: u64,
    manifest_actions: u64,
    wayland_connection_us: u64,
    viewporter_advertised: bool,
    fractional_scale_advertised: bool,
    queue_handle: Option<QueueHandle<Self>>,
    single_exit_after_commit_count: Option<u64>,
    auto_cycles_remaining: u32,
    auto_cycles_completed: u32,
    auto_waiting_overlay_frame: bool,
    auto_reopen_after_release: bool,
    auto_started: bool,
    startup_overlay_requested: bool,
    overlay_open_started: Option<Instant>,
    overlay_close_started: Option<Instant>,
    overlay_open_count: u64,
    overlay_close_count: u64,
    first_overlay_frame_latency_us: u64,
    last_overlay_close_latency_us: u64,
    combined_mapped_memory_peak: usize,
    failure: Option<String>,
    clock: ClockService,
    battery: PowerService,
    pipewire: PipeWireSource,
    #[cfg(feature = "gpu-renderer")]
    gpu_requested: bool,
    #[cfg(feature = "gpu-renderer")]
    wayland_display: Option<NonNull<std::ffi::c_void>>,
    #[cfg(feature = "gpu-renderer")]
    gpu: Option<LiveGpuPresenter>,
    #[cfg(feature = "gpu-renderer")]
    gpu_device_generation: u64,
}

impl State {
    fn new(options: SessionOptions, started: Instant, wayland_connection_us: u64) -> Self {
        let auto_cycles_remaining = match &options {
            SessionOptions::Multi(options) => options.automatic_overlay_cycles,
            SessionOptions::Single(_) | SessionOptions::Manifest(_) => 0,
        };
        Self {
            options,
            started,
            running: true,
            compositor: None,
            shm: None,
            shm_argb8888: false,
            output_catalog: OutputCatalog::default(),
            outputs: Vec::new(),
            selected_output: None,
            seat: None,
            seat_global_name: None,
            pointer: None,
            pointer_focus: None,
            layer_shell: None,
            viewporter: None,
            fractional_scale_manager: None,
            layer_shell_version: 0,
            surfaces: Vec::new(),
            shared: SharedShellState {
                last_action: "Ready".into(),
                ..SharedShellState::default()
            },
            output_instances: Vec::new(),
            next_surface_owner: 100,
            next_instance_generation: 0,
            initial_discovery_complete: false,
            output_additions: 0,
            output_removals: 0,
            unsupported_scale_outputs: 0,
            peak_output_instances: 0,
            peak_runtime_documents: 0,
            stale_callbacks_contained: 0,
            stale_releases_contained: 0,
            stale_scale_events_contained: 0,
            first_output_instance_us: 0,
            last_output_teardown_us: 0,
            manifest_actions: 0,
            wayland_connection_us,
            viewporter_advertised: false,
            fractional_scale_advertised: false,
            queue_handle: None,
            single_exit_after_commit_count: None,
            auto_cycles_remaining,
            auto_cycles_completed: 0,
            auto_waiting_overlay_frame: false,
            auto_reopen_after_release: false,
            auto_started: false,
            startup_overlay_requested: false,
            overlay_open_started: None,
            overlay_close_started: None,
            overlay_open_count: 0,
            overlay_close_count: 0,
            first_overlay_frame_latency_us: 0,
            last_overlay_close_latency_us: 0,
            combined_mapped_memory_peak: 0,
            failure: None,
            clock: ClockService::default(),
            battery: PowerService::default(),
            pipewire: PipeWireSource::default(),
            #[cfg(feature = "gpu-renderer")]
            gpu_requested: internal_gpu_renderer_requested(),
            #[cfg(feature = "gpu-renderer")]
            wayland_display: None,
            #[cfg(feature = "gpu-renderer")]
            gpu: None,
            #[cfg(feature = "gpu-renderer")]
            gpu_device_generation: 0,
        }
    }

    fn validate_globals(&self) -> Result<(), ShellHostError> {
        let require_output = !matches!(self.options, SessionOptions::Manifest(_));
        RequiredGlobals {
            compositor: self.compositor.is_some(),
            shm: self.shm.is_some(),
            argb8888: self.shm_argb8888,
            output: !self.output_catalog.present().is_empty(),
            seat: self.seat.is_some(),
            pointer: self.pointer.is_some(),
            layer_shell: self.layer_shell.is_some(),
        }
        .validate(require_output)
    }

    fn fractional_available(&self) -> bool {
        self.viewporter.is_some() && self.fractional_scale_manager.is_some()
    }

    fn start(&mut self, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        self.output_catalog.finalize_initial();
        self.initial_discovery_complete = true;
        self.validate_globals()?;
        match self.options.clone() {
            SessionOptions::Single(options) => {
                self.select_legacy_output()?;
                self.create_surface(qh, SurfaceKind::SingleOverlay, options.package, true, 0)?
            }
            SessionOptions::Multi(options) => {
                self.select_legacy_output()?;
                if options.panel_height == 0 || options.panel_height > i32::MAX as u32 {
                    return Err(ShellHostError::InvalidDimensions(format!(
                        "panel height {} is outside the layer-shell range",
                        options.panel_height
                    )));
                }
                self.create_surface(
                    qh,
                    SurfaceKind::Panel,
                    options.package.clone(),
                    true,
                    options.panel_height,
                )?;
                self.create_surface(qh, SurfaceKind::Overlay, options.package, false, 0)?;
            }
            SessionOptions::Manifest(_) => self.reconcile_all_outputs(qh),
        }
        Ok(())
    }

    fn select_legacy_output(&mut self) -> Result<(), ShellHostError> {
        let key = self
            .output_catalog
            .eligible(self.fractional_available())
            .first()
            .map(|record| record.key)
            .ok_or(ShellHostError::MissingGlobal("eligible wl_output"))?;
        self.selected_output = Some(key);
        Ok(())
    }

    fn output_proxy(&self, key: OutputKey) -> Option<wl_output::WlOutput> {
        self.outputs
            .iter()
            .find(|output| output.key == key)
            .map(|output| output.proxy.clone())
    }

    fn create_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        kind: SurfaceKind,
        package: PathBuf,
        desired_mapped: bool,
        panel_height: u32,
    ) -> Result<(), ShellHostError> {
        let output_key = self
            .selected_output
            .ok_or(ShellHostError::MissingGlobal("wl_output"))?;
        let namespace = match kind {
            SurfaceKind::Panel => PANEL_NAMESPACE,
            SurfaceKind::SingleOverlay => SINGLE_NAMESPACE,
            SurfaceKind::Overlay => OVERLAY_NAMESPACE,
        };
        self.create_surface_for_output(
            qh,
            kind,
            kind.owner(),
            output_key,
            package,
            None,
            namespace,
            namespace,
            None,
            desired_mapped,
            panel_height,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_surface_for_output(
        &mut self,
        _qh: &QueueHandle<Self>,
        kind: SurfaceKind,
        owner: u64,
        output_key: OutputKey,
        package: PathBuf,
        document: Option<PathBuf>,
        template_id: &str,
        namespace: &str,
        instance_context: Option<(String, String)>,
        desired_mapped: bool,
        panel_height: u32,
        reserve_space: bool,
    ) -> Result<(), ShellHostError> {
        self.surfaces.push(ShellSurfaceState {
            owner,
            output_key,
            template_id: template_id.into(),
            instance_generation: self.next_instance_generation,
            document,
            package_snapshot: None,
            instance_context,
            kind,
            package,
            namespace: namespace.into(),
            panel_height,
            reserve_space,
            surface: None,
            layer_surface: None,
            viewport: None,
            fractional_scale: None,
            role_generation: 0,
            scale_state: SurfaceScaleState::new(0, false),
            runtime: None,
            pool: ShmBufferPool::new(owner),
            scheduler: FrameScheduler::default(),
            lifecycle: LayerLifecycle::default(),
            configures: ConfigureCoalescer::default(),
            desired_mapped,
            mapped: false,
            map_state: SurfaceMapState::AwaitingConfigure,
            summary: SurfaceHostSummary::default(),
            maximum_mapped_bytes: 0,
            last_click_count: 0,
            presentation_failed: false,
            pending_scale_started: None,
            scaled_commit_started: None,
            pending_binding_mutation_started: None,
            binding_commit_started: None,
            #[cfg(feature = "gpu-renderer")]
            presenter: SurfacePresenter::new(0),
            #[cfg(feature = "gpu-renderer")]
            gpu_consecutive_timeouts: 0,
        });
        #[cfg(feature = "gpu-renderer")]
        {
            let state = self.surfaces.last_mut().expect("surface pushed above");
            state.summary.gpu.requested = self.gpu_requested;
            state.summary.gpu.presenter_state = "uninitialized".into();
        }
        if desired_mapped && let Err(error) = self.ensure_surface_role(owner) {
            self.surfaces.retain(|surface| surface.owner != owner);
            return Err(error);
        }
        Ok(())
    }

    fn ensure_surface_role(&mut self, owner: u64) -> Result<bool, ShellHostError> {
        let index = self
            .surface_index_by_owner(owner)
            .ok_or_else(|| ShellHostError::Wayland("surface instance is stale".into()))?;
        if self.surfaces[index].surface.is_some() {
            return Ok(false);
        }
        if self.surfaces[index].viewport.is_some()
            || self.surfaces[index].fractional_scale.is_some()
        {
            return Err(ShellHostError::Wayland(
                "surface extensions exist without their wl_surface".into(),
            ));
        }
        let qh = self
            .queue_handle
            .as_ref()
            .ok_or_else(|| ShellHostError::Wayland("Wayland queue is unavailable".into()))?
            .clone();
        let compositor = self
            .compositor
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_compositor"))?;
        let output = self
            .output_proxy(self.surfaces[index].output_key)
            .ok_or(ShellHostError::MissingGlobal("wl_output"))?;
        let layer_shell = self
            .layer_shell
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("zwlr_layer_shell_v1"))?;
        let viewporter = self.viewporter.clone();
        let fractional_manager = self.fractional_scale_manager.clone();
        let fractional_available = viewporter.is_some() && fractional_manager.is_some();
        let surface_generation = self.surfaces[index].role_generation.saturating_add(1);
        let kind = self.surfaces[index].kind;
        let panel_height = self.surfaces[index].panel_height;
        let reserve_space = self.surfaces[index].reserve_space;
        let (layer, anchors, width, height, exclusive_zone) = match kind {
            SurfaceKind::Panel => (
                zwlr_layer_shell_v1::Layer::Top,
                zwlr_layer_surface_v1::Anchor::Top
                    | zwlr_layer_surface_v1::Anchor::Left
                    | zwlr_layer_surface_v1::Anchor::Right,
                0,
                panel_height,
                if reserve_space {
                    panel_height as i32
                } else {
                    0
                },
            ),
            SurfaceKind::SingleOverlay | SurfaceKind::Overlay => (
                zwlr_layer_shell_v1::Layer::Overlay,
                zwlr_layer_surface_v1::Anchor::Top
                    | zwlr_layer_surface_v1::Anchor::Bottom
                    | zwlr_layer_surface_v1::Anchor::Left
                    | zwlr_layer_surface_v1::Anchor::Right,
                0,
                0,
                0,
            ),
        };
        let surface = compositor.create_surface(&qh, SurfaceData { owner });
        surface.set_buffer_scale(1);
        let (viewport, fractional_scale) = match (viewporter, fractional_manager) {
            (Some(viewporter), Some(fractional_manager)) => (
                Some(viewporter.get_viewport(&surface, &qh, ())),
                Some(fractional_manager.get_fractional_scale(
                    &surface,
                    &qh,
                    ScaleData {
                        owner,
                        surface_generation,
                    },
                )),
            ),
            _ => (None, None),
        };
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            Some(&output),
            layer,
            self.surfaces[index].namespace.clone(),
            &qh,
            LayerData { owner },
        );
        layer_surface.set_anchor(anchors);
        layer_surface.set_size(width, height);
        layer_surface.set_exclusive_zone(exclusive_zone);
        layer_surface
            .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        let state = &mut self.surfaces[index];
        state.lifecycle = LayerLifecycle::default();
        state
            .lifecycle
            .assign_role()
            .map_err(|error| ShellHostError::Wayland(error.into()))?;
        surface.commit();
        state
            .lifecycle
            .initial_bufferless_commit()
            .map_err(|error| ShellHostError::Wayland(error.into()))?;
        state.surface = Some(surface);
        state.layer_surface = Some(layer_surface);
        state.viewport = viewport;
        state.fractional_scale = fractional_scale;
        state.role_generation = surface_generation;
        #[cfg(feature = "gpu-renderer")]
        {
            state.presenter = SurfacePresenter::new(surface_generation);
            state.summary.gpu.requested = self.gpu_requested;
            state.summary.gpu.presenter_state = "uninitialized".into();
        }
        state.scale_state = SurfaceScaleState::new(surface_generation, fractional_available);
        state.map_state = SurfaceMapState::AwaitingConfigure;
        #[cfg(feature = "gpu-renderer")]
        {
            state.gpu_consecutive_timeouts = 0;
        }
        Ok(true)
    }

    fn destroy_surface_protocol_objects(state: &mut ShellSurfaceState) {
        if let Some(layer_surface) = state.layer_surface.take() {
            layer_surface.destroy();
        }
        if let Some(fractional_scale) = state.fractional_scale.take() {
            fractional_scale.destroy();
        }
        if let Some(viewport) = state.viewport.take() {
            viewport.destroy();
        }
        if let Some(wayland_surface) = state.surface.take() {
            wayland_surface.destroy();
        }
    }

    fn destroy_transient_surface_role(&mut self, owner: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return;
        };
        #[cfg(feature = "gpu-renderer")]
        self.release_gpu_surface(index, true);
        let state = &mut self.surfaces[index];
        Self::destroy_surface_protocol_objects(state);
        state.lifecycle = LayerLifecycle::default();
        state.configures = ConfigureCoalescer::default();
        state.scheduler = FrameScheduler::default();
        state.map_state = SurfaceMapState::AwaitingConfigure;
        state.role_generation = state.role_generation.saturating_add(1);
        state.scale_state = SurfaceScaleState::new(state.role_generation, false);
        state.pending_scale_started = None;
        state.scaled_commit_started = None;
        state.mapped = false;
        #[cfg(feature = "gpu-renderer")]
        {
            state.gpu_consecutive_timeouts = 0;
        }
    }

    fn surface_index(&self, kind: SurfaceKind) -> Option<usize> {
        self.surfaces
            .iter()
            .position(|surface| surface.kind == kind)
    }

    fn surface_index_by_owner(&self, owner: u64) -> Option<usize> {
        self.surfaces
            .iter()
            .position(|surface| surface.owner == owner)
    }

    #[cfg(feature = "gpu-renderer")]
    fn gpu_surface_id(&self, index: usize) -> RenderSurfaceId {
        RenderSurfaceId {
            instance: self.surfaces[index].owner,
            generation: self.surfaces[index].role_generation,
        }
    }

    #[cfg(feature = "gpu-renderer")]
    fn ensure_gpu_surface(
        &mut self,
        index: usize,
        width: u32,
        height: u32,
    ) -> Result<bool, LiveGpuError> {
        if !self.gpu_requested {
            self.surfaces[index].presenter.select_cpu();
            self.surfaces[index].summary.gpu.presenter_state = "cpu".into();
            return Ok(false);
        }
        if self.surfaces[index].presenter.generation() != self.surfaces[index].role_generation {
            return Err(LiveGpuError::host(
                LiveGpuErrorKind::StaleGeneration,
                "presenter generation does not match the layer surface",
                false,
            ));
        }
        match self.surfaces[index].presenter.state() {
            PresenterState::GpuReady => {
                let id = self.gpu_surface_id(index);
                let needs_configuration = self
                    .gpu
                    .as_ref()
                    .and_then(|gpu| gpu.configuration(id))
                    .is_none_or(|configuration| {
                        configuration.width != width || configuration.height != height
                    });
                if needs_configuration {
                    let configuration = self
                        .gpu
                        .as_mut()
                        .ok_or_else(|| {
                            LiveGpuError::host(
                                LiveGpuErrorKind::BackendUnavailable,
                                "live GPU presenter disappeared",
                                true,
                            )
                        })?
                        .configure(id, width, height)?;
                    self.record_gpu_configuration(index, &configuration);
                }
                self.sync_gpu_summary(index);
                return Ok(true);
            }
            PresenterState::Cpu | PresenterState::FallingBack | PresenterState::Destroyed => {
                return Ok(false);
            }
            PresenterState::GpuCreating | PresenterState::GpuRecovering => {
                return Err(LiveGpuError::host(
                    LiveGpuErrorKind::InvalidConfiguration,
                    "live GPU presenter has an unfinished lifecycle transition",
                    true,
                ));
            }
            PresenterState::Uninitialized => {}
        }
        if !self.surfaces[index].presenter.begin_gpu() {
            return Ok(false);
        }
        self.surfaces[index].summary.gpu.presenter_state = "gpu-creating".into();
        let id = self.gpu_surface_id(index);
        let display = self.wayland_display.ok_or_else(|| {
            LiveGpuError::host(
                LiveGpuErrorKind::SurfaceCreation,
                "libwayland display handle is unavailable",
                false,
            )
        })?;
        let surface_pointer = self.surfaces[index]
            .surface
            .as_ref()
            .map(|surface| surface.id().as_ptr().cast::<std::ffi::c_void>())
            .ok_or_else(|| {
                LiveGpuError::host(
                    LiveGpuErrorKind::SurfaceCreation,
                    "layer-shell surface is unavailable",
                    false,
                )
            })?;
        // SAFETY: the system wayland backend exposes pointers for the same
        // live connection and wl_surface. Presenter teardown always precedes
        // destruction of those protocol objects.
        let handle = unsafe { LiveWaylandHandle::new(display.as_ptr(), surface_pointer) }?;
        let result = if let Some(gpu) = &mut self.gpu {
            // SAFETY: the handle lifetime is bounded by this surface
            // generation and `release_gpu_surface`.
            unsafe { gpu.create_surface(id, handle) }
        } else {
            let device_generation = self.gpu_device_generation.checked_add(1).ok_or_else(|| {
                LiveGpuError::host(
                    LiveGpuErrorKind::DeviceLost,
                    "live GPU device generation exhausted",
                    false,
                )
            })?;
            // SAFETY: the connection outlives State and every presenter; the
            // first surface is released before its wl_surface is destroyed.
            unsafe { LiveGpuPresenter::new_with_generation(id, handle, device_generation) }.map(
                |gpu| {
                    self.gpu_device_generation = device_generation;
                    self.gpu = Some(gpu);
                },
            )
        };
        if let Err(error) = result {
            self.surfaces[index].presenter.fall_back();
            self.surfaces[index].summary.gpu.presenter_state = "falling-back".into();
            self.surfaces[index].summary.gpu.last_error = error.to_string();
            return Err(error);
        }
        self.surfaces[index].summary.gpu.presenter_creations = self.surfaces[index]
            .summary
            .gpu
            .presenter_creations
            .saturating_add(1);
        match self
            .gpu
            .as_mut()
            .expect("created above")
            .configure(id, width, height)
        {
            Ok(configuration) => {
                self.surfaces[index].presenter.gpu_ready();
                self.surfaces[index].summary.gpu.presenter_state = "gpu-ready".into();
                self.record_gpu_configuration(index, &configuration);
                self.sync_gpu_summary(index);
                Ok(true)
            }
            Err(error) => {
                self.gpu
                    .as_mut()
                    .expect("created above")
                    .release_surface(id);
                self.surfaces[index].presenter.fall_back();
                self.surfaces[index].summary.gpu.presenter_state = "falling-back".into();
                self.surfaces[index].summary.gpu.last_error = error.to_string();
                self.sync_gpu_summary(index);
                Err(error)
            }
        }
    }

    #[cfg(feature = "gpu-renderer")]
    fn record_gpu_configuration(&mut self, index: usize, configuration: &LiveGpuConfiguration) {
        let successful_gpu_frame = self.surfaces[index].presenter.gpu_succeeded();
        let summary = &mut self.surfaces[index].summary.gpu;
        summary.successful_gpu_frame = successful_gpu_frame;
        if summary.configuration_generation == 0 {
            summary.configurations = summary.configurations.saturating_add(1);
        } else {
            summary.reconfigurations = summary.reconfigurations.saturating_add(1);
        }
        summary.surface_format = configuration.format.clone();
        summary.present_mode = configuration.present_mode.clone();
        summary.alpha_mode = configuration.alpha_mode.clone();
        summary.configuration_generation = configuration.generation;
    }

    #[cfg(feature = "gpu-renderer")]
    fn sync_gpu_summary(&mut self, index: usize) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let backend = gpu.backend_info();
        let (entries, bytes, uploads, hits) = gpu.resource_statistics();
        let successful_gpu_frame = self.surfaces[index].presenter.gpu_succeeded();
        let summary = &mut self.surfaces[index].summary.gpu;
        summary.successful_gpu_frame = successful_gpu_frame;
        summary.adapter = backend.adapter;
        summary.graphics_api = backend.graphics_api;
        summary.device_type = backend.device_type;
        summary.driver = backend.driver;
        summary.device_generation = backend.device_generation;
        summary.resource_entries = entries;
        summary.resource_bytes = bytes;
        summary.resource_uploads = uploads;
        summary.cache_hits = hits;
    }

    #[cfg(feature = "gpu-renderer")]
    fn release_gpu_surface(&mut self, index: usize, destroyed: bool) {
        let id = self.gpu_surface_id(index);
        let had_gpu_target = matches!(
            self.surfaces[index].presenter.state(),
            PresenterState::GpuCreating | PresenterState::GpuReady | PresenterState::GpuRecovering
        );
        if let Some(gpu) = &mut self.gpu {
            gpu.release_surface(id);
        }
        if had_gpu_target {
            self.surfaces[index].summary.gpu.presenter_releases = self.surfaces[index]
                .summary
                .gpu
                .presenter_releases
                .saturating_add(1);
        }
        if destroyed {
            self.surfaces[index].presenter.destroy();
            self.surfaces[index].summary.gpu.presenter_state = "destroyed".into();
        } else {
            self.surfaces[index].presenter.fall_back();
            self.surfaces[index].summary.gpu.presenter_state = "falling-back".into();
        }
        self.sync_gpu_summary(index);
    }

    #[cfg(feature = "gpu-renderer")]
    fn fall_back_gpu_surface(&mut self, index: usize, error: &LiveGpuError) {
        if let Some(gpu) = &mut self.gpu {
            gpu.record_cpu_fallback();
        }
        self.release_gpu_surface(index, false);
        let summary = &mut self.surfaces[index].summary.gpu;
        summary.cpu_fallbacks = summary.cpu_fallbacks.saturating_add(1);
        summary.last_error = error.to_string();
    }

    #[cfg(feature = "gpu-renderer")]
    fn record_gpu_render_error(&mut self, index: usize, error: &LiveGpuError) {
        let summary = &mut self.surfaces[index].summary.gpu;
        match error.kind {
            LiveGpuErrorKind::SurfaceTimeout => {
                summary.acquisition_failures = summary.acquisition_failures.saturating_add(1);
                summary.surface_timeouts = summary.surface_timeouts.saturating_add(1);
            }
            LiveGpuErrorKind::SurfaceOccluded => {
                summary.acquisition_failures = summary.acquisition_failures.saturating_add(1);
            }
            LiveGpuErrorKind::SurfaceLost => {
                summary.acquisition_failures = summary.acquisition_failures.saturating_add(1);
                summary.surface_losses = summary.surface_losses.saturating_add(1);
            }
            LiveGpuErrorKind::SurfaceOutdated => {
                summary.acquisition_failures = summary.acquisition_failures.saturating_add(1);
                summary.surface_outdated = summary.surface_outdated.saturating_add(1);
            }
            LiveGpuErrorKind::DeviceLost => {
                summary.device_losses = summary.device_losses.saturating_add(1);
            }
            _ => {}
        }
    }

    #[cfg(feature = "gpu-renderer")]
    fn handle_gpu_device_loss(&mut self, failing_index: usize, error: &LiveGpuError) {
        if let Some(gpu) = &mut self.gpu {
            gpu.model_device_loss();
        }
        // A real lost wgpu device cannot be reused. Drop the one process
        // backend after invalidating its generation, targets, pipelines, and
        // cache. Current surface generations remain on CPU; a later Wayland
        // surface generation may construct one fresh process backend.
        self.gpu.take();
        for index in 0..self.surfaces.len() {
            if matches!(
                self.surfaces[index].presenter.state(),
                PresenterState::GpuCreating
                    | PresenterState::GpuReady
                    | PresenterState::GpuRecovering
            ) {
                self.release_gpu_surface(index, false);
                let summary = &mut self.surfaces[index].summary.gpu;
                if index != failing_index {
                    summary.device_losses = summary.device_losses.saturating_add(1);
                }
                summary.last_error = error.to_string();
                if index != failing_index {
                    summary.cpu_fallbacks = summary.cpu_fallbacks.saturating_add(1);
                }
                self.surfaces[index].scheduler.mark_dirty();
            }
        }
    }

    fn reconcile_all_outputs(&mut self, qh: &QueueHandle<Self>) {
        let keys: Vec<_> = self
            .output_catalog
            .present()
            .iter()
            .map(|record| record.key)
            .collect();
        for key in keys {
            self.reconcile_output(key, qh);
        }
        if self.output_instances.is_empty() {
            eprintln!("htmshell-live: no eligible output is currently present");
        }
    }

    fn reconcile_output(&mut self, key: OutputKey, qh: &QueueHandle<Self>) {
        if !matches!(self.options, SessionOptions::Manifest(_)) {
            return;
        }
        let eligibility = self
            .output_catalog
            .get(key)
            .map(|record| record.eligibility(self.fractional_available()))
            .unwrap_or(OutputEligibility::Removed);
        let instantiated = self
            .output_instances
            .iter()
            .any(|instance| instance.key == key);
        match (eligibility, instantiated) {
            (
                OutputEligibility::EligibleScale1 | OutputEligibility::EligibleFractional(_),
                false,
            ) => {
                if let Err(error) = self.create_manifest_output(key, qh) {
                    eprintln!(
                        "htmshell-live: output {} could not be instantiated: {error}",
                        key.global_name
                    );
                }
            }
            (OutputEligibility::UnsupportedScale(scale), true) => {
                self.unsupported_scale_outputs = self.unsupported_scale_outputs.saturating_add(1);
                eprintln!(
                    "htmshell-live: output {} advertises scale {scale}; fractional protocols are incomplete, so this output is unavailable",
                    key.global_name
                );
                self.destroy_output_instance(key);
            }
            (OutputEligibility::UnsupportedScale(scale), false) => {
                self.unsupported_scale_outputs = self.unsupported_scale_outputs.saturating_add(1);
                eprintln!(
                    "htmshell-live: output {} advertises scale {scale}; fractional protocols are incomplete, so this output is unavailable",
                    key.global_name
                );
            }
            (OutputEligibility::Removed, true) => self.destroy_output_instance(key),
            _ => {}
        }
    }

    fn create_manifest_output(
        &mut self,
        key: OutputKey,
        qh: &QueueHandle<Self>,
    ) -> Result<(), ShellHostError> {
        let options = match &self.options {
            SessionOptions::Manifest(options) => options.clone(),
            _ => return Ok(()),
        };
        let output = self
            .output_catalog
            .get(key)
            .ok_or_else(|| ShellHostError::Wayland("output generation is stale".into()))?;
        if !matches!(
            output.eligibility(self.fractional_available()),
            OutputEligibility::EligibleScale1 | OutputEligibility::EligibleFractional(_)
        ) {
            return Ok(());
        }
        let diagnostic_label = output.diagnostic_label();
        let output_ready_us = self
            .outputs
            .iter()
            .find(|bound| bound.key == key)
            .map(|bound| elapsed_us(bound.advertised_at))
            .unwrap_or_default();
        let panel = options
            .manifest
            .manifest()
            .surfaces
            .iter()
            .find(|surface| surface.kind() == ManifestSurfaceKind::Panel)
            .cloned()
            .ok_or_else(|| ShellHostError::Manifest("panel template is missing".into()))?;
        let overlay = options
            .manifest
            .manifest()
            .surfaces
            .iter()
            .find(|surface| surface.kind() == ManifestSurfaceKind::Overlay)
            .cloned()
            .ok_or_else(|| ShellHostError::Manifest("overlay template is missing".into()))?;
        self.next_surface_owner = self.next_surface_owner.saturating_add(1);
        let panel_owner = self.next_surface_owner;
        self.next_surface_owner = self.next_surface_owner.saturating_add(1);
        let overlay_owner = self.next_surface_owner;
        self.next_instance_generation = self.next_instance_generation.saturating_add(1);
        let panel_generation = self.next_instance_generation;
        let overlay_initially_open = overlay
            .overlay()
            .ok_or_else(|| ShellHostError::Manifest("overlay preset is invalid".into()))?
            .initially_open;
        let panel_preset = panel
            .panel()
            .ok_or_else(|| ShellHostError::Manifest("panel preset is invalid".into()))?;
        let mut panel_runtime = LiveDocument::load_surface_snapshot(
            Arc::clone(options.manifest.snapshot()),
            &panel,
            LiveDocumentKind::Panel,
            1,
            panel_preset.thickness,
        )?;
        panel_runtime.set_instance_context(panel.id(), &diagnostic_label)?;
        panel_runtime.update_panel_state(overlay_initially_open, "Ready")?;
        apply_surface_bindings(
            &mut panel_runtime,
            panel.id(),
            &diagnostic_label,
            LIVE_SCALE_DENOMINATOR,
            PresentationProfile::Scale1,
            overlay_initially_open,
            0,
            "Ready",
        )?;
        self.next_instance_generation = self.next_instance_generation.saturating_add(1);
        let overlay_generation = self.next_instance_generation;
        let mut overlay_runtime = LiveDocument::load_surface_snapshot(
            Arc::clone(options.manifest.snapshot()),
            &overlay,
            LiveDocumentKind::TransientOverlay,
            1,
            1,
        )?;
        overlay_runtime.set_instance_context(overlay.id(), &diagnostic_label)?;
        overlay_runtime.update_overlay_state(0, "Ready")?;
        apply_surface_bindings(
            &mut overlay_runtime,
            overlay.id(),
            &diagnostic_label,
            LIVE_SCALE_DENOMINATOR,
            PresentationProfile::Scale1,
            overlay_initially_open,
            0,
            "Ready",
        )?;
        self.create_surface_for_output(
            qh,
            SurfaceKind::Panel,
            panel_owner,
            key,
            options.manifest.package_root().to_path_buf(),
            Some(panel.document().to_path_buf()),
            panel.id(),
            panel.namespace(),
            Some((panel.id().into(), diagnostic_label.clone())),
            true,
            panel_preset.thickness,
            panel_preset.reserve_space,
        )?;
        if let Some(index) = self.surface_index_by_owner(panel_owner) {
            self.surfaces[index].instance_generation = panel_generation;
            self.surfaces[index].runtime = Some(panel_runtime);
            self.surfaces[index].package_snapshot = Some(Arc::clone(options.manifest.snapshot()));
        }
        if let Err(error) = self.create_surface_for_output(
            qh,
            SurfaceKind::Overlay,
            overlay_owner,
            key,
            options.manifest.package_root().to_path_buf(),
            Some(overlay.document().to_path_buf()),
            overlay.id(),
            overlay.namespace(),
            Some((overlay.id().into(), diagnostic_label.clone())),
            overlay_initially_open,
            0,
            false,
        ) {
            self.destroy_surface_owner(panel_owner);
            return Err(error);
        }
        if let Some(index) = self.surface_index_by_owner(overlay_owner) {
            self.surfaces[index].instance_generation = overlay_generation;
            self.surfaces[index].runtime = Some(overlay_runtime);
            self.surfaces[index].package_snapshot = Some(Arc::clone(options.manifest.snapshot()));
        }
        self.output_instances.push(OutputShellInstance {
            key,
            diagnostic_label,
            panel_owner,
            overlay_owner,
            shared: SharedShellState {
                overlay_open: overlay_initially_open,
                last_action: "Ready".into(),
                ..SharedShellState::default()
            },
            created_at: Instant::now(),
            output_ready_us,
            first_panel_frame_us: 0,
        });
        self.output_instances.sort_by_key(|instance| instance.key);
        self.output_additions = self.output_additions.saturating_add(1);
        if self.first_output_instance_us == 0 {
            self.first_output_instance_us = elapsed_us(self.started);
        }
        self.peak_output_instances = self.peak_output_instances.max(self.output_instances.len());
        self.peak_runtime_documents = self
            .peak_runtime_documents
            .max(self.output_instances.len().saturating_mul(2));
        if let Err(error) = self.reconcile_clock_subscribers() {
            self.destroy_output_instance(key);
            return Err(error);
        }
        self.reconcile_battery_subscribers();
        if let Err(error) = self.reconcile_pipewire_demand() {
            self.destroy_output_instance(key);
            return Err(error);
        }
        self.maybe_stop_after_output_events();
        Ok(())
    }

    fn maybe_stop_after_output_events(&mut self) {
        let target = match &self.options {
            SessionOptions::Manifest(options) => options.exit_after_output_events,
            _ => None,
        };
        if target.is_some_and(|target| {
            self.output_additions.saturating_add(self.output_removals) >= target
                && !self.output_instances.is_empty()
                && self
                    .surfaces
                    .iter()
                    .filter(|surface| surface.desired_mapped)
                    .all(|surface| surface.summary.frame_callbacks > 0)
        }) {
            self.running = false;
        }
    }

    fn destroy_output_instance(&mut self, key: OutputKey) {
        let started = Instant::now();
        let Some(index) = self
            .output_instances
            .iter()
            .position(|instance| instance.key == key)
        else {
            return;
        };
        let instance = self.output_instances.remove(index);
        self.destroy_surface_owner(instance.overlay_owner);
        self.destroy_surface_owner(instance.panel_owner);
        self.last_output_teardown_us = elapsed_us(started);
        if let Err(error) = self.reconcile_clock_subscribers() {
            self.fail(format!("clock subscription teardown failed: {error}"));
        }
        self.reconcile_battery_subscribers();
        if let Err(error) = self.reconcile_pipewire_demand() {
            self.fail(format!("PipeWire demand teardown failed: {error}"));
        }
    }

    fn clock_subscriber_count(&self) -> usize {
        self.surfaces
            .iter()
            .filter(|surface| {
                surface.runtime.as_ref().is_some_and(|runtime| {
                    runtime.text_binding_target_count(StateBindingKey::ClockTime) > 0
                        || !runtime.clock_declarations().is_empty()
                })
            })
            .count()
    }

    fn reconcile_clock_subscribers(&mut self) -> Result<(), ShellHostError> {
        let subscribers = self.clock_subscriber_count();
        let previous = self.clock.subscriber_count();
        let legacy_consumers = self
            .surfaces
            .iter()
            .filter(|surface| {
                surface.runtime.as_ref().is_some_and(|runtime| {
                    runtime.text_binding_target_count(StateBindingKey::ClockTime) > 0
                })
            })
            .count();
        let declarations = self
            .surfaces
            .iter()
            .filter_map(|surface| surface.runtime.as_ref())
            .flat_map(LiveDocument::clock_declarations)
            .collect();
        let update = self
            .clock
            .reconcile(subscribers, legacy_consumers, declarations)
            .map_err(|error| ShellHostError::Clock(error.to_string()))?;
        let update_had_legacy = update
            .as_ref()
            .is_some_and(|update| update.legacy.is_some());
        if let Some(update) = update {
            self.fanout_clock_update(&update);
        }
        if subscribers > previous
            && legacy_consumers > 0
            && !update_had_legacy
            && let Some(snapshot) = self.clock.current_snapshot().cloned()
        {
            self.fanout_clock_update(&ClockUpdate {
                legacy: Some(snapshot),
                declarations: Vec::new(),
                sequence: self.clock.summary().sequence,
            });
        }
        Ok(())
    }

    fn handle_clock_ready(&mut self) -> Result<(), ShellHostError> {
        let snapshot = self
            .clock
            .handle_ready()
            .map_err(|error| ShellHostError::Clock(error.to_string()))?;
        if let Some(snapshot) = snapshot {
            self.fanout_clock_update(&snapshot);
        }
        self.maybe_stop_after_clock_updates();
        Ok(())
    }

    fn power_subscriber_counts(&self) -> (usize, usize) {
        let mut upower = 0usize;
        let mut profiles = 0usize;
        for runtime in self
            .surfaces
            .iter()
            .filter_map(|surface| surface.runtime.as_ref())
        {
            let declarations = runtime.built_in_declarations();
            let upower_bound =
                StateBindingKey::ALL.into_iter().any(|key| {
                    (key.as_str().starts_with("upower.") || key.as_str().starts_with("battery."))
                        && runtime.binding_target_count(key) > 0
                }) || runtime.repeat_source_target_count(RepeatSource::UPowerDevices) > 0;
            let profile_bound = StateBindingKey::ALL.into_iter().any(|key| {
                key.as_str().starts_with("power_profile.") && runtime.binding_target_count(key) > 0
            }) || runtime
                .repeat_source_target_count(RepeatSource::PowerProfileHolds)
                > 0
                || declarations.iter().any(|declaration| {
                    declaration
                        .action
                        .is_some_and(|action| action.as_str().starts_with("power_profile."))
                        || declaration
                            .enabled_binding
                            .is_some_and(|key| key.as_str().starts_with("power_profile."))
                });
            upower = upower.saturating_add(usize::from(upower_bound));
            profiles = profiles.saturating_add(usize::from(profile_bound));
        }
        (upower, profiles)
    }

    fn reconcile_battery_subscribers(&mut self) {
        let (upower, profiles) = self.power_subscriber_counts();
        let previous_upower = self.battery.upower_subscriber_count();
        let previous_profiles = self.battery.profile_subscriber_count();
        let snapshot = self.battery.set_subscriber_counts(upower, profiles);
        if upower > previous_upower || profiles > previous_profiles {
            let snapshot = snapshot.unwrap_or_else(|| self.battery.current_snapshot().clone());
            self.fanout_battery_snapshot(&snapshot);
        }
    }

    fn aggregate_pipewire_demand(&self) -> Result<PipeWireDemand, ShellHostError> {
        let mut demand = PipeWireDemand::default();
        for surface in &self.surfaces {
            let Some(runtime) = surface.runtime.as_ref() else {
                continue;
            };
            demand.add_document(&runtime.pipewire_demand());
            demand.add_peak_declarations(runtime.pipewire_peak_demands(surface.mapped));
        }
        if demand.property_keys.len() > MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS {
            return Err(ShellHostError::Wayland(format!(
                "PipeWire property-key demand exceeds {MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS} unique keys"
            )));
        }
        if demand.peak_declarations.len() > 4096 {
            return Err(ShellHostError::Wayland(
                "PipeWire peak monitor demand exceeds 4096 active declaration identities".into(),
            ));
        }
        let mut peak_targets = std::collections::BTreeMap::<PipeWirePeakTarget, usize>::new();
        for target in demand.peak_declarations.values() {
            let count = peak_targets.entry(target.clone()).or_default();
            *count = count.saturating_add(1);
            if *count > MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET {
                return Err(ShellHostError::Wayland(format!(
                    "PipeWire peak monitor demand exceeds {MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET} active declarations for one target"
                )));
            }
        }
        Ok(demand)
    }

    fn reconcile_pipewire_demand(&mut self) -> Result<(), ShellHostError> {
        let demand = self.aggregate_pipewire_demand()?;
        let demand_changed = self.pipewire.set_demand(demand.clone());
        if demand_changed || !demand.is_empty() {
            let snapshot = self.pipewire.snapshot().clone();
            self.fanout_pipewire_snapshot(&snapshot, &demand);
        }
        Ok(())
    }

    fn fanout_current_pipewire_snapshot(&mut self) {
        let Ok(demand) = self.aggregate_pipewire_demand() else {
            return;
        };
        let snapshot = self.pipewire.snapshot().clone();
        self.fanout_pipewire_snapshot(&snapshot, &demand);
    }

    fn fanout_pipewire_snapshot(&mut self, snapshot: &PipeWireSnapshot, demand: &PipeWireDemand) {
        let mut projections = snapshot.public_projections(demand);
        let peak_projections = self.pipewire.peak_projections();
        for repeat in projections
            .repeats
            .iter_mut()
            .filter(|repeat| repeat.source == RepeatSource::PipeWireNodes)
        {
            for item in &mut repeat.items {
                let can_monitor = peak_projections.monitorable_nodes.contains(&item.key);
                item.text.insert(
                    ItemBindingKey::CanMonitorPeaks,
                    if can_monitor { "true" } else { "false" }.into(),
                );
                item.tokens.insert(
                    ItemBindingKey::CanMonitorPeaks,
                    if can_monitor { "true" } else { "false" }.into(),
                );
            }
        }
        for (key, item_key) in [
            (
                StateBindingKey::PipeWireDefaultSinkCanMonitorPeaks,
                peak_projections.default_sink_item_key.as_deref(),
            ),
            (
                StateBindingKey::PipeWireDefaultSourceCanMonitorPeaks,
                peak_projections.default_source_item_key.as_deref(),
            ),
        ] {
            let can_monitor = item_key
                .is_some_and(|item_key| peak_projections.monitorable_nodes.contains(item_key));
            if let Some((_, value)) = projections
                .text
                .iter_mut()
                .find(|(binding, _)| *binding == key)
            {
                *value = if can_monitor { "true" } else { "false" }.into();
            }
            if let Some((_, value)) = projections
                .tokens
                .iter_mut()
                .find(|(binding, _)| *binding == key)
            {
                *value = if can_monitor {
                    StateToken::True
                } else {
                    StateToken::False
                };
            }
            if let Some((_, value)) = projections
                .booleans
                .iter_mut()
                .find(|(binding, _)| *binding == key)
            {
                *value = Some(can_monitor);
            }
        }
        for surface in &mut self.surfaces {
            let Some(runtime) = surface.runtime.as_mut() else {
                continue;
            };
            if runtime.pipewire_demand().is_empty() {
                continue;
            }
            let result = (|| {
                let state = runtime.apply_bound_state(&projections.text, &projections.tokens)?;
                let values = runtime.apply_bound_values(&projections.values)?;
                let booleans = runtime.apply_bound_booleans(&projections.booleans)?;
                let mut changed = state
                    .changed_elements
                    .saturating_add(values.changed_elements)
                    .saturating_add(booleans.changed_elements);
                for repeat in &projections.repeats {
                    let mutation = runtime.apply_repeat_source(repeat)?;
                    changed = changed
                        .saturating_add(mutation.insertions)
                        .saturating_add(mutation.removals)
                        .saturating_add(mutation.moves)
                        .saturating_add(mutation.property_updates);
                }
                changed = changed.saturating_add(
                    runtime.apply_pipewire_peak_projections(&peak_projections, surface.mapped)?,
                );
                Ok::<usize, htm_runtime::RuntimeError>(changed)
            })();
            match result {
                Ok(changed) if changed > 0 && surface.desired_mapped => {
                    surface
                        .pending_binding_mutation_started
                        .get_or_insert_with(Instant::now);
                    surface.scheduler.mark_dirty();
                }
                Ok(_) => {}
                Err(error) => eprintln!(
                    "htmshell-live: PipeWire update for surface {} was contained: {error}",
                    surface.owner
                ),
            }
        }
    }

    fn handle_battery_bus_ready(&mut self) {
        if let Some(snapshot) = self.battery.handle_bus_ready() {
            self.fanout_battery_snapshot(&snapshot);
        }
        self.maybe_stop_after_battery_updates();
    }

    fn handle_battery_immediate_dispatch(&mut self) {
        if let Some(snapshot) = self.battery.handle_immediate_dispatch() {
            self.fanout_battery_snapshot(&snapshot);
        }
        self.maybe_stop_after_battery_updates();
    }

    fn handle_battery_deadline_ready(&mut self) {
        if let Some(snapshot) = self.battery.handle_deadline_ready() {
            self.fanout_battery_snapshot(&snapshot);
        }
        self.maybe_stop_after_battery_updates();
    }

    fn fanout_battery_snapshot(&mut self, snapshot: &PowerSnapshot) {
        let started = Instant::now();
        let projection_started = Instant::now();
        let projections = snapshot.projections();
        let projection_us = elapsed_us(projection_started);
        let mut documents = 0usize;
        let mut elements = 0usize;
        let mut frames = 0usize;
        let mut closed_frames_suppressed = 0usize;
        let mut failures = 0usize;
        for surface in &mut self.surfaces {
            let Some(runtime) = surface.runtime.as_mut() else {
                continue;
            };
            let subscribes = StateBindingKey::ALL.into_iter().any(|key| {
                (key.as_str().starts_with("battery.")
                    || key.as_str().starts_with("upower.")
                    || key.as_str().starts_with("power_profile."))
                    && runtime.binding_target_count(key) > 0
            }) || runtime.repeat_source_target_count(RepeatSource::UPowerDevices)
                > 0
                || runtime.repeat_source_target_count(RepeatSource::PowerProfileHolds) > 0;
            if !subscribes {
                continue;
            }
            documents = documents.saturating_add(1);
            let result = (|| {
                let state = runtime.apply_bound_state(&projections.text, &projections.tokens)?;
                let values = runtime.apply_bound_values(&projections.values)?;
                let booleans = runtime.apply_bound_booleans(&projections.booleans)?;
                let mut changed = state
                    .changed_elements
                    .saturating_add(values.changed_elements)
                    .saturating_add(booleans.changed_elements);
                for repeat in &projections.repeats {
                    let mutation = runtime.apply_repeat_source(repeat)?;
                    changed = changed
                        .saturating_add(mutation.insertions)
                        .saturating_add(mutation.removals)
                        .saturating_add(mutation.moves)
                        .saturating_add(mutation.property_updates);
                }
                Ok::<usize, htm_runtime::RuntimeError>(changed)
            })();
            match result {
                Ok(changed) => {
                    elements = elements.saturating_add(changed);
                    if changed > 0 {
                        if surface.desired_mapped {
                            surface
                                .pending_binding_mutation_started
                                .get_or_insert_with(Instant::now);
                            surface.scheduler.mark_dirty();
                            frames = frames.saturating_add(1);
                        } else {
                            closed_frames_suppressed = closed_frames_suppressed.saturating_add(1);
                        }
                    }
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    eprintln!(
                        "htmshell-live: battery binding update for surface {} was contained: {error}",
                        surface.owner
                    );
                }
            }
        }
        self.battery.record_fanout(PowerFanoutMetrics {
            documents,
            elements,
            frames,
            closed_frames_suppressed,
            failures,
            fanout_us: elapsed_us(started),
            projection_us,
        });
    }

    fn fanout_clock_update(&mut self, update: &ClockUpdate) {
        let started = Instant::now();
        let mut visited = std::collections::BTreeSet::new();
        let mut changed_documents = std::collections::BTreeSet::new();
        let mut elements = 0usize;
        let mut panel_frames = 0usize;
        let mut closed_frames_suppressed = 0usize;
        let mut failures = 0usize;
        if let Some(snapshot) = &update.legacy {
            for surface in &mut self.surfaces {
                let Some(runtime) = surface.runtime.as_mut() else {
                    continue;
                };
                if runtime.text_binding_target_count(StateBindingKey::ClockTime) == 0 {
                    continue;
                }
                visited.insert(surface.owner);
                let values = [(StateBindingKey::ClockTime, snapshot.display_text.clone())];
                match runtime.apply_bound_text(&values) {
                    Ok(binding) => {
                        elements = elements.saturating_add(binding.changed_elements);
                        if binding.changed_elements > 0 {
                            changed_documents.insert(surface.owner);
                        }
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        eprintln!(
                            "htmshell-live: clock binding update for surface {} was contained: {error}",
                            surface.owner
                        );
                    }
                }
            }
        }
        for declaration in &update.declarations {
            let Some(surface) = self.surfaces.iter_mut().find(|surface| {
                surface.runtime.as_ref().is_some_and(|runtime| {
                    runtime.validate_element_identity(&declaration.id).is_ok()
                })
            }) else {
                failures = failures.saturating_add(1);
                continue;
            };
            visited.insert(surface.owner);
            let runtime = surface.runtime.as_mut().expect("matched above");
            match runtime.apply_clock_output(
                &declaration.id,
                &declaration.display_text,
                &declaration.datetime,
                declaration.enabled,
            ) {
                Ok(mutation) => {
                    let changed = usize::from(mutation.changed_text)
                        + usize::from(mutation.changed_datetime)
                        + usize::from(mutation.changed_enabled_state);
                    elements = elements.saturating_add(changed);
                    if mutation.changed() {
                        changed_documents.insert(surface.owner);
                    }
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    eprintln!(
                        "htmshell-live: clock declaration update for surface {} was contained: {error}",
                        surface.owner
                    );
                }
            }
        }
        for owner in changed_documents {
            let Some(surface) = self
                .surfaces
                .iter_mut()
                .find(|surface| surface.owner == owner)
            else {
                continue;
            };
            if surface.desired_mapped {
                surface
                    .pending_binding_mutation_started
                    .get_or_insert_with(Instant::now);
                surface.scheduler.mark_dirty();
                if surface.kind == SurfaceKind::Panel {
                    panel_frames = panel_frames.saturating_add(1);
                }
            } else {
                closed_frames_suppressed = closed_frames_suppressed.saturating_add(1);
            }
        }
        self.clock.record_fanout(
            visited.len(),
            elements,
            panel_frames,
            closed_frames_suppressed,
            failures,
            elapsed_us(started),
        );
    }

    fn destroy_surface_owner(&mut self, owner: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return;
        };
        #[cfg(feature = "gpu-renderer")]
        self.release_gpu_surface(index, true);
        if self.pointer_focus == Some(owner) {
            self.clear_pointer_focus();
        }
        let mut surface = self.surfaces.swap_remove(index);
        surface.lifecycle.output_lost();
        surface.desired_mapped = false;
        surface.mapped = false;
        surface.scheduler.stop_scheduling();
        surface.map_state.close();
        surface.pool.destroy_all();
        Self::destroy_surface_protocol_objects(&mut surface);
    }

    fn maybe_render_all(&mut self, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        let owners: Vec<u64> = self.surfaces.iter().map(|surface| surface.owner).collect();
        for owner in owners {
            if let Err(error) = self.maybe_render(owner, qh) {
                if matches!(self.options, SessionOptions::Manifest(_)) {
                    if let Some(index) = self.surface_index_by_owner(owner) {
                        self.surfaces[index].presentation_failed = true;
                        self.surfaces[index].scheduler.stop_scheduling();
                    }
                    eprintln!("htmshell-live: surface instance {owner} stopped: {error}");
                    continue;
                }
                return Err(error);
            }
        }
        self.update_combined_memory()?;
        Ok(())
    }

    #[cfg(feature = "gpu-renderer")]
    fn recover_gpu_surface_for_frame(
        &mut self,
        index: usize,
        width: u32,
        height: u32,
    ) -> Result<(), LiveGpuError> {
        if !self.surfaces[index].presenter.begin_recovery() {
            return Err(LiveGpuError::host(
                LiveGpuErrorKind::SurfaceLost,
                "live GPU surface recovery limit was reached",
                true,
            ));
        }
        self.surfaces[index].summary.gpu.presenter_state = "gpu-recovering".into();
        let id = self.gpu_surface_id(index);
        let gpu = self.gpu.as_mut().ok_or_else(|| {
            LiveGpuError::host(
                LiveGpuErrorKind::BackendUnavailable,
                "live GPU presenter disappeared during recovery",
                true,
            )
        })?;
        gpu.recover_surface(id)?;
        self.surfaces[index].summary.gpu.target_recreations = self.surfaces[index]
            .summary
            .gpu
            .target_recreations
            .saturating_add(1);
        let configuration = gpu.configure(id, width, height)?;
        self.surfaces[index].presenter.gpu_ready();
        self.surfaces[index].summary.gpu.presenter_state = "gpu-ready".into();
        self.record_gpu_configuration(index, &configuration);
        self.sync_gpu_summary(index);
        Ok(())
    }

    #[cfg(feature = "gpu-renderer")]
    fn gpu_frame_fallback(
        &mut self,
        index: usize,
        prepared: LiveGpuPreparedFrame,
        error: LiveGpuError,
    ) -> Result<GpuFrameAttempt, ShellHostError> {
        self.fall_back_gpu_surface(index, &error);
        let frame = self.surfaces[index]
            .runtime
            .as_mut()
            .expect("runtime initialized before presentation")
            .render_gpu_frame_on_cpu(prepared)?;
        Ok(GpuFrameAttempt::CpuFallback(frame))
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "gpu-renderer")]
    fn try_present_gpu_frame(
        &mut self,
        index: usize,
        request: LiveRenderRequest,
        logical_width: u32,
        logical_height: u32,
        compositor: &wl_compositor::WlCompositor,
        wayland_surface: &wl_surface::WlSurface,
        qh: &QueueHandle<Self>,
    ) -> Result<GpuFrameAttempt, ShellHostError> {
        let owner = self.surfaces[index].owner;
        let generation = self.surfaces[index].role_generation;
        let Some(prepared) = self.surfaces[index]
            .runtime
            .as_mut()
            .expect("runtime initialized before presentation")
            .prepare_gpu_pending_for(request, owner, generation)?
        else {
            self.surfaces[index].scheduler.mark_clean();
            self.surfaces[index]
                .summary
                .gpu
                .duplicate_frame_suppressions = self.surfaces[index]
                .summary
                .gpu
                .duplicate_frame_suppressions
                .saturating_add(1);
            return Ok(GpuFrameAttempt::NoFrame);
        };
        self.surfaces[index].summary.gpu.frames_planned = self.surfaces[index]
            .summary
            .gpu
            .frames_planned
            .saturating_add(1);
        let first = self
            .gpu
            .as_mut()
            .expect("configured GPU presenter")
            .render(&prepared);
        let pending = match first {
            Ok(pending) => pending,
            Err(error)
                if matches!(
                    error.kind,
                    LiveGpuErrorKind::SurfaceLost | LiveGpuErrorKind::SurfaceOutdated
                ) =>
            {
                self.record_gpu_render_error(index, &error);
                if let Err(recovery_error) = self.recover_gpu_surface_for_frame(
                    index,
                    request.buffer_width,
                    request.buffer_height,
                ) {
                    self.record_gpu_render_error(index, &recovery_error);
                    if recovery_error.kind == LiveGpuErrorKind::DeviceLost {
                        self.handle_gpu_device_loss(index, &recovery_error);
                    }
                    return self.gpu_frame_fallback(index, prepared, recovery_error);
                }
                match self
                    .gpu
                    .as_mut()
                    .expect("recovered GPU presenter")
                    .render(&prepared)
                {
                    Ok(pending) => pending,
                    Err(retry_error) => {
                        self.record_gpu_render_error(index, &retry_error);
                        if retry_error.kind == LiveGpuErrorKind::DeviceLost {
                            self.handle_gpu_device_loss(index, &retry_error);
                        }
                        return self.gpu_frame_fallback(index, prepared, retry_error);
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind,
                    LiveGpuErrorKind::SurfaceTimeout | LiveGpuErrorKind::SurfaceOccluded
                ) && self.surfaces[index].mapped
                    && self.surfaces[index].gpu_consecutive_timeouts < 2 =>
            {
                self.record_gpu_render_error(index, &error);
                self.surfaces[index].gpu_consecutive_timeouts = self.surfaces[index]
                    .gpu_consecutive_timeouts
                    .saturating_add(1);
                self.surfaces[index]
                    .runtime
                    .as_mut()
                    .expect("runtime initialized before presentation")
                    .reject_gpu_frame(prepared, true);
                // Keep the prior GPU buffer attached and ask the compositor
                // for one later opportunity. This is not a presentation and
                // retains the dirty visual state without a retry loop.
                wayland_surface.frame(qh, CallbackData::Frame { owner, generation });
                wayland_surface.commit();
                self.surfaces[index].scheduler.frame_committed();
                self.surfaces[index].scheduler.mark_dirty();
                self.surfaces[index].summary.gpu.frame_callbacks_requested = self.surfaces[index]
                    .summary
                    .gpu
                    .frame_callbacks_requested
                    .saturating_add(1);
                self.sync_gpu_summary(index);
                return Ok(GpuFrameAttempt::NoFrame);
            }
            Err(error) => {
                self.record_gpu_render_error(index, &error);
                if error.kind == LiveGpuErrorKind::DeviceLost {
                    self.handle_gpu_device_loss(index, &error);
                }
                return self.gpu_frame_fallback(index, prepared, error);
            }
        };
        let partial = pending.partial();
        let logical_damage_rectangles = pending.logical_damage_rectangles();
        let physical_damage = pending.physical_damage_rects();
        let physical_damaged_pixels = pending.physical_damaged_pixels();
        let selected_tiles = pending.selected_tiles();
        let rasterized_pixels = pending.rasterized_pixels();
        let backing_updated_pixels = pending.backing_updated_pixels();
        let surface_converted_pixels = pending.surface_converted_pixels();
        {
            let summary = &mut self.surfaces[index].summary.gpu;
            summary.frames_rendered = summary.frames_rendered.saturating_add(1);
            summary.frames_submitted = summary.frames_submitted.saturating_add(1);
            summary.surface_acquisitions = summary.surface_acquisitions.saturating_add(1);
            summary.conversion_passes = summary.conversion_passes.saturating_add(1);
            summary.physical_damage_rectangles = summary
                .physical_damage_rectangles
                .saturating_add(u64::try_from(physical_damage.len()).unwrap_or(u64::MAX));
            summary.logical_damage_rectangles = summary
                .logical_damage_rectangles
                .saturating_add(u64::try_from(logical_damage_rectangles).unwrap_or(u64::MAX));
            summary.physical_damaged_pixels = summary
                .physical_damaged_pixels
                .saturating_add(physical_damaged_pixels);
            summary.selected_tiles = summary
                .selected_tiles
                .saturating_add(u64::try_from(selected_tiles).unwrap_or(u64::MAX));
            summary.vello_rasterized_pixels = summary
                .vello_rasterized_pixels
                .saturating_add(rasterized_pixels);
            summary.backing_updated_pixels = summary
                .backing_updated_pixels
                .saturating_add(backing_updated_pixels);
            summary.surface_converted_pixels = summary
                .surface_converted_pixels
                .saturating_add(surface_converted_pixels);
            if partial {
                summary.partial_renders = summary.partial_renders.saturating_add(1);
            } else {
                summary.full_target_renders = summary.full_target_renders.saturating_add(1);
            }
        }
        update_input_region(
            compositor,
            wayland_surface,
            prepared.logical_width,
            prepared.logical_height,
            &prepared.input_regions,
            qh,
        );
        if self.surfaces[index].scale_state.profile() == PresentationProfile::FractionalViewport {
            let viewport = self.surfaces[index].viewport.as_ref().ok_or_else(|| {
                ShellHostError::Wayland("fractional profile has no viewport object".into())
            })?;
            viewport.set_destination(logical_width as i32, logical_height as i32);
        }
        // wgpu's Wayland present path attaches its wl_buffer and commits this
        // wl_surface. All double-buffered state and the one frame callback must
        // therefore be requested before SurfaceTexture::present().
        let damage_mode = queue_gpu_wayland_damage(
            wayland_surface,
            &physical_damage,
            logical_width,
            logical_height,
        );
        let authoritative_full_damage = physical_damage.len() == 1
            && physical_damage[0][0] == 0
            && physical_damage[0][1] == 0
            && physical_damage[0][2] == request.buffer_width
            && physical_damage[0][3] == request.buffer_height;
        let (wayland_damage_rectangles, wayland_full_damage) = match damage_mode {
            GpuWaylandDamageMode::Buffer => (physical_damage.len(), authoritative_full_damage),
            GpuWaylandDamageMode::SurfaceFull => (1, true),
        };
        let wayland_damaged_pixels = if wayland_full_damage {
            u64::from(request.buffer_width) * u64::from(request.buffer_height)
        } else {
            physical_damaged_pixels
        };
        self.gpu
            .as_mut()
            .expect("configured GPU presenter")
            .record_wayland_damage(
                wayland_damage_rectangles,
                wayland_damaged_pixels,
                wayland_full_damage,
            );
        {
            let summary = &mut self.surfaces[index].summary.gpu;
            match damage_mode {
                GpuWaylandDamageMode::Buffer => {
                    summary.wayland_damage_rectangles = summary
                        .wayland_damage_rectangles
                        .saturating_add(u64::try_from(physical_damage.len()).unwrap_or(u64::MAX));
                    summary.wayland_damaged_pixels = summary
                        .wayland_damaged_pixels
                        .saturating_add(wayland_damaged_pixels);
                    if authoritative_full_damage {
                        summary.full_wayland_damage_frames =
                            summary.full_wayland_damage_frames.saturating_add(1);
                    } else {
                        summary.narrow_wayland_damage_frames =
                            summary.narrow_wayland_damage_frames.saturating_add(1);
                    }
                }
                GpuWaylandDamageMode::SurfaceFull => {
                    summary.wayland_damage_rectangles =
                        summary.wayland_damage_rectangles.saturating_add(1);
                    summary.full_wayland_damage_frames =
                        summary.full_wayland_damage_frames.saturating_add(1);
                    summary.wayland_damaged_pixels = summary
                        .wayland_damaged_pixels
                        .saturating_add(wayland_damaged_pixels);
                }
            }
        }
        wayland_surface.frame(qh, CallbackData::Frame { owner, generation });
        self.surfaces[index].summary.gpu.frame_callbacks_requested = self.surfaces[index]
            .summary
            .gpu
            .frame_callbacks_requested
            .saturating_add(1);
        let rendered_micros = pending.rendered_micros();
        let suboptimal = pending.suboptimal();
        self.gpu
            .as_mut()
            .expect("configured GPU presenter")
            .present(pending)
            .map_err(|error| {
                ShellHostError::Wayland(format!("live GPU present failed: {error}"))
            })?;
        self.surfaces[index].summary.gpu.frames_presented = self.surfaces[index]
            .summary
            .gpu
            .frames_presented
            .saturating_add(1);
        let mut reconfigured_after_present = false;
        if suboptimal {
            let id = self.gpu_surface_id(index);
            let reconfigured = self
                .gpu
                .as_mut()
                .expect("configured GPU presenter")
                .reconfigure(id)
                .and_then(|()| {
                    self.gpu
                        .as_ref()
                        .and_then(|gpu| gpu.configuration(id))
                        .cloned()
                        .ok_or_else(|| {
                            LiveGpuError::host(
                                LiveGpuErrorKind::InvalidConfiguration,
                                "suboptimal surface reconfiguration was not retained",
                                true,
                            )
                        })
                });
            match reconfigured {
                Ok(configuration) => {
                    self.record_gpu_configuration(index, &configuration);
                    self.surfaces[index].scheduler.mark_dirty();
                    reconfigured_after_present = true;
                }
                Err(error) => {
                    self.record_gpu_render_error(index, &error);
                    self.fall_back_gpu_surface(index, &error);
                    self.surfaces[index].scheduler.mark_dirty();
                }
            }
        }
        let runtime = self.surfaces[index]
            .runtime
            .as_mut()
            .expect("runtime initialized before presentation");
        let runtime_render_ms = runtime.accept_gpu_frame(prepared);
        if reconfigured_after_present {
            runtime.request_gpu_full_repaint();
        }
        self.surfaces[index].presenter.gpu_presented();
        self.surfaces[index].gpu_consecutive_timeouts = 0;
        self.sync_gpu_summary(index);
        Ok(GpuFrameAttempt::Presented(PresentedFrame {
            buffer_width: request.buffer_width,
            buffer_height: request.buffer_height,
            render_us: rendered_micros.max(milliseconds_to_microseconds(runtime_render_ms)),
            conversion_us: 0,
        }))
    }

    fn maybe_render(&mut self, owner: u64, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return Ok(());
        };
        let was_mapped = self.surfaces[index].mapped;
        let manifest_shared = self
            .output_instance_index_by_owner(owner)
            .map(|group| self.output_instances[group].shared.clone());
        let shm = self
            .shm
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_shm"))?
            .clone();
        let compositor = self
            .compositor
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_compositor"))?
            .clone();
        let started = self.started;
        let current_total_mapped = self
            .surfaces
            .iter()
            .try_fold(0usize, |total, surface| {
                total.checked_add(surface.pool.stats().total_mapped_bytes)
            })
            .ok_or_else(|| ShellHostError::Buffer("combined mapped bytes overflow".into()))?;
        let surface_state = &mut self.surfaces[index];
        if surface_state.presentation_failed || !surface_state.desired_mapped {
            #[cfg(feature = "gpu-renderer")]
            if self.gpu_requested && !surface_state.desired_mapped {
                surface_state.summary.gpu.closed_surface_suppressions = surface_state
                    .summary
                    .gpu
                    .closed_surface_suppressions
                    .saturating_add(1);
            }
            return Ok(());
        }
        let wayland_surface = surface_state
            .surface
            .as_ref()
            .ok_or_else(|| ShellHostError::Wayland("surface role is not mapped".into()))?
            .clone();
        let Some((logical_width, logical_height)) = surface_state.configures.latest() else {
            return Ok(());
        };
        if !surface_state.lifecycle.can_attach_buffer() {
            return Ok(());
        }
        if surface_state.runtime.is_none() {
            let mut runtime = match &surface_state.document {
                Some(document) => {
                    if let Some(snapshot) = &surface_state.package_snapshot {
                        let template = snapshot
                            .root_manifest()
                            .and_then(|manifest| {
                                manifest
                                    .surfaces
                                    .iter()
                                    .find(|template| template.id() == surface_state.template_id)
                            })
                            .ok_or_else(|| {
                                ShellHostError::Manifest(format!(
                                    "surface template `{}` is absent from package snapshot {}",
                                    surface_state.template_id,
                                    snapshot.generation().get()
                                ))
                            })?;
                        LiveDocument::load_surface_snapshot(
                            Arc::clone(snapshot),
                            template,
                            surface_state.kind.document_kind(),
                            logical_width,
                            logical_height,
                        )?
                    } else {
                        LiveDocument::load_surface_document(
                            &surface_state.package,
                            document,
                            surface_state.kind.document_kind(),
                            logical_width,
                            logical_height,
                        )?
                    }
                }
                None => LiveDocument::load_surface(
                    &surface_state.package,
                    surface_state.kind.document_kind(),
                    logical_width,
                    logical_height,
                )?,
            };
            if let Some((template_id, output_label)) = &surface_state.instance_context {
                runtime.set_instance_context(template_id, output_label)?;
            }
            if let Some(shared) = &manifest_shared {
                match surface_state.kind {
                    SurfaceKind::Panel => {
                        runtime.update_panel_state(shared.overlay_open, &shared.last_action)?;
                    }
                    SurfaceKind::Overlay => {
                        runtime.update_overlay_state(
                            shared.overlay_activation_count,
                            &shared.last_action,
                        )?;
                    }
                    SurfaceKind::SingleOverlay => {}
                }
                apply_surface_bindings(
                    &mut runtime,
                    &surface_state.template_id,
                    surface_state
                        .instance_context
                        .as_ref()
                        .map(|(_, output_label)| output_label.as_str())
                        .unwrap_or("output"),
                    surface_state.scale_state.effective_numerator(),
                    surface_state.scale_state.profile(),
                    shared.overlay_open,
                    shared.overlay_activation_count,
                    &shared.last_action,
                )?;
            }
            surface_state.runtime = Some(runtime);
            if surface_state.desired_mapped {
                surface_state.scheduler.mark_dirty();
            } else {
                surface_state.scheduler.stop_scheduling();
            }
        }
        if !surface_state.desired_mapped || !surface_state.scheduler.dirty() {
            return Ok(());
        }
        if surface_state.scheduler.frame_callback_outstanding() {
            return Ok(());
        }
        {
            let runtime = surface_state.runtime.as_mut().expect("initialized above");
            if runtime.set_viewport(logical_width, logical_height)? {
                surface_state.scheduler.mark_dirty();
            }
        }
        surface_state
            .scale_state
            .set_logical_size(logical_width, logical_height);
        let render_request = surface_state
            .scale_state
            .render_request()?
            .ok_or_else(|| ShellHostError::Wayland("presentation size is unavailable".into()))?;
        if let Some(shared) = &manifest_shared {
            surface_state
                .runtime
                .as_mut()
                .expect("initialized above")
                .apply_bound_state(
                    &built_in_binding_values(
                        &surface_state.template_id,
                        surface_state
                            .instance_context
                            .as_ref()
                            .map(|(_, output_label)| output_label.as_str())
                            .unwrap_or("output"),
                        surface_state.scale_state.effective_numerator(),
                        shared.overlay_open,
                        shared.overlay_activation_count,
                        &shared.last_action,
                    ),
                    &built_in_token_values(
                        surface_state.scale_state.profile(),
                        shared.overlay_open,
                    ),
                )?;
        }
        let buffer_width = render_request.buffer_width;
        let buffer_height = render_request.buffer_height;
        let mut presented = None;
        #[cfg(feature = "gpu-renderer")]
        let mut cpu_fallback_frame = None;
        #[cfg(feature = "gpu-renderer")]
        {
            let use_gpu = match self.ensure_gpu_surface(index, buffer_width, buffer_height) {
                Ok(use_gpu) => use_gpu,
                Err(error) => {
                    self.fall_back_gpu_surface(index, &error);
                    false
                }
            };
            if use_gpu {
                match self.surfaces[index].scheduler.decision(true, true) {
                    ScheduleDecision::Idle
                    | ScheduleDecision::WaitForFrameCallback
                    | ScheduleDecision::WaitForBuffer => return Ok(()),
                    ScheduleDecision::Render => {}
                }
                match self.try_present_gpu_frame(
                    index,
                    render_request,
                    logical_width,
                    logical_height,
                    &compositor,
                    &wayland_surface,
                    qh,
                )? {
                    GpuFrameAttempt::Presented(frame) => presented = Some(frame),
                    GpuFrameAttempt::CpuFallback(frame) => cpu_fallback_frame = Some(frame),
                    GpuFrameAttempt::NoFrame => return Ok(()),
                }
            }
        }
        if presented.is_none() {
            let surface_state = &mut self.surfaces[index];
            #[cfg(feature = "gpu-renderer")]
            if surface_state.presenter.state() == PresenterState::FallingBack {
                surface_state.presenter.cpu_ready();
                surface_state.summary.gpu.presenter_state = "cpu".into();
            }
            if surface_state
                .pool
                .requires_resize(buffer_width, buffer_height)?
            {
                let current_surface = surface_state.pool.stats().total_mapped_bytes;
                let proposed = surface_state
                    .pool
                    .projected_total_mapped_bytes(buffer_width, buffer_height)?;
                if current_total_mapped
                    .checked_sub(current_surface)
                    .and_then(|total| total.checked_add(proposed))
                    .is_none_or(|total| total > MAX_SESSION_MAPPED_BYTES)
                {
                    return Err(ShellHostError::Buffer(format!(
                        "surface instance {owner} would exceed the {MAX_SESSION_MAPPED_BYTES}-byte aggregate SHM limit"
                    )));
                }
            }
            let size_ready =
                surface_state
                    .pool
                    .ensure_size(&shm, qh, buffer_width, buffer_height)?;
            let free_buffer = size_ready && surface_state.pool.has_free();
            match surface_state.scheduler.decision(true, free_buffer) {
                ScheduleDecision::Idle
                | ScheduleDecision::WaitForFrameCallback
                | ScheduleDecision::WaitForBuffer => return Ok(()),
                ScheduleDecision::Render => {}
            }
            #[cfg(feature = "gpu-renderer")]
            let frame = if let Some(frame) = cpu_fallback_frame {
                frame
            } else {
                let Some(frame) = surface_state
                    .runtime
                    .as_mut()
                    .expect("initialized above")
                    .render_pending_for(render_request, owner, surface_state.role_generation)?
                else {
                    surface_state.scheduler.mark_clean();
                    return Ok(());
                };
                frame
            };
            #[cfg(not(feature = "gpu-renderer"))]
            let frame = {
                let Some(frame) = surface_state
                    .runtime
                    .as_mut()
                    .expect("initialized above")
                    .render_pending_for(render_request, owner, surface_state.role_generation)?
                else {
                    surface_state.scheduler.mark_clean();
                    return Ok(());
                };
                frame
            };
            let Some((_id, buffer, conversion_us)) = surface_state
                .pool
                .acquire_and_write(&frame.premultiplied_rgba)?
            else {
                surface_state.scheduler.mark_dirty();
                return Ok(());
            };
            update_input_region(
                &compositor,
                &wayland_surface,
                frame.logical_width,
                frame.logical_height,
                &frame.input_regions,
                qh,
            );
            if surface_state.scale_state.profile() == PresentationProfile::FractionalViewport {
                let viewport = surface_state.viewport.as_ref().ok_or_else(|| {
                    ShellHostError::Wayland("fractional profile has no viewport object".into())
                })?;
                viewport.set_destination(logical_width as i32, logical_height as i32);
            }
            wayland_surface.attach(Some(&buffer), 0, 0);
            // wl_surface.damage is expressed in surface-local logical coordinates.
            wayland_surface.damage(
                0,
                0,
                logical_width.min(i32::MAX as u32) as i32,
                logical_height.min(i32::MAX as u32) as i32,
            );
            wayland_surface.frame(
                qh,
                CallbackData::Frame {
                    owner,
                    generation: surface_state.role_generation,
                },
            );
            wayland_surface.commit();
            #[cfg(feature = "gpu-renderer")]
            {
                surface_state.summary.gpu.shm_frames =
                    surface_state.summary.gpu.shm_frames.saturating_add(1);
                surface_state.summary.gpu.frame_callbacks_requested = surface_state
                    .summary
                    .gpu
                    .frame_callbacks_requested
                    .saturating_add(1);
            }
            presented = Some(PresentedFrame {
                buffer_width: frame.buffer_width,
                buffer_height: frame.buffer_height,
                render_us: milliseconds_to_microseconds(frame.render_ms),
                conversion_us,
            });
        }
        let presented = presented.expect("a presenter completed the selected frame");
        let surface_state = &mut self.surfaces[index];
        surface_state.scheduler.frame_committed();
        surface_state.mapped = true;
        surface_state.map_state.mapped();
        surface_state.configures.mark_presented();
        surface_state.scale_state.mark_applied();
        surface_state.summary.frames_committed =
            surface_state.summary.frames_committed.saturating_add(1);
        surface_state.summary.logical_width = logical_width;
        surface_state.summary.logical_height = logical_height;
        surface_state.summary.buffer_width = presented.buffer_width;
        surface_state.summary.buffer_height = presented.buffer_height;
        surface_state.summary.preferred_scale_numerator =
            surface_state.scale_state.preferred_numerator();
        surface_state.summary.scale_denominator = htm_runtime::LIVE_SCALE_DENOMINATOR;
        surface_state.summary.fractional_viewport_active =
            surface_state.scale_state.profile() == PresentationProfile::FractionalViewport;
        surface_state.summary.last_render_us = presented.render_us;
        surface_state.summary.last_pixel_conversion_us = presented.conversion_us;
        if let Some(scale_started) = surface_state.pending_scale_started.take() {
            surface_state.summary.last_scale_change_to_commit_us = elapsed_us(scale_started);
            surface_state.scaled_commit_started = Some(scale_started);
        }
        if let Some(mutation_started) = surface_state.pending_binding_mutation_started.take() {
            surface_state.summary.last_state_mutation_to_commit_us = elapsed_us(mutation_started);
            surface_state.binding_commit_started = Some(mutation_started);
        }
        if surface_state.summary.first_commit_us == 0 {
            surface_state.summary.first_commit_us = elapsed_us(started);
        }
        if let Ok(snapshot) = surface_state
            .runtime
            .as_ref()
            .expect("initialized above")
            .snapshot()
        {
            surface_state.summary.html_parse_count = snapshot.document_parse_count;
            surface_state.last_click_count = snapshot.interaction.click_count;
        }
        let runtime_measurements = surface_state
            .runtime
            .as_ref()
            .expect("initialized above")
            .measurements();
        surface_state.summary.package_read_us =
            milliseconds_to_microseconds(runtime_measurements.package_read_ms);
        surface_state.summary.html_parse_us =
            milliseconds_to_microseconds(runtime_measurements.html_parse_ms);
        surface_state.summary.initial_resolve_us =
            milliseconds_to_microseconds(runtime_measurements.initial_resolve_ms);
        surface_state.summary.last_resolve_us =
            milliseconds_to_microseconds(runtime_measurements.last_resolve_ms);
        surface_state.summary.registry_initialization_us =
            milliseconds_to_microseconds(runtime_measurements.registry_initialization_ms);
        surface_state.summary.declaration_discovery_us =
            milliseconds_to_microseconds(runtime_measurements.declaration_discovery_ms);
        surface_state.summary.registered_element_count =
            runtime_measurements.registered_element_count;
        surface_state.summary.binding_count = runtime_measurements.binding_count;
        surface_state.summary.text_binding_count = runtime_measurements.text_binding_count;
        surface_state.summary.token_binding_count = runtime_measurements.token_binding_count;
        surface_state.summary.registered_action_count = runtime_measurements.action_count;
        surface_state.summary.clock_declaration_count =
            runtime_measurements.clock_declaration_count;
        surface_state.summary.repeat_declaration_count =
            runtime_measurements.repeat_declaration_count;
        surface_state.summary.registry_scan_count = runtime_measurements.registry_scan_count;
        surface_state.summary.suppressed_binding_updates =
            runtime_measurements.suppressed_binding_updates;
        surface_state.summary.changed_token_updates = runtime_measurements.changed_token_updates;
        surface_state.summary.suppressed_token_updates =
            runtime_measurements.suppressed_token_updates;
        surface_state.summary.repeat_insertions = runtime_measurements.repeat_insertions;
        surface_state.summary.repeat_removals = runtime_measurements.repeat_removals;
        surface_state.summary.repeat_moves = runtime_measurements.repeat_moves;
        surface_state.summary.repeat_property_updates =
            runtime_measurements.repeat_property_updates;
        surface_state.summary.repeat_unchanged_items = runtime_measurements.repeat_unchanged_items;
        surface_state.summary.repeat_subtree_clones = runtime_measurements.repeat_subtree_clones;
        surface_state.summary.repeat_identity_reuses = runtime_measurements.repeat_identity_reuses;
        surface_state.summary.repeated_item_count = runtime_measurements.repeated_item_count;
        surface_state.summary.cloned_node_count = runtime_measurements.cloned_node_count;
        surface_state.summary.contextual_item_count = runtime_measurements.contextual_item_count;
        surface_state.summary.channel_source_activations =
            runtime_measurements.channel_source_activations;
        surface_state.summary.channel_source_releases =
            runtime_measurements.channel_source_releases;
        surface_state.summary.channel_insertions = runtime_measurements.channel_insertions;
        surface_state.summary.channel_removals = runtime_measurements.channel_removals;
        surface_state.summary.channel_moves = runtime_measurements.channel_moves;
        surface_state.summary.channel_layout_replacements =
            runtime_measurements.channel_layout_replacements;
        surface_state.summary.channel_value_mutations =
            runtime_measurements.channel_value_mutations;
        surface_state.summary.contextual_subtree_clones =
            runtime_measurements.contextual_subtree_clones;
        surface_state.summary.retained_channel_identities =
            runtime_measurements.retained_channel_identities;
        surface_state.summary.duplicate_channel_suppressions =
            runtime_measurements.duplicate_channel_suppressions;
        surface_state.summary.link_insertions = runtime_measurements.link_insertions;
        surface_state.summary.link_removals = runtime_measurements.link_removals;
        surface_state.summary.link_state_mutations = runtime_measurements.link_state_mutations;
        surface_state.summary.link_relation_mutations =
            runtime_measurements.link_relation_mutations;
        surface_state.summary.link_moves = runtime_measurements.link_moves;
        surface_state.summary.group_insertions = runtime_measurements.group_insertions;
        surface_state.summary.group_removals = runtime_measurements.group_removals;
        surface_state.summary.group_member_insertions =
            runtime_measurements.group_member_insertions;
        surface_state.summary.group_member_removals = runtime_measurements.group_member_removals;
        surface_state.summary.representative_changes = runtime_measurements.representative_changes;
        surface_state.summary.group_state_mutations = runtime_measurements.group_state_mutations;
        surface_state.summary.node_tracker_insertions =
            runtime_measurements.node_tracker_insertions;
        surface_state.summary.node_tracker_removals = runtime_measurements.node_tracker_removals;
        surface_state.summary.peer_relation_mutations =
            runtime_measurements.peer_relation_mutations;
        surface_state.summary.retained_link_identities =
            runtime_measurements.retained_link_identities;
        surface_state.summary.retained_group_identities =
            runtime_measurements.retained_group_identities;
        surface_state.summary.retained_tracker_identities =
            runtime_measurements.retained_tracker_identities;
        surface_state.summary.duplicate_graph_suppressions =
            runtime_measurements.duplicate_graph_suppressions;
        surface_state.summary.last_reconciliation_us =
            milliseconds_to_microseconds(runtime_measurements.last_reconciliation_ms);
        surface_state.summary.last_state_projection_us =
            milliseconds_to_microseconds(runtime_measurements.last_state_projection_ms);
        surface_state.summary.last_attribute_mutation_us =
            milliseconds_to_microseconds(runtime_measurements.last_attribute_mutation_ms);
        surface_state.refresh_pool_summary();
        if !was_mapped {
            self.reconcile_pipewire_demand()?;
        }
        Ok(())
    }

    fn pointer_move(&mut self, owner: u64, x: f64, y: f64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return;
        };
        let surface = &mut self.surfaces[index];
        if !surface.desired_mapped {
            return;
        }
        let result = surface
            .runtime
            .as_mut()
            .map(|runtime| runtime.pointer_move(x, y));
        match result {
            Some(Ok(true)) => {
                surface.scheduler.mark_dirty();
                let action = surface.runtime.as_mut().and_then(LiveDocument::take_action);
                if let Some(action) = action
                    && let Err(error) = self.handle_action(owner, action)
                {
                    self.fail(format!("live range action rejected: {error}"));
                }
            }
            Some(Ok(false)) | None => {}
            Some(Err(error)) => {
                self.fail(format!("pointer motion rejected: {error}"));
            }
        }
    }

    fn pointer_button(&mut self, owner: u64, pressed: bool) {
        if matches!(
            &self.options,
            SessionOptions::Multi(options) if options.automatic_overlay_cycles > 0
        ) {
            return;
        }
        let release_started = (!pressed).then(Instant::now);
        let Some(index) = self.surface_index_by_owner(owner) else {
            return;
        };
        let result = self.surfaces[index]
            .runtime
            .as_mut()
            .map(|runtime| runtime.pointer_primary(pressed));
        match result {
            Some(Ok(true)) => {
                self.surfaces[index].scheduler.mark_dirty();
                let action = self.surfaces[index]
                    .runtime
                    .as_mut()
                    .and_then(LiveDocument::take_action);
                if let Some(action) = action {
                    self.surfaces[index].summary.action_count =
                        self.surfaces[index].summary.action_count.saturating_add(1);
                    if let Some(release_started) = release_started {
                        self.surfaces[index]
                            .summary
                            .last_pointer_release_to_action_dispatch_us =
                            elapsed_us(release_started);
                    }
                    let dispatch_started = Instant::now();
                    if let Err(error) = self.handle_action(owner, action) {
                        self.fail(format!("live action rejected: {error}"));
                    } else if let Some(index) = self.surface_index_by_owner(owner) {
                        self.surfaces[index]
                            .summary
                            .last_action_dispatch_to_state_mutation_us =
                            elapsed_us(dispatch_started);
                    }
                }
            }
            Some(Ok(false)) | None => {}
            Some(Err(error)) => {
                self.fail(format!("pointer button rejected: {error}"));
            }
        }
    }

    fn handle_action(&mut self, owner: u64, action: LiveAction) -> Result<(), ShellHostError> {
        if matches!(
            action,
            LiveAction::ClockEnable(_) | LiveAction::ClockDisable(_) | LiveAction::ClockToggle(_)
        ) {
            self.handle_clock_action(owner, action)?;
            if matches!(self.options, SessionOptions::Manifest(_)) {
                self.manifest_actions = self.manifest_actions.saturating_add(1);
            }
            return Ok(());
        }
        if matches!(
            action,
            LiveAction::PipeWireAudio(_)
                | LiveAction::PipeWireDefault(_)
                | LiveAction::PipeWirePeak(_)
        ) {
            self.handle_pipewire_action(owner, action)?;
            if matches!(self.options, SessionOptions::Manifest(_)) {
                self.manifest_actions = self.manifest_actions.saturating_add(1);
            }
            return Ok(());
        }
        if matches!(
            action,
            LiveAction::PowerProfileSetPowerSaver
                | LiveAction::PowerProfileSetBalanced
                | LiveAction::PowerProfileSetPerformance
        ) {
            self.handle_power_profile_action(owner, action)?;
            if matches!(self.options, SessionOptions::Manifest(_)) {
                self.manifest_actions = self.manifest_actions.saturating_add(1);
            }
            return Ok(());
        }
        if matches!(self.options, SessionOptions::Manifest(_)) {
            return self.handle_manifest_action(owner, action);
        }
        match action {
            LiveAction::SingleOverlayActivate => {
                if let SessionOptions::Single(options) = &self.options
                    && options.exit_after_click
                {
                    let callbacks = self
                        .surface_index(SurfaceKind::SingleOverlay)
                        .map(|index| self.surfaces[index].summary.frame_callbacks)
                        .unwrap_or_default();
                    self.single_exit_after_commit_count = Some(callbacks.saturating_add(1));
                }
            }
            LiveAction::ToggleOverlay => {
                if self.shared.overlay_open {
                    self.close_overlay("Closed from panel")?;
                } else {
                    self.open_overlay("Opened from panel")?;
                }
            }
            LiveAction::CloseOverlay => self.close_overlay("Closed from overlay")?,
            LiveAction::ActivateOverlay => {
                self.shared.overlay_activation_count =
                    self.shared.overlay_activation_count.saturating_add(1);
                self.shared.last_action = "Overlay state updated".into();
                if let Some(index) = self.surface_index(SurfaceKind::Overlay) {
                    self.surfaces[index]
                        .runtime
                        .as_mut()
                        .ok_or_else(|| ShellHostError::Wayland("overlay runtime missing".into()))?
                        .update_overlay_state(
                            self.shared.overlay_activation_count,
                            &self.shared.last_action,
                        )?;
                    self.surfaces[index].scheduler.mark_dirty();
                }
            }
            LiveAction::ClockEnable(_)
            | LiveAction::ClockDisable(_)
            | LiveAction::ClockToggle(_)
            | LiveAction::PowerProfileSetPowerSaver
            | LiveAction::PowerProfileSetBalanced
            | LiveAction::PowerProfileSetPerformance
            | LiveAction::PipeWireAudio(_)
            | LiveAction::PipeWireDefault(_)
            | LiveAction::PipeWirePeak(_) => unreachable!("handled above"),
        }
        Ok(())
    }

    fn handle_power_profile_action(
        &mut self,
        owner: u64,
        action: LiveAction,
    ) -> Result<(), ShellHostError> {
        let index = self
            .surface_index_by_owner(owner)
            .ok_or_else(|| ShellHostError::Wayland(format!("surface instance {owner} is stale")))?;
        if !self.surfaces[index].desired_mapped {
            return Err(ShellHostError::Wayland(
                "unmapped surface cannot dispatch a power-profile action".into(),
            ));
        }
        let profile = match action {
            LiveAction::PowerProfileSetPowerSaver => PowerProfile::PowerSaver,
            LiveAction::PowerProfileSetBalanced => PowerProfile::Balanced,
            LiveAction::PowerProfileSetPerformance => PowerProfile::Performance,
            _ => {
                return Err(ShellHostError::Wayland(
                    "non-profile action entered power-profile dispatch".into(),
                ));
            }
        };
        self.battery
            .request_profile(profile)
            .map_err(ShellHostError::Wayland)?;
        Ok(())
    }

    fn handle_pipewire_action(
        &mut self,
        owner: u64,
        action: LiveAction,
    ) -> Result<(), ShellHostError> {
        let index = self
            .surface_index_by_owner(owner)
            .ok_or_else(|| ShellHostError::Wayland(format!("surface instance {owner} is stale")))?;
        if !self.surfaces[index].desired_mapped {
            return Err(ShellHostError::Wayland(
                "unmapped surface cannot dispatch a PipeWire audio action".into(),
            ));
        }
        if let LiveAction::PipeWirePeak(request) = &action {
            let changed = self.surfaces[index]
                .runtime
                .as_mut()
                .ok_or_else(|| ShellHostError::Wayland("PipeWire runtime is missing".into()))?
                .apply_pipewire_peak_action(request)?;
            if changed {
                self.surfaces[index].scheduler.mark_dirty();
            }
            self.reconcile_pipewire_demand()?;
            self.fanout_current_pipewire_snapshot();
            return Ok(());
        }
        let (control, result) = match action {
            LiveAction::PipeWireAudio(request) => {
                let control = request.control.clone();
                (control, self.pipewire.request_control(request))
            }
            LiveAction::PipeWireDefault(request) => {
                let control = request.control.clone();
                (control, self.pipewire.request_default_control(request))
            }
            _ => {
                return Err(ShellHostError::Wayland(
                    "non-PipeWire action entered PipeWire dispatch".into(),
                ));
            }
        };
        if let Err(error) = result {
            let state = if error.contains("unavailable")
                || error.contains("unresolved")
                || error.contains("stale")
                || error.contains("no longer present")
            {
                htm_runtime::PipeWireControlState::Unavailable
            } else {
                htm_runtime::PipeWireControlState::Failed
            };
            if let Some(runtime) = self.surfaces[index].runtime.as_mut() {
                let _ = runtime.apply_pipewire_control_state(&control, state);
                self.surfaces[index].scheduler.mark_dirty();
            }
            self.fanout_current_pipewire_snapshot();
            eprintln!("htmshell-live: PipeWire request was contained: {error}");
            return Ok(());
        }
        self.fanout_pipewire_control_outcomes();
        Ok(())
    }

    fn fanout_pipewire_control_outcomes(&mut self) {
        let mut restore_authoritative_values = false;
        for outcome in self.pipewire.take_control_outcomes() {
            restore_authoritative_values |= matches!(
                outcome.state,
                htm_runtime::PipeWireControlState::Failed
                    | htm_runtime::PipeWireControlState::Unavailable
            );
            for surface in &mut self.surfaces {
                let Some(runtime) = surface.runtime.as_mut() else {
                    continue;
                };
                match runtime.apply_pipewire_control_state(&outcome.control, outcome.state) {
                    Ok(true) if surface.desired_mapped => surface.scheduler.mark_dirty(),
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        }
        if restore_authoritative_values {
            self.fanout_current_pipewire_snapshot();
        }
    }

    fn handle_clock_action(
        &mut self,
        owner: u64,
        action: LiveAction,
    ) -> Result<(), ShellHostError> {
        let (target, requested) = match action {
            LiveAction::ClockEnable(target) => (target, Some(true)),
            LiveAction::ClockDisable(target) => (target, Some(false)),
            LiveAction::ClockToggle(target) => (target, None),
            _ => {
                return Err(ShellHostError::Wayland(
                    "non-clock action entered clock dispatch".into(),
                ));
            }
        };
        let index = self
            .surface_index_by_owner(owner)
            .ok_or_else(|| ShellHostError::Wayland(format!("surface instance {owner} is stale")))?;
        if !self.surfaces[index].desired_mapped {
            return Err(ShellHostError::Wayland(
                "unmapped surface cannot dispatch a clock action".into(),
            ));
        }
        let runtime = self.surfaces[index]
            .runtime
            .as_mut()
            .ok_or_else(|| ShellHostError::Wayland("clock action runtime is missing".into()))?;
        let current = runtime.clock_enabled(&target)?;
        let enabled = requested.unwrap_or(!current);
        if !runtime.set_clock_enabled(&target, enabled)? {
            return Ok(());
        }
        if let Err(error) = self.reconcile_clock_subscribers() {
            if let Some(runtime) = self.surfaces[index].runtime.as_mut() {
                let _ = runtime.set_clock_enabled(&target, current);
            }
            return Err(error);
        }
        Ok(())
    }

    fn output_instance_index_by_owner(&self, owner: u64) -> Option<usize> {
        self.output_instances
            .iter()
            .position(|instance| instance.panel_owner == owner || instance.overlay_owner == owner)
    }

    fn handle_manifest_action(
        &mut self,
        owner: u64,
        action: LiveAction,
    ) -> Result<(), ShellHostError> {
        let group_index = self
            .output_instance_index_by_owner(owner)
            .ok_or_else(|| ShellHostError::Wayland(format!("surface instance {owner} is stale")))?;
        let instance = &self.output_instances[group_index];
        validate_manifest_action_source(
            &action,
            owner,
            instance.panel_owner,
            instance.overlay_owner,
            instance.shared.overlay_open,
        )
        .map_err(|message| ShellHostError::Wayland(message.into()))?;
        match action {
            LiveAction::ToggleOverlay => {
                if self.output_instances[group_index].shared.overlay_open {
                    self.close_manifest_overlay(group_index, "Closed from panel")?;
                } else {
                    self.open_manifest_overlay(group_index, "Opened from panel")?;
                }
            }
            LiveAction::CloseOverlay => {
                self.close_manifest_overlay(group_index, "Closed from overlay")?;
            }
            LiveAction::ActivateOverlay => {
                let (count, action, overlay_owner, panel_owner) = {
                    let instance = &mut self.output_instances[group_index];
                    instance.shared.overlay_activation_count =
                        instance.shared.overlay_activation_count.saturating_add(1);
                    instance.shared.last_action = "Overlay state updated".into();
                    (
                        instance.shared.overlay_activation_count,
                        instance.shared.last_action.clone(),
                        instance.overlay_owner,
                        instance.panel_owner,
                    )
                };
                if let Some(index) = self.surface_index_by_owner(overlay_owner) {
                    let legacy_changed = self.surfaces[index]
                        .runtime
                        .as_mut()
                        .ok_or_else(|| ShellHostError::Wayland("overlay runtime missing".into()))?
                        .update_overlay_state(count, &action)?;
                    if legacy_changed {
                        self.surfaces[index].scheduler.mark_dirty();
                    }
                }
                self.refresh_manifest_surface_bindings(overlay_owner)?;
                self.refresh_manifest_surface_bindings(panel_owner)?;
            }
            LiveAction::SingleOverlayActivate => {
                return Err(ShellHostError::Wayland(
                    "single-overlay action is invalid in manifest mode".into(),
                ));
            }
            LiveAction::ClockEnable(_)
            | LiveAction::ClockDisable(_)
            | LiveAction::ClockToggle(_)
            | LiveAction::PowerProfileSetPowerSaver
            | LiveAction::PowerProfileSetBalanced
            | LiveAction::PowerProfileSetPerformance
            | LiveAction::PipeWireAudio(_)
            | LiveAction::PipeWireDefault(_)
            | LiveAction::PipeWirePeak(_) => {
                unreachable!("handled before manifest dispatch")
            }
        }
        self.manifest_actions = self.manifest_actions.saturating_add(1);
        Ok(())
    }

    fn maybe_stop_after_manifest_actions(&mut self) {
        let target = match &self.options {
            SessionOptions::Manifest(options) => options.exit_after_actions,
            _ => None,
        };
        if target.is_some_and(|target| {
            self.manifest_actions >= target
                && self.surfaces.iter().all(|surface| {
                    !surface.scheduler.dirty() && !surface.scheduler.frame_callback_outstanding()
                })
        }) {
            self.running = false;
        }
    }

    fn maybe_stop_after_manifest_scale_changes(&mut self) {
        let target = match &self.options {
            SessionOptions::Manifest(options) => options.exit_after_scale_changes,
            _ => None,
        };
        let changes = self.surfaces.iter().fold(0_u64, |total, surface| {
            total.saturating_add(surface.summary.preferred_scale_changes)
        });
        if target.is_some_and(|target| {
            changes >= target
                && self.surfaces.iter().all(|surface| {
                    !surface.scheduler.dirty() && !surface.scheduler.frame_callback_outstanding()
                })
        }) {
            self.running = false;
        }
    }

    fn maybe_stop_after_clock_updates(&mut self) {
        let target = match &self.options {
            SessionOptions::Manifest(options) => options.exit_after_clock_updates,
            _ => None,
        };
        let changed_values = self.clock.summary().changed_values;
        if target.is_some_and(|target| {
            changed_values >= target
                && self.surfaces.iter().all(|surface| {
                    !surface.scheduler.dirty() && !surface.scheduler.frame_callback_outstanding()
                })
        }) {
            self.running = false;
        }
    }

    fn maybe_stop_after_battery_updates(&mut self) {
        let target = match &self.options {
            SessionOptions::Manifest(options) => options.exit_after_battery_updates,
            _ => None,
        };
        let changed_snapshots = self.battery.summary().changed_snapshots;
        if target.is_some_and(|target| {
            changed_snapshots >= target
                && self.surfaces.iter().all(|surface| {
                    !surface.scheduler.dirty() && !surface.scheduler.frame_callback_outstanding()
                })
        }) {
            self.running = false;
        }
    }

    fn open_manifest_overlay(
        &mut self,
        group_index: usize,
        action: &str,
    ) -> Result<(), ShellHostError> {
        if self.output_instances[group_index].shared.overlay_open {
            return Ok(());
        }
        let instance = &mut self.output_instances[group_index];
        instance.shared.overlay_open = true;
        instance.shared.last_action = action.into();
        let overlay_owner = instance.overlay_owner;
        let panel_owner = instance.panel_owner;
        let count = instance.shared.overlay_activation_count;
        let last_action = instance.shared.last_action.clone();
        let created = self.ensure_surface_role(overlay_owner)?;
        if let Some(index) = self.surface_index_by_owner(overlay_owner) {
            let configured = self.surfaces[index].configures.latest().is_some();
            self.surfaces[index].desired_mapped = true;
            self.surfaces[index].map_state.request_map(configured);
            if !configured && !created {
                self.surfaces[index]
                    .surface
                    .as_ref()
                    .ok_or_else(|| ShellHostError::Wayland("overlay role is missing".into()))?
                    .commit();
            }
            if let Some(runtime) = &mut self.surfaces[index].runtime {
                runtime.update_overlay_state(count, &last_action)?;
            }
            self.surfaces[index].scheduler.mark_dirty();
        }
        self.refresh_manifest_surface_bindings(overlay_owner)?;
        self.update_manifest_panel(panel_owner, true, &last_action)?;
        self.reconcile_pipewire_demand()
    }

    fn close_manifest_overlay(
        &mut self,
        group_index: usize,
        action: &str,
    ) -> Result<(), ShellHostError> {
        if !self.output_instances[group_index].shared.overlay_open {
            return Ok(());
        }
        let instance = &mut self.output_instances[group_index];
        instance.shared.overlay_open = false;
        instance.shared.last_action = action.into();
        let overlay_owner = instance.overlay_owner;
        let panel_owner = instance.panel_owner;
        let last_action = instance.shared.last_action.clone();
        if self.pointer_focus == Some(overlay_owner) {
            self.clear_pointer_focus();
        }
        if let Some(index) = self.surface_index_by_owner(overlay_owner) {
            #[cfg(feature = "gpu-renderer")]
            self.release_gpu_surface(index, true);
            let surface = &mut self.surfaces[index];
            if surface
                .runtime
                .as_mut()
                .is_some_and(LiveDocument::pointer_leave)
            {
                surface.scheduler.mark_dirty();
            }
            surface.desired_mapped = false;
            surface.scheduler.stop_scheduling();
            if surface.mapped {
                surface.map_state.begin_unmap();
                let wayland_surface = surface
                    .surface
                    .as_ref()
                    .ok_or_else(|| ShellHostError::Wayland("overlay role is missing".into()))?;
                wayland_surface.attach(None, 0, 0);
                wayland_surface.commit();
                surface
                    .lifecycle
                    .unmap()
                    .map_err(|error| ShellHostError::Wayland(error.into()))?;
                surface.configures.invalidate_for_unmap();
                surface.map_state.finish_unmap();
                surface.mapped = false;
            } else {
                surface.map_state.configured(false);
            }
            surface.pool.deactivate();
            surface.refresh_pool_summary();
        }
        self.destroy_transient_surface_role(overlay_owner);
        self.refresh_manifest_surface_bindings(overlay_owner)?;
        self.update_manifest_panel(panel_owner, false, &last_action)?;
        self.reconcile_pipewire_demand()
    }

    fn update_manifest_panel(
        &mut self,
        panel_owner: u64,
        overlay_open: bool,
        last_action: &str,
    ) -> Result<(), ShellHostError> {
        if let Some(index) = self.surface_index_by_owner(panel_owner) {
            let legacy_changed = self.surfaces[index]
                .runtime
                .as_mut()
                .map(|runtime| runtime.update_panel_state(overlay_open, last_action))
                .transpose()?
                .unwrap_or(false);
            if legacy_changed {
                self.surfaces[index].scheduler.mark_dirty();
            }
        }
        self.refresh_manifest_surface_bindings(panel_owner)?;
        Ok(())
    }

    fn refresh_manifest_surface_bindings(&mut self, owner: u64) -> Result<bool, ShellHostError> {
        let Some(group_index) = self.output_instance_index_by_owner(owner) else {
            return Ok(false);
        };
        let shared = self.output_instances[group_index].shared.clone();
        let Some(index) = self.surface_index_by_owner(owner) else {
            return Ok(false);
        };
        let surface = &mut self.surfaces[index];
        let Some(runtime) = surface.runtime.as_mut() else {
            return Ok(false);
        };
        let output_label = surface
            .instance_context
            .as_ref()
            .map(|(_, label)| label.as_str())
            .unwrap_or("output");
        let update = apply_surface_bindings(
            runtime,
            &surface.template_id,
            output_label,
            surface.scale_state.effective_numerator(),
            surface.scale_state.profile(),
            shared.overlay_open,
            shared.overlay_activation_count,
            &shared.last_action,
        )?;
        let changed = update.changed_elements > 0;
        if changed && surface.desired_mapped {
            surface
                .pending_binding_mutation_started
                .get_or_insert_with(Instant::now);
            surface.scheduler.mark_dirty();
        }
        Ok(changed)
    }

    fn open_overlay(&mut self, action: &str) -> Result<(), ShellHostError> {
        if self.shared.overlay_open {
            return Ok(());
        }
        self.shared.overlay_open = true;
        self.shared.last_action = action.into();
        self.overlay_open_count = self.overlay_open_count.saturating_add(1);
        self.overlay_open_started = Some(Instant::now());
        let created = self.ensure_surface_role(OVERLAY_OWNER)?;
        if let Some(index) = self.surface_index(SurfaceKind::Overlay) {
            let configured = self.surfaces[index].configures.latest().is_some();
            self.surfaces[index].desired_mapped = true;
            self.surfaces[index].map_state.request_map(configured);
            if !configured && !created {
                // A null-buffer unmap returns layer shell to its initial state.
                // A new bufferless commit requests the configure required for remap.
                self.surfaces[index]
                    .surface
                    .as_ref()
                    .ok_or_else(|| ShellHostError::Wayland("overlay role is missing".into()))?
                    .commit();
            }
            if let Some(runtime) = &mut self.surfaces[index].runtime {
                runtime.update_overlay_state(
                    self.shared.overlay_activation_count,
                    &self.shared.last_action,
                )?;
            }
            self.surfaces[index].scheduler.mark_dirty();
        }
        self.update_panel_document()?;
        self.reconcile_pipewire_demand()
    }

    fn close_overlay(&mut self, action: &str) -> Result<(), ShellHostError> {
        if !self.shared.overlay_open {
            return Ok(());
        }
        self.shared.overlay_open = false;
        self.shared.last_action = action.into();
        self.overlay_close_count = self.overlay_close_count.saturating_add(1);
        self.overlay_close_started = Some(Instant::now());
        if self.pointer_focus == Some(OVERLAY_OWNER) {
            self.clear_pointer_focus();
        }
        #[cfg(feature = "gpu-renderer")]
        let replace_gpu_surface_generation = self
            .surface_index(SurfaceKind::Overlay)
            .is_some_and(|index| self.surfaces[index].presenter.gpu_succeeded());
        if let Some(index) = self.surface_index(SurfaceKind::Overlay) {
            #[cfg(feature = "gpu-renderer")]
            self.release_gpu_surface(index, true);
            let surface = &mut self.surfaces[index];
            if surface
                .runtime
                .as_mut()
                .is_some_and(LiveDocument::pointer_leave)
            {
                surface.scheduler.mark_dirty();
            }
            surface.desired_mapped = false;
            surface.scheduler.stop_scheduling();
            if surface.mapped {
                surface.map_state.begin_unmap();
                let wayland_surface = surface
                    .surface
                    .as_ref()
                    .ok_or_else(|| ShellHostError::Wayland("overlay role is missing".into()))?;
                wayland_surface.attach(None, 0, 0);
                wayland_surface.commit();
                surface
                    .lifecycle
                    .unmap()
                    .map_err(|error| ShellHostError::Wayland(error.into()))?;
                surface.configures.invalidate_for_unmap();
                surface.map_state.finish_unmap();
                surface.mapped = false;
            } else {
                surface.map_state.configured(false);
            }
            surface.pool.deactivate();
            surface.refresh_pool_summary();
        }
        #[cfg(feature = "gpu-renderer")]
        if replace_gpu_surface_generation {
            // A closed retained overlay releases its swapchain completely.
            // Recreate the Wayland role on the next open so stale wgpu
            // surfaces and frame callbacks cannot cross presenter generations.
            self.destroy_transient_surface_role(OVERLAY_OWNER);
        }
        self.update_panel_document()?;
        self.reconcile_pipewire_demand()
    }

    fn update_panel_document(&mut self) -> Result<(), ShellHostError> {
        if let Some(index) = self.surface_index(SurfaceKind::Panel)
            && let Some(runtime) = &mut self.surfaces[index].runtime
        {
            runtime.update_panel_state(self.shared.overlay_open, &self.shared.last_action)?;
            self.surfaces[index].scheduler.mark_dirty();
        }
        Ok(())
    }

    fn pointer_leave_owner(&mut self, owner: u64) {
        if let Some(index) = self.surface_index_by_owner(owner) {
            let surface = &mut self.surfaces[index];
            if surface
                .runtime
                .as_mut()
                .is_some_and(LiveDocument::pointer_leave)
            {
                surface.scheduler.mark_dirty();
            }
        }
    }

    fn clear_pointer_focus(&mut self) {
        if let Some(owner) = self.pointer_focus.take() {
            self.pointer_leave_owner(owner);
        }
    }

    fn on_frame_done(&mut self, owner: u64, generation: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            self.stale_callbacks_contained = self.stale_callbacks_contained.saturating_add(1);
            return;
        };
        if self.surfaces[index].role_generation != generation {
            self.stale_callbacks_contained = self.stale_callbacks_contained.saturating_add(1);
            return;
        }
        let kind = self.surfaces[index].kind;
        self.surfaces[index].scheduler.frame_callback_done();
        self.surfaces[index].summary.frame_callbacks = self.surfaces[index]
            .summary
            .frame_callbacks
            .saturating_add(1);
        #[cfg(feature = "gpu-renderer")]
        {
            self.surfaces[index].summary.gpu.frame_callbacks_completed = self.surfaces[index]
                .summary
                .gpu
                .frame_callbacks_completed
                .saturating_add(1);
        }
        if let Some(scale_started) = self.surfaces[index].scaled_commit_started.take() {
            self.surfaces[index]
                .summary
                .last_scale_change_to_frame_callback_us = elapsed_us(scale_started);
        }
        if let Some(mutation_started) = self.surfaces[index].binding_commit_started.take() {
            self.surfaces[index]
                .summary
                .last_state_mutation_to_frame_callback_us = elapsed_us(mutation_started);
        }
        if matches!(self.options, SessionOptions::Manifest(_))
            && kind == SurfaceKind::Panel
            && let Some(group) = self.output_instance_index_by_owner(owner)
            && self.output_instances[group].first_panel_frame_us == 0
        {
            self.output_instances[group].first_panel_frame_us =
                elapsed_us(self.output_instances[group].created_at);
        }
        if self.surfaces[index].summary.first_frame_callback_us == 0 {
            self.surfaces[index].summary.first_frame_callback_us = elapsed_us(self.started);
        }
        if kind == SurfaceKind::Overlay
            && let Some(started) = self.overlay_open_started.take()
            && self.first_overlay_frame_latency_us == 0
        {
            self.first_overlay_frame_latency_us = elapsed_us(started);
        }
        if kind == SurfaceKind::SingleOverlay
            && let SessionOptions::Single(options) = &self.options
            && (options
                .exit_after_frames
                .is_some_and(|target| self.surfaces[index].summary.frame_callbacks >= target)
                || self
                    .single_exit_after_commit_count
                    .is_some_and(|target| self.surfaces[index].summary.frame_callbacks >= target))
        {
            self.running = false;
        }
        if kind == SurfaceKind::Panel
            && !self.startup_overlay_requested
            && matches!(
                &self.options,
                SessionOptions::Multi(options) if options.open_overlay_on_start
            )
        {
            self.startup_overlay_requested = true;
            if let Err(error) = self.open_overlay("Startup overlay open") {
                self.fail(format!("startup overlay open failed: {error}"));
            }
        } else if kind == SurfaceKind::Panel && self.auto_cycles_remaining > 0 && !self.auto_started
        {
            self.auto_started = true;
            self.auto_waiting_overlay_frame = true;
            if let Err(error) = self.open_overlay("Automatic lifecycle open") {
                self.fail(format!("automatic overlay open failed: {error}"));
            }
        } else if kind == SurfaceKind::Overlay && self.auto_waiting_overlay_frame {
            self.auto_waiting_overlay_frame = false;
            if let Err(error) = self.close_overlay("Automatic lifecycle close") {
                self.fail(format!("automatic overlay close failed: {error}"));
            } else {
                self.auto_cycles_remaining = self.auto_cycles_remaining.saturating_sub(1);
                self.auto_cycles_completed = self.auto_cycles_completed.saturating_add(1);
                self.auto_reopen_after_release = self.auto_cycles_remaining > 0;
                // A GPU-presented overlay has no SHM release event. Its target
                // is dropped synchronously during close, so the same bounded
                // release gate may reopen immediately when no pool buffer is
                // outstanding. CPU presentation continues to wait for the
                // compositor's wl_buffer.release event.
                self.maybe_reopen_automatic_overlay();
            }
        } else if kind == SurfaceKind::Panel
            && self.auto_started
            && self.auto_cycles_remaining == 0
            && !self.shared.overlay_open
            && !self.surfaces[index].scheduler.dirty()
            && matches!(
                &self.options,
                SessionOptions::Multi(options) if options.exit_after_automatic_cycles
            )
        {
            self.running = false;
        }
        if matches!(
            &self.options,
            SessionOptions::Manifest(ManifestHostOptions {
                exit_after_initial_frames: true,
                ..
            })
        ) && !self.output_instances.is_empty()
            && self
                .surfaces
                .iter()
                .filter(|surface| surface.desired_mapped)
                .all(|surface| surface.summary.frame_callbacks > 0)
        {
            self.running = false;
        }
        self.maybe_stop_after_output_events();
        self.maybe_stop_after_manifest_actions();
        self.maybe_stop_after_manifest_scale_changes();
        self.maybe_stop_after_clock_updates();
        self.maybe_stop_after_battery_updates();
    }

    fn on_buffer_release(&mut self, owner: u64, id: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            self.stale_releases_contained = self.stale_releases_contained.saturating_add(1);
            return;
        };
        let kind = self.surfaces[index].kind;
        self.surfaces[index].pool.release(id);
        self.surfaces[index].refresh_pool_summary();
        if kind == SurfaceKind::Overlay {
            self.maybe_reopen_automatic_overlay();
        }
        let _ = self.update_combined_memory();
    }

    fn maybe_reopen_automatic_overlay(&mut self) {
        let overlay_released = self
            .surface_index(SurfaceKind::Overlay)
            .is_some_and(|index| self.surfaces[index].pool.all_released());
        if self.auto_reopen_after_release && overlay_released {
            self.auto_reopen_after_release = false;
            self.auto_waiting_overlay_frame = true;
            if let Err(error) = self.open_overlay("Automatic lifecycle reopen") {
                self.fail(format!("automatic overlay reopen failed: {error}"));
            }
        }
    }

    fn update_combined_memory(&mut self) -> Result<(), ShellHostError> {
        let combined = self.surfaces.iter().try_fold(0usize, |total, surface| {
            total.checked_add(surface.pool.stats().total_mapped_bytes)
        });
        let combined = combined
            .ok_or_else(|| ShellHostError::Buffer("combined mapped bytes overflow".into()))?;
        if combined > MAX_SESSION_MAPPED_BYTES {
            return Err(ShellHostError::Buffer(format!(
                "surface pools require {combined} bytes; session limit is {MAX_SESSION_MAPPED_BYTES}"
            )));
        }
        self.combined_mapped_memory_peak = self.combined_mapped_memory_peak.max(combined);
        Ok(())
    }

    fn maybe_stop_after_peak_publications(&mut self) {
        let target = match &self.options {
            SessionOptions::Multi(options) => options.exit_after_peak_publications,
            SessionOptions::Single(_) | SessionOptions::Manifest(_) => None,
        };
        if target.is_some_and(|target| {
            self.pipewire.resource_counters().peak_vectors_published >= target
        }) {
            self.running = false;
        }
    }

    fn fail(&mut self, message: String) {
        if self.failure.is_none() {
            self.failure = Some(message);
        }
        self.running = false;
    }

    fn begin_shutdown(&mut self) {
        self.pipewire.shutdown();
        self.battery.shutdown();
        if let Err(error) = self.clock.shutdown() {
            self.fail(format!("clock shutdown failed: {error}"));
        }
        self.clear_pointer_focus();
        #[cfg(feature = "gpu-renderer")]
        for index in 0..self.surfaces.len() {
            self.release_gpu_surface(index, true);
        }
        for surface in &mut self.surfaces {
            if surface.mapped || surface.desired_mapped {
                if let Some(wayland_surface) = &surface.surface {
                    wayland_surface.attach(None, 0, 0);
                    wayland_surface.commit();
                }
                surface.mapped = false;
                surface.desired_mapped = false;
                surface.scheduler.stop_scheduling();
                surface.map_state.begin_unmap();
                surface.map_state.finish_unmap();
            }
        }
    }

    fn all_released(&self) -> bool {
        self.surfaces.iter().all(ShellSurfaceState::all_released)
    }

    fn destroy_objects(&mut self) {
        #[cfg(feature = "gpu-renderer")]
        {
            for index in 0..self.surfaces.len() {
                self.release_gpu_surface(index, true);
            }
            self.gpu.take();
        }
        for surface in &mut self.surfaces {
            surface.pool.destroy_all();
            Self::destroy_surface_protocol_objects(surface);
        }
        if let Some(pointer) = self.pointer.take() {
            release_pointer(pointer);
        }
        if let Some(seat) = self.seat.take() {
            release_seat(seat);
        }
        for output in self.outputs.drain(..) {
            release_output(output.proxy);
        }
        if let Some(shm) = self.shm.take() {
            release_shm(shm);
        }
        if let Some(layer_shell) = self.layer_shell.take()
            && layer_shell.version() >= 3
        {
            layer_shell.destroy();
        }
        if let Some(fractional_manager) = self.fractional_scale_manager.take() {
            fractional_manager.destroy();
        }
        if let Some(viewporter) = self.viewporter.take() {
            viewporter.destroy();
        }
    }

    fn single_summary(&self) -> LiveHostSummary {
        let surface = self
            .surface_index(SurfaceKind::SingleOverlay)
            .map(|index| &self.surfaces[index]);
        let summary = surface.map(|surface| &surface.summary);
        let runtime = surface.and_then(|surface| surface.runtime.as_ref());
        let runtime_measurements = runtime.map(LiveDocument::measurements).unwrap_or_default();
        LiveHostSummary {
            layer_shell_version: self.layer_shell_version,
            logical_width: summary
                .map(|summary| summary.logical_width)
                .unwrap_or_default(),
            logical_height: summary
                .map(|summary| summary.logical_height)
                .unwrap_or_default(),
            buffer_width: summary
                .map(|summary| summary.buffer_width)
                .unwrap_or_default(),
            buffer_height: summary
                .map(|summary| summary.buffer_height)
                .unwrap_or_default(),
            output_scale: self.legacy_output_scale(),
            viewporter_advertised: self.viewporter_advertised,
            fractional_scale_advertised: self.fractional_scale_advertised,
            preferred_scale_numerator: summary
                .map(|summary| summary.preferred_scale_numerator)
                .unwrap_or(htm_runtime::LIVE_SCALE_DENOMINATOR),
            scale_denominator: htm_runtime::LIVE_SCALE_DENOMINATOR,
            fractional_viewport_active: summary
                .is_some_and(|summary| summary.fractional_viewport_active),
            html_parse_count: summary
                .map(|summary| summary.html_parse_count)
                .unwrap_or_default(),
            frames_committed: summary
                .map(|summary| summary.frames_committed)
                .unwrap_or_default(),
            full_damage_commits: summary
                .map(|summary| summary.frames_committed)
                .unwrap_or_default(),
            partial_damage_commits: 0,
            frame_callbacks: summary
                .map(|summary| summary.frame_callbacks)
                .unwrap_or_default(),
            buffer_releases: summary
                .map(|summary| summary.buffer_releases)
                .unwrap_or_default(),
            pointer_enters: summary
                .map(|summary| summary.pointer_enters)
                .unwrap_or_default(),
            pointer_motions: summary
                .map(|summary| summary.pointer_motions)
                .unwrap_or_default(),
            pointer_buttons: summary
                .map(|summary| summary.pointer_buttons)
                .unwrap_or_default(),
            click_mutations: summary
                .map(|summary| summary.action_count)
                .unwrap_or_default(),
            buffers_allocated: summary
                .map(|summary| summary.buffer_allocations)
                .unwrap_or_default(),
            buffer_reallocations: summary
                .map(|summary| summary.buffer_reallocations)
                .unwrap_or_default(),
            frames_skipped_busy: summary
                .map(|summary| summary.busy_buffer_skips)
                .unwrap_or_default(),
            maximum_mapped_bytes: summary
                .map(|summary| summary.mapped_memory_peak)
                .unwrap_or_default(),
            wayland_connection_us: self.wayland_connection_us,
            first_configure_us: summary
                .map(|summary| summary.first_configure_us)
                .unwrap_or_default(),
            first_commit_us: summary
                .map(|summary| summary.first_commit_us)
                .unwrap_or_default(),
            first_frame_callback_us: summary
                .map(|summary| summary.first_frame_callback_us)
                .unwrap_or_default(),
            package_read_us: milliseconds_to_microseconds(runtime_measurements.package_read_ms),
            html_parse_us: milliseconds_to_microseconds(runtime_measurements.html_parse_ms),
            initial_resolve_us: milliseconds_to_microseconds(
                runtime_measurements.initial_resolve_ms,
            ),
            last_resolve_us: milliseconds_to_microseconds(runtime_measurements.last_resolve_ms),
            last_render_us: summary
                .map(|summary| summary.last_render_us)
                .unwrap_or_default(),
            last_pixel_conversion_us: summary
                .map(|summary| summary.last_pixel_conversion_us)
                .unwrap_or_default(),
            #[cfg(feature = "gpu-renderer")]
            gpu: summary
                .map(|summary| summary.gpu.clone())
                .unwrap_or_default(),
        }
    }

    fn multi_summary(&self) -> MultiSurfaceHostSummary {
        let panel = self
            .surface_index(SurfaceKind::Panel)
            .map(|index| self.surfaces[index].summary.clone())
            .unwrap_or_default();
        let overlay = self
            .surface_index(SurfaceKind::Overlay)
            .map(|index| self.surfaces[index].summary.clone())
            .unwrap_or_default();
        let resources = self.pipewire.resource_counters();
        MultiSurfaceHostSummary {
            layer_shell_version: self.layer_shell_version,
            output_scale: self.legacy_output_scale(),
            viewporter_advertised: self.viewporter_advertised,
            fractional_scale_advertised: self.fractional_scale_advertised,
            wayland_connection_us: self.wayland_connection_us,
            panel,
            overlay,
            overlay_open_count: self.overlay_open_count,
            overlay_close_count: self.overlay_close_count,
            overlay_activation_count: self.shared.overlay_activation_count,
            panel_click_to_overlay_frame_us: self.first_overlay_frame_latency_us,
            overlay_close_to_unmap_us: self.last_overlay_close_latency_us,
            combined_mapped_memory_peak: self.combined_mapped_memory_peak,
            automatic_cycles_completed: self.auto_cycles_completed,
            last_action: self.shared.last_action.clone(),
            pipewire_peaks: PipeWirePeakHostSummary {
                active_streams: resources.peak_stream_count,
                stream_starts: resources.peak_stream_starts,
                stream_stops: resources.peak_stream_stops,
                process_callbacks: resources.peak_process_callbacks,
                callbacks_coalesced: resources.peak_callbacks_coalesced,
                vectors_published: resources.peak_vectors_published,
                duplicate_vectors_suppressed: resources.peak_duplicate_vectors_suppressed,
            },
        }
    }

    fn legacy_output_scale(&self) -> i32 {
        self.selected_output
            .and_then(|key| self.output_catalog.get(key))
            .map(|record| record.scale)
            .unwrap_or(1)
    }

    fn manifest_summary(&self) -> ManifestHostSummary {
        let options = match &self.options {
            SessionOptions::Manifest(options) => options,
            _ => unreachable!("manifest summary requested for another session"),
        };
        let measurements = options.manifest.measurements();
        let active_outputs = self
            .output_instances
            .iter()
            .map(|instance| {
                let surface_summary = |owner: u64| {
                    self.surface_index_by_owner(owner).map(|index| {
                        let surface = &self.surfaces[index];
                        let mut metrics = surface.summary.clone();
                        if let Some(runtime) = surface.runtime.as_ref() {
                            if let Ok(snapshot) = runtime.snapshot() {
                                metrics.html_parse_count = snapshot.document_parse_count;
                            }
                            let measurements = runtime.measurements();
                            metrics.package_read_us =
                                milliseconds_to_microseconds(measurements.package_read_ms);
                            metrics.html_parse_us =
                                milliseconds_to_microseconds(measurements.html_parse_ms);
                            metrics.initial_resolve_us =
                                milliseconds_to_microseconds(measurements.initial_resolve_ms);
                            metrics.last_resolve_us =
                                milliseconds_to_microseconds(measurements.last_resolve_ms);
                            metrics.registry_initialization_us = milliseconds_to_microseconds(
                                measurements.registry_initialization_ms,
                            );
                            metrics.declaration_discovery_us =
                                milliseconds_to_microseconds(measurements.declaration_discovery_ms);
                            metrics.registered_element_count =
                                measurements.registered_element_count;
                            metrics.binding_count = measurements.binding_count;
                            metrics.text_binding_count = measurements.text_binding_count;
                            metrics.token_binding_count = measurements.token_binding_count;
                            metrics.registered_action_count = measurements.action_count;
                            metrics.clock_declaration_count = measurements.clock_declaration_count;
                            metrics.repeat_declaration_count =
                                measurements.repeat_declaration_count;
                            metrics.registry_scan_count = measurements.registry_scan_count;
                            metrics.suppressed_binding_updates =
                                measurements.suppressed_binding_updates;
                            metrics.changed_token_updates = measurements.changed_token_updates;
                            metrics.suppressed_token_updates =
                                measurements.suppressed_token_updates;
                            metrics.repeat_insertions = measurements.repeat_insertions;
                            metrics.repeat_removals = measurements.repeat_removals;
                            metrics.repeat_moves = measurements.repeat_moves;
                            metrics.repeat_property_updates = measurements.repeat_property_updates;
                            metrics.repeat_unchanged_items = measurements.repeat_unchanged_items;
                            metrics.repeat_subtree_clones = measurements.repeat_subtree_clones;
                            metrics.repeat_identity_reuses = measurements.repeat_identity_reuses;
                            metrics.repeated_item_count = measurements.repeated_item_count;
                            metrics.cloned_node_count = measurements.cloned_node_count;
                            metrics.contextual_item_count = measurements.contextual_item_count;
                            metrics.channel_source_activations =
                                measurements.channel_source_activations;
                            metrics.channel_source_releases = measurements.channel_source_releases;
                            metrics.channel_insertions = measurements.channel_insertions;
                            metrics.channel_removals = measurements.channel_removals;
                            metrics.channel_moves = measurements.channel_moves;
                            metrics.channel_layout_replacements =
                                measurements.channel_layout_replacements;
                            metrics.channel_value_mutations = measurements.channel_value_mutations;
                            metrics.contextual_subtree_clones =
                                measurements.contextual_subtree_clones;
                            metrics.retained_channel_identities =
                                measurements.retained_channel_identities;
                            metrics.duplicate_channel_suppressions =
                                measurements.duplicate_channel_suppressions;
                            metrics.link_insertions = measurements.link_insertions;
                            metrics.link_removals = measurements.link_removals;
                            metrics.link_state_mutations = measurements.link_state_mutations;
                            metrics.link_relation_mutations = measurements.link_relation_mutations;
                            metrics.link_moves = measurements.link_moves;
                            metrics.group_insertions = measurements.group_insertions;
                            metrics.group_removals = measurements.group_removals;
                            metrics.group_member_insertions = measurements.group_member_insertions;
                            metrics.group_member_removals = measurements.group_member_removals;
                            metrics.representative_changes = measurements.representative_changes;
                            metrics.group_state_mutations = measurements.group_state_mutations;
                            metrics.node_tracker_insertions = measurements.node_tracker_insertions;
                            metrics.node_tracker_removals = measurements.node_tracker_removals;
                            metrics.peer_relation_mutations = measurements.peer_relation_mutations;
                            metrics.retained_link_identities =
                                measurements.retained_link_identities;
                            metrics.retained_group_identities =
                                measurements.retained_group_identities;
                            metrics.retained_tracker_identities =
                                measurements.retained_tracker_identities;
                            metrics.duplicate_graph_suppressions =
                                measurements.duplicate_graph_suppressions;
                            metrics.last_reconciliation_us =
                                milliseconds_to_microseconds(measurements.last_reconciliation_ms);
                            metrics.last_state_projection_us =
                                milliseconds_to_microseconds(measurements.last_state_projection_ms);
                            metrics.last_attribute_mutation_us = milliseconds_to_microseconds(
                                measurements.last_attribute_mutation_ms,
                            );
                        }
                        ManifestSurfaceHostSummary {
                            template_id: surface.template_id.clone(),
                            owner: surface.owner,
                            instance_generation: surface.instance_generation,
                            output_key: Some(surface.output_key),
                            metrics,
                        }
                    })
                };
                ManifestOutputHostSummary {
                    output_key: Some(instance.key),
                    diagnostic_label: instance.diagnostic_label.clone(),
                    overlay_open: instance.shared.overlay_open,
                    overlay_activation_count: instance.shared.overlay_activation_count,
                    output_ready_us: instance.output_ready_us,
                    first_panel_frame_us: instance.first_panel_frame_us,
                    panel: surface_summary(instance.panel_owner),
                    overlay: surface_summary(instance.overlay_owner),
                }
            })
            .collect();
        ManifestHostSummary {
            manifest_id: options.manifest.manifest().id.clone(),
            manifest_parse_count: options.manifest.parse_count(),
            manifest_parse_us: measurements.parse_us,
            manifest_validation_us: measurements.validation_us,
            package_snapshot_generation: options.manifest.snapshot().generation().get(),
            package_count: options.manifest.snapshot().packages().len(),
            layer_shell_version: self.layer_shell_version,
            viewporter_advertised: self.viewporter_advertised,
            fractional_scale_advertised: self.fractional_scale_advertised,
            output_generations: self.output_catalog.generation_count(),
            output_additions: self.output_additions,
            output_removals: self.output_removals,
            unsupported_scale_outputs: self.unsupported_scale_outputs,
            active_outputs,
            peak_output_instances: self.peak_output_instances,
            peak_runtime_documents: self.peak_runtime_documents,
            combined_mapped_memory_peak: self.combined_mapped_memory_peak,
            aggregate_shm_limit: MAX_SESSION_MAPPED_BYTES,
            stale_callbacks_contained: self.stale_callbacks_contained,
            stale_releases_contained: self.stale_releases_contained,
            stale_scale_events_contained: self.stale_scale_events_contained,
            first_output_instance_us: self.first_output_instance_us,
            last_output_teardown_us: self.last_output_teardown_us,
            actions: self.manifest_actions,
            clock: self.clock.summary(),
            battery: self.battery.summary(),
        }
    }
}

pub fn run_live_overlay(options: LiveHostOptions) -> Result<LiveHostSummary, ShellHostError> {
    run_session(SessionOptions::Single(options)).map(|state| state.single_summary())
}

pub fn run_multi_surface_shell(
    options: MultiSurfaceHostOptions,
) -> Result<MultiSurfaceHostSummary, ShellHostError> {
    run_session(SessionOptions::Multi(options)).map(|state| state.multi_summary())
}

pub fn run_manifest_shell(
    options: ManifestHostOptions,
) -> Result<ManifestHostSummary, ShellHostError> {
    run_session(SessionOptions::Manifest(options)).map(|state| state.manifest_summary())
}

fn run_session(options: SessionOptions) -> Result<State, ShellHostError> {
    let started = Instant::now();
    let connection = Connection::connect_to_env().map_err(ShellHostError::wayland)?;
    let wayland_connection_us = elapsed_us(started);
    let mut event_queue = connection.new_event_queue();
    let qh = event_queue.handle();
    connection.display().get_registry(&qh, ());
    let mut state = State::new(options, started, wayland_connection_us);
    #[cfg(feature = "gpu-renderer")]
    {
        state.wayland_display = NonNull::new(
            connection
                .backend()
                .display_ptr()
                .cast::<std::ffi::c_void>(),
        );
    }
    state.queue_handle = Some(qh.clone());
    event_queue
        .roundtrip(&mut state)
        .map_err(ShellHostError::wayland)?;
    event_queue
        .roundtrip(&mut state)
        .map_err(ShellHostError::wayland)?;
    state.start(&qh)?;
    connection.flush().map_err(ShellHostError::wayland)?;

    while state.running {
        while event_queue
            .dispatch_pending(&mut state)
            .map_err(ShellHostError::wayland)?
            > 0
        {}
        state.maybe_render_all(&qh)?;
        state.reconcile_pipewire_demand()?;
        state.maybe_stop_after_peak_publications();
        connection.flush().map_err(ShellHostError::wayland)?;
        state.maybe_reopen_automatic_overlay();
        state.maybe_render_all(&qh)?;
        connection.flush().map_err(ShellHostError::wayland)?;
        if let Some(started) = state.overlay_close_started.take() {
            state.last_overlay_close_latency_us = elapsed_us(started);
            if matches!(
                &state.options,
                SessionOptions::Multi(options) if options.exit_after_overlay_close
            ) && state.overlay_close_count > 0
            {
                state.running = false;
            }
        }
        if state.running {
            wait_for_event_sources(&mut event_queue, &mut state)?;
        }
    }

    state.begin_shutdown();
    connection.flush().map_err(ShellHostError::wayland)?;
    for _ in 0..SHUTDOWN_ROUNDTRIPS {
        event_queue
            .roundtrip(&mut state)
            .map_err(ShellHostError::wayland)?;
        if state.all_released() {
            break;
        }
    }
    state.destroy_objects();
    connection.flush().map_err(ShellHostError::wayland)?;
    event_queue
        .roundtrip(&mut state)
        .map_err(ShellHostError::wayland)?;
    if let Some(message) = state.failure.take() {
        Err(ShellHostError::Wayland(message))
    } else {
        Ok(state)
    }
}

fn wait_for_event_sources(
    event_queue: &mut wayland_client::EventQueue<State>,
    state: &mut State,
) -> Result<(), ShellHostError> {
    if state.pipewire.reconnect_if_due(Instant::now()) {
        state.fanout_pipewire_control_outcomes();
        state.fanout_current_pipewire_snapshot();
    }
    if state.battery.needs_immediate_dispatch() {
        state.handle_battery_immediate_dispatch();
        return Ok(());
    }
    let Some(read_guard) = event_queue.prepare_read() else {
        return Ok(());
    };
    let wayland_fd = read_guard.connection_fd();
    let clock_fd = state.clock.poll_fd();
    let battery_watch = state.battery.bus_watch();
    let battery_deadline_fd = state.battery.deadline_fd();
    let pipewire_fd = state.pipewire.raw_poll_fd();
    let pipewire_timeout = state
        .pipewire
        .retry_timeout(Instant::now())
        .map(duration_to_timespec);
    let mut descriptors = Vec::with_capacity(5);
    descriptors.push(PollFd::from_borrowed_fd(
        wayland_fd,
        PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
    ));
    let clock_index = clock_fd.map(|clock_fd| {
        let index = descriptors.len();
        descriptors.push(PollFd::from_borrowed_fd(
            clock_fd,
            PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
        ));
        index
    });
    let battery_bus_fd = battery_watch.map(|watch| {
        let mut flags = PollFlags::ERR | PollFlags::HUP;
        if watch.read {
            flags |= PollFlags::IN;
        }
        if watch.write {
            flags |= PollFlags::OUT;
        }
        // The direct libdbus channel owns this descriptor for the complete
        // lifetime of `battery_watch` and remains alive until polling ends.
        let fd = unsafe { BorrowedFd::borrow_raw(watch.fd) };
        let index = descriptors.len();
        descriptors.push(PollFd::from_borrowed_fd(fd, flags));
        index
    });
    let battery_deadline_index = battery_deadline_fd.map(|deadline_fd| {
        let index = descriptors.len();
        descriptors.push(PollFd::from_borrowed_fd(
            deadline_fd,
            PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
        ));
        index
    });
    let pipewire_index = pipewire_fd.map(|raw_fd| {
        // The process PipeWire source owns this descriptor until polling
        // finishes and is not mutated while the borrowed descriptor is live.
        let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        let index = descriptors.len();
        descriptors.push(PollFd::from_borrowed_fd(
            fd,
            PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
        ));
        index
    });
    match poll(&mut descriptors, pipewire_timeout.as_ref()) {
        Ok(_) => {}
        Err(error) if error == rustix::io::Errno::INTR => {
            drop(descriptors);
            drop(read_guard);
            return Ok(());
        }
        Err(error) => {
            drop(descriptors);
            drop(read_guard);
            return Err(ShellHostError::Wayland(format!(
                "poll event-source descriptors: {error}"
            )));
        }
    }
    let wayland_events = descriptors[0].revents();
    let clock_events = clock_index.map(|index| descriptors[index].revents());
    let battery_bus_events = battery_bus_fd.map(|index| descriptors[index].revents());
    let battery_deadline_events = battery_deadline_index.map(|index| descriptors[index].revents());
    let pipewire_events = pipewire_index.map(|index| descriptors[index].revents());
    drop(descriptors);

    let (wayland_ready, clock_ready) =
        classify_poll_readiness(wayland_events, clock_events).map_err(ShellHostError::Clock)?;
    if wayland_ready {
        match read_guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(ShellHostError::wayland(error)),
        }
    } else {
        drop(read_guard);
    }

    if clock_ready {
        state.handle_clock_ready()?;
    }
    if let Some(events) = battery_bus_events {
        if events.intersects(PollFlags::NVAL) {
            if let Some(snapshot) = state
                .battery
                .handle_bus_failure("battery system-bus descriptor became invalid")
            {
                state.fanout_battery_snapshot(&snapshot);
            }
        } else if events
            .intersects(PollFlags::IN | PollFlags::OUT | PollFlags::ERR | PollFlags::HUP)
        {
            state.handle_battery_bus_ready();
        }
    }
    if let Some(events) = battery_deadline_events {
        if events.intersects(PollFlags::NVAL) {
            if let Some(snapshot) = state
                .battery
                .handle_deadline_failure("battery deadline descriptor became invalid")
            {
                state.fanout_battery_snapshot(&snapshot);
            }
        } else if events.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP) {
            state.handle_battery_deadline_ready();
        }
    }
    if let Some(events) = pipewire_events {
        let changed = match pipewire_poll_ready(events) {
            Ok(true) => state.pipewire.dispatch_ready(),
            Ok(false) => false,
            Err(error) => state.pipewire.handle_poll_error(error),
        };
        state.fanout_pipewire_control_outcomes();
        if changed {
            state.fanout_current_pipewire_snapshot();
        }
    }
    if state.pipewire.reconnect_if_due(Instant::now()) {
        state.fanout_pipewire_control_outcomes();
        state.fanout_current_pipewire_snapshot();
    } else {
        state.fanout_pipewire_control_outcomes();
    }
    Ok(())
}

fn classify_poll_readiness(
    wayland: PollFlags,
    timer: Option<PollFlags>,
) -> Result<(bool, bool), String> {
    if wayland.intersects(PollFlags::NVAL) {
        return Err("Wayland descriptor became invalid".into());
    }
    if timer.is_some_and(|events| events.intersects(PollFlags::NVAL)) {
        return Err("clock timer descriptor became invalid".into());
    }
    Ok((
        wayland.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP),
        timer.is_some_and(|events| {
            events.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP)
        }),
    ))
}

fn pipewire_poll_ready(events: PollFlags) -> Result<bool, String> {
    if events.intersects(PollFlags::NVAL) {
        return Err("PipeWire loop descriptor became invalid".into());
    }
    Ok(events.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP))
}

fn update_input_region(
    compositor: &wl_compositor::WlCompositor,
    surface: &wl_surface::WlSurface,
    logical_width: u32,
    logical_height: u32,
    input_regions: &[htm_runtime::LiveFrameRect],
    qh: &QueueHandle<State>,
) {
    let region = compositor.create_region(qh, ());
    for rect in input_regions {
        if let Some((x, y, width, height)) = rounded_region(rect, logical_width, logical_height) {
            region.add(x, y, width, height);
        }
    }
    surface.set_input_region(Some(&region));
    region.destroy();
}

fn rounded_region(
    rect: &htm_runtime::LiveFrameRect,
    logical_width: u32,
    logical_height: u32,
) -> Option<(i32, i32, i32, i32)> {
    let values = [rect.x, rect.y, rect.width, rect.height];
    if !values.into_iter().all(f32::is_finite) || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let x1 = rect.x.floor().clamp(0.0, logical_width as f32);
    let y1 = rect.y.floor().clamp(0.0, logical_height as f32);
    let x2 = (rect.x + rect.width)
        .ceil()
        .clamp(0.0, logical_width as f32);
    let y2 = (rect.y + rect.height)
        .ceil()
        .clamp(0.0, logical_height as f32);
    let width = (x2 - x1).max(0.0);
    let height = (y2 - y1).max(0.0);
    if width < 1.0 || height < 1.0 {
        return None;
    }
    Some((x1 as i32, y1 as i32, width as i32, height as i32))
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

fn milliseconds_to_microseconds(value: f64) -> u64 {
    (value * 1_000.0).round().clamp(0.0, u64::MAX as f64) as u64
}

fn built_in_binding_values(
    template_id: &str,
    output_label: &str,
    scale_numerator: u32,
    overlay_open: bool,
    overlay_activation_count: u64,
    last_action: &str,
) -> Vec<(StateBindingKey, String)> {
    let scale = f64::from(scale_numerator) / f64::from(LIVE_SCALE_DENOMINATOR);
    vec![
        (
            StateBindingKey::OutputLabel,
            format!("Output: {output_label}"),
        ),
        (StateBindingKey::OutputScale, format!("Scale: {scale:.2}×")),
        (
            StateBindingKey::SurfaceTemplateId,
            format!("Surface: {template_id}"),
        ),
        (
            StateBindingKey::OverlayStatus,
            format!("Overlay: {}", if overlay_open { "open" } else { "closed" }),
        ),
        (
            StateBindingKey::OverlayActivationCount,
            format!("Activations: {overlay_activation_count}"),
        ),
        (
            StateBindingKey::ShellLastAction,
            format!("Last action: {last_action}"),
        ),
    ]
}

fn built_in_token_values(
    profile: PresentationProfile,
    overlay_open: bool,
) -> [(StateBindingKey, StateToken); 2] {
    [
        (
            StateBindingKey::OverlayStatus,
            if overlay_open {
                StateToken::Open
            } else {
                StateToken::Closed
            },
        ),
        (
            StateBindingKey::SurfaceScaleProfile,
            match profile {
                PresentationProfile::Scale1 => StateToken::Scale1,
                PresentationProfile::FractionalViewport => StateToken::Fractional,
            },
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn apply_surface_bindings(
    runtime: &mut LiveDocument,
    template_id: &str,
    output_label: &str,
    scale_numerator: u32,
    profile: PresentationProfile,
    overlay_open: bool,
    overlay_activation_count: u64,
    last_action: &str,
) -> Result<htm_runtime::BindingUpdate, htm_runtime::RuntimeError> {
    runtime.apply_bound_state(
        &built_in_binding_values(
            template_id,
            output_label,
            scale_numerator,
            overlay_open,
            overlay_activation_count,
            last_action,
        ),
        &built_in_token_values(profile, overlay_open),
    )
}

fn validate_manifest_action_source(
    action: &LiveAction,
    owner: u64,
    panel_owner: u64,
    overlay_owner: u64,
    overlay_open: bool,
) -> Result<(), &'static str> {
    match action {
        LiveAction::ToggleOverlay if owner == panel_owner => Ok(()),
        LiveAction::ToggleOverlay => Err("overlay toggle came from a non-panel instance"),
        LiveAction::CloseOverlay | LiveAction::ActivateOverlay if owner != overlay_owner => {
            Err("overlay action came from a non-overlay instance")
        }
        LiveAction::CloseOverlay | LiveAction::ActivateOverlay if !overlay_open => {
            Err("closed overlay cannot dispatch an action")
        }
        LiveAction::CloseOverlay | LiveAction::ActivateOverlay => Ok(()),
        LiveAction::SingleOverlayActivate => {
            Err("single-overlay action is invalid in manifest mode")
        }
        LiveAction::ClockEnable(_) | LiveAction::ClockDisable(_) | LiveAction::ClockToggle(_) => {
            Ok(())
        }
        LiveAction::PowerProfileSetPowerSaver
        | LiveAction::PowerProfileSetBalanced
        | LiveAction::PowerProfileSetPerformance
        | LiveAction::PipeWireAudio(_)
        | LiveAction::PipeWireDefault(_)
        | LiveAction::PipeWirePeak(_) => Ok(()),
    }
}

fn release_pointer(pointer: wl_pointer::WlPointer) {
    if pointer.version() >= WL_POINTER_RELEASE_VERSION {
        pointer.release();
    }
}

fn release_seat(seat: wl_seat::WlSeat) {
    if seat.version() >= WL_SEAT_RELEASE_VERSION {
        seat.release();
    }
}

fn release_output(output: wl_output::WlOutput) {
    if output.version() >= WL_OUTPUT_RELEASE_VERSION {
        output.release();
    }
}

fn release_shm(shm: wl_shm::WlShm) {
    if shm.version() >= WL_SHM_RELEASE_VERSION {
        shm.release();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" if state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                }
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, version.min(2), qh, ()));
                }
                "wl_output" => {
                    let key = match state.output_catalog.add(name, version) {
                        Ok(key) => key,
                        Err(error) => {
                            state.fail(error.to_string());
                            return;
                        }
                    };
                    let proxy = registry.bind(name, version.min(4), qh, OutputData { key });
                    state.outputs.push(BoundOutput {
                        key,
                        proxy,
                        advertised_at: Instant::now(),
                    });
                    if state.initial_discovery_complete {
                        connection
                            .display()
                            .sync(qh, CallbackData::OutputReady(key));
                    }
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat_global_name = Some(name);
                    state.seat = Some(registry.bind(name, version.min(9), qh, ()));
                }
                "zwlr_layer_shell_v1" if state.layer_shell.is_none() => {
                    let selected = version.min(LAYER_SHELL_MAX_VERSION);
                    state.layer_shell_version = selected;
                    state.layer_shell = Some(registry.bind(name, selected, qh, ()));
                }
                "wp_viewporter" if state.viewporter.is_none() => {
                    state.viewporter_advertised = true;
                    state.viewporter =
                        Some(registry.bind(name, version.min(VIEWPORTER_VERSION), qh, ()));
                }
                "wp_fractional_scale_manager_v1" if state.fractional_scale_manager.is_none() => {
                    state.fractional_scale_advertised = true;
                    state.fractional_scale_manager =
                        Some(registry.bind(name, version.min(FRACTIONAL_SCALE_VERSION), qh, ()));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name }
                if state.output_catalog.key_for_global(name).is_some() =>
            {
                let key = state
                    .output_catalog
                    .key_for_global(name)
                    .expect("guarded above");
                state.destroy_output_instance(key);
                state.output_catalog.remove(name);
                if let Some(index) = state.outputs.iter().position(|output| output.key == key) {
                    let output = state.outputs.swap_remove(index);
                    release_output(output.proxy);
                }
                state.output_removals = state.output_removals.saturating_add(1);
                state.maybe_stop_after_output_events();
                if state.selected_output == Some(key) {
                    state.selected_output = None;
                    state.running = false;
                } else if matches!(state.options, SessionOptions::Manifest(_))
                    && state.output_instances.is_empty()
                {
                    eprintln!("htmshell-live: no eligible output is currently present");
                }
            }
            wl_registry::Event::GlobalRemove { name } if state.seat_global_name == Some(name) => {
                if let Some(pointer) = state.pointer.take() {
                    release_pointer(pointer);
                }
                state.clear_pointer_focus();
                if let Some(seat) = state.seat.take() {
                    release_seat(seat);
                }
                state.seat_global_name = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_shm::WlShm,
        event: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format {
            format: WEnum::Value(wl_shm::Format::Argb8888),
        } = event
        {
            state.shm_argb8888 = true;
        }
    }
}

impl Dispatch<wl_output::WlOutput, OutputData> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        data: &OutputData,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Scale { factor } if factor > 0 => {
                let changed = state.output_catalog.set_scale(data.key, factor);
                if changed && state.initial_discovery_complete {
                    state.reconcile_output(data.key, qh);
                }
            }
            wl_output::Event::Name { name } => {
                state.output_catalog.set_name(data.key, name);
            }
            wl_output::Event::Description { description } => {
                state.output_catalog.set_description(data.key, description);
            }
            wl_output::Event::Done
                if state.output_catalog.mark_ready(data.key)
                    && state.initial_discovery_complete =>
            {
                state.reconcile_output(data.key, qh);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities { capabilities } = event else {
            return;
        };
        let has_pointer = matches!(
            capabilities,
            WEnum::Value(value) if value.contains(wl_seat::Capability::Pointer)
        );
        match (has_pointer, state.pointer.is_some()) {
            (true, false) => state.pointer = Some(seat.get_pointer(qh, ())),
            (false, true) => {
                if let Some(pointer) = state.pointer.take() {
                    release_pointer(pointer);
                }
                state.clear_pointer_focus();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                let owner = state
                    .surfaces
                    .iter()
                    .find(|candidate| {
                        candidate
                            .surface
                            .as_ref()
                            .is_some_and(|candidate| candidate.id() == surface.id())
                    })
                    .map(|candidate| candidate.owner);
                if let Some(owner) = owner {
                    if state.pointer_focus != Some(owner) {
                        state.clear_pointer_focus();
                        state.pointer_focus = Some(owner);
                    }
                    if let Some(index) = state.surface_index_by_owner(owner) {
                        state.surfaces[index].summary.pointer_enters = state.surfaces[index]
                            .summary
                            .pointer_enters
                            .saturating_add(1);
                    }
                    state.pointer_move(owner, surface_x, surface_y);
                }
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if let Some(owner) = state.pointer_focus {
                    if let Some(index) = state.surface_index_by_owner(owner) {
                        state.surfaces[index].summary.pointer_motions = state.surfaces[index]
                            .summary
                            .pointer_motions
                            .saturating_add(1);
                    }
                    state.pointer_move(owner, surface_x, surface_y);
                }
            }
            wl_pointer::Event::Leave { surface, .. } => {
                let leaving = state
                    .surfaces
                    .iter()
                    .find(|candidate| {
                        candidate
                            .surface
                            .as_ref()
                            .is_some_and(|candidate| candidate.id() == surface.id())
                    })
                    .map(|candidate| candidate.owner);
                if leaving == state.pointer_focus {
                    state.clear_pointer_focus();
                }
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } if button == BTN_LEFT => {
                if let Some(owner) = state.pointer_focus {
                    if let Some(index) = state.surface_index_by_owner(owner) {
                        state.surfaces[index].summary.pointer_buttons = state.surfaces[index]
                            .summary
                            .pointer_buttons
                            .saturating_add(1);
                    }
                    match button_state {
                        WEnum::Value(wl_pointer::ButtonState::Pressed) => {
                            state.pointer_button(owner, true)
                        }
                        WEnum::Value(wl_pointer::ButtonState::Released) => {
                            state.pointer_button(owner, false)
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, LayerData> for State {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        data: &LayerData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(index) = state.surface_index_by_owner(data.owner) else {
            return;
        };
        if state.surfaces[index]
            .layer_surface
            .as_ref()
            .is_none_or(|current| current.id() != layer_surface.id())
        {
            return;
        }
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                if width == 0 || height == 0 {
                    state.fail("layer-shell configure returned a zero dimension".into());
                    return;
                }
                let surface = &mut state.surfaces[index];
                if let Err(error) = surface.lifecycle.configure(serial, width, height) {
                    state.fail(format!("layer-shell configure rejected: {error}"));
                    return;
                }
                layer_surface.ack_configure(serial);
                if let Err(error) = surface.lifecycle.acknowledge(serial) {
                    state.fail(format!("layer-shell acknowledgement rejected: {error}"));
                    return;
                }
                surface.configures.receive(width, height);
                surface.scale_state.set_logical_size(width, height);
                surface.map_state.configured(surface.desired_mapped);
                surface.summary.configure_count = surface.summary.configure_count.saturating_add(1);
                if surface.desired_mapped {
                    surface.scheduler.mark_dirty();
                }
                if surface.summary.first_configure_us == 0 {
                    surface.summary.first_configure_us = elapsed_us(state.started);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                let output_key = state.surfaces[index].output_key;
                let surface = &mut state.surfaces[index];
                surface.lifecycle.close();
                surface.desired_mapped = false;
                surface.mapped = false;
                surface.scheduler.stop_scheduling();
                surface.map_state.close();
                if matches!(state.options, SessionOptions::Manifest(_)) {
                    state.destroy_output_instance(output_key);
                } else {
                    state.running = false;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ScaleData> for State {
    fn event(
        state: &mut Self,
        fractional_scale: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        data: &ScaleData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else {
            return;
        };
        let Some(index) = state.surface_index_by_owner(data.owner) else {
            state.stale_scale_events_contained =
                state.stale_scale_events_contained.saturating_add(1);
            return;
        };
        let current = &state.surfaces[index];
        if current.role_generation != data.surface_generation
            || current
                .fractional_scale
                .as_ref()
                .is_none_or(|current| current.id() != fractional_scale.id())
        {
            state.stale_scale_events_contained =
                state.stale_scale_events_contained.saturating_add(1);
            return;
        }

        let result = state.surfaces[index]
            .scale_state
            .receive_preferred(data.surface_generation, scale);
        match result {
            Ok(true) => {
                let surface = &mut state.surfaces[index];
                surface.summary.preferred_scale_changes =
                    surface.summary.preferred_scale_changes.saturating_add(1);
                surface.pending_scale_started = Some(Instant::now());
                if surface.desired_mapped {
                    surface.scheduler.mark_dirty();
                }
            }
            Ok(false) => {}
            Err(error) => {
                if matches!(state.options, SessionOptions::Manifest(_)) {
                    let surface = &mut state.surfaces[index];
                    surface.presentation_failed = true;
                    surface.scheduler.stop_scheduling();
                    eprintln!(
                        "htmshell-live: surface instance {} rejected preferred scale: {error}",
                        data.owner
                    );
                } else {
                    state.fail(format!("preferred scale rejected: {error}"));
                }
            }
        }
    }
}

impl Dispatch<wl_callback::WlCallback, CallbackData> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        data: &CallbackData,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_callback::Event::Done { .. }) {
            match *data {
                CallbackData::Frame { owner, generation } => state.on_frame_done(owner, generation),
                CallbackData::OutputReady(key) => {
                    state.output_catalog.mark_ready(key);
                    state.reconcile_output(key, qh);
                }
            }
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, BufferData> for State {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        data: &BufferData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) {
            state.on_buffer_release(data.owner, data.id);
        }
    }
}

impl Dispatch<wl_surface::WlSurface, SurfaceData> for State {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        data: &SurfaceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let _ = data.owner;
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_region::WlRegion);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore ZwlrLayerShellV1);
delegate_noop!(State: ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(State: ignore wp_viewporter::WpViewporter);
delegate_noop!(State: ignore wp_viewport::WpViewport);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_owners_are_disjoint() {
        assert_ne!(SurfaceKind::Panel.owner(), SurfaceKind::Overlay.owner());
        assert_ne!(
            SurfaceKind::SingleOverlay.owner(),
            SurfaceKind::Panel.owner()
        );
    }

    #[test]
    fn poll_readiness_keeps_wayland_and_clock_sources_independent() {
        assert_eq!(
            classify_poll_readiness(PollFlags::IN, None).unwrap(),
            (true, false)
        );
        assert_eq!(
            classify_poll_readiness(PollFlags::empty(), Some(PollFlags::IN)).unwrap(),
            (false, true)
        );
        assert_eq!(
            classify_poll_readiness(PollFlags::IN, Some(PollFlags::IN)).unwrap(),
            (true, true)
        );
        assert_eq!(
            classify_poll_readiness(PollFlags::empty(), Some(PollFlags::empty())).unwrap(),
            (false, false)
        );
        assert!(classify_poll_readiness(PollFlags::NVAL, None).is_err());
        assert!(classify_poll_readiness(PollFlags::empty(), Some(PollFlags::NVAL)).is_err());
    }

    #[test]
    fn pipewire_readiness_is_independent_and_invalid_descriptors_are_contained() {
        assert!(!pipewire_poll_ready(PollFlags::empty()).unwrap());
        assert!(pipewire_poll_ready(PollFlags::IN).unwrap());
        assert!(pipewire_poll_ready(PollFlags::ERR).unwrap());
        assert!(pipewire_poll_ready(PollFlags::HUP).unwrap());
        assert!(pipewire_poll_ready(PollFlags::NVAL).is_err());
    }

    #[test]
    fn built_in_display_values_are_typed_deterministic_and_output_local() {
        let a = built_in_binding_values("panel", "output-a", 192, true, 7, "Opened");
        let b = built_in_binding_values("panel", "output-b", 120, false, 0, "Ready");
        assert_eq!(a.len(), 6);
        assert_eq!(
            a[0],
            (StateBindingKey::OutputLabel, "Output: output-a".into())
        );
        assert_eq!(a[1], (StateBindingKey::OutputScale, "Scale: 1.60×".into()));
        assert_eq!(
            a[3],
            (StateBindingKey::OverlayStatus, "Overlay: open".into())
        );
        assert_eq!(
            a[4],
            (
                StateBindingKey::OverlayActivationCount,
                "Activations: 7".into(),
            )
        );
        assert_ne!(a, b);
        assert_eq!(b[0].1, "Output: output-b");
        assert_eq!(b[1].1, "Scale: 1.00×");
        assert_eq!(b[3].1, "Overlay: closed");
        assert_eq!(
            built_in_token_values(PresentationProfile::Scale1, false),
            [
                (StateBindingKey::OverlayStatus, StateToken::Closed),
                (StateBindingKey::SurfaceScaleProfile, StateToken::Scale1,),
            ]
        );
        assert_eq!(
            built_in_token_values(PresentationProfile::FractionalViewport, true),
            [
                (StateBindingKey::OverlayStatus, StateToken::Open),
                (StateBindingKey::SurfaceScaleProfile, StateToken::Fractional,),
            ]
        );
    }

    #[test]
    fn manifest_action_source_policy_rejects_wrong_stale_or_closed_sources() {
        assert!(
            validate_manifest_action_source(&LiveAction::ToggleOverlay, 10, 10, 11, false).is_ok()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::ToggleOverlay, 11, 10, 11, true).is_err()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::CloseOverlay, 11, 10, 11, true).is_ok()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::ActivateOverlay, 11, 10, 11, true).is_ok()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::CloseOverlay, 11, 10, 11, false).is_err()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::ActivateOverlay, 11, 10, 11, false)
                .is_err()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::ActivateOverlay, 12, 10, 11, true)
                .is_err()
        );
        assert!(
            validate_manifest_action_source(&LiveAction::SingleOverlayActivate, 10, 10, 11, true)
                .is_err()
        );
    }

    #[test]
    fn configure_coalescing_retains_latest_size_only() {
        let mut coalescer = ConfigureCoalescer::default();
        for width in 800..900 {
            coalescer.receive(width, 600);
        }
        assert_eq!(coalescer.received, 100);
        assert_eq!(coalescer.latest(), Some((899, 600)));
        assert_eq!(coalescer.presented, 0);
        coalescer.mark_presented();
        assert_eq!(coalescer.presented, 1);
    }

    #[test]
    fn outstanding_callback_coalesces_many_configures_into_one_presentation() {
        let mut coalescer = ConfigureCoalescer::default();
        let mut scheduler = FrameScheduler::default();
        let mut allocations = Vec::new();
        scheduler.mark_dirty();
        scheduler.frame_committed();
        for width in 1000..1100 {
            coalescer.receive(width, 700);
            scheduler.mark_dirty();
            assert_eq!(
                scheduler.decision(true, true),
                ScheduleDecision::WaitForFrameCallback
            );
        }
        scheduler.frame_callback_done();
        assert_eq!(scheduler.decision(true, true), ScheduleDecision::Render);
        allocations.push(coalescer.latest().expect("latest configure"));
        coalescer.mark_presented();
        assert_eq!(coalescer.received, 100);
        assert_eq!(coalescer.presented, 1);
        assert_eq!(coalescer.latest(), Some((1099, 700)));
        assert_eq!(allocations, [(1099, 700)]);
    }

    #[test]
    fn configure_and_scale_bursts_share_one_latest_presentation() {
        for configure_first in [true, false] {
            let mut scale = SurfaceScaleState::new(9, true);
            let mut scheduler = FrameScheduler::default();
            scheduler.mark_dirty();
            scheduler.frame_committed();

            if configure_first {
                scale.set_logical_size(100, 50);
                scale.receive_preferred(9, 150).unwrap();
            } else {
                scale.receive_preferred(9, 150).unwrap();
                scale.set_logical_size(100, 50);
            }
            scale.set_logical_size(101, 51);
            scale.receive_preferred(9, 180).unwrap();
            scheduler.mark_dirty();
            assert_eq!(
                scheduler.decision(true, true),
                ScheduleDecision::WaitForFrameCallback
            );

            scheduler.frame_callback_done();
            let request = scale.render_request().unwrap().unwrap();
            assert_eq!((request.logical_width, request.logical_height), (101, 51));
            assert_eq!((request.buffer_width, request.buffer_height), (152, 77));
            assert_eq!(scheduler.decision(true, true), ScheduleDecision::Render);
            scale.mark_applied();
            assert_eq!(scale.pending_revision(), scale.applied_revision());
        }
    }

    #[test]
    fn transient_mapping_is_idempotent_across_repeated_cycles() {
        let mut mapping = SurfaceMapState::AwaitingConfigure;
        mapping.configured(false);
        assert_eq!(mapping, SurfaceMapState::Unmapped);
        for _ in 0..50 {
            mapping.request_map(true);
            mapping.request_map(true);
            assert_eq!(mapping, SurfaceMapState::PendingMap);
            mapping.mapped();
            assert_eq!(mapping, SurfaceMapState::Mapped);
            mapping.begin_unmap();
            mapping.begin_unmap();
            assert_eq!(mapping, SurfaceMapState::Unmapping);
            mapping.finish_unmap();
            assert_eq!(mapping, SurfaceMapState::Unmapped);
        }
        mapping.close();
        mapping.request_map(true);
        assert_eq!(mapping, SurfaceMapState::Closed);
    }

    #[test]
    fn independent_schedulers_do_not_cross_dirty_state() {
        let mut panel = FrameScheduler::default();
        let mut overlay = FrameScheduler::default();
        panel.mark_dirty();
        assert_eq!(panel.decision(true, true), ScheduleDecision::Render);
        assert_eq!(overlay.decision(true, true), ScheduleDecision::Idle);
        panel.frame_committed();
        overlay.mark_dirty();
        assert_eq!(panel.decision(true, true), ScheduleDecision::Idle);
        assert_eq!(overlay.decision(true, true), ScheduleDecision::Render);
    }

    #[test]
    fn busy_surface_does_not_stall_the_other_surface() {
        let mut panel = FrameScheduler::default();
        let mut overlay = FrameScheduler::default();
        panel.mark_dirty();
        overlay.mark_dirty();
        assert_eq!(panel.decision(true, false), ScheduleDecision::WaitForBuffer);
        assert_eq!(overlay.decision(true, true), ScheduleDecision::Render);
        overlay.frame_committed();
        assert_eq!(panel.decision(true, true), ScheduleDecision::Render);
        assert_eq!(overlay.decision(true, true), ScheduleDecision::Idle);
    }

    #[test]
    fn input_region_rounds_outward_and_excludes_transparent_area() {
        let rect = htm_runtime::LiveFrameRect {
            x: 189.25,
            y: 164.5,
            width: 421.2,
            height: 270.2,
        };
        assert_eq!(rounded_region(&rect, 800, 600), Some((189, 164, 422, 271)));
    }

    #[test]
    fn invalid_or_empty_input_regions_are_ignored() {
        assert!(
            rounded_region(
                &htm_runtime::LiveFrameRect {
                    x: f32::NAN,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                800,
                600,
            )
            .is_none()
        );
        assert!(
            rounded_region(
                &htm_runtime::LiveFrameRect {
                    x: 2.0,
                    y: 2.0,
                    width: 0.0,
                    height: 10.0,
                },
                800,
                600,
            )
            .is_none()
        );
    }

    #[test]
    fn required_global_failures_are_specific() {
        let complete = RequiredGlobals {
            compositor: true,
            shm: true,
            argb8888: true,
            output: true,
            seat: true,
            pointer: true,
            layer_shell: true,
        };
        assert!(complete.validate(true).is_ok());
        assert!(matches!(
            RequiredGlobals {
                layer_shell: false,
                ..complete
            }
            .validate(true),
            Err(ShellHostError::MissingGlobal("zwlr_layer_shell_v1"))
        ));
        assert!(matches!(
            RequiredGlobals {
                pointer: false,
                ..complete
            }
            .validate(true),
            Err(ShellHostError::MissingPointerCapability)
        ));
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn live_gpu_activation_is_exact_and_internal() {
        assert!(internal_gpu_renderer_value(Some("vello")));
        for value in [None, Some(""), Some("cpu"), Some("Vello"), Some("vello ")] {
            assert!(!internal_gpu_renderer_value(value));
        }
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn gpu_damage_uses_buffer_coordinates_when_protocol_supports_them() {
        assert_eq!(
            select_gpu_wayland_damage_mode(4),
            GpuWaylandDamageMode::Buffer
        );
        assert_eq!(
            select_gpu_wayland_damage_mode(6),
            GpuWaylandDamageMode::Buffer
        );
        assert_eq!(
            select_gpu_wayland_damage_mode(3),
            GpuWaylandDamageMode::SurfaceFull
        );
    }
}
