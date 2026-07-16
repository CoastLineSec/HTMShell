mod model;

use std::{
    env,
    error::Error,
    fs::{self, File},
    io::{Seek, SeekFrom, Write},
    os::fd::AsFd,
    path::PathBuf,
    process,
    time::{Duration, Instant},
};

use htm_compositor_client::protocol::htm_shell_v1::{
    htm_shell_manager_v1::{self, Capability, HtmShellManagerV1, Role},
    htm_shell_root_v1::{self, HtmShellRootV1},
};
use model::{
    CapabilitySet, ConformanceReport, KnownCapability, ResultCategory, TestResult,
    compatible_bind_version, redact_detail,
};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{MemfdFlags, memfd_create},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
};

const CLAIM_ENV: &str = "HTM_SHELL_PROBE_TOKEN";
const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const MAX_REPEAT: u32 = 100;

type ToolResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestGroup {
    All,
    Discovery,
    Authorization,
    Root,
    Input,
    Cleanup,
}

impl TestGroup {
    fn parse(value: &str) -> ToolResult<Self> {
        match value {
            "all" | "baseline" => Ok(Self::All),
            "discovery" => Ok(Self::Discovery),
            "authorization" => Ok(Self::Authorization),
            "root" | "lifecycle" => Ok(Self::Root),
            "input" => Ok(Self::Input),
            "cleanup" => Ok(Self::Cleanup),
            _ => Err(format!("unknown test group: {value}").into()),
        }
    }

    fn includes(self, group: &str) -> bool {
        self == Self::All
            || matches!(
                (self, group),
                (Self::Discovery, "discovery")
                    | (Self::Authorization, "authorization")
                    | (Self::Root, "root")
                    | (Self::Input, "input")
                    | (Self::Cleanup, "cleanup")
            )
    }
}

#[derive(Debug)]
struct Config {
    group: TestGroup,
    timeout: Duration,
    repeat: u32,
    output: Option<PathBuf>,
}

impl Config {
    fn parse() -> ToolResult<Self> {
        let mut group = TestGroup::All;
        let mut timeout = Duration::from_millis(DEFAULT_TIMEOUT_MS);
        let mut repeat = 1;
        let mut output = None;
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--group" => {
                    group = TestGroup::parse(&args.next().ok_or("--group requires a value")?)?;
                }
                "--timeout-ms" => {
                    let value = args.next().ok_or("--timeout-ms requires a value")?;
                    let millis = value.parse::<u64>()?;
                    if millis == 0 || millis > 60_000 {
                        return Err("--timeout-ms must be between 1 and 60000".into());
                    }
                    timeout = Duration::from_millis(millis);
                }
                "--repeat" => {
                    let value = args.next().ok_or("--repeat requires a value")?;
                    repeat = value.parse::<u32>()?;
                    if repeat == 0 || repeat > MAX_REPEAT {
                        return Err(format!("--repeat must be between 1 and {MAX_REPEAT}").into());
                    }
                }
                "--output" => {
                    output = Some(PathBuf::from(
                        args.next().ok_or("--output requires a path")?,
                    ));
                }
                "--help" | "-h" => {
                    println!(
                        "usage: htm-shell-conformance [--group GROUP] [--timeout-ms N] [--repeat N] [--output PATH]"
                    );
                    process::exit(0);
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }

