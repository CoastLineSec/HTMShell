use crate::ShellHostError;
use crate::buffer::{BufferData, BufferPoolStats, ShmBufferPool};
use crate::lifecycle::LayerLifecycle;
use crate::scheduler::{FrameScheduler, ScheduleDecision};
use htm_runtime::{LiveDocument, LiveFrame};
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

const NAMESPACE: &str = "htmshell";
const LAYER_SHELL_MAX_VERSION: u32 = 5;
// Linux input event code named by the wl_pointer protocol.
const BTN_LEFT: u32 = 0x110;
const SHUTDOWN_ROUNDTRIPS: usize = 4;
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

#[derive(Debug, Clone, Copy)]
struct OutputData {
    global_name: u32,
}

#[derive(Debug, Clone, Copy)]
struct FrameData {
    generation: u64,
}

#[derive(Debug, Clone, Copy)]
struct LayerData;

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

struct State {
    options: LiveHostOptions,
    started: Instant,
    running: bool,
    configured: Option<(u32, u32)>,
    latest_configure_serial: Option<u32>,
    compositor: Option<wl_compositor::WlCompositor>,
    compositor_version: u32,
    shm: Option<wl_shm::WlShm>,
    shm_argb8888: bool,
    output: Option<wl_output::WlOutput>,
    output_global_name: Option<u32>,
    output_scale: i32,
    seat: Option<wl_seat::WlSeat>,
    seat_global_name: Option<u32>,
    pointer: Option<wl_pointer::WlPointer>,
    layer_shell: Option<ZwlrLayerShellV1>,
    layer_shell_version: u32,
    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    runtime: Option<LiveDocument>,
    pool: ShmBufferPool,
    scheduler: FrameScheduler,
    summary: LiveHostSummary,
    last_click_count: u64,
    maximum_mapped_bytes: usize,
    viewporter_advertised: bool,
    fractional_scale_advertised: bool,
    exit_after_commit_count: Option<u64>,
    lifecycle: LayerLifecycle,
}

impl State {
    fn new(options: LiveHostOptions, started: Instant, wayland_connection_us: u64) -> Self {
        let summary = LiveHostSummary {
            wayland_connection_us,
            ..LiveHostSummary::default()
        };
        Self {
            options,
            started,
            running: true,
            configured: None,
            latest_configure_serial: None,
            compositor: None,
            compositor_version: 0,
            shm: None,
            shm_argb8888: false,
            output: None,
            output_global_name: None,
            output_scale: 1,
            seat: None,
            seat_global_name: None,
            pointer: None,
            layer_shell: None,
            layer_shell_version: 0,
            surface: None,
            layer_surface: None,
            runtime: None,
            pool: ShmBufferPool::new(),
            scheduler: FrameScheduler::default(),
            summary,
            last_click_count: 0,
            maximum_mapped_bytes: 0,
            viewporter_advertised: false,
            fractional_scale_advertised: false,
            exit_after_commit_count: None,
            lifecycle: LayerLifecycle::default(),
        }
    }

    fn start(&mut self, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        RequiredGlobals {
            compositor: self.compositor.is_some(),
            shm: self.shm.is_some(),
            argb8888: self.shm_argb8888,
            output: self.output.is_some(),
            seat: self.seat.is_some(),
            pointer: self.pointer.is_some(),
            layer_shell: self.layer_shell.is_some(),
        }
        .validate()?;
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

        let surface = compositor.create_surface(qh, ());
        self.lifecycle
            .assign_role()
            .map_err(|error| ShellHostError::Wayland(error.into()))?;
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            Some(output),
            zwlr_layer_shell_v1::Layer::Overlay,
            NAMESPACE.into(),
            qh,
            LayerData,
        );
        let anchors = zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Bottom
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right;
        layer_surface.set_anchor(anchors);
        layer_surface.set_size(0, 0);
        layer_surface.set_exclusive_zone(0);
        layer_surface
            .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        self.surface = Some(surface.clone());
        self.layer_surface = Some(layer_surface);

