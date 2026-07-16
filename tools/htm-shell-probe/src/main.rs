use std::{
    env,
    error::Error,
    fs::File,
    io::{Seek, SeekFrom, Write},
    os::fd::AsFd,
    time::{Duration, Instant},
};

use htm_compositor_client::protocol::htm_shell_v1::{
    htm_shell_manager_v1::{self, Capability, HtmShellManagerV1, Role},
    htm_shell_root_v1::{self, HtmShellRootV1},
};
use rustix::fs::{MemfdFlags, memfd_create};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop, event_created_child,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::{
    ext::workspace::v1::client::{
        ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
        ext_workspace_handle_v1::{
            self, ExtWorkspaceHandleV1, State as WorkspaceState, WorkspaceCapabilities,
        },
        ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
    },
    wp::{
        fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
        viewporter::client::{wp_viewport, wp_viewporter},
    },
    xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
};

const TOKEN_ENV: &str = "HTM_SHELL_PROBE_TOKEN";

type ProbeResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Run,
    Unauthorized,
    RequestBeforeAuth,
    DuplicateAuth,
    DuplicateRoot,
    InvalidRole,
    SurfaceWithRole,
    BufferBeforeAck,
    SurfaceFirst,
    Workspace,
    Abrupt,
    Stall,
}

impl Mode {
    fn parse() -> ProbeResult<Self> {
        let arg = env::args().nth(1).unwrap_or_else(|| "run".into());
        let mode = match arg.as_str() {
            "run" => Self::Run,
            "unauthorized" => Self::Unauthorized,
            "request-before-auth" => Self::RequestBeforeAuth,
            "duplicate-auth" => Self::DuplicateAuth,
            "duplicate-root" => Self::DuplicateRoot,
            "invalid-role" => Self::InvalidRole,
            "surface-with-role" => Self::SurfaceWithRole,
            "buffer-before-ack" => Self::BufferBeforeAck,
            "surface-first" => Self::SurfaceFirst,
            "workspace" => Self::Workspace,
            "abrupt" => Self::Abrupt,
            "stall" => Self::Stall,
            _ => return Err(format!("unknown probe mode: {arg}").into()),
        };
        Ok(mode)
    }