        Ok(Self {
            group,
            timeout,
            repeat,
            output,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct RootData;

#[derive(Clone, Copy, Debug)]
struct BufferData {
    index: usize,
}

#[derive(Clone, Copy, Debug)]
struct FrameData {
    number: u8,
}

struct ShmBuffer {
    proxy: wl_buffer::WlBuffer,
    _file: File,
}

#[derive(Default)]
struct Measurements {
    discovered: Option<Duration>,
    ready: Option<Duration>,
    root_requested: Option<Duration>,
    configured: Option<Duration>,
    first_commit: Option<Duration>,
    first_frame: Option<Duration>,
    second_commit: Option<Duration>,
    second_frame: Option<Duration>,
    releases: [Option<Duration>; 2],
    pointer_complete: Option<Duration>,
    complete: Option<Duration>,
}

struct State {
    started: Instant,
    claim: String,
    capabilities: CapabilitySet,
    manager_version: Option<u32>,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    output: Option<wl_output::WlOutput>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    manager: Option<HtmShellManagerV1>,
    surface: Option<wl_surface::WlSurface>,
    root: Option<HtmShellRootV1>,
    buffers: Vec<ShmBuffer>,
    authorization_sent: bool,
    ready: bool,
    root_requested: bool,
    configured_size: Option<(u32, u32)>,
    configure_acknowledged: bool,
    frames_committed: u8,
    frames_completed: u8,
    released: [bool; 2],
    pointer_enters: u32,
    pointer_motions: u32,
    pointer_buttons: u32,
    pointer_leaves: u32,
    wait_for_pointer: bool,
    cleanup_sent: bool,
    running: bool,
    connection_failed: bool,
    measurements: Measurements,
}

impl State {
    fn new(claim: String, wait_for_pointer: bool) -> Self {
        Self {
            started: Instant::now(),
            claim,
            capabilities: CapabilitySet::default(),
            manager_version: None,
            compositor: None,
            shm: None,
            output: None,
            seat: None,
            pointer: None,
            manager: None,
            surface: None,
            root: None,
            buffers: Vec::new(),
            authorization_sent: false,
            ready: false,
            root_requested: false,
            configured_size: None,
            configure_acknowledged: false,
            frames_committed: 0,
            frames_completed: 0,
            released: [false; 2],
            pointer_enters: 0,
            pointer_motions: 0,
            pointer_buttons: 0,
            pointer_leaves: 0,
            wait_for_pointer,
            cleanup_sent: false,
            running: true,
            connection_failed: false,
            measurements: Measurements::default(),
        }
    }

    fn standard_globals_ready(&self) -> bool {
        self.compositor.is_some()
            && self.shm.is_some()
            && self.output.is_some()
            && self.seat.is_some()
    }

    fn progress(&mut self, qh: &QueueHandle<Self>) -> ToolResult<()> {
        if !self.authorization_sent
            && self.standard_globals_ready()
            && let Some(manager) = &self.manager
        {
            manager.authenticate(self.claim.clone());
            self.authorization_sent = true;
        }

        if self.ready
            && !self.root_requested
            && self.capabilities.contains(KnownCapability::RootOverlay)
        {
            let surface = self
                .compositor
                .as_ref()
                .ok_or("wl_compositor disappeared")?
                .create_surface(qh, ());
            let output = self.output.as_ref().ok_or("wl_output disappeared")?;
            let root = self
                .manager
                .as_ref()
                .ok_or("shell manager disappeared")?
                .get_root(&surface, output, Role::Overlay, qh, RootData);
            self.surface = Some(surface);
            self.root = Some(root);
            self.root_requested = true;
            self.measurements.root_requested = Some(self.started.elapsed());
        }

        if self.configure_acknowledged && self.frames_committed == 0 {
            self.commit_first_frame(qh)?;
        }

        let pointer_complete = self.pointer_enters > 0
            && self.pointer_motions > 0
            && self.pointer_buttons > 0
            && self.pointer_leaves > 0;
        let pointer_ready = !self.wait_for_pointer
            || !self
                .capabilities
                .contains(KnownCapability::StandardPointerFocus)
            || pointer_complete;
        if self.frames_completed >= 2
            && self.released.iter().all(|released| *released)
            && pointer_ready
        {
            self.cleanup();
            self.measurements.complete = Some(self.started.elapsed());
            self.running = false;
        }

        Ok(())
    }

    fn commit_first_frame(&mut self, qh: &QueueHandle<Self>) -> ToolResult<()> {
        let (width, height) = self.configured_size.ok_or("configure dimensions missing")?;
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err("configure dimensions are outside conformance-tool limits".into());
        }
        self.buffers = vec![
            self.create_buffer(qh, width, height, 0)?,
            self.create_buffer(qh, width, height, 1)?,
        ];
        self.commit_buffer(qh, 0, 1);
        Ok(())
    }

    fn create_buffer(
        &self,
        qh: &QueueHandle<Self>,
        width: u32,
        height: u32,
        frame: usize,
    ) -> ToolResult<ShmBuffer> {
        let stride = width.checked_mul(4).ok_or("buffer stride overflow")?;
        let len = stride
            .checked_mul(height)
            .ok_or("buffer allocation overflow")?;
        let fd = memfd_create("htm-shell-conformance", MemfdFlags::CLOEXEC)?;
        let mut file = File::from(fd);
        file.set_len(len.into())?;
        write_pattern(&mut file, width, height, frame)?;
        file.seek(SeekFrom::Start(0))?;

        let pool = self.shm.as_ref().ok_or("wl_shm disappeared")?.create_pool(
            file.as_fd(),
            len as i32,
            qh,
            (),
        );
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            BufferData { index: frame },
        );
        pool.destroy();
        Ok(ShmBuffer {
            proxy: buffer,
            _file: file,
        })
    }

    fn commit_buffer(&mut self, qh: &QueueHandle<Self>, index: usize, frame: u8) {
        let surface = self.surface.as_ref().expect("configured surface exists");
        surface.attach(Some(&self.buffers[index].proxy), 0, 0);
        surface.damage(0, 0, i32::MAX, i32::MAX);
        surface.frame(qh, FrameData { number: frame });
        surface.commit();
        self.frames_committed = frame;
        if frame == 1 {
            self.measurements.first_commit = Some(self.started.elapsed());
        } else if frame == 2 {
            self.measurements.second_commit = Some(self.started.elapsed());
        }
    }

    fn on_frame(&mut self, qh: &QueueHandle<Self>, number: u8) {
        self.frames_completed = self.frames_completed.max(number);
        match number {
            1 => {
                self.measurements.first_frame = Some(self.started.elapsed());
                self.commit_buffer(qh, 1, 2);
            }
            2 => {
                self.measurements.second_frame = Some(self.started.elapsed());
                if let Some(surface) = &self.surface {
                    surface.attach(None, 0, 0);
                    surface.commit();
                }
            }
            _ => {}
        }
    }

    fn on_release(&mut self, index: usize) {
        if index >= self.released.len() {
            return;
        }
        self.released[index] = true;
        self.measurements.releases[index] = Some(self.started.elapsed());
    }

    fn cleanup(&mut self) {
        for buffer in self.buffers.drain(..) {
            buffer.proxy.destroy();
        }
        if let Some(root) = self.root.take() {
            root.destroy();
        }
        if let Some(surface) = self.surface.take() {
            surface.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
        self.cleanup_sent = true;
    }
}

fn write_pattern(file: &mut File, width: u32, height: u32, frame: usize) -> ToolResult<()> {
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let border = x < 6 || y < 6 || x + 6 >= width || y + 6 >= height;
            let (red, green, blue, alpha) = if border {
                (248, 250, 255, 255)
            } else {
                match (frame, x >= width / 2, y >= height / 2) {
                    (0, false, false) => (24, 48, 88, 255),
                    (0, true, false) => (45, 205, 221, 255),
                    (0, false, true) => (206, 78, 171, 255),
                    (0, true, true) => (247, 153, 70, 168),
                    (_, false, false) => (45, 205, 221, 255),
                    (_, true, false) => (24, 48, 88, 255),
                    (_, false, true) => (247, 153, 70, 168),
                    (_, true, true) => (206, 78, 171, 255),
                }
            };
            bytes.extend_from_slice(&[blue, green, red, alpha]);
        }
    }
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
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
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };

        match interface.as_str() {
            "wl_compositor" if state.compositor.is_none() => {
                state.compositor = Some(registry.bind(name, version.min(6), qh, ()))
            }
            "wl_shm" if state.shm.is_none() => {
                state.shm = Some(registry.bind(name, version.min(1), qh, ()))
            }
            "wl_output" if state.output.is_none() => {
                state.output = Some(registry.bind(name, version.min(4), qh, ()))
            }
            "wl_seat" if state.seat.is_none() => {
                state.seat = Some(registry.bind(name, version.min(9), qh, ()))
            }
            "htm_shell_manager_v1" if state.manager.is_none() => {
                state.manager_version = Some(version);
                if let Some(bind_version) = compatible_bind_version(version) {
                    state.manager = Some(registry.bind(name, bind_version, qh, ()));
                    state.measurements.discovered = Some(state.started.elapsed());
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<HtmShellManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &HtmShellManagerV1,
        event: htm_shell_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            htm_shell_manager_v1::Event::Capability { capability } => match capability {
                WEnum::Value(Capability::RootOverlay) => state.capabilities.insert(1),
                WEnum::Value(Capability::StandardPointerFocus) => state.capabilities.insert(2),
                WEnum::Unknown(value) => state.capabilities.insert(value),
                _ => {}
            },
            htm_shell_manager_v1::Event::Ready => {
                state.ready = true;
                state.measurements.ready = Some(state.started.elapsed());
            }
            _ => {}
        }
    }
}

