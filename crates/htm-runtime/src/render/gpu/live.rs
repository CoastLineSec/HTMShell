use super::partial::{
    DAMAGE_TILE_GUARD, DAMAGE_TILE_SIZE, DamageRenderDecision, LiveGpuFrameMode,
    bounded_wayland_damage, select_damage_work,
};
use super::{
    BackendError, BackendErrorKind, DeviceGeneration, GpuCoverage, GpuPreparedScene, RenderTarget,
    VelloOffscreenRenderer, VelloScenePainter, validate_plan, validate_target,
};
use crate::LiveGpuPreparedFrame;
use crate::render::{
    DamageRegion, PhysicalDamageRect, PixelFormat, RenderSurfaceId, SceneRevision,
};
use kurbo::Affine;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fmt;
use std::ptr::NonNull;
use std::time::Instant;
use vello::wgpu;
use wgpu::rwh::{HasDisplayHandle, RawDisplayHandle, RawWindowHandle};

const MAX_LIVE_BACKING_BYTES: u64 = 256 * 1024 * 1024;
const MAX_LIVE_SCRATCH_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LIVE_GPU_ERROR_MESSAGE_BYTES: usize = 1_024;

const CONVERSION_SHADER_LINEAR: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let value = textureLoad(source, vec2<i32>(position.xy), 0);
    return vec4<f32>(value.rgb * value.a, value.a);
}
"#;

const CONVERSION_SHADER_SRGB: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;