    fn expects_protocol_error(self) -> bool {
        matches!(
            self,
            Self::Unauthorized
                | Self::RequestBeforeAuth
                | Self::DuplicateAuth
                | Self::DuplicateRoot
                | Self::InvalidRole
                | Self::SurfaceWithRole
                | Self::BufferBeforeAck
        )
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

struct WorkspaceRecord {
    proxy: ExtWorkspaceHandleV1,
    id: Option<String>,
    name: Option<String>,
    active: bool,
    can_activate: bool,
    removed: bool,
}

#[derive(Default)]
struct Timings {
    protocol_discovered: Option<Duration>,
    authorized: Option<Duration>,
    root_created: Option<Duration>,
    first_frame: Option<Duration>,
    second_frame: Option<Duration>,
    first_release: Option<Duration>,
    second_release: Option<Duration>,
}

struct State {
    mode: Mode,
    started: Instant,
    token: String,
    running: bool,
    ready: bool,
    overlay_capability: bool,
    pointer_capability: bool,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    output: Option<wl_output::WlOutput>,
    output_name: Option<String>,
    output_scale: i32,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    manager: Option<HtmShellManagerV1>,
    surface: Option<wl_surface::WlSurface>,
    root: Option<HtmShellRootV1>,
    viewport: Option<wp_viewport::WpViewport>,
    fractional_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    fractional_manager: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    viewporter: Option<wp_viewporter::WpViewporter>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    workspace_manager: Option<ExtWorkspaceManagerV1>,
    workspace_groups: Vec<ExtWorkspaceGroupHandleV1>,
    workspaces: Vec<WorkspaceRecord>,
    workspace_done_events: u32,
    workspace_initial_active: Option<u32>,
    workspace_change_requested: bool,
    workspace_update_seen: bool,
    configured_size: Option<(u32, u32)>,
    preferred_scale_120: u32,
    buffers: Vec<ShmBuffer>,
    released: [bool; 2],
    committed_frames: u8,
    completed_frames: u8,
    pointer_enters: u32,
    pointer_motions: u32,
    pointer_buttons: u32,
    timings: Timings,
}

impl State {
    fn new(mode: Mode, token: String) -> Self {
        Self {
            mode,
            started: Instant::now(),
            token,
            running: true,
            ready: false,
            overlay_capability: false,
            pointer_capability: false,
            compositor: None,
            shm: None,
            output: None,
            output_name: None,
            output_scale: 1,
            seat: None,
            pointer: None,
            manager: None,
            surface: None,
            root: None,
            viewport: None,
            fractional_scale: None,
            fractional_manager: None,
            viewporter: None,
            wm_base: None,
            workspace_manager: None,
            workspace_groups: Vec::new(),
            workspaces: Vec::new(),
            workspace_done_events: 0,
            workspace_initial_active: None,
            workspace_change_requested: false,
            workspace_update_seen: false,
            configured_size: None,
            preferred_scale_120: 120,
            buffers: Vec::new(),
            released: [false; 2],
            committed_frames: 0,
            completed_frames: 0,
            pointer_enters: 0,
            pointer_motions: 0,
            pointer_buttons: 0,
            timings: Timings::default(),
        }
    }

    fn require_globals(&self) -> ProbeResult<()> {
        if self.compositor.is_none() {
            return Err("wl_compositor is unavailable".into());
        }
        if self.shm.is_none() {
            return Err("wl_shm is unavailable".into());
        }
        if self.output.is_none() {
            return Err("wl_output is unavailable".into());
        }
        if self.manager.is_none() {
            return Err("htm_shell_manager_v1 is unavailable".into());
        }
        if self.mode == Mode::Workspace && self.workspace_manager.is_none() {
            return Err("ext_workspace_manager_v1 is unavailable".into());
        }
        Ok(())
    }

    fn begin(&mut self, qh: &QueueHandle<Self>) -> ProbeResult<()> {
        self.require_globals()?;
        match self.mode {
            Mode::Unauthorized => self
                .manager
                .as_ref()
                .expect("manager checked")
                .authenticate("deliberately-invalid-probe-token".into()),
            Mode::RequestBeforeAuth => self.create_root(qh, Role::Overlay, false)?,
            Mode::DuplicateAuth => {
                let manager = self.manager.as_ref().expect("manager checked");
                manager.authenticate(self.token.clone());
                manager.authenticate(self.token.clone());
            }
            _ => self
                .manager
                .as_ref()
                .expect("manager checked")
                .authenticate(self.token.clone()),
        }
        Ok(())
    }

    fn on_ready(&mut self, qh: &QueueHandle<Self>) -> ProbeResult<()> {
        self.ready = true;
        self.timings.authorized = Some(self.started.elapsed());
        if !self.overlay_capability {
            return Err("host did not advertise root_overlay".into());
        }

        match self.mode {
            Mode::InvalidRole => self.create_unknown_role(qh)?,
            Mode::SurfaceWithRole => self.create_surface_with_xdg_role(qh)?,
            _ => {
                self.create_root(qh, Role::Overlay, false)?;
                if self.mode == Mode::DuplicateRoot {
                    self.create_root(qh, Role::Overlay, true)?;
                }
            }
        }
        Ok(())
    }

    fn new_surface(&mut self, qh: &QueueHandle<Self>) -> ProbeResult<wl_surface::WlSurface> {
        if let Some(surface) = &self.surface {
            return Ok(surface.clone());
        }

        let surface = self
            .compositor
            .as_ref()
            .ok_or("wl_compositor disappeared")?
            .create_surface(qh, ());

        if let Some(manager) = &self.fractional_manager {
            self.fractional_scale = Some(manager.get_fractional_scale(&surface, qh, ()));
        }
        if let Some(viewporter) = &self.viewporter {
            self.viewport = Some(viewporter.get_viewport(&surface, qh, ()));
        }

        self.surface = Some(surface.clone());
        Ok(surface)
    }

    fn create_root(
        &mut self,
        qh: &QueueHandle<Self>,
        role: Role,
        duplicate: bool,
    ) -> ProbeResult<()> {
        let surface = self.new_surface(qh)?;
        let output = self.output.as_ref().ok_or("wl_output disappeared")?.clone();
        let manager = self.manager.as_ref().ok_or("manager disappeared")?.clone();
        let root = manager.get_root(&surface, &output, role, qh, RootData);
        if !duplicate {
            self.root = Some(root);
            self.timings.root_created = Some(self.started.elapsed());
        }
        Ok(())
    }

    fn create_unknown_role(&mut self, qh: &QueueHandle<Self>) -> ProbeResult<()> {
        let surface = self.new_surface(qh)?;
        let output = self.output.as_ref().ok_or("wl_output disappeared")?.clone();
        let manager = self.manager.as_ref().ok_or("manager disappeared")?.clone();
        let request = htm_shell_manager_v1::Request::GetRoot {
            surface,
            output,
            role: WEnum::Unknown(0xffff_fffe),
        };
        let _: HtmShellRootV1 =
            manager.send_constructor(request, qh.make_data::<HtmShellRootV1, _>(RootData))?;
        Ok(())
    }

    fn create_surface_with_xdg_role(&mut self, qh: &QueueHandle<Self>) -> ProbeResult<()> {
        let surface = self.new_surface(qh)?;
        let wm_base = self
            .wm_base
            .as_ref()
            .ok_or("xdg_wm_base is unavailable for surface-with-role test")?;
        let xdg_surface = wm_base.get_xdg_surface(&surface, qh, ());
        let _toplevel = xdg_surface.get_toplevel(qh, ());
        self.create_root(qh, Role::Overlay, false)
    }

    fn maybe_commit_first(&mut self, qh: &QueueHandle<Self>) -> ProbeResult<()> {
        if self.committed_frames != 0 {
            return Ok(());
        }
        let Some((logical_width, logical_height)) = self.configured_size else {
            return Ok(());
        };

        let scale_120 = self.preferred_scale_120.max(120);
        let pixel_width = logical_width.saturating_mul(scale_120).div_ceil(120);
        let pixel_height = logical_height.saturating_mul(scale_120).div_ceil(120);
        if pixel_width == 0 || pixel_height == 0 || pixel_width > 4096 || pixel_height > 4096 {
            return Err("configured buffer dimensions are outside probe limits".into());
        }

        self.buffers = vec![
            self.create_buffer(qh, pixel_width, pixel_height, 0)?,
            self.create_buffer(qh, pixel_width, pixel_height, 1)?,
        ];

        if let Some(viewport) = &self.viewport {
            viewport.set_destination(logical_width as i32, logical_height as i32);
        } else {
            let integer_scale = self.output_scale.max(1);
            self.surface
                .as_ref()
                .expect("surface configured")
                .set_buffer_scale(integer_scale);
        }

        self.commit_buffer(qh, 0, 1);
        Ok(())
    }

    fn create_buffer(
        &self,
        qh: &QueueHandle<Self>,
        width: u32,
        height: u32,
        frame: usize,
    ) -> ProbeResult<ShmBuffer> {
        let stride = width.checked_mul(4).ok_or("buffer stride overflow")?;
        let len = stride
            .checked_mul(height)
            .ok_or("buffer allocation overflow")?;
        let fd = memfd_create("htm-shell-probe", MemfdFlags::CLOEXEC)?;
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
        let surface = self.surface.as_ref().expect("surface configured");
        surface.attach(Some(&self.buffers[index].proxy), 0, 0);
        surface.damage(0, 0, i32::MAX, i32::MAX);
        surface.frame(qh, FrameData { number: frame });
        surface.commit();
        self.committed_frames = frame;
    }

    fn on_frame(&mut self, qh: &QueueHandle<Self>, number: u8) {
        self.completed_frames = self.completed_frames.max(number);
        match number {
            1 => {
                self.timings.first_frame = Some(self.started.elapsed());
                self.commit_buffer(qh, 1, 2);
            }
            2 => {
                self.timings.second_frame = Some(self.started.elapsed());
                if self.mode == Mode::Abrupt {
                    std::process::exit(0);
                }
                if self.mode == Mode::Stall {
                    std::thread::sleep(Duration::from_secs(30));
                    std::process::exit(0);
                }
                let surface = self.surface.as_ref().expect("surface configured");
                surface.attach(None, 0, 0);
                surface.commit();
            }
            _ => {}
        }
        self.maybe_finish();
    }

    fn on_release(&mut self, index: usize) {
        if index < self.released.len() {
            self.released[index] = true;
            let elapsed = self.started.elapsed();
            if self.timings.first_release.is_none() {
                self.timings.first_release = Some(elapsed);
            } else if self.timings.second_release.is_none() {
                self.timings.second_release = Some(elapsed);
            }
        }
        self.maybe_finish();
    }

    fn maybe_finish(&mut self) {
        let workspace_complete = self.mode != Mode::Workspace || self.workspace_update_seen;
        if self.completed_frames >= 2
            && self.released.iter().all(|released| *released)
            && workspace_complete
        {
            for buffer in &self.buffers {
                buffer.proxy.destroy();
            }
            if let Some(root) = self.root.take() {
                root.destroy();
            }
            if let Some(scale) = self.fractional_scale.take() {
                scale.destroy();
            }
            if let Some(viewport) = self.viewport.take() {
                viewport.destroy();
            }
            if let Some(surface) = self.surface.take() {
                surface.destroy();
            }
            if let Some(manager) = self.manager.take() {
                manager.destroy();
            }
            for workspace in &self.workspaces {
                workspace.proxy.destroy();
            }
            for group in &self.workspace_groups {
                group.destroy();
            }
            if let Some(manager) = self.workspace_manager.take() {
                manager.stop();
            }
            self.running = false;
        }
    }

    fn on_workspace_done(&mut self) {
        self.workspace_done_events += 1;
        if self.mode != Mode::Workspace {
            return;
        }

        let active = self
            .workspaces
            .iter()
            .find(|workspace| workspace.active && !workspace.removed)
            .map(|workspace| workspace.proxy.id().protocol_id());

        if !self.workspace_change_requested {
            self.workspace_initial_active = active;
            if let Some(target) = self
                .workspaces
                .iter()
                .find(|workspace| !workspace.active && workspace.can_activate && !workspace.removed)
            {
                target.proxy.activate();
                if let Some(manager) = &self.workspace_manager {
                    manager.commit();
                }
                self.workspace_change_requested = true;
            }
        } else if active.is_some() && active != self.workspace_initial_active {
            self.workspace_update_seen = true;
            self.maybe_finish();
        }
    }

    fn print_summary(&self) {
        println!("probe_result=success");
        println!("mode={:?}", self.mode);
        println!(
            "output={}",
            self.output_name.as_deref().unwrap_or("unknown")
        );
        println!("output_integer_scale={}", self.output_scale);
        println!("preferred_scale_120={}", self.preferred_scale_120);
        println!("overlay_capability={}", self.overlay_capability);
        println!(
            "standard_pointer_focus_capability={}",
            self.pointer_capability
        );
        println!("frames_committed={}", self.committed_frames);
        println!("frames_completed={}", self.completed_frames);
        println!(
            "buffers_released={}",
            self.released.iter().filter(|v| **v).count()
        );
        println!("pointer_enters={}", self.pointer_enters);
        println!("pointer_motions={}", self.pointer_motions);
        println!("pointer_buttons={}", self.pointer_buttons);
        println!(
            "workspace_count={}",
            self.workspaces
                .iter()
                .filter(|workspace| !workspace.removed)
                .count()
        );
        println!("workspace_done_events={}", self.workspace_done_events);
        println!(
            "workspace_change_requested={}",
            self.workspace_change_requested
        );
        println!("workspace_update_seen={}", self.workspace_update_seen);
        let mut workspace_labels = self
            .workspaces
            .iter()
            .filter(|workspace| !workspace.removed)
            .map(|workspace| {
                format!(
                    "{}:{}:{}",
                    workspace.id.as_deref().unwrap_or("no-id"),
                    workspace.name.as_deref().unwrap_or("no-name"),
                    if workspace.active {
                        "active"
                    } else {
                        "inactive"
                    }
                )
            })
            .collect::<Vec<_>>();
        workspace_labels.sort();
        println!("workspaces={}", workspace_labels.join(","));
        print_timing("protocol_discovered_us", self.timings.protocol_discovered);
        print_timing("authorized_us", self.timings.authorized);
        print_timing("root_created_us", self.timings.root_created);
        print_timing("first_frame_us", self.timings.first_frame);
        print_timing("second_frame_us", self.timings.second_frame);
        print_timing("first_release_us", self.timings.first_release);
        print_timing("second_release_us", self.timings.second_release);
    }
}

fn print_timing(name: &str, timing: Option<Duration>) {
    match timing {
        Some(value) => println!("{name}={}", value.as_micros()),
        None => println!("{name}=unavailable"),
    }
}

fn write_pattern(file: &mut File, width: u32, height: u32, frame: usize) -> ProbeResult<()> {
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let border = x < 6 || y < 6 || x + 6 >= width || y + 6 >= height;
            let (r, g, b, a) = if border {
                (245, 247, 255, 255)
            } else {
                let right = x >= width / 2;
                let bottom = y >= height / 2;
                match (frame, right, bottom) {
                    (0, false, false) => (22, 42, 78, 255),
                    (0, true, false) => (38, 198, 218, 255),
                    (0, false, true) => (201, 72, 164, 255),
                    (0, true, true) => (245, 148, 62, 168),
                    (_, false, false) => (38, 198, 218, 255),
                    (_, true, false) => (22, 42, 78, 255),
                    (_, false, true) => (245, 148, 62, 168),
                    (_, true, true) => (201, 72, 164, 255),
                }
            };
            bytes.extend_from_slice(&[b, g, r, a]);
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
                state.manager = Some(registry.bind(name, 1, qh, ()));
                state.timings.protocol_discovered = Some(state.started.elapsed());
            }
            "wp_fractional_scale_manager_v1" if state.fractional_manager.is_none() => {
                state.fractional_manager = Some(registry.bind(name, 1, qh, ()))
            }
            "wp_viewporter" if state.viewporter.is_none() => {
                state.viewporter = Some(registry.bind(name, 1, qh, ()))
            }
            "xdg_wm_base" if state.wm_base.is_none() => {
                state.wm_base = Some(registry.bind(name, version.min(6), qh, ()))
            }
            "ext_workspace_manager_v1" if state.workspace_manager.is_none() => {
                state.workspace_manager = Some(registry.bind(name, 1, qh, ()))
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
        qh: &QueueHandle<Self>,
    ) {
        match event {
            htm_shell_manager_v1::Event::Capability { capability } => match capability {
                WEnum::Value(Capability::RootOverlay) => state.overlay_capability = true,
                WEnum::Value(Capability::StandardPointerFocus) => state.pointer_capability = true,
                WEnum::Unknown(_) => {}
                _ => {}
            },
            htm_shell_manager_v1::Event::Ready => {
                if let Err(error) = state.on_ready(qh) {
                    eprintln!("probe setup failed: {error}");
                    state.running = false;
                }
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
            if state.mode != Mode::BufferBeforeAck {
                root.ack_configure(serial);
            }
            if state.mode == Mode::SurfaceFirst {
                state.configured_size = None;
                if let Some(surface) = state.surface.take() {
                    surface.destroy();
                }
                state.root.take();
                if let Some(manager) = state.manager.take() {
                    manager.destroy();
                }
                state.running = false;
            }
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

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Name { name } => state.output_name = Some(name),
            wl_output::Event::Scale { factor } if factor > 0 => state.output_scale = factor,
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
            _ => {}
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.preferred_scale_120 = scale.max(120);
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.workspace_groups.push(workspace_group);
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                state.workspaces.push(WorkspaceRecord {
                    proxy: workspace,
                    id: None,
                    name: None,
                    active: false,
                    can_activate: false,
                    removed: false,
                });
            }
            ext_workspace_manager_v1::Event::Done => state.on_workspace_done(),
            ext_workspace_manager_v1::Event::Finished => {}
            _ => {}
        }
    }

    event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtWorkspaceGroupHandleV1,
        _: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(record) = state
            .workspaces
            .iter_mut()
            .find(|record| record.proxy == *proxy)
        else {
            return;
        };

        match event {
            ext_workspace_handle_v1::Event::Id { id } => record.id = Some(id),
            ext_workspace_handle_v1::Event::Name { name } => record.name = Some(name),
            ext_workspace_handle_v1::Event::State {
                state: workspace_state,
            } => {
                record.active = matches!(workspace_state, WEnum::Value(value) if value.contains(WorkspaceState::Active));
            }
            ext_workspace_handle_v1::Event::Capabilities { capabilities } => {
                record.can_activate = matches!(capabilities, WEnum::Value(value) if value.contains(WorkspaceCapabilities::Activate));
            }
            ext_workspace_handle_v1::Event::Removed => record.removed = true,
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(State: ignore wp_viewporter::WpViewporter);
delegate_noop!(State: ignore wp_viewport::WpViewport);
delegate_noop!(State: ignore xdg_surface::XdgSurface);
delegate_noop!(State: ignore xdg_toplevel::XdgToplevel);

fn main() -> ProbeResult<()> {
    let mode = Mode::parse()?;
    let token = env::var(TOKEN_ENV).map_err(|_| format!("{TOKEN_ENV} is not set"))?;
    if token.len() < 32 || token.len() > 256 {
        return Err(format!("{TOKEN_ENV} must contain 32 to 256 bytes").into());
    }

    let connection = Connection::connect_to_env()?;
    let mut event_queue = connection.new_event_queue();
    let qh = event_queue.handle();
    connection.display().get_registry(&qh, ());

    let mut state = State::new(mode, token);
    event_queue.roundtrip(&mut state)?;
    state.begin(&qh)?;
    connection.flush()?;

    loop {
        if !state.running {
            break;
        }
        match event_queue.blocking_dispatch(&mut state) {
            Ok(_) => state.maybe_commit_first(&qh)?,
            Err(error) if mode.expects_protocol_error() => {
                println!("probe_result=expected_protocol_rejection");
                println!("mode={mode:?}");
                println!("wayland_error={error}");
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
    }

    connection.flush()?;
    state.print_summary();
    Ok(())
}
