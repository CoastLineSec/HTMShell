use crate::ShellHostError;
use crate::buffer::{BufferData, BufferPoolStats, ShmBufferPool};
use crate::lifecycle::LayerLifecycle;
use crate::manifest::{SurfaceKind as ManifestSurfaceKind, ValidatedManifest};
use crate::output::{OutputCatalog, OutputEligibility, OutputKey};
use crate::scale::{PresentationProfile, SurfaceScaleState};
use crate::scheduler::{FrameScheduler, ScheduleDecision};
use htm_runtime::{
    LIVE_SCALE_DENOMINATOR, LiveAction, LiveDocument, LiveDocumentKind, LiveFrame, StateBindingKey,
};
use std::path::PathBuf;
use std::time::Instant;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop,
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
}

#[derive(Debug, Clone)]
pub struct MultiSurfaceHostOptions {
    pub package: PathBuf,
    pub panel_height: u32,
    pub automatic_overlay_cycles: u32,
    pub exit_after_automatic_cycles: bool,
    pub exit_after_overlay_close: bool,
    pub open_overlay_on_start: bool,
}

#[derive(Debug, Clone)]
pub struct ManifestHostOptions {
    pub manifest: ValidatedManifest,
    pub exit_after_initial_frames: bool,
    pub exit_after_output_events: Option<u64>,
    pub exit_after_actions: Option<u64>,
    pub exit_after_scale_changes: Option<u64>,
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
    pub last_render_us: u64,
    pub last_pixel_conversion_us: u64,
    pub registry_initialization_us: u64,
    pub declaration_discovery_us: u64,
    pub registered_element_count: u64,
    pub binding_count: u64,
    pub registered_action_count: u64,
    pub registry_scan_count: u64,
    pub suppressed_binding_updates: u64,
    pub last_pointer_release_to_action_dispatch_us: u64,
    pub last_action_dispatch_to_state_mutation_us: u64,
    pub last_state_mutation_to_commit_us: u64,
    pub last_state_mutation_to_frame_callback_us: u64,
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
        });
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
        state.scale_state = SurfaceScaleState::new(surface_generation, fractional_available);
        state.map_state = SurfaceMapState::AwaitingConfigure;
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
        let mut panel_runtime = LiveDocument::load_surface_document(
            options.manifest.package_root(),
            panel.document(),
            LiveDocumentKind::Panel,
            1,
            panel_preset.thickness,
        )?;
        panel_runtime.set_instance_context(panel.id(), &diagnostic_label)?;
        panel_runtime.update_panel_state(overlay_initially_open, "Ready")?;
        panel_runtime.apply_bound_text(&built_in_binding_values(
            panel.id(),
            &diagnostic_label,
            LIVE_SCALE_DENOMINATOR,
            overlay_initially_open,
            0,
            "Ready",
        ))?;
        self.next_instance_generation = self.next_instance_generation.saturating_add(1);
        let overlay_generation = self.next_instance_generation;
        let mut overlay_runtime = LiveDocument::load_surface_document(
            options.manifest.package_root(),
            overlay.document(),
            LiveDocumentKind::TransientOverlay,
            1,
            1,
        )?;
        overlay_runtime.set_instance_context(overlay.id(), &diagnostic_label)?;
        overlay_runtime.update_overlay_state(0, "Ready")?;
        overlay_runtime.apply_bound_text(&built_in_binding_values(
            overlay.id(),
            &diagnostic_label,
            LIVE_SCALE_DENOMINATOR,
            overlay_initially_open,
            0,
            "Ready",
        ))?;
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
    }

    fn destroy_surface_owner(&mut self, owner: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return;
        };
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

    fn maybe_render(&mut self, owner: u64, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return Ok(());
        };
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
                Some(document) => LiveDocument::load_surface_document(
                    &surface_state.package,
                    document,
                    surface_state.kind.document_kind(),
                    logical_width,
                    logical_height,
                )?,
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
                runtime.apply_bound_text(&built_in_binding_values(
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
                ))?;
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
                .apply_bound_text(&built_in_binding_values(
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
                ))?;
        }
        let buffer_width = render_request.buffer_width;
        let buffer_height = render_request.buffer_height;
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
        let surface_state = &mut self.surfaces[index];
        let size_ready = surface_state
            .pool
            .ensure_size(&shm, qh, buffer_width, buffer_height)?;
        let free_buffer = size_ready && surface_state.pool.has_free();
        match surface_state.scheduler.decision(true, free_buffer) {
            ScheduleDecision::Idle
            | ScheduleDecision::WaitForFrameCallback
            | ScheduleDecision::WaitForBuffer => return Ok(()),
            ScheduleDecision::Render => {}
        }
        let frame = surface_state
            .runtime
            .as_mut()
            .expect("initialized above")
            .render_for(render_request)?;
        let Some((_id, buffer, conversion_us)) = surface_state
            .pool
            .acquire_and_write(&frame.premultiplied_rgba)?
        else {
            surface_state.scheduler.mark_dirty();
            return Ok(());
        };
        update_input_region(&compositor, &wayland_surface, &frame, qh);
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
        surface_state.scheduler.frame_committed();
        surface_state.mapped = true;
        surface_state.map_state.mapped();
        surface_state.configures.mark_presented();
        surface_state.scale_state.mark_applied();
        surface_state.summary.frames_committed =
            surface_state.summary.frames_committed.saturating_add(1);
        surface_state.summary.logical_width = logical_width;
        surface_state.summary.logical_height = logical_height;
        surface_state.summary.buffer_width = frame.buffer_width;
        surface_state.summary.buffer_height = frame.buffer_height;
        surface_state.summary.preferred_scale_numerator =
            surface_state.scale_state.preferred_numerator();
        surface_state.summary.scale_denominator = htm_runtime::LIVE_SCALE_DENOMINATOR;
        surface_state.summary.fractional_viewport_active =
            surface_state.scale_state.profile() == PresentationProfile::FractionalViewport;
        surface_state.summary.last_render_us = milliseconds_to_microseconds(frame.render_ms);
        surface_state.summary.last_pixel_conversion_us = conversion_us;
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
        surface_state.summary.registry_initialization_us =
            milliseconds_to_microseconds(runtime_measurements.registry_initialization_ms);
        surface_state.summary.declaration_discovery_us =
            milliseconds_to_microseconds(runtime_measurements.declaration_discovery_ms);
        surface_state.summary.registered_element_count =
            runtime_measurements.registered_element_count;
        surface_state.summary.binding_count = runtime_measurements.binding_count;
        surface_state.summary.registered_action_count = runtime_measurements.action_count;
        surface_state.summary.registry_scan_count = runtime_measurements.registry_scan_count;
        surface_state.summary.suppressed_binding_updates =
            runtime_measurements.suppressed_binding_updates;
        surface_state.refresh_pool_summary();
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
        match surface
            .runtime
            .as_mut()
            .map(|runtime| runtime.pointer_move(x, y))
        {
            Some(Ok(true)) => surface.scheduler.mark_dirty(),
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
                if !pressed {
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
            }
            Some(Ok(false)) | None => {}
            Some(Err(error)) => {
                self.fail(format!("pointer button rejected: {error}"));
            }
        }
    }

    fn handle_action(&mut self, owner: u64, action: LiveAction) -> Result<(), ShellHostError> {
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
            action,
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
        self.update_manifest_panel(panel_owner, true, &last_action)
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
        self.update_manifest_panel(panel_owner, false, &last_action)
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
        let update = runtime.apply_bound_text(&built_in_binding_values(
            &surface.template_id,
            output_label,
            surface.scale_state.effective_numerator(),
            shared.overlay_open,
            shared.overlay_activation_count,
            &shared.last_action,
        ))?;
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
        Ok(())
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
        if let Some(index) = self.surface_index(SurfaceKind::Overlay) {
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
        self.update_panel_document()?;
        Ok(())
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

    fn fail(&mut self, message: String) {
        if self.failure.is_none() {
            self.failure = Some(message);
        }
        self.running = false;
    }

    fn begin_shutdown(&mut self) {
        self.clear_pointer_focus();
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
                            metrics.registry_initialization_us = milliseconds_to_microseconds(
                                measurements.registry_initialization_ms,
                            );
                            metrics.declaration_discovery_us =
                                milliseconds_to_microseconds(measurements.declaration_discovery_ms);
                            metrics.registered_element_count =
                                measurements.registered_element_count;
                            metrics.binding_count = measurements.binding_count;
                            metrics.registered_action_count = measurements.action_count;
                            metrics.registry_scan_count = measurements.registry_scan_count;
                            metrics.suppressed_binding_updates =
                                measurements.suppressed_binding_updates;
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
        event_queue
            .blocking_dispatch(&mut state)
            .map_err(ShellHostError::wayland)?;
        while event_queue
            .dispatch_pending(&mut state)
            .map_err(ShellHostError::wayland)?
            > 0
        {}
        state.maybe_render_all(&qh)?;
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

fn update_input_region(
    compositor: &wl_compositor::WlCompositor,
    surface: &wl_surface::WlSurface,
    frame: &LiveFrame,
    qh: &QueueHandle<State>,
) {
    let region = compositor.create_region(qh, ());
    for rect in &frame.input_regions {
        if let Some((x, y, width, height)) =
            rounded_region(rect, frame.logical_width, frame.logical_height)
        {
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

fn validate_manifest_action_source(
    action: LiveAction,
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
    fn built_in_display_values_are_typed_deterministic_and_output_local() {
        let a = built_in_binding_values("panel", "output-a", 192, true, 7, "Opened");
        let b = built_in_binding_values("panel", "output-b", 120, false, 0, "Ready");
        assert_eq!(a.len(), StateBindingKey::ALL.len());
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
    }

    #[test]
    fn manifest_action_source_policy_rejects_wrong_stale_or_closed_sources() {
        assert!(
            validate_manifest_action_source(LiveAction::ToggleOverlay, 10, 10, 11, false).is_ok()
        );
        assert!(
            validate_manifest_action_source(LiveAction::ToggleOverlay, 11, 10, 11, true).is_err()
        );
        assert!(
            validate_manifest_action_source(LiveAction::CloseOverlay, 11, 10, 11, true).is_ok()
        );
        assert!(
            validate_manifest_action_source(LiveAction::ActivateOverlay, 11, 10, 11, true).is_ok()
        );
        assert!(
            validate_manifest_action_source(LiveAction::CloseOverlay, 11, 10, 11, false).is_err()
        );
        assert!(
            validate_manifest_action_source(LiveAction::ActivateOverlay, 11, 10, 11, false)
                .is_err()
        );
        assert!(
            validate_manifest_action_source(LiveAction::ActivateOverlay, 12, 10, 11, true).is_err()
        );
        assert!(
            validate_manifest_action_source(LiveAction::SingleOverlayActivate, 10, 10, 11, true)
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
}