fn srgb_decode(value: vec3<f32>) -> vec3<f32> {
    let low = value / vec3<f32>(12.92);
    let high = pow((value + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(low, high, value > vec3<f32>(0.04045));
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let value = textureLoad(source, vec2<i32>(position.xy), 0);
    let encoded_premultiplied = value.rgb * value.a;
    return vec4<f32>(srgb_decode(encoded_premultiplied), value.a);
}
"#;

const CONVERSION_SHADER_LINEAR_STRAIGHT: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}
"#;

const CONVERSION_SHADER_SRGB_STRAIGHT: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;

fn srgb_decode(value: vec3<f32>) -> vec3<f32> {
    let low = value / vec3<f32>(12.92);
    let high = pow((value + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(low, high, value > vec3<f32>(0.04045));
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let value = textureLoad(source, vec2<i32>(position.xy), 0);
    return vec4<f32>(srgb_decode(value.rgb), value.a);
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveGpuErrorKind {
    BackendUnavailable,
    SurfaceCreation,
    UnsupportedFormat,
    UnsupportedAlpha,
    InvalidConfiguration,
    StaleGeneration,
    ResourcePreparation,
    Render,
    SurfaceTimeout,
    SurfaceOccluded,
    SurfaceOutdated,
    SurfaceLost,
    DeviceLost,
    OutOfMemory,
    Validation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGpuError {
    pub kind: LiveGpuErrorKind,
    pub message: String,
    pub recoverable: bool,
}

impl LiveGpuError {
    fn new(kind: LiveGpuErrorKind, message: impl Into<String>, recoverable: bool) -> Self {
        let mut message = message.into();
        if message.len() > MAX_LIVE_GPU_ERROR_MESSAGE_BYTES {
            let mut boundary = MAX_LIVE_GPU_ERROR_MESSAGE_BYTES;
            while !message.is_char_boundary(boundary) {
                boundary -= 1;
            }
            message.truncate(boundary);
        }
        Self {
            kind,
            message,
            recoverable,
        }
    }

    /// Creates a bounded presentation-boundary diagnostic.
    ///
    /// This remains an internal feature-gated renderer API and never enters
    /// document state.
    pub fn host(kind: LiveGpuErrorKind, message: impl Into<String>, recoverable: bool) -> Self {
        Self::new(kind, message, recoverable)
    }
}

impl fmt::Display for LiveGpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for LiveGpuError {}

impl From<BackendError> for LiveGpuError {
    fn from(error: BackendError) -> Self {
        let kind = match error.kind {
            BackendErrorKind::BackendUnavailable | BackendErrorKind::AdapterUnavailable => {
                LiveGpuErrorKind::BackendUnavailable
            }
            BackendErrorKind::DeviceCreation | BackendErrorKind::DeviceLost => {
                LiveGpuErrorKind::DeviceLost
            }
            BackendErrorKind::UnsupportedCapability | BackendErrorKind::FallbackRequired => {
                LiveGpuErrorKind::ResourcePreparation
            }
            BackendErrorKind::InvalidPlan => LiveGpuErrorKind::Validation,
            BackendErrorKind::TargetAllocation => LiveGpuErrorKind::OutOfMemory,
            BackendErrorKind::ResourcePreparation => LiveGpuErrorKind::ResourcePreparation,
            BackendErrorKind::Timeout => LiveGpuErrorKind::SurfaceTimeout,
            BackendErrorKind::StaleGeneration => LiveGpuErrorKind::StaleGeneration,
            BackendErrorKind::PipelineCreation
            | BackendErrorKind::CommandEncoding
            | BackendErrorKind::Submission
            | BackendErrorKind::Render
            | BackendErrorKind::Readback
            | BackendErrorKind::BackendReset => LiveGpuErrorKind::Render,
        };
        Self::new(kind, error.message, error.recoverable)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LiveWaylandHandle {
    display: NonNull<c_void>,
    surface: NonNull<c_void>,
}

impl LiveWaylandHandle {
    /// Creates the raw handle pair used only by the live presentation adapter.
    ///
    /// # Safety
    ///
    /// Both pointers must identify live libwayland objects on the same
    /// connection. They must remain valid until the matching GPU presentation
    /// target is released.
    pub unsafe fn new(display: *mut c_void, surface: *mut c_void) -> Result<Self, LiveGpuError> {
        let display = NonNull::new(display).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::SurfaceCreation,
                "Wayland display pointer is null",
                false,
            )
        })?;
        let surface = NonNull::new(surface).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::SurfaceCreation,
                "Wayland surface pointer is null",
                false,
            )
        })?;
        Ok(Self { display, surface })
    }
}

#[derive(Debug, Clone)]
struct OwnedWaylandDisplay(NonNull<c_void>);

// SAFETY: This wrapper never performs Wayland protocol operations. It only
// keeps a stable libwayland display handle alive for wgpu/driver
// initialization. The host retains the connection and serializes its own
// protocol requests on the existing event-loop thread until the GPU backend is
// dropped.
unsafe impl Send for OwnedWaylandDisplay {}
// SAFETY: Same invariant as `Send`; the pointer value is immutable and the
// libwayland connection outlives every wgpu object created from it.
unsafe impl Sync for OwnedWaylandDisplay {}

impl HasDisplayHandle for OwnedWaylandDisplay {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        let raw = RawDisplayHandle::Wayland(wgpu::rwh::WaylandDisplayHandle::new(self.0));
        // SAFETY: The returned borrow is tied to this owned wrapper, whose
        // pointer validity is guaranteed by `LiveWaylandHandle::new`.
        Ok(unsafe { wgpu::rwh::DisplayHandle::borrow_raw(raw) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGpuBackendInfo {
    pub adapter: String,
    pub graphics_api: String,
    pub device_type: String,
    pub driver: String,
    pub device_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveGpuConfiguration {
    pub format: String,
    pub present_mode: String,
    pub alpha_mode: String,
    pub width: u32,
    pub height: u32,
    pub desired_maximum_frame_latency: u32,
    pub generation: u64,
    pub srgb: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveGpuStatistics {
    pub presenter_creations: u64,
    pub presenter_releases: u64,
    pub configurations: u64,
    pub reconfigurations: u64,
    pub frames_planned: u64,
    pub frames_rendered: u64,
    pub frames_submitted: u64,
    pub frames_presented: u64,
    pub acquisitions: u64,
    pub acquisition_failures: u64,
    pub conversion_passes: u64,
    pub full_target_renders: u64,
    pub partial_renders: u64,
    pub cpu_fallback_frames: u64,
    pub logical_damage_rectangles: u64,
    pub physical_damage_rectangles: u64,
    pub selected_tiles: u64,
    pub tile_pixels_rendered: u64,
    pub full_target_pixels_rendered: u64,
    pub backing_pixels_updated: u64,
    pub surface_pixels_converted: u64,
    pub wayland_damage_rectangles: u64,
    pub wayland_damaged_pixels: u64,
    pub full_wayland_damage_frames: u64,
    pub narrow_wayland_damage_frames: u64,
    pub partial_eligibility_rejections: u64,
    pub tile_threshold_fallbacks: u64,
    pub guard_band_pixels: u64,
    pub scratch_reuses: u64,
    pub scratch_replacements: u64,
    pub backing_creations: u64,
    pub backing_replacements: u64,
    pub backing_releases: u64,
    pub partial_transaction_failures: u64,
    pub surface_losses: u64,
    pub surface_timeouts: u64,
    pub surface_outdated: u64,
    pub surface_occluded: u64,
    pub device_losses: u64,
    pub target_recreations: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ConversionProfile {
    RgbaLinear,
    BgraLinear,
    RgbaSrgb,
    BgraSrgb,
    RgbaLinearStraight,
    BgraLinearStraight,
    RgbaSrgbStraight,
    BgraSrgbStraight,
}

impl ConversionProfile {
    fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::RgbaLinear | Self::RgbaLinearStraight => wgpu::TextureFormat::Rgba8Unorm,
            Self::BgraLinear | Self::BgraLinearStraight => wgpu::TextureFormat::Bgra8Unorm,
            Self::RgbaSrgb | Self::RgbaSrgbStraight => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::BgraSrgb | Self::BgraSrgbStraight => wgpu::TextureFormat::Bgra8UnormSrgb,
        }
    }

    fn srgb(self) -> bool {
        matches!(
            self,
            Self::RgbaSrgb | Self::BgraSrgb | Self::RgbaSrgbStraight | Self::BgraSrgbStraight
        )
    }

    fn straight(self) -> bool {
        matches!(
            self,
            Self::RgbaLinearStraight
                | Self::BgraLinearStraight
                | Self::RgbaSrgbStraight
                | Self::BgraSrgbStraight
        )
    }

    fn with_straight_alpha(self) -> Self {
        match self {
            Self::RgbaLinear | Self::RgbaLinearStraight => Self::RgbaLinearStraight,
            Self::BgraLinear | Self::BgraLinearStraight => Self::BgraLinearStraight,
            Self::RgbaSrgb | Self::RgbaSrgbStraight => Self::RgbaSrgbStraight,
            Self::BgraSrgb | Self::BgraSrgbStraight => Self::BgraSrgbStraight,
        }
    }
}

struct BackingImage {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

struct ScratchTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct PersistentBacking {
    current: BackingImage,
    transaction: BackingImage,
    scratch: Vec<ScratchTarget>,
    initialized: bool,
    revision: Option<SceneRevision>,
    force_full_repaint: bool,
}

struct LiveTarget {
    handle: LiveWaylandHandle,
    surface: wgpu::Surface<'static>,
    configuration: Option<wgpu::SurfaceConfiguration>,
    public_configuration: Option<LiveGpuConfiguration>,
    profile: Option<ConversionProfile>,
    backing: Option<PersistentBacking>,
    configuration_generation: u64,
    acquired: bool,
}

pub struct PendingLiveGpuFrame {
    surface: RenderSurfaceId,
    scene_revision: SceneRevision,
    texture: wgpu::SurfaceTexture,
    suboptimal: bool,
    rendered_micros: u64,
    mode: LiveGpuFrameMode,
    logical_damage_rectangles: usize,
    physical_damage: Vec<PhysicalDamageRect>,
    selected_tiles: usize,
    rasterized_pixels: u64,
    backing_updated_pixels: u64,
    surface_converted_pixels: u64,
}

impl PendingLiveGpuFrame {
    pub fn surface(&self) -> RenderSurfaceId {
        self.surface
    }

    pub fn scene_revision(&self) -> SceneRevision {
        self.scene_revision
    }

    pub fn suboptimal(&self) -> bool {
        self.suboptimal
    }

    pub fn rendered_micros(&self) -> u64 {
        self.rendered_micros
    }

    pub fn partial(&self) -> bool {
        self.mode == LiveGpuFrameMode::Partial
    }

    pub fn physical_damage_rects(&self) -> Vec<[u32; 4]> {
        self.physical_damage
            .iter()
            .map(|rect| [rect.x, rect.y, rect.width, rect.height])
            .collect()
    }

    pub fn logical_damage_rectangles(&self) -> usize {
        self.logical_damage_rectangles
    }

    pub fn physical_damaged_pixels(&self) -> u64 {
        self.physical_damage.iter().fold(0u64, |total, rect| {
            total.saturating_add(u64::from(rect.width) * u64::from(rect.height))
        })
    }

    pub fn selected_tiles(&self) -> usize {
        self.selected_tiles
    }

    pub fn rasterized_pixels(&self) -> u64 {
        self.rasterized_pixels
    }

    pub fn backing_updated_pixels(&self) -> u64 {
        self.backing_updated_pixels
    }

    pub fn surface_converted_pixels(&self) -> u64 {
        self.surface_converted_pixels
    }
}

pub struct LiveGpuPresenter {
    backend: VelloOffscreenRenderer,
    targets: BTreeMap<RenderSurfaceId, LiveTarget>,
    bind_group_layout: wgpu::BindGroupLayout,
    pipelines: BTreeMap<ConversionProfile, wgpu::RenderPipeline>,
    statistics: LiveGpuStatistics,
    display: NonNull<c_void>,
}

impl LiveGpuPresenter {
    /// Initializes the one process-level presenter and registers its first
    /// Wayland surface.
    ///
    /// # Safety
    ///
    /// The handle invariants documented by `LiveWaylandHandle::new` must hold
    /// until `release_surface` and presenter drop have completed.
    pub unsafe fn new(
        first_surface: RenderSurfaceId,
        handle: LiveWaylandHandle,
    ) -> Result<Self, LiveGpuError> {
        // SAFETY: forwarded unchanged to `new_with_generation`.
        unsafe { Self::new_with_generation(first_surface, handle, 1) }
    }

    /// Initializes a replacement process backend with an explicit generation.
    ///
    /// # Safety
    ///
    /// The handle invariants documented by `LiveWaylandHandle::new` must hold
    /// until `release_surface` and presenter drop have completed.
    pub unsafe fn new_with_generation(
        first_surface: RenderSurfaceId,
        handle: LiveWaylandHandle,
        device_generation: u64,
    ) -> Result<Self, LiveGpuError> {
        if device_generation == 0 {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "live GPU device generation must be nonzero",
                false,
            ));
        }
        let descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            OwnedWaylandDisplay(handle.display),
        ));
        let instance = wgpu::Instance::new(descriptor);
        // SAFETY: The host owns both libwayland objects and guarantees their
        // lifetime through presenter teardown. The returned surface does not
        // own or destroy either object.
        let surface = unsafe { create_surface(&instance, handle)? };
        let mut backend =
            VelloOffscreenRenderer::new_with_instance(instance, false, Some(&surface))?;
        backend.device_generation = DeviceGeneration(device_generation);
        let bind_group_layout =
            backend
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("HTMShell live output conversion source"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    }],
                });
        let mut targets = BTreeMap::new();
        targets.insert(first_surface, new_live_target(handle, surface));
        Ok(Self {
            backend,
            targets,
            bind_group_layout,
            pipelines: BTreeMap::new(),
            statistics: LiveGpuStatistics {
                presenter_creations: 1,
                ..LiveGpuStatistics::default()
            },
            display: handle.display,
        })
    }

    /// Registers another layer-shell surface on the existing device.
    ///
    /// # Safety
    ///
    /// The handle must remain valid until the target is released and must use
    /// the same display as the first target.
    pub unsafe fn create_surface(
        &mut self,
        surface: RenderSurfaceId,
        handle: LiveWaylandHandle,
    ) -> Result<(), LiveGpuError> {
        if handle.display != self.display {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::SurfaceCreation,
                "live GPU surfaces must share the process Wayland display",
                false,
            ));
        }
        if self.targets.contains_key(&surface) {
            return Ok(());
        }
        // SAFETY: The caller supplies a live surface on the retained display
        // and releases it only after `release_surface`.
        let created = unsafe { create_surface(&self.backend.instance, handle)? };
        let capabilities = created.get_capabilities(&self.backend.adapter);
        if capabilities.formats.is_empty() {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::SurfaceCreation,
                "Wayland surface is incompatible with the selected adapter",
                true,
            ));
        }
        self.targets
            .insert(surface, new_live_target(handle, created));
        self.statistics.presenter_creations = self.statistics.presenter_creations.saturating_add(1);
        Ok(())
    }

    pub fn configure(
        &mut self,
        surface: RenderSurfaceId,
        width: u32,
        height: u32,
    ) -> Result<LiveGpuConfiguration, LiveGpuError> {
        let target_descriptor = RenderTarget {
            width,
            height,
            pixel_format: PixelFormat::PremultipliedRgba8,
        };
        validate_target(target_descriptor, &self.backend.info)?;
        let paired_backing_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(8))
            .ok_or_else(|| {
                LiveGpuError::new(
                    LiveGpuErrorKind::OutOfMemory,
                    "live GPU backing byte size overflow",
                    false,
                )
            })?;
        if paired_backing_bytes > MAX_LIVE_BACKING_BYTES {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::OutOfMemory,
                "live GPU persistent backing transaction exceeds 256 MiB",
                true,
            ));
        }
        let target = self.targets.get_mut(&surface).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::StaleGeneration,
                "cannot configure a stale live GPU surface",
                true,
            )
        })?;
        if target.acquired {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "cannot reconfigure a live GPU surface with an acquired texture",
                true,
            ));
        }
        let capabilities = target.surface.get_capabilities(&self.backend.adapter);
        let mut profile = choose_profile(&capabilities.formats).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::UnsupportedFormat,
                "Wayland surface has no supported RGBA or BGRA unorm format",
                true,
            )
        })?;
        let present_mode = if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "Wayland surface does not expose FIFO presentation",
                true,
            ));
        };
        let (alpha_mode, straight_alpha) =
            choose_alpha_mode(&capabilities.alpha_modes).ok_or_else(|| {
                LiveGpuError::new(
                LiveGpuErrorKind::UnsupportedAlpha,
                    format!(
                        "Wayland surface has no transparency-compatible alpha mode; available: {:?}",
                        capabilities.alpha_modes
                    ),
                true,
                )
            })?;
        if straight_alpha {
            profile = profile.with_straight_alpha();
        }
        let configuration = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: profile.format(),
            width,
            height,
            desired_maximum_frame_latency: 2,
            present_mode,
            alpha_mode,
            view_formats: vec![],
        };
        let replacement = target.configuration.is_some();
        target
            .surface
            .configure(&self.backend.device, &configuration);
        target.configuration_generation = target
            .configuration_generation
            .checked_add(1)
            .ok_or_else(|| {
                LiveGpuError::new(
                    LiveGpuErrorKind::InvalidConfiguration,
                    "GPU surface configuration generation exhausted",
                    false,
                )
            })?;
        let current = create_backing_image(
            &self.backend.device,
            &self.bind_group_layout,
            width,
            height,
            "HTMShell live persistent backing",
        );
        let transaction = create_backing_image(
            &self.backend.device,
            &self.bind_group_layout,
            width,
            height,
            "HTMShell live backing transaction",
        );
        let backing = PersistentBacking {
            current,
            transaction,
            scratch: Vec::new(),
            initialized: false,
            revision: None,
            force_full_repaint: true,
        };
        let public = LiveGpuConfiguration {
            format: format!("{:?}", configuration.format),
            present_mode: format!("{:?}", configuration.present_mode),
            alpha_mode: format!("{:?}", configuration.alpha_mode),
            width,
            height,
            desired_maximum_frame_latency: configuration.desired_maximum_frame_latency,
            generation: target.configuration_generation,
            srgb: profile.srgb(),
        };
        target.configuration = Some(configuration);
        target.public_configuration = Some(public.clone());
        target.profile = Some(profile);
        let replaced_backings = usize::from(target.backing.is_some()) * 2;
        destroy_target_images(target);
        target.backing = Some(backing);
        self.statistics.backing_creations = self.statistics.backing_creations.saturating_add(2);
        if replaced_backings > 0 {
            self.statistics.backing_replacements = self
                .statistics
                .backing_replacements
                .saturating_add(u64::try_from(replaced_backings).unwrap_or(u64::MAX));
            self.statistics.backing_releases = self
                .statistics
                .backing_releases
                .saturating_add(u64::try_from(replaced_backings).unwrap_or(u64::MAX));
        }
        if replacement {
            self.statistics.reconfigurations = self.statistics.reconfigurations.saturating_add(1);
            self.statistics.target_recreations =
                self.statistics.target_recreations.saturating_add(1);
        } else {
            self.statistics.configurations = self.statistics.configurations.saturating_add(1);
        }
        Ok(public)
    }

    pub fn configuration(&self, surface: RenderSurfaceId) -> Option<&LiveGpuConfiguration> {
        self.targets
            .get(&surface)
            .and_then(|target| target.public_configuration.as_ref())
    }

    pub fn backend_info(&self) -> LiveGpuBackendInfo {
        LiveGpuBackendInfo {
            adapter: self.backend.info.name.clone(),
            graphics_api: self.backend.info.backend.clone(),
            device_type: self.backend.info.device_type.clone(),
            driver: self.backend.info.driver.clone(),
            device_generation: self.backend.device_generation.0,
        }
    }

    pub fn statistics(&self) -> LiveGpuStatistics {
        self.statistics
    }

    pub fn record_wayland_damage(&mut self, rectangles: usize, pixels: u64, full: bool) {
        self.statistics.wayland_damage_rectangles = self
            .statistics
            .wayland_damage_rectangles
            .saturating_add(u64::try_from(rectangles).unwrap_or(u64::MAX));
        self.statistics.wayland_damaged_pixels = self
            .statistics
            .wayland_damaged_pixels
            .saturating_add(pixels);
        if full {
            self.statistics.full_wayland_damage_frames =
                self.statistics.full_wayland_damage_frames.saturating_add(1);
        } else {
            self.statistics.narrow_wayland_damage_frames = self
                .statistics
                .narrow_wayland_damage_frames
                .saturating_add(1);
        }
    }

    pub fn record_cpu_fallback(&mut self) {
        self.statistics.cpu_fallback_frames = self.statistics.cpu_fallback_frames.saturating_add(1);
    }

    pub fn resource_statistics(&self) -> (usize, u64, u64, u64) {
        let (entries, bytes) = self.backend.cache_usage();
        let statistics = self.backend.statistics();
        (
            entries,
            bytes,
            statistics.resource_uploads,
            statistics.cache_hits,
        )
    }

    pub fn render(
        &mut self,
        frame: &LiveGpuPreparedFrame,
    ) -> Result<PendingLiveGpuFrame, LiveGpuError> {
        let started = Instant::now();
        let plan = frame.plan();
        validate_plan(plan)?;
        self.statistics.frames_planned = self.statistics.frames_planned.saturating_add(1);
        if matches!(plan.damage, DamageRegion::Empty) {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::Validation,
                "an empty-damage frame cannot be presented",
                false,
            ));
        }
        if VelloOffscreenRenderer::coverage(plan)? == GpuCoverage::CpuFrameFallback {
            let eligibility = LiveGpuFrameMode::CpuFallback;
            debug_assert_eq!(eligibility, LiveGpuFrameMode::CpuFallback);
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::ResourcePreparation,
                "live scene coverage requires complete CPU fallback",
                true,
            ));
        }
        let gpu_prepared = GpuPreparedScene::from_cpu(
            plan.document,
            frame.prepared().prepared.clone(),
            plan.scene.live_resources(),
        );
        use crate::render::Renderer;
        self.backend.prepare(plan, gpu_prepared)?;
        let prepared = self
            .backend
            .prepared
            .get(&(plan.document, plan.scene_revision))
            .ok_or_else(|| {
                LiveGpuError::new(
                    LiveGpuErrorKind::ResourcePreparation,
                    "live Vello recording is unavailable",
                    true,
                )
            })?
            .clone();
        let profile = self
            .targets
            .get(&plan.surface)
            .and_then(|target| target.profile)
            .ok_or_else(|| {
                LiveGpuError::new(
                    LiveGpuErrorKind::InvalidConfiguration,
                    "live GPU surface is not configured",
                    true,
                )
            })?;
        let pipeline = self.pipeline(profile)?;
        let target = self.targets.get_mut(&plan.surface).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::StaleGeneration,
                "live frame targets a stale GPU surface",
                true,
            )
        })?;
        let configuration = target.configuration.as_ref().ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "live GPU surface is not configured",
                true,
            )
        })?;
        if configuration.width != plan.physical_width
            || configuration.height != plan.physical_height
        {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "live GPU target dimensions do not match the frame plan",
                true,
            ));
        }
        if target.acquired {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::Validation,
                "a live surface already has an acquired texture",
                false,
            ));
        }
        let backing = target.backing.as_mut().ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "live GPU persistent backing is missing",
                true,
            )
        })?;
        let decision = select_damage_work(plan, backing.initialized, backing.force_full_repaint);
        let mode = decision.mode();
        if mode == LiveGpuFrameMode::NoFrame {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::Validation,
                "an unchanged live GPU frame cannot be presented",
                false,
            ));
        }
        let physical_damage = bounded_wayland_damage(
            decision.physical_damage(),
            configuration.width,
            configuration.height,
        );
        if physical_damage.is_empty() {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::Validation,
                "live GPU frame contains incomplete physical damage",
                false,
            ));
        }
        let update =
            update_persistent_backing(&mut self.backend, backing, &prepared, plan, &decision);
        let update = match update {
            Ok(update) => update,
            Err(error) => {
                if mode == LiveGpuFrameMode::Partial {
                    self.statistics.partial_transaction_failures = self
                        .statistics
                        .partial_transaction_failures
                        .saturating_add(1);
                    backing.force_full_repaint = true;
                }
                return Err(error);
            }
        };
        let current = target.surface.get_current_texture();
        let (texture, suboptimal) = match current {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.statistics.acquisition_failures =
                    self.statistics.acquisition_failures.saturating_add(1);
                self.statistics.surface_timeouts =
                    self.statistics.surface_timeouts.saturating_add(1);
                return Err(LiveGpuError::new(
                    LiveGpuErrorKind::SurfaceTimeout,
                    "Wayland surface texture acquisition timed out",
                    true,
                ));
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.statistics.acquisition_failures =
                    self.statistics.acquisition_failures.saturating_add(1);
                self.statistics.surface_occluded =
                    self.statistics.surface_occluded.saturating_add(1);
                return Err(LiveGpuError::new(
                    LiveGpuErrorKind::SurfaceOccluded,
                    "Wayland surface is occluded",
                    true,
                ));
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.statistics.acquisition_failures =
                    self.statistics.acquisition_failures.saturating_add(1);
                self.statistics.surface_outdated =
                    self.statistics.surface_outdated.saturating_add(1);
                return Err(LiveGpuError::new(
                    LiveGpuErrorKind::SurfaceOutdated,
                    "Wayland surface configuration is outdated",
                    true,
                ));
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.statistics.acquisition_failures =
                    self.statistics.acquisition_failures.saturating_add(1);
                self.statistics.surface_losses = self.statistics.surface_losses.saturating_add(1);
                return Err(LiveGpuError::new(
                    LiveGpuErrorKind::SurfaceLost,
                    "Wayland GPU surface was lost",
                    true,
                ));
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.statistics.acquisition_failures =
                    self.statistics.acquisition_failures.saturating_add(1);
                return Err(LiveGpuError::new(
                    LiveGpuErrorKind::Validation,
                    "Wayland surface acquisition failed validation",
                    false,
                ));
            }
        };
        let backing = target
            .backing
            .as_mut()
            .expect("validated persistent backing");
        // Surface acquisition can fail after the backing update. Retaining the
        // prior revision until acquisition succeeds makes the same immutable
        // frame replay idempotently instead of accepting an unpresented image.
        backing.force_full_repaint = false;
        backing.initialized = true;
        backing.revision = Some(plan.scene_revision);
        target.acquired = true;
        self.statistics.acquisitions = self.statistics.acquisitions.saturating_add(1);
        let bind_group = &target
            .backing
            .as_ref()
            .expect("configured persistent backing")
            .current
            .bind_group;
        let destination = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.backend
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("HTMShell live output conversion encoder"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("HTMShell live output conversion pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.backend.queue.submit([encoder.finish()]);
        self.statistics.frames_rendered = self.statistics.frames_rendered.saturating_add(1);
        self.statistics.frames_submitted = self.statistics.frames_submitted.saturating_add(1);
        self.statistics.conversion_passes = self.statistics.conversion_passes.saturating_add(1);
        let physical_damage_count = u64::try_from(physical_damage.len()).unwrap_or(u64::MAX);
        self.statistics.logical_damage_rectangles = self
            .statistics
            .logical_damage_rectangles
            .saturating_add(match &plan.damage {
                DamageRegion::Empty => 0,
                DamageRegion::Rects(rects) => u64::try_from(rects.len()).unwrap_or(u64::MAX),
                DamageRegion::Full => 1,
            });
        self.statistics.physical_damage_rectangles = self
            .statistics
            .physical_damage_rectangles
            .saturating_add(physical_damage_count);
        self.statistics.surface_pixels_converted = self
            .statistics
            .surface_pixels_converted
            .saturating_add(update.surface_pixels);
        match mode {
            LiveGpuFrameMode::NoFrame | LiveGpuFrameMode::CpuFallback => {
                return Err(LiveGpuError::new(
                    LiveGpuErrorKind::Validation,
                    "invalid live GPU eligibility reached frame accounting",
                    false,
                ));
            }
            LiveGpuFrameMode::Partial => {
                self.statistics.partial_renders = self.statistics.partial_renders.saturating_add(1);
                self.statistics.selected_tiles = self
                    .statistics
                    .selected_tiles
                    .saturating_add(u64::try_from(update.selected_tiles).unwrap_or(u64::MAX));
                self.statistics.tile_pixels_rendered = self
                    .statistics
                    .tile_pixels_rendered
                    .saturating_add(update.rasterized_pixels);
                self.statistics.guard_band_pixels = self
                    .statistics
                    .guard_band_pixels
                    .saturating_add(update.guard_pixels);
                self.statistics.scratch_reuses = self
                    .statistics
                    .scratch_reuses
                    .saturating_add(u64::try_from(update.scratch_reused).unwrap_or(u64::MAX));
                self.statistics.scratch_replacements = self
                    .statistics
                    .scratch_replacements
                    .saturating_add(u64::try_from(update.scratch_created).unwrap_or(u64::MAX));
            }
            LiveGpuFrameMode::FullGpu => {
                self.statistics.full_target_renders =
                    self.statistics.full_target_renders.saturating_add(1);
                self.statistics.full_target_pixels_rendered = self
                    .statistics
                    .full_target_pixels_rendered
                    .saturating_add(update.rasterized_pixels);
                if matches!(
                    decision,
                    DamageRenderDecision::FullGpu {
                        reason: super::partial::FullRenderReason::ReplayThreshold
                            | super::partial::FullRenderReason::AreaThreshold
                            | super::partial::FullRenderReason::Fragmentation,
                        ..
                    }
                ) {
                    self.statistics.partial_eligibility_rejections = self
                        .statistics
                        .partial_eligibility_rejections
                        .saturating_add(1);
                    self.statistics.tile_threshold_fallbacks =
                        self.statistics.tile_threshold_fallbacks.saturating_add(1);
                }
            }
        }
        self.statistics.backing_pixels_updated = self
            .statistics
            .backing_pixels_updated
            .saturating_add(update.backing_pixels);
        Ok(PendingLiveGpuFrame {
            surface: plan.surface,
            scene_revision: plan.scene_revision,
            texture,
            suboptimal,
            rendered_micros: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            mode,
            logical_damage_rectangles: match &plan.damage {
                DamageRegion::Empty => 0,
                DamageRegion::Rects(rects) => rects.len(),
                DamageRegion::Full => 1,
            },
            physical_damage,
            selected_tiles: update.selected_tiles,
            rasterized_pixels: update.rasterized_pixels,
            backing_updated_pixels: update.backing_pixels,
            surface_converted_pixels: update.surface_pixels,
        })
    }

    pub fn present(&mut self, pending: PendingLiveGpuFrame) -> Result<(), LiveGpuError> {
        let target = self.targets.get_mut(&pending.surface).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::StaleGeneration,
                "cannot present a stale GPU surface texture",
                true,
            )
        })?;
        if !target.acquired {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::Validation,
                "live GPU surface has no acquired texture to present",
                false,
            ));
        }
        target.acquired = false;
        pending.texture.present();
        self.statistics.frames_presented = self.statistics.frames_presented.saturating_add(1);
        Ok(())
    }

    pub fn reconfigure(&mut self, surface: RenderSurfaceId) -> Result<(), LiveGpuError> {
        let configuration = self.configuration(surface).cloned().ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "cannot reconfigure an unconfigured GPU surface",
                true,
            )
        })?;
        self.configure(surface, configuration.width, configuration.height)?;
        Ok(())
    }

    pub fn recover_surface(&mut self, surface: RenderSurfaceId) -> Result<(), LiveGpuError> {
        let target = self.targets.remove(&surface).ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::StaleGeneration,
                "cannot recover a stale GPU surface",
                true,
            )
        })?;
        let configuration = target.public_configuration.clone();
        let handle = target.handle;
        drop(target);
        // SAFETY: The host has not destroyed the retained wl_surface, and all
        // acquired textures were consumed before this synchronous recovery.
        let recreated = unsafe { create_surface(&self.backend.instance, handle)? };
        self.targets
            .insert(surface, new_live_target(handle, recreated));
        self.statistics.target_recreations = self.statistics.target_recreations.saturating_add(1);
        if let Some(configuration) = configuration {
            self.configure(surface, configuration.width, configuration.height)?;
        }
        Ok(())
    }

    pub fn release_surface(&mut self, surface: RenderSurfaceId) {
        if let Some(mut target) = self.targets.remove(&surface) {
            let releases = u64::from(target.backing.is_some()) * 2;
            destroy_target_images(&mut target);
            self.statistics.backing_releases =
                self.statistics.backing_releases.saturating_add(releases);
            drop(target);
            self.statistics.presenter_releases =
                self.statistics.presenter_releases.saturating_add(1);
        }
    }

    pub fn model_device_loss(&mut self) {
        self.targets.clear();
        self.pipelines.clear();
        self.backend.prepared.clear();
        self.backend.cache.clear();
        self.backend.device_generation =
            DeviceGeneration(self.backend.device_generation.0.saturating_add(1));
        self.statistics.device_losses = self.statistics.device_losses.saturating_add(1);
    }

    fn pipeline(
        &mut self,
        profile: ConversionProfile,
    ) -> Result<wgpu::RenderPipeline, LiveGpuError> {
        if let Some(pipeline) = self.pipelines.get(&profile) {
            return Ok(pipeline.clone());
        }
        let shader = self
            .backend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("HTMShell live output conversion shader"),
                source: wgpu::ShaderSource::Wgsl(
                    match (profile.srgb(), profile.straight()) {
                        (false, false) => CONVERSION_SHADER_LINEAR,
                        (true, false) => CONVERSION_SHADER_SRGB,
                        (false, true) => CONVERSION_SHADER_LINEAR_STRAIGHT,
                        (true, true) => CONVERSION_SHADER_SRGB_STRAIGHT,
                    }
                    .into(),
                ),
            });
        let layout = self
            .backend
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("HTMShell live output conversion layout"),
                bind_group_layouts: &[Some(&self.bind_group_layout)],
                immediate_size: 0,
            });
        let pipeline =
            self.backend
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("HTMShell live output conversion pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: profile.format(),
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                });
        self.pipelines.insert(profile, pipeline.clone());
        Ok(pipeline)
    }
}

