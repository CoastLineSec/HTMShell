use crate::ShellHostError;
use crate::buffer::{BufferData, BufferPoolStats, ShmBufferPool};
use crate::lifecycle::LayerLifecycle;
use crate::scheduler::{FrameScheduler, ScheduleDecision};
use htm_runtime::{LiveAction, LiveDocument, LiveDocumentKind, LiveFrame};
use std::path::PathBuf;
use std::time::Instant;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_output, wl_pointer, wl_region, wl_registry,
        wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

const SINGLE_OWNER: u8 = 0;
const PANEL_OWNER: u8 = 1;
const OVERLAY_OWNER: u8 = 2;
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

#[derive(Debug, Clone, Default)]
pub struct SurfaceHostSummary {
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceKind {
    SingleOverlay,
    Panel,
    Overlay,
}

impl SurfaceKind {
    fn owner(self) -> u8 {
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
}

#[derive(Debug, Clone, Default)]
struct SharedShellState {
    overlay_open: bool,
    overlay_activation_count: u64,
    last_action: String,
}

#[derive(Debug, Clone, Copy)]
struct OutputData {
    global_name: u32,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceData {
    owner: u8,
}

#[derive(Debug, Clone, Copy)]
struct FrameData {
    owner: u8,
    generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct LayerData {
    owner: u8,
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
    fn validate(self) -> Result<(), ShellHostError> {
        for (present, interface) in [
            (self.compositor, "wl_compositor"),
            (self.shm, "wl_shm"),
            (self.output, "wl_output"),
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
    kind: SurfaceKind,
    package: PathBuf,
    surface: wl_surface::WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
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
    output: Option<wl_output::WlOutput>,
    output_global_name: Option<u32>,
    output_scale: i32,
    seat: Option<wl_seat::WlSeat>,
    seat_global_name: Option<u32>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_focus: Option<SurfaceKind>,
    layer_shell: Option<ZwlrLayerShellV1>,
    layer_shell_version: u32,
    surfaces: Vec<ShellSurfaceState>,
    shared: SharedShellState,
    wayland_connection_us: u64,
    viewporter_advertised: bool,
    fractional_scale_advertised: bool,
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
            SessionOptions::Single(_) => 0,
        };
        Self {
            options,
            started,
            running: true,
            compositor: None,
            shm: None,
            shm_argb8888: false,
            output: None,
            output_global_name: None,
            output_scale: 1,
            seat: None,
            seat_global_name: None,
            pointer: None,
            pointer_focus: None,
            layer_shell: None,
            layer_shell_version: 0,
            surfaces: Vec::new(),
            shared: SharedShellState {
                last_action: "Ready".into(),
                ..SharedShellState::default()
            },
            wayland_connection_us,
            viewporter_advertised: false,
            fractional_scale_advertised: false,
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
        RequiredGlobals {
            compositor: self.compositor.is_some(),
            shm: self.shm.is_some(),
            argb8888: self.shm_argb8888,
            output: self.output.is_some(),
            seat: self.seat.is_some(),
            pointer: self.pointer.is_some(),
            layer_shell: self.layer_shell.is_some(),
        }
        .validate()
    }

    fn start(&mut self, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        self.validate_globals()?;
        match self.options.clone() {
            SessionOptions::Single(options) => {
                self.create_surface(qh, SurfaceKind::SingleOverlay, options.package, true, 0)?
            }
            SessionOptions::Multi(options) => {
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
        }
        Ok(())
    }

    fn create_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        kind: SurfaceKind,
        package: PathBuf,
        desired_mapped: bool,
        panel_height: u32,
    ) -> Result<(), ShellHostError> {
        let compositor = self
            .compositor
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_compositor"))?;
        let output = self
            .output
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_output"))?;
        let layer_shell = self
            .layer_shell
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("zwlr_layer_shell_v1"))?;
        let owner = kind.owner();
        let surface = compositor.create_surface(qh, SurfaceData { owner });
        let (layer, namespace, anchors, width, height, exclusive_zone) = match kind {
            SurfaceKind::Panel => (
                zwlr_layer_shell_v1::Layer::Top,
                PANEL_NAMESPACE,
                zwlr_layer_surface_v1::Anchor::Top
                    | zwlr_layer_surface_v1::Anchor::Left
                    | zwlr_layer_surface_v1::Anchor::Right,
                0,
                panel_height,
                panel_height as i32,
            ),
            SurfaceKind::SingleOverlay => (
                zwlr_layer_shell_v1::Layer::Overlay,
                SINGLE_NAMESPACE,
                zwlr_layer_surface_v1::Anchor::Top
                    | zwlr_layer_surface_v1::Anchor::Bottom
                    | zwlr_layer_surface_v1::Anchor::Left
                    | zwlr_layer_surface_v1::Anchor::Right,
                0,
                0,
                0,
            ),
            SurfaceKind::Overlay => (
                zwlr_layer_shell_v1::Layer::Overlay,
                OVERLAY_NAMESPACE,
                zwlr_layer_surface_v1::Anchor::Top
                    | zwlr_layer_surface_v1::Anchor::Bottom
                    | zwlr_layer_surface_v1::Anchor::Left
                    | zwlr_layer_surface_v1::Anchor::Right,
                0,
                0,
                0,
            ),
        };
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            Some(output),
            layer,
            namespace.into(),
            qh,
            LayerData { owner },
        );
        layer_surface.set_anchor(anchors);
        layer_surface.set_size(width, height);
        layer_surface.set_exclusive_zone(exclusive_zone);
        layer_surface
            .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        let mut lifecycle = LayerLifecycle::default();
        lifecycle
            .assign_role()
            .map_err(|error| ShellHostError::Wayland(error.into()))?;
        surface.commit();
        lifecycle
            .initial_bufferless_commit()
            .map_err(|error| ShellHostError::Wayland(error.into()))?;
        self.surfaces.push(ShellSurfaceState {
            kind,
            package,
            surface,
            layer_surface,
            runtime: None,
            pool: ShmBufferPool::new(owner),
            scheduler: FrameScheduler::default(),
            lifecycle,
            configures: ConfigureCoalescer::default(),
            desired_mapped,
            mapped: false,
            map_state: SurfaceMapState::AwaitingConfigure,
            summary: SurfaceHostSummary::default(),
            maximum_mapped_bytes: 0,
            last_click_count: 0,
        });
        Ok(())
    }