        // The layer-shell construction state requires a commit with no buffer.
        surface.commit();
        self.lifecycle
            .initial_bufferless_commit()
            .map_err(|error| ShellHostError::Wayland(error.into()))?;
        Ok(())
    }

    fn maybe_render(&mut self, qh: &QueueHandle<Self>) -> Result<(), ShellHostError> {
        let Some((logical_width, logical_height)) = self.configured else {
            return Ok(());
        };
        if !self.lifecycle.can_attach_buffer() {
            return Ok(());
        }
        let shm = self
            .shm
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_shm"))?;

        if let Some(runtime) = &mut self.runtime {
            if runtime.set_viewport(logical_width, logical_height)? {
                self.scheduler.mark_dirty();
            }
        } else {
            self.runtime = Some(LiveDocument::load(
                &self.options.package,
                logical_width,
                logical_height,
            )?);
            self.scheduler.mark_dirty();
        }

        let size_ready = self
            .pool
            .ensure_size(shm, qh, logical_width, logical_height)?;
        let free_buffer = size_ready && self.pool.has_free();
        match self.scheduler.decision(true, free_buffer) {
            ScheduleDecision::Idle
            | ScheduleDecision::WaitForFrameCallback
            | ScheduleDecision::WaitForBuffer => return Ok(()),
            ScheduleDecision::Render => {}
        }

        let frame = self
            .runtime
            .as_mut()
            .expect("runtime initialized above")
            .render()?;
        let Some((_buffer_id, buffer, conversion_us)) =
            self.pool.acquire_and_write(&frame.premultiplied_rgba)?
        else {
            self.scheduler.mark_dirty();
            return Ok(());
        };
        self.update_input_region(&frame, qh)?;

        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| ShellHostError::Wayland("layer surface disappeared".into()))?;
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(
            0,
            0,
            logical_width.min(i32::MAX as u32) as i32,
            logical_height.min(i32::MAX as u32) as i32,
        );
        surface.frame(
            qh,
            FrameData {
                generation: frame.generation,
            },
        );
        surface.commit();
        self.scheduler.frame_committed();

        self.summary.frames_committed = self.summary.frames_committed.saturating_add(1);
        self.summary.full_damage_commits = self.summary.full_damage_commits.saturating_add(1);
        if self.summary.first_commit_us == 0 {
            self.summary.first_commit_us = elapsed_us(self.started);
        }
        self.summary.last_render_us = (frame.render_ms * 1_000.0)
            .round()
            .clamp(0.0, u64::MAX as f64) as u64;
        self.summary.last_pixel_conversion_us = conversion_us;
        let runtime_snapshot = self
            .runtime
            .as_ref()
            .expect("runtime initialized")
            .snapshot()?;
        self.summary.html_parse_count = runtime_snapshot.document_parse_count;
        self.summary.logical_width = logical_width;
        self.summary.logical_height = logical_height;
        self.summary.buffer_width = frame.buffer_width;
        self.summary.buffer_height = frame.buffer_height;
        let measurements = self
            .runtime
            .as_ref()
            .expect("runtime initialized")
            .measurements();
        self.summary.package_read_us = milliseconds_to_microseconds(measurements.package_read_ms);
        self.summary.html_parse_us = milliseconds_to_microseconds(measurements.html_parse_ms);
        self.summary.initial_resolve_us =
            milliseconds_to_microseconds(measurements.initial_resolve_ms);
        self.summary.last_resolve_us = milliseconds_to_microseconds(measurements.last_resolve_ms);
        self.last_click_count = runtime_snapshot.interaction.click_count;
        self.update_pool_summary();
        Ok(())
    }

    fn update_input_region(
        &self,
        frame: &LiveFrame,
        qh: &QueueHandle<Self>,
    ) -> Result<(), ShellHostError> {
        let compositor = self
            .compositor
            .as_ref()
            .ok_or(ShellHostError::MissingGlobal("wl_compositor"))?;
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| ShellHostError::Wayland("surface disappeared".into()))?;
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
        Ok(())
    }

    fn pointer_move(&mut self, x: f64, y: f64) {
        let Some(runtime) = &mut self.runtime else {
            return;
        };
        match runtime.pointer_move(x, y) {
            Ok(true) => self.scheduler.mark_dirty(),
            Ok(false) => {}
            Err(error) => {
                eprintln!("pointer motion rejected: {error}");
                self.running = false;
            }
        }
    }

    fn pointer_button(&mut self, pressed: bool) {
        let Some(runtime) = &mut self.runtime else {
            return;
        };
        match runtime.pointer_primary(pressed) {
            Ok(true) => {
                self.scheduler.mark_dirty();
                if !pressed
                    && let Ok(snapshot) = runtime.snapshot()
                    && snapshot.interaction.click_count > self.last_click_count
                {
                    self.summary.click_mutations = snapshot.interaction.click_count;
                    self.last_click_count = snapshot.interaction.click_count;
                    if self.options.exit_after_click {
                        self.exit_after_commit_count =
                            Some(self.summary.frames_committed.saturating_add(1));
                    }
                }
            }
            Ok(false) => {}
            Err(error) => {
                eprintln!("pointer button rejected: {error}");
                self.running = false;
            }
        }
    }

    fn pointer_leave(&mut self) {
        if self
            .runtime
            .as_mut()
            .is_some_and(LiveDocument::pointer_leave)
        {
            self.scheduler.mark_dirty();
        }
    }

    fn on_frame_done(&mut self, generation: u64) {
        self.scheduler.frame_callback_done();
        self.summary.frame_callbacks = self.summary.frame_callbacks.saturating_add(1);
        if self.summary.first_frame_callback_us == 0 {
            self.summary.first_frame_callback_us = elapsed_us(self.started);
        }
        if self
            .options
            .exit_after_frames
            .is_some_and(|target| self.summary.frame_callbacks >= target)
            || self
                .exit_after_commit_count
                .is_some_and(|target| self.summary.frame_callbacks >= target)
        {
            self.running = false;
        }
        let _ = generation;
    }

    fn on_buffer_release(&mut self, id: u64) {
        self.pool.release(id);
        self.update_pool_summary();
    }

    fn update_pool_summary(&mut self) {
        let BufferPoolStats {
            allocations,
            reallocations,
            releases,
            skipped_no_free_buffer,
            total_mapped_bytes,
        } = self.pool.stats();
        self.summary.buffers_allocated = allocations;
        self.summary.buffer_reallocations = reallocations;
        self.summary.buffer_releases = releases;
        self.summary.frames_skipped_busy = skipped_no_free_buffer;
        self.maximum_mapped_bytes = self.maximum_mapped_bytes.max(total_mapped_bytes);
        self.summary.maximum_mapped_bytes = self.maximum_mapped_bytes;
    }

    fn begin_shutdown(&mut self) {
        self.pointer_leave();
        if let Some(surface) = &self.surface {
            surface.attach(None, 0, 0);
            surface.commit();
        }
    }

    fn destroy_objects(&mut self) {
        self.pool.destroy_all();
        if let Some(pointer) = self.pointer.take() {
            release_pointer(pointer);
        }
        if let Some(layer_surface) = self.layer_surface.take() {
            layer_surface.destroy();
        }
        if let Some(surface) = self.surface.take() {
            surface.destroy();
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
}

pub fn run_live_overlay(options: LiveHostOptions) -> Result<LiveHostSummary, ShellHostError> {
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
        state.maybe_render(&qh)?;
        connection.flush().map_err(ShellHostError::wayland)?;
    }

    state.begin_shutdown();
    connection.flush().map_err(ShellHostError::wayland)?;
    for _ in 0..SHUTDOWN_ROUNDTRIPS {
        if state.pool.all_released() {
            break;
        }
        event_queue
            .roundtrip(&mut state)
            .map_err(ShellHostError::wayland)?;
    }
    state.destroy_objects();
    connection.flush().map_err(ShellHostError::wayland)?;
    state.summary.layer_shell_version = state.layer_shell_version;
    state.summary.output_scale = state.output_scale;
    state.summary.viewporter_advertised = state.viewporter_advertised;
    state.summary.fractional_scale_advertised = state.fractional_scale_advertised;
    Ok(state.summary)
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
                    let selected = version.min(6);
                    state.compositor_version = selected;
                    state.compositor = Some(registry.bind(name, selected, qh, ()));
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
                state.configured = None;
                state.lifecycle.output_lost();
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
                state.pointer_leave();
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
        if data.global_name != state.output_global_name.unwrap_or_default() {
            return;
        }
        if let wl_output::Event::Scale { factor } = event
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
                state.pointer_leave();
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
                surface_x,
                surface_y,
                ..
            } => {
                state.summary.pointer_enters = state.summary.pointer_enters.saturating_add(1);
                state.pointer_move(surface_x, surface_y);
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                state.summary.pointer_motions = state.summary.pointer_motions.saturating_add(1);
                state.pointer_move(surface_x, surface_y);
            }
            wl_pointer::Event::Leave { .. } => state.pointer_leave(),
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } if button == BTN_LEFT => {
                state.summary.pointer_buttons = state.summary.pointer_buttons.saturating_add(1);
                match button_state {
                    WEnum::Value(wl_pointer::ButtonState::Pressed) => state.pointer_button(true),
                    WEnum::Value(wl_pointer::ButtonState::Released) => state.pointer_button(false),
                    _ => {}
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
        _: &LayerData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                if width == 0 || height == 0 {
                    eprintln!(
                        "layer-shell configure returned a zero dimension; full-output size is required"
                    );
                    state.running = false;
                    return;
                }
                if let Err(error) = state.lifecycle.configure(serial, width, height) {
                    eprintln!("layer-shell configure rejected: {error}");
                    state.running = false;
                    return;
                }
                layer_surface.ack_configure(serial);
                if let Err(error) = state.lifecycle.acknowledge(serial) {
                    eprintln!("layer-shell acknowledgement rejected: {error}");
                    state.running = false;
                    return;
                }
                state.latest_configure_serial = Some(serial);
                state.configured = Some((width, height));
                state.scheduler.mark_dirty();
                if state.summary.first_configure_us == 0 {
                    state.summary.first_configure_us = elapsed_us(state.started);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.configured = None;
                state.lifecycle.close();
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
            state.on_frame_done(data.generation);
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
            state.on_buffer_release(data.id);
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_region::WlRegion);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore ZwlrLayerShellV1);

#[cfg(test)]
mod tests {
    use super::*;
    use htm_runtime::LiveFrame;

    fn frame_with_region(rect: htm_runtime::LiveFrameRect) -> LiveFrame {
        LiveFrame {
            logical_width: 800,
            logical_height: 600,
            buffer_width: 800,
            buffer_height: 600,
            premultiplied_rgba: Vec::new(),
            damage_estimate: htm_runtime::LiveFrameRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            input_regions: vec![rect.clone()],
            interactive_region: rect,
            generation: 1,
            render_ms: 0.0,
        }
    }

    #[test]
    fn input_region_rounds_outward_and_excludes_transparent_area() {
        let frame = frame_with_region(htm_runtime::LiveFrameRect {
            x: 189.25,
            y: 164.5,
            width: 421.2,
            height: 270.2,
        });
        let region = rounded_region(&frame.input_regions[0], 800, 600).unwrap();
        assert_eq!(region, (189, 164, 422, 271));
        assert!(region.2 < 800);
        assert!(region.3 < 600);
    }

    #[test]
    fn invalid_or_empty_input_regions_are_ignored() {
        let invalid = htm_runtime::LiveFrameRect {
            x: f32::NAN,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(rounded_region(&invalid, 800, 600).is_none());
        let empty = htm_runtime::LiveFrameRect {
            x: 2.0,
            y: 2.0,
            width: 0.0,
            height: 10.0,
        };
        assert!(rounded_region(&empty, 800, 600).is_none());
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
        for (incomplete, expected) in [
            (
                RequiredGlobals {
                    compositor: false,
                    ..complete
                },
                "wl_compositor",
            ),
            (
                RequiredGlobals {
                    shm: false,
                    ..complete
                },
                "wl_shm",
            ),
            (
                RequiredGlobals {
                    output: false,
                    ..complete
                },
                "wl_output",
            ),
            (
                RequiredGlobals {
                    seat: false,
                    ..complete
                },
                "wl_seat",
            ),
        ] {
            assert!(matches!(
                incomplete.validate(),
                Err(ShellHostError::MissingGlobal(interface)) if interface == expected
            ));
        }
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
                argb8888: false,
                ..complete
            }
            .validate(),
            Err(ShellHostError::UnsupportedShmFormat)
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