#[derive(Debug, Clone, Copy)]
struct BackingUpdate {
    selected_tiles: usize,
    rasterized_pixels: u64,
    backing_pixels: u64,
    surface_pixels: u64,
    guard_pixels: u64,
    scratch_created: usize,
    scratch_reused: usize,
}

fn update_persistent_backing(
    backend: &mut VelloOffscreenRenderer,
    backing: &mut PersistentBacking,
    prepared: &GpuPreparedScene,
    plan: &crate::render::FramePlan,
    decision: &DamageRenderDecision,
) -> Result<BackingUpdate, LiveGpuError> {
    let width = plan.physical_width;
    let height = plan.physical_height;
    let surface_pixels = u64::from(width) * u64::from(height);
    if matches!(decision, DamageRenderDecision::Partial { .. })
        && backing.revision != plan.prior_scene_revision
    {
        backing.force_full_repaint = true;
        return Err(LiveGpuError::new(
            LiveGpuErrorKind::StaleGeneration,
            "partial frame does not descend from the current GPU backing revision",
            true,
        ));
    }
    match decision {
        DamageRenderDecision::NoFrame => Err(LiveGpuError::new(
            LiveGpuErrorKind::Validation,
            "no-frame damage decision reached the renderer",
            false,
        )),
        DamageRenderDecision::FullGpu { .. } => {
            let scene = build_scene(prepared, plan, 0, 0)?;
            render_vello_target(
                backend,
                &scene,
                &backing.transaction.view,
                width,
                height,
                "full persistent backing",
            )?;
            std::mem::swap(&mut backing.current, &mut backing.transaction);
            Ok(BackingUpdate {
                selected_tiles: 0,
                rasterized_pixels: surface_pixels,
                backing_pixels: surface_pixels,
                surface_pixels,
                guard_pixels: 0,
                scratch_created: 0,
                scratch_reused: 0,
            })
        }
        DamageRenderDecision::Partial {
            tiles, tile_pixels, ..
        } => {
            let scratch_extent = DAMAGE_TILE_SIZE.saturating_add(DAMAGE_TILE_GUARD * 2);
            let existing_scratch = backing.scratch.len();
            while backing.scratch.len() < tiles.len() {
                backing.scratch.push(create_scratch_target(&backend.device));
            }
            let mut guard_pixels = 0u64;
            for (index, tile) in tiles.iter().enumerate() {
                let scene =
                    build_scene(prepared, plan, tile.scratch_origin_x, tile.scratch_origin_y)?;
                render_vello_target(
                    backend,
                    &scene,
                    &backing.scratch[index].view,
                    scratch_extent,
                    scratch_extent,
                    "damage tile",
                )?;
            }
            let mut encoder =
                backend
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("HTMShell atomic damage tile copy encoder"),
                    });
            for (index, tile) in tiles.iter().enumerate() {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &backing.scratch[index].texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: tile.source_x,
                            y: tile.source_y,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &backing.current.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: tile.core.x,
                            y: tile.core.y,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: tile.core.width,
                        height: tile.core.height,
                        depth_or_array_layers: 1,
                    },
                );
                guard_pixels = guard_pixels.saturating_add(
                    u64::from(scratch_extent) * u64::from(scratch_extent) - tile.core_pixels(),
                );
            }
            backend.queue.submit([encoder.finish()]);
            Ok(BackingUpdate {
                selected_tiles: tiles.len(),
                rasterized_pixels: u64::try_from(tiles.len())
                    .unwrap_or(u64::MAX)
                    .saturating_mul(u64::from(scratch_extent) * u64::from(scratch_extent)),
                backing_pixels: *tile_pixels,
                surface_pixels,
                guard_pixels,
                scratch_created: tiles.len().saturating_sub(existing_scratch),
                scratch_reused: tiles.len().min(existing_scratch),
            })
        }
    }
}