    fn surface_index(&self, kind: SurfaceKind) -> Option<usize> {
        self.surfaces
            .iter()
            .position(|surface| surface.kind == kind)
    }

    fn surface_index_by_owner(&self, owner: u8) -> Option<usize> {
        self.surfaces
            .iter()
            .position(|surface| surface.kind.owner() == owner)
    }

    fn maybe_render_all(&mut self, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        let owners: Vec<u8> = self
            .surfaces
            .iter()
            .map(|surface| surface.kind.owner())
            .collect();
        for owner in owners {
            self.maybe_render(owner, qh)?;
        }
        self.update_combined_memory()?;
        Ok(())
    }

    fn maybe_render(&mut self, owner: u8, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return Ok(());
        };
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
        let surface_state = &mut self.surfaces[index];
        let Some((logical_width, logical_height)) = surface_state.configures.latest() else {
            return Ok(());
        };
        if !surface_state.lifecycle.can_attach_buffer() {
            return Ok(());
        }
        if surface_state.runtime.is_none() {
            surface_state.runtime = Some(LiveDocument::load_surface(
                &surface_state.package,
                surface_state.kind.document_kind(),
                logical_width,
                logical_height,
            )?);
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
        let runtime = surface_state.runtime.as_mut().expect("initialized above");
        if runtime.set_viewport(logical_width, logical_height)? {
            surface_state.scheduler.mark_dirty();
        }
        let size_ready = surface_state
            .pool
            .ensure_size(&shm, qh, logical_width, logical_height)?;
        let free_buffer = size_ready && surface_state.pool.has_free();
        match surface_state.scheduler.decision(true, free_buffer) {
            ScheduleDecision::Idle
            | ScheduleDecision::WaitForFrameCallback
            | ScheduleDecision::WaitForBuffer => return Ok(()),
            ScheduleDecision::Render => {}
        }
        let frame = runtime.render()?;
        let Some((_id, buffer, conversion_us)) = surface_state
            .pool
            .acquire_and_write(&frame.premultiplied_rgba)?
        else {
            surface_state.scheduler.mark_dirty();
            return Ok(());
        };
        update_input_region(&compositor, &surface_state.surface, &frame, qh);
        surface_state.surface.attach(Some(&buffer), 0, 0);
        surface_state.surface.damage(
            0,
            0,
            logical_width.min(i32::MAX as u32) as i32,
            logical_height.min(i32::MAX as u32) as i32,
        );
        surface_state.surface.frame(
            qh,
            FrameData {
                owner,
                generation: frame.generation,
            },
        );
        surface_state.surface.commit();
        surface_state.scheduler.frame_committed();
        surface_state.mapped = true;
        surface_state.map_state.mapped();
        surface_state.configures.mark_presented();
        surface_state.summary.frames_committed =
            surface_state.summary.frames_committed.saturating_add(1);
        surface_state.summary.logical_width = logical_width;
        surface_state.summary.logical_height = logical_height;
        surface_state.summary.buffer_width = frame.buffer_width;
        surface_state.summary.buffer_height = frame.buffer_height;
        surface_state.summary.last_render_us = milliseconds_to_microseconds(frame.render_ms);
        surface_state.summary.last_pixel_conversion_us = conversion_us;
        if surface_state.summary.first_commit_us == 0 {
            surface_state.summary.first_commit_us = elapsed_us(started);
        }
        if let Ok(snapshot) = runtime.snapshot() {
            surface_state.summary.html_parse_count = snapshot.document_parse_count;
            surface_state.last_click_count = snapshot.interaction.click_count;
        }
        surface_state.refresh_pool_summary();
        Ok(())
    }