impl Dispatch<HtmShellRootV1, RootData> for State {
    fn event(
        state: &mut Self,
        root: &HtmShellRootV1,
        event: htm_shell_root_v1::Event,
        _: &RootData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let htm_shell_root_v1::Event::Configure {
            serial,
            logical_width,
            logical_height,
        } = event
        {
            state.configured_size = Some((logical_width, logical_height));
            root.ack_configure(serial);
            state.configure_acknowledged = true;
            state.measurements.configured = Some(state.started.elapsed());
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
        qh: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_callback::Event::Done { .. }) {
            state.on_frame(qh, data.number);
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
            state.on_release(data.index);
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
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
            && capabilities.contains(wl_seat::Capability::Pointer)
            && state.pointer.is_none()
        {
            state.pointer = Some(seat.get_pointer(qh, ()));
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
            wl_pointer::Event::Enter { .. } => state.pointer_enters += 1,
            wl_pointer::Event::Motion { .. } => state.pointer_motions += 1,
            wl_pointer::Event::Button { .. } => state.pointer_buttons += 1,
            wl_pointer::Event::Leave { .. } => state.pointer_leaves += 1,
            _ => {}
        }
        if state.pointer_enters > 0
            && state.pointer_motions > 0
            && state.pointer_buttons > 0
            && state.pointer_leaves > 0
            && state.measurements.pointer_complete.is_none()
        {
            state.measurements.pointer_complete = Some(state.started.elapsed());
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);

struct CycleOutcome {
    state: State,
    timed_out: bool,
    connection_closed: bool,
}

fn run_cycle(claim: String, timeout: Duration, wait_for_pointer: bool) -> ToolResult<CycleOutcome> {
    let connection = Connection::connect_to_env()?;
    let mut event_queue = connection.new_event_queue();
    let qh = event_queue.handle();
    connection.display().get_registry(&qh, ());
    connection.flush()?;

    let mut state = State::new(claim, wait_for_pointer);
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;

    while state.running {
        if let Err(_error) = event_queue.dispatch_pending(&mut state) {
            state.connection_failed = true;
            break;
        }
        state.progress(&qh)?;
        if !state.running {
            break;
        }

        let now = Instant::now();
        if now >= deadline {
            timed_out = true;
            break;
        }

        connection.flush()?;
        let Some(guard) = event_queue.prepare_read() else {
            continue;
        };
        let remaining = deadline.saturating_duration_since(now);
        let timeout_spec = Timespec {
            tv_sec: remaining.as_secs() as i64,
            tv_nsec: remaining.subsec_nanos() as i64,
        };
        let mut poll_fd = PollFd::from_borrowed_fd(guard.connection_fd(), PollFlags::IN);
        let ready = poll(std::slice::from_mut(&mut poll_fd), Some(&timeout_spec))?;
        if ready == 0 {
            drop(guard);
            timed_out = true;
            break;
        }
        if poll_fd
            .revents()
            .intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
        {
            drop(guard);
            state.connection_failed = true;
            break;
        }
        guard.read()?;
    }

    if !state.cleanup_sent && !state.connection_failed {
        state.cleanup();
    }
    let flush_ok = connection.flush().is_ok();
    drop(event_queue);
    drop(connection);

    Ok(CycleOutcome {
        state,
        timed_out,
        connection_closed: flush_ok,
    })
}

fn outcome_tests(
    outcome: &CycleOutcome,
    selected_group: TestGroup,
    cycle: u32,
    repeated: bool,
) -> Vec<TestResult> {
    let state = &outcome.state;
    let mut tests = Vec::new();
    let prefix = if repeated {
        format!("cycle.{cycle:03}.")
    } else {
        String::new()
    };

    let mut add =
        |name: &str, group: &str, required: bool, result: ResultCategory, detail: Option<&str>| {
            if selected_group.includes(group) {
                tests.push(TestResult::new(
                    format!("{prefix}{name}"),
                    group,
                    required,
                    result,
                    detail.map(str::to_owned),
                ));
            }
        };

    add(
        "discovery.manager_global",
        "discovery",
        true,
        category(state.manager_version.is_some(), outcome.timed_out),
        (!state.manager_version.is_some()).then_some("manager global was not observed"),
    );
    add(
        "discovery.interface_version",
        "discovery",
        true,
        category(
            state.manager_version.is_some_and(|version| version >= 1),
            outcome.timed_out,
        ),
        (!state.manager_version.is_some_and(|version| version >= 1))
            .then_some("interface version 1 was not advertised"),
    );
    add(
        "discovery.standard_globals",
        "discovery",
        true,
        category(state.standard_globals_ready(), outcome.timed_out),
        (!state.standard_globals_ready())
            .then_some("one or more baseline Wayland globals were absent"),
    );
    add(
        "authorization.controller_claim",
        "authorization",
        true,
        category(state.ready, outcome.timed_out),
        (!state.ready).then_some("controller authority was not granted"),
    );
    add(
        "capability.root_overlay",
        "authorization",
        true,
        bool_category(state.capabilities.contains(KnownCapability::RootOverlay)),
        (!state.capabilities.contains(KnownCapability::RootOverlay))
            .then_some("mandatory overlay-root capability was not advertised"),
    );
    add(
        "capability.standard_pointer_focus",
        "authorization",
        true,
        bool_category(
            state
                .capabilities
                .contains(KnownCapability::StandardPointerFocus),
        ),
        (!state
            .capabilities
            .contains(KnownCapability::StandardPointerFocus))
        .then_some("mandatory standard-pointer capability was not advertised"),
    );
    let unknown_capabilities = state.capabilities.unknown_values();
    add(
        "capability.unknown_values_contained",
        "authorization",
        false,
        ResultCategory::Pass,
        (!unknown_capabilities.is_empty()).then_some("unknown optional values were ignored"),
    );
    add(
        "root.role_requested",
        "root",
        true,
        category(state.root_requested, outcome.timed_out),
        None,
    );
    add(
        "root.configure_received",
        "root",
        true,
        category(state.configured_size.is_some(), outcome.timed_out),
        None,
    );
    add(
        "root.configure_acknowledged",
        "root",
        true,
        category(state.configure_acknowledged, outcome.timed_out),
        None,
    );
    add(
        "root.first_buffer_committed",
        "root",
        true,
        category(state.frames_committed >= 1, outcome.timed_out),
        None,
    );
    add(
        "root.first_frame_callback",
        "root",
        true,
        category(state.frames_completed >= 1, outcome.timed_out),
        None,
    );
    add(
        "root.first_buffer_release",
        "root",
        true,
        category(state.released[0], outcome.timed_out),
        None,
    );
    add(
        "root.second_buffer_committed",
        "root",
        true,
        category(state.frames_committed >= 2, outcome.timed_out),
        None,
    );
    add(
        "root.second_frame_callback",
        "root",
        true,
        category(state.frames_completed >= 2, outcome.timed_out),
        None,
    );
    add(
        "root.second_buffer_release",
        "root",
        true,
        category(state.released[1], outcome.timed_out),
        None,
    );

    let pointer_advertised = state
        .capabilities
        .contains(KnownCapability::StandardPointerFocus);
    let pointer_observed = state.pointer_enters > 0
        && state.pointer_motions > 0
        && state.pointer_buttons > 0
        && state.pointer_leaves > 0;
    let pointer_result = if !pointer_advertised {
        ResultCategory::Fail
    } else if pointer_observed {
        ResultCategory::Pass
    } else if outcome.timed_out {
        ResultCategory::Timeout
    } else {
        ResultCategory::Inconclusive
    };
    add(
        "input.standard_pointer_delivery",
        "input",
        true,
        pointer_result,
        (!pointer_advertised)
            .then_some("reference host does not integrate roots into normal hit testing"),
    );
    add(
        "cleanup.destroy_requests",
        "cleanup",
        true,
        bool_category(state.cleanup_sent),
        None,
    );
    add(
        "cleanup.connection_closed",
        "cleanup",
        true,
        bool_category(outcome.connection_closed && !state.connection_failed),
        state
            .connection_failed
            .then_some("Wayland connection ended before orderly cleanup"),
    );

    tests
}

fn category(observed: bool, timed_out: bool) -> ResultCategory {
    if observed {
        ResultCategory::Pass
    } else if timed_out {
        ResultCategory::Timeout
    } else {
        ResultCategory::Fail
    }
}

fn bool_category(observed: bool) -> ResultCategory {
    if observed {
        ResultCategory::Pass
    } else {
        ResultCategory::Fail
    }
}

fn print_measurements(cycle: u32, state: &State) {
    let measurements = &state.measurements;
    eprintln!("cycle={cycle}");
    print_duration("discovery.manager_global", measurements.discovered);
    print_duration("authorization.controller_claim", measurements.ready);
    print_duration("root.role_requested", measurements.root_requested);
    print_duration("root.configure_received", measurements.configured);
    print_duration("root.first_buffer_committed", measurements.first_commit);
    print_duration("root.first_frame_callback", measurements.first_frame);
    print_duration("root.first_buffer_release", measurements.releases[0]);
    print_duration("root.second_buffer_committed", measurements.second_commit);
    print_duration("root.second_frame_callback", measurements.second_frame);
    print_duration("root.second_buffer_release", measurements.releases[1]);
    print_duration(
        "input.standard_pointer_delivery",
        measurements.pointer_complete,
    );
    print_duration("cleanup.destroy_requests", measurements.complete);
}

fn print_duration(name: &str, value: Option<Duration>) {
    match value {
        Some(duration) => eprintln!("observed_us {name}={}", duration.as_micros()),
        None => eprintln!("observed_us {name}=unavailable"),
    }
}

fn process_high_water_rss_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn execute() -> ToolResult<i32> {
    let config = Config::parse()?;
    let claim = env::var(CLAIM_ENV).map_err(|_| format!("{CLAIM_ENV} is not set"))?;
    if claim.len() < 32 || claim.len() > 256 {
        return Err(format!("{CLAIM_ENV} must contain 32 to 256 bytes").into());
    }

    let mut tests = Vec::new();
    for cycle in 1..=config.repeat {
        let wait_for_pointer = matches!(config.group, TestGroup::All | TestGroup::Input);
        let outcome = run_cycle(claim.clone(), config.timeout, wait_for_pointer)?;
        print_measurements(cycle, &outcome.state);
        tests.extend(outcome_tests(
            &outcome,
            config.group,
            cycle,
            config.repeat > 1,
        ));
    }

    let report = ConformanceReport::new(tests);
    let json = report.to_pretty_json()?;
    if let Some(path) = &config.output {
        fs::write(path, &json)?;
        eprintln!("report={}", path.display());
    } else {
        print!("{json}");
    }
    eprintln!("result={:?}", report.result);
    match process_high_water_rss_kib() {
        Some(value) => eprintln!("process_max_rss_kib={value}"),
        None => eprintln!("process_max_rss_kib=unavailable"),
    }

    Ok(if report.result == ResultCategory::Pass {
        0
    } else {
        1
    })
}

fn main() {
    match execute() {
        Ok(code) => process::exit(code),
        Err(error) => {
            let message = error.to_string();
            let redacted = env::var(CLAIM_ENV)
                .map(|claim| redact_detail(&message, &claim))
                .unwrap_or(message);
            eprintln!("htm-shell-conformance: {redacted}");
            process::exit(2);
        }
    }
}