fn build_scene(
    prepared: &GpuPreparedScene,
    plan: &crate::render::FramePlan,
    physical_origin_x: u32,
    physical_origin_y: u32,
) -> Result<vello::Scene, LiveGpuError> {
    let mut scene = vello::Scene::new();
    let mut painter = VelloScenePainter::new(&mut scene);
    let scale = f64::from(plan.scale_numerator) / f64::from(plan.scale_denominator);
    let transform =
        Affine::translate((-f64::from(physical_origin_x), -f64::from(physical_origin_y)))
            * Affine::scale(scale);
    use anyrender::PaintScene;
    painter.append_scene(prepared.recording.clone(), transform);
    if painter.unsupported() {
        return Err(LiveGpuError::new(
            LiveGpuErrorKind::ResourcePreparation,
            "live scene requires complete CPU fallback",
            true,
        ));
    }
    Ok(scene)
}

fn render_vello_target(
    backend: &mut VelloOffscreenRenderer,
    scene: &vello::Scene,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
    description: &str,
) -> Result<(), LiveGpuError> {
    backend
        .renderer
        .render_to_texture(
            &backend.device,
            &backend.queue,
            scene,
            view,
            &vello::RenderParams {
                base_color: vello::peniko::Color::TRANSPARENT,
                width,
                height,
                antialiasing_method: vello::AaConfig::Area,
            },
        )
        .map_err(|error| {
            LiveGpuError::new(
                LiveGpuErrorKind::Render,
                format!("live Vello {description} rendering failed: {error}"),
                true,
            )
        })
}