    fn pointer_move(&mut self, kind: SurfaceKind, x: f64, y: f64) {
        let Some(index) = self.surface_index(kind) else {
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

    fn pointer_button(&mut self, kind: SurfaceKind, pressed: bool) {
        if matches!(
            &self.options,
            SessionOptions::Multi(options) if options.automatic_overlay_cycles > 0
        ) {
            return;
        }
        let Some(index) = self.surface_index(kind) else {
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
                        if let Err(error) = self.handle_action(action) {
                            self.fail(format!("live action rejected: {error}"));
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

    fn handle_action(&mut self, action: LiveAction) -> Result<(), ShellHostError> {
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

    fn open_overlay(&mut self, action: &str) -> Result<(), ShellHostError> {
        if self.shared.overlay_open {
            return Ok(());
        }
        self.shared.overlay_open = true;
        self.shared.last_action = action.into();
        self.overlay_open_count = self.overlay_open_count.saturating_add(1);
        self.overlay_open_started = Some(Instant::now());
        if let Some(index) = self.surface_index(SurfaceKind::Overlay) {
            let configured = self.surfaces[index].configures.latest().is_some();
            self.surfaces[index].desired_mapped = true;
            self.surfaces[index].map_state.request_map(configured);
            if !configured {
                // A null-buffer unmap returns layer shell to its initial state.
                // A new bufferless commit requests the configure required for remap.
                self.surfaces[index].surface.commit();
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
        if self.pointer_focus == Some(SurfaceKind::Overlay) {
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
                surface.surface.attach(None, 0, 0);
                surface.surface.commit();
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

    fn pointer_leave_kind(&mut self, kind: SurfaceKind) {
        if let Some(index) = self.surface_index(kind) {
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
        if let Some(kind) = self.pointer_focus.take() {
            self.pointer_leave_kind(kind);
        }
    }

    fn on_frame_done(&mut self, owner: u8, generation: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
            return;
        };
        let kind = self.surfaces[index].kind;
        self.surfaces[index].scheduler.frame_callback_done();
        self.surfaces[index].summary.frame_callbacks = self.surfaces[index]
            .summary
            .frame_callbacks
            .saturating_add(1);
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
        let _ = generation;
    }

    fn on_buffer_release(&mut self, owner: u8, id: u64) {
        let Some(index) = self.surface_index_by_owner(owner) else {
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
                surface.surface.attach(None, 0, 0);
                surface.surface.commit();
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
            surface.layer_surface.destroy();
            surface.surface.destroy();
        }
        if let Some(pointer) = self.pointer.take() {
            release_pointer(pointer);
        }
        if let Some(seat) = self.seat.take() {
            release_seat(seat);
        }
        if let Some(output) = self.output.take() {
            release_output(output);
        }
        if let Some(shm) = self.shm.take() {
            release_shm(shm);
        }
        if let Some(layer_shell) = self.layer_shell.take()
            && layer_shell.version() >= 3
        {
            layer_shell.destroy();
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
            output_scale: self.output_scale,
            viewporter_advertised: self.viewporter_advertised,
            fractional_scale_advertised: self.fractional_scale_advertised,
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
            output_scale: self.output_scale,
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
}

pub fn run_live_overlay(options: LiveHostOptions) -> Result<LiveHostSummary, ShellHostError> {
    run_session(SessionOptions::Single(options)).map(|state| state.single_summary())
}

pub fn run_multi_surface_shell(
    options: MultiSurfaceHostOptions,
) -> Result<MultiSurfaceHostSummary, ShellHostError> {
    run_session(SessionOptions::Multi(options)).map(|state| state.multi_summary())
}

fn run_session(options: SessionOptions) -> Result<State, ShellHostError> {
    let started = Instant::now();
    let connection = Connection::connect_to_env().map_err(ShellHostError::wayland)?;
    let wayland_connection_us = elapsed_us(started);
    let mut event_queue = connection.new_event_queue();
    let qh = event_queue.handle();
    connection.display().get_registry(&qh, ());
    let mut state = State::new(options, started, wayland_connection_us);
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
        if state.all_released() {
            break;
        }
        event_queue
            .roundtrip(&mut state)
            .map_err(ShellHostError::wayland)?;
    }
    state.destroy_objects();
    connection.flush().map_err(ShellHostError::wayland)?;
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
        _: &Connection,
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
                "wl_output" if state.output.is_none() => {
                    state.output_global_name = Some(name);
                    state.output = Some(registry.bind(
                        name,
                        version.min(4),
                        qh,
                        OutputData { global_name: name },
                    ));
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
                "wp_viewporter" => state.viewporter_advertised = true,
                "wp_fractional_scale_manager_v1" => state.fractional_scale_advertised = true,
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } if state.output_global_name == Some(name) => {
                state.clear_pointer_focus();
                for surface in &mut state.surfaces {
                    surface.lifecycle.output_lost();
                    surface.desired_mapped = false;
                    surface.mapped = false;
                    surface.scheduler.stop_scheduling();
                    surface.map_state.close();
                }
                if let Some(output) = state.output.take() {
                    release_output(output);
                }
                state.output_global_name = None;
                state.running = false;
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
        _: &QueueHandle<Self>,
    ) {
        if data.global_name == state.output_global_name.unwrap_or_default()
            && let wl_output::Event::Scale { factor } = event
            && factor > 0
        {
            state.output_scale = factor;
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
                let kind = state
                    .surfaces
                    .iter()
                    .find(|candidate| candidate.surface.id() == surface.id())
                    .map(|candidate| candidate.kind);
                if let Some(kind) = kind {
                    if state.pointer_focus != Some(kind) {
                        state.clear_pointer_focus();
                        state.pointer_focus = Some(kind);
                    }
                    if let Some(index) = state.surface_index(kind) {
                        state.surfaces[index].summary.pointer_enters = state.surfaces[index]
                            .summary
                            .pointer_enters
                            .saturating_add(1);
                    }
                    state.pointer_move(kind, surface_x, surface_y);
                }
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if let Some(kind) = state.pointer_focus {
                    if let Some(index) = state.surface_index(kind) {
                        state.surfaces[index].summary.pointer_motions = state.surfaces[index]
                            .summary
                            .pointer_motions
                            .saturating_add(1);
                    }
                    state.pointer_move(kind, surface_x, surface_y);
                }
            }
            wl_pointer::Event::Leave { surface, .. } => {
                let leaving = state
                    .surfaces
                    .iter()
                    .find(|candidate| candidate.surface.id() == surface.id())
                    .map(|candidate| candidate.kind);
                if leaving == state.pointer_focus {
                    state.clear_pointer_focus();
                }
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } if button == BTN_LEFT => {
                if let Some(kind) = state.pointer_focus {
                    if let Some(index) = state.surface_index(kind) {
                        state.surfaces[index].summary.pointer_buttons = state.surfaces[index]
                            .summary
                            .pointer_buttons
                            .saturating_add(1);
                    }
                    match button_state {
                        WEnum::Value(wl_pointer::ButtonState::Pressed) => {
                            state.pointer_button(kind, true)
                        }
                        WEnum::Value(wl_pointer::ButtonState::Released) => {
                            state.pointer_button(kind, false)
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
                let surface = &mut state.surfaces[index];
                surface.lifecycle.close();
                surface.desired_mapped = false;
                surface.mapped = false;
                surface.scheduler.stop_scheduling();
                surface.map_state.close();
                state.running = false;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, FrameData> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        data: &FrameData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_callback::Event::Done { .. }) {
            state.on_frame_done(data.owner, data.generation);
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
        coalescer.mark_presented();
        assert_eq!(coalescer.received, 100);
        assert_eq!(coalescer.presented, 1);
        assert_eq!(coalescer.latest(), Some((1099, 700)));
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
        assert!(complete.validate().is_ok());
        assert!(matches!(
            RequiredGlobals {
                layer_shell: false,
                ..complete
            }
            .validate(),
            Err(ShellHostError::MissingGlobal("zwlr_layer_shell_v1"))
        ));
        assert!(matches!(
            RequiredGlobals {
                pointer: false,
                ..complete
            }
            .validate(),
            Err(ShellHostError::MissingPointerCapability)
        ));
    }
}