fn create_backing_image(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
    label: &'static str,
) -> BackingImage {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("HTMShell live output conversion bind group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    BackingImage {
        texture,
        view,
        bind_group,
    }
}

fn create_scratch_target(device: &wgpu::Device) -> ScratchTarget {
    let extent = DAMAGE_TILE_SIZE.saturating_add(DAMAGE_TILE_GUARD * 2);
    let scratch_bytes = u64::from(extent) * u64::from(extent) * 4;
    debug_assert!(
        scratch_bytes.saturating_mul(super::partial::MAX_PARTIAL_TILE_REPLAYS as u64)
            <= MAX_LIVE_SCRATCH_BYTES
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("HTMShell live damage tile scratch"),
        size: wgpu::Extent3d {
            width: extent,
            height: extent,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    ScratchTarget { texture, view }
}

fn destroy_target_images(target: &mut LiveTarget) {
    if let Some(backing) = target.backing.take() {
        backing.current.texture.destroy();
        backing.transaction.texture.destroy();
        for scratch in backing.scratch {
            scratch.texture.destroy();
        }
    }
}

impl Drop for LiveGpuPresenter {
    fn drop(&mut self) {
        for (_, mut target) in std::mem::take(&mut self.targets) {
            destroy_target_images(&mut target);
        }
    }
}

fn new_live_target(handle: LiveWaylandHandle, surface: wgpu::Surface<'static>) -> LiveTarget {
    LiveTarget {
        handle,
        surface,
        configuration: None,
        public_configuration: None,
        profile: None,
        backing: None,
        configuration_generation: 0,
        acquired: false,
    }
}

unsafe fn create_surface(
    instance: &wgpu::Instance,
    handle: LiveWaylandHandle,
) -> Result<wgpu::Surface<'static>, LiveGpuError> {
    let display = RawDisplayHandle::Wayland(wgpu::rwh::WaylandDisplayHandle::new(handle.display));
    let window = RawWindowHandle::Wayland(wgpu::rwh::WaylandWindowHandle::new(handle.surface));
    // SAFETY: The caller guarantees that both handles remain valid until the
    // returned wgpu surface is dropped.
    unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display),
            raw_window_handle: window,
        })
    }
    .map_err(|error| {
        LiveGpuError::new(
            LiveGpuErrorKind::SurfaceCreation,
            format!("wgpu Wayland surface creation failed: {error}"),
            true,
        )
    })
}

fn choose_profile(formats: &[wgpu::TextureFormat]) -> Option<ConversionProfile> {
    [
        ConversionProfile::RgbaLinear,
        ConversionProfile::BgraLinear,
        ConversionProfile::RgbaSrgb,
        ConversionProfile::BgraSrgb,
    ]
    .into_iter()
    .find(|profile| formats.contains(&profile.format()))
}

fn choose_alpha_mode(
    modes: &[wgpu::CompositeAlphaMode],
) -> Option<(wgpu::CompositeAlphaMode, bool)> {
    if modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
        Some((wgpu::CompositeAlphaMode::PreMultiplied, false))
    } else if modes.contains(&wgpu::CompositeAlphaMode::Inherit) {
        // Wayland inherits its native premultiplied buffer interpretation. The
        // live proof verifies transparency on this common Vulkan WSI profile.
        Some((wgpu::CompositeAlphaMode::Inherit, false))
    } else if modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
        Some((wgpu::CompositeAlphaMode::PostMultiplied, true))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        FramePlan, FrameReasonSet, SceneDelta, SceneNodeId, SceneRevision, SceneSubpart,
    };
    use crate::{ExperimentalDocumentIdentity, ViewportSpec};
    use std::sync::Arc;
    use std::time::Duration;
    use vello::peniko::{Color, Fill};

    fn srgb_decode(value: f32) -> f32 {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn srgb_encode(value: f32) -> f32 {
        if value <= 0.003_130_8 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        }
    }

    fn reference_conversion(profile: ConversionProfile, mut value: [f32; 4]) -> [f32; 4] {
        if !profile.straight() {
            let alpha = value[3];
            for channel in &mut value[..3] {
                *channel *= alpha;
            }
        }
        if profile.srgb() {
            for channel in &mut value[..3] {
                *channel = srgb_decode(*channel);
            }
        }
        value
    }

    #[test]
    fn surface_format_selection_is_deterministic() {
        assert_eq!(
            choose_profile(&[
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Bgra8Unorm,
            ]),
            Some(ConversionProfile::BgraLinear)
        );
        assert_eq!(
            choose_profile(&[wgpu::TextureFormat::Rgba8UnormSrgb]),
            Some(ConversionProfile::RgbaSrgb)
        );
        assert_eq!(choose_profile(&[wgpu::TextureFormat::R16Float]), None);
    }

    #[test]
    fn transparent_alpha_selection_is_explicit() {
        assert_eq!(
            choose_alpha_mode(&[
                wgpu::CompositeAlphaMode::Inherit,
                wgpu::CompositeAlphaMode::PreMultiplied,
            ]),
            Some((wgpu::CompositeAlphaMode::PreMultiplied, false))
        );
        assert_eq!(
            choose_alpha_mode(&[wgpu::CompositeAlphaMode::Inherit]),
            Some((wgpu::CompositeAlphaMode::Inherit, false))
        );
        assert_eq!(
            choose_alpha_mode(&[wgpu::CompositeAlphaMode::PostMultiplied]),
            Some((wgpu::CompositeAlphaMode::PostMultiplied, true))
        );
        assert_eq!(
            choose_alpha_mode(&[
                wgpu::CompositeAlphaMode::Opaque,
                wgpu::CompositeAlphaMode::Auto,
            ]),
            None
        );
    }

    #[test]
    fn alpha_modes_are_explicit_in_locked_api() {
        let supported = [
            wgpu::CompositeAlphaMode::Auto,
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ];
        assert!(supported.contains(&wgpu::CompositeAlphaMode::PreMultiplied));
        assert!(
            ![
                wgpu::CompositeAlphaMode::Opaque,
                wgpu::CompositeAlphaMode::Inherit,
            ]
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        );
    }

    #[test]
    fn conversion_shader_has_finite_premultiplication_and_srgb_profiles() {
        for shader in [CONVERSION_SHADER_LINEAR, CONVERSION_SHADER_SRGB] {
            assert!(shader.contains("value.rgb * value.a"));
            assert!(shader.len() < 4_096);
            assert!(!shader.contains("storage"));
        }
        assert!(CONVERSION_SHADER_SRGB.contains("srgb_decode"));
        assert!(!CONVERSION_SHADER_LINEAR.contains("srgb_decode"));
        assert!(!CONVERSION_SHADER_LINEAR_STRAIGHT.contains("value.rgb * value.a"));
        assert!(CONVERSION_SHADER_SRGB_STRAIGHT.contains("srgb_decode"));
    }

    #[test]
    fn conversion_profiles_preserve_color_alpha_and_format_contracts() {
        assert_eq!(
            reference_conversion(ConversionProfile::RgbaLinear, [1.0, 0.0, 0.0, 0.5]),
            [0.5, 0.0, 0.0, 0.5]
        );
        assert_eq!(
            reference_conversion(ConversionProfile::BgraLinear, [0.0, 1.0, 0.0, 0.0]),
            [0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            reference_conversion(ConversionProfile::RgbaLinearStraight, [0.25, 0.5, 1.0, 0.5],),
            [0.25, 0.5, 1.0, 0.5]
        );
        assert_eq!(
            ConversionProfile::RgbaLinear.format(),
            wgpu::TextureFormat::Rgba8Unorm
        );
        assert_eq!(
            ConversionProfile::BgraLinear.format(),
            wgpu::TextureFormat::Bgra8Unorm
        );
    }

    #[test]
    fn srgb_target_round_trip_retains_encoded_premultiplied_values() {
        let converted = reference_conversion(ConversionProfile::RgbaSrgb, [0.8, 0.4, 0.2, 0.5]);
        let encoded = [
            srgb_encode(converted[0]),
            srgb_encode(converted[1]),
            srgb_encode(converted[2]),
            converted[3],
        ];
        for (actual, expected) in encoded.into_iter().zip([0.4, 0.2, 0.1, 0.5]) {
            assert!((actual - expected).abs() < 0.000_01);
        }
    }

    #[test]
    fn raw_handles_reject_null_pointers() {
        // SAFETY: Null is used only to exercise constructor validation and is
        // never passed to wgpu.
        let error = unsafe {
            LiveWaylandHandle::new(
                std::ptr::null_mut(),
                NonNull::<u8>::dangling().as_ptr().cast(),
            )
        }
        .unwrap_err();
        assert_eq!(error.kind, LiveGpuErrorKind::SurfaceCreation);
    }

    #[test]
    fn live_gpu_errors_are_utf8_safe_and_strictly_bounded() {
        let error = LiveGpuError::host(
            LiveGpuErrorKind::Render,
            "é".repeat(MAX_LIVE_GPU_ERROR_MESSAGE_BYTES),
            true,
        );
        assert_eq!(error.kind, LiveGpuErrorKind::Render);
        assert!(error.recoverable);
        assert!(error.message.len() <= MAX_LIVE_GPU_ERROR_MESSAGE_BYTES);
        assert!(error.message.is_char_boundary(error.message.len()));
    }

    fn partial_plan(revision: u64, damage: DamageRegion) -> FramePlan {
        let document = ExperimentalDocumentIdentity { serial: 41 };
        let scene_revision = SceneRevision(revision);
        let root = SceneNodeId {
            document,
            dom: None,
            subpart: SceneSubpart::Root,
            ordinal: 0,
        };
        FramePlan {
            surface: RenderSurfaceId {
                instance: 71,
                generation: 3,
            },
            document,
            scene_revision,
            prior_scene_revision: (revision > 1).then_some(SceneRevision(revision - 1)),
            logical_width: 768,
            logical_height: 768,
            physical_width: 768,
            physical_height: 768,
            scale_numerator: 120,
            scale_denominator: 120,
            pixel_format: PixelFormat::PremultipliedRgba8,
            clear: true,
            scene: Arc::new(crate::render::RetainedScene {
                document,
                revision: scene_revision,
                viewport: ViewportSpec {
                    logical_width: 768,
                    logical_height: 768,
                    ..ViewportSpec::default()
                },
                root,
                nodes: Vec::new(),
                resources: Vec::new(),
                content_fingerprint: revision,
            }),
            delta: SceneDelta {
                from_revision: (revision > 1).then_some(SceneRevision(revision - 1)),
                to_revision: scene_revision,
                changes: Vec::new(),
                resource_changes: Vec::new(),
                full_scene_replacement: revision == 1,
                unchanged_nodes: 0,
            },
            damage,
            reasons: FrameReasonSet::new(),
            full_repaint: revision == 1,
            presentation_eligible: true,
        }
    }

    fn persistent_backing(
        backend: &VelloOffscreenRenderer,
        layout: &wgpu::BindGroupLayout,
    ) -> PersistentBacking {
        PersistentBacking {
            current: create_backing_image(
                &backend.device,
                layout,
                768,
                768,
                "HTMShell partial proof current",
            ),
            transaction: create_backing_image(
                &backend.device,
                layout,
                768,
                768,
                "HTMShell partial proof transaction",
            ),
            scratch: Vec::new(),
            initialized: false,
            revision: None,
            force_full_repaint: true,
        }
    }

    fn proof_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HTMShell partial proof layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        })
    }

    fn proof_recording(primary_x: f64, include_removed: bool) -> anyrender::Scene {
        let mut recording = anyrender::Scene::new();
        use anyrender::PaintScene;
        recording.draw_box_shadow(
            Affine::IDENTITY,
            kurbo::Rect::new(primary_x, 40.0, primary_x + 80.0, 120.0),
            Color::from_rgba8(0x08, 0x18, 0x30, 0xa0),
            8.0,
            3.0,
        );
        recording.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0x20, 0x70, 0xc0, 0xff),
            None,
            &kurbo::Rect::new(primary_x, 40.0, primary_x + 80.0, 120.0),
        );
        if include_removed {
            recording.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgba8(0xe0, 0x30, 0x20, 0xff),
                None,
                &kurbo::RoundedRect::from_rect(kurbo::Rect::new(300.0, 300.0, 340.0, 340.0), 6.0),
            );
        }
        recording
    }

    fn read_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let row_bytes = width * 4;
        let padded = row_bytes.next_multiple_of(256);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HTMShell partial proof readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("HTMShell partial proof readback encoder"),
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(5)),
            })
            .unwrap();
        receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let mapped = slice.get_mapped_range();
        let mut output = vec![0; row_bytes as usize * height as usize];
        for row in 0..height as usize {
            output[row * row_bytes as usize..(row + 1) * row_bytes as usize].copy_from_slice(
                &mapped[row * padded as usize..row * padded as usize + row_bytes as usize],
            );
        }
        drop(mapped);
        buffer.unmap();
        output
    }

    #[test]
    #[ignore = "requires a compatible Vulkan or GLES adapter"]
    fn persistent_backing_partial_removal_matches_fresh_full_render() {
        let mut backend =
            VelloOffscreenRenderer::new(false).expect("compatible offscreen GPU adapter");
        let layout = proof_layout(&backend.device);
        let mut partial = persistent_backing(&backend, &layout);
        let initial_plan = partial_plan(1, DamageRegion::Full);
        let initial_prepared = GpuPreparedScene {
            document: initial_plan.document,
            revision: initial_plan.scene_revision,
            recording: proof_recording(40.0, true),
            resources: Vec::new(),
        };
        let initial_decision = select_damage_work(&initial_plan, false, true);
        update_persistent_backing(
            &mut backend,
            &mut partial,
            &initial_prepared,
            &initial_plan,
            &initial_decision,
        )
        .unwrap();
        partial.initialized = true;
        partial.revision = Some(initial_plan.scene_revision);
        partial.force_full_repaint = false;
        let initial = read_texture(
            &backend.device,
            &backend.queue,
            &partial.current.texture,
            768,
            768,
        );

        let updated_plan = partial_plan(
            2,
            DamageRegion::Rects(vec![crate::model::LogicalRect {
                x: 296.0,
                y: 296.0,
                width: 48.0,
                height: 48.0,
            }]),
        );
        let updated_prepared = GpuPreparedScene {
            document: updated_plan.document,
            revision: updated_plan.scene_revision,
            recording: proof_recording(40.0, false),
            resources: Vec::new(),
        };
        let decision = select_damage_work(&updated_plan, true, false);
        assert!(matches!(decision, DamageRenderDecision::Partial { .. }));
        let update = update_persistent_backing(
            &mut backend,
            &mut partial,
            &updated_prepared,
            &updated_plan,
            &decision,
        )
        .unwrap();
        partial.revision = Some(updated_plan.scene_revision);
        assert!(update.selected_tiles > 0);
        assert!(update.selected_tiles <= super::super::partial::MAX_PARTIAL_TILE_REPLAYS);
        assert!(update.rasterized_pixels < update.surface_pixels);
        let actual = read_texture(
            &backend.device,
            &backend.queue,
            &partial.current.texture,
            768,
            768,
        );

        let mut fresh = persistent_backing(&backend, &layout);
        let full_decision = DamageRenderDecision::FullGpu {
            damage: vec![PhysicalDamageRect {
                x: 0,
                y: 0,
                width: 768,
                height: 768,
            }],
            reason: super::super::partial::FullRenderReason::ForcedRecovery,
        };
        update_persistent_backing(
            &mut backend,
            &mut fresh,
            &updated_prepared,
            &updated_plan,
            &full_decision,
        )
        .unwrap();
        let expected = read_texture(
            &backend.device,
            &backend.queue,
            &fresh.current.texture,
            768,
            768,
        );
        assert_eq!(actual, expected);
        let unchanged_pixel = (60 * 768 + 60) * 4;
        assert_eq!(
            &actual[unchanged_pixel..unchanged_pixel + 4],
            &initial[unchanged_pixel..unchanged_pixel + 4]
        );
        let removed_pixel = (320 * 768 + 320) * 4;
        assert_eq!(&actual[removed_pixel..removed_pixel + 4], &[0, 0, 0, 0]);

        let moved_plan = partial_plan(
            3,
            DamageRegion::Rects(vec![
                crate::model::LogicalRect {
                    x: 30.0,
                    y: 30.0,
                    width: 100.0,
                    height: 100.0,
                },
                crate::model::LogicalRect {
                    x: 390.0,
                    y: 30.0,
                    width: 100.0,
                    height: 100.0,
                },
            ]),
        );
        let moved_prepared = GpuPreparedScene {
            document: moved_plan.document,
            revision: moved_plan.scene_revision,
            recording: proof_recording(400.0, false),
            resources: Vec::new(),
        };
        let moved_decision = select_damage_work(&moved_plan, true, false);
        assert!(matches!(
            moved_decision,
            DamageRenderDecision::Partial { .. }
        ));
        let moved_update = update_persistent_backing(
            &mut backend,
            &mut partial,
            &moved_prepared,
            &moved_plan,
            &moved_decision,
        )
        .unwrap();
        assert!(moved_update.selected_tiles > 0);
        assert!(moved_update.rasterized_pixels < moved_update.surface_pixels);
        let moved = read_texture(
            &backend.device,
            &backend.queue,
            &partial.current.texture,
            768,
            768,
        );
        let old_pixel = (60 * 768 + 60) * 4;
        let new_pixel = (60 * 768 + 420) * 4;
        assert_eq!(&moved[old_pixel..old_pixel + 4], &[0, 0, 0, 0]);
        assert_eq!(&moved[new_pixel..new_pixel + 4], &[0x20, 0x70, 0xc0, 0xff]);

        let mut moved_fresh = persistent_backing(&backend, &layout);
        update_persistent_backing(
            &mut backend,
            &mut moved_fresh,
            &moved_prepared,
            &moved_plan,
            &full_decision,
        )
        .unwrap();
        let moved_expected = read_texture(
            &backend.device,
            &backend.queue,
            &moved_fresh.current.texture,
            768,
            768,
        );
        assert_eq!(moved, moved_expected);

        let stale_plan = partial_plan(
            4,
            DamageRegion::Rects(vec![crate::model::LogicalRect {
                x: 400.0,
                y: 40.0,
                width: 80.0,
                height: 80.0,
            }]),
        );
        let stale_prepared = GpuPreparedScene {
            document: stale_plan.document,
            revision: stale_plan.scene_revision,
            recording: proof_recording(410.0, false),
            resources: Vec::new(),
        };
        let stale_decision = select_damage_work(&stale_plan, true, false);
        let before_stale = read_texture(
            &backend.device,
            &backend.queue,
            &partial.current.texture,
            768,
            768,
        );
        let error = update_persistent_backing(
            &mut backend,
            &mut partial,
            &stale_prepared,
            &stale_plan,
            &stale_decision,
        )
        .unwrap_err();
        assert_eq!(error.kind, LiveGpuErrorKind::StaleGeneration);
        assert!(partial.force_full_repaint);
        let after_stale = read_texture(
            &backend.device,
            &backend.queue,
            &partial.current.texture,
            768,
            768,
        );
        assert_eq!(after_stale, before_stale);
    }

    #[test]
    fn swapchain_images_are_fully_replaced_from_complete_backing() {
        let backing = vec![0x5a; 64 * 48 * 4];
        for stale in [0x00, 0x7f, 0xff] {
            let mut acquired = vec![stale; backing.len()];
            acquired.copy_from_slice(&backing);
            assert_eq!(acquired, backing);
        }
        for shader in [
            CONVERSION_SHADER_LINEAR,
            CONVERSION_SHADER_SRGB,
            CONVERSION_SHADER_LINEAR_STRAIGHT,
            CONVERSION_SHADER_SRGB_STRAIGHT,
        ] {
            assert!(shader.contains("textureLoad"));
            assert!(shader.contains("position.xy"));
        }
    }
}
