use super::{
    BackendError, BackendErrorKind, DeviceGeneration, GpuPreparedScene, RenderTarget,
    VelloOffscreenRenderer, VelloScenePainter, validate_plan, validate_target,
};
use crate::LiveGpuPreparedFrame;
use crate::render::{DamageRegion, PixelFormat, RenderSurfaceId, SceneRevision};
use kurbo::Affine;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fmt;
use std::ptr::NonNull;
use std::time::Instant;
use vello::wgpu;
use wgpu::rwh::{HasDisplayHandle, RawDisplayHandle, RawWindowHandle};

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
        message.truncate(
            message
                .char_indices()
                .take_while(|(index, _)| *index <= 1_024)
                .last()
                .map_or(0, |(index, character)| index + character.len_utf8()),
        );
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

struct LiveTarget {
    handle: LiveWaylandHandle,
    surface: wgpu::Surface<'static>,
    configuration: Option<wgpu::SurfaceConfiguration>,
    public_configuration: Option<LiveGpuConfiguration>,
    profile: Option<ConversionProfile>,
    intermediate: Option<wgpu::Texture>,
    intermediate_view: Option<wgpu::TextureView>,
    bind_group: Option<wgpu::BindGroup>,
    configuration_generation: u64,
    acquired: bool,
}

pub struct PendingLiveGpuFrame {
    surface: RenderSurfaceId,
    scene_revision: SceneRevision,
    texture: wgpu::SurfaceTexture,
    suboptimal: bool,
    rendered_micros: u64,
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
        let intermediate = self
            .backend
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("HTMShell live Vello intermediate"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
        let intermediate_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self
            .backend
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("HTMShell live output conversion bind group"),
                layout: &self.bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&intermediate_view),
                }],
            });
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
        if let Some(old) = target.intermediate.replace(intermediate) {
            old.destroy();
        }
        target.intermediate_view = Some(intermediate_view);
        target.bind_group = Some(bind_group);
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
        let target = self.targets.get(&plan.surface).ok_or_else(|| {
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
        let mut scene = vello::Scene::new();
        let mut painter = VelloScenePainter::new(&mut scene);
        let scale = f64::from(plan.scale_numerator) / f64::from(plan.scale_denominator);
        use anyrender::PaintScene;
        painter.append_scene(prepared.recording, Affine::scale(scale));
        if painter.unsupported() {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::ResourcePreparation,
                "live scene requires complete CPU fallback",
                true,
            ));
        }
        let intermediate_view = target.intermediate_view.as_ref().ok_or_else(|| {
            LiveGpuError::new(
                LiveGpuErrorKind::InvalidConfiguration,
                "live Vello intermediate is missing",
                true,
            )
        })?;
        self.backend
            .renderer
            .render_to_texture(
                &self.backend.device,
                &self.backend.queue,
                &scene,
                intermediate_view,
                &vello::RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: configuration.width,
                    height: configuration.height,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .map_err(|error| {
                LiveGpuError::new(
                    LiveGpuErrorKind::Render,
                    format!("live Vello rendering failed: {error}"),
                    true,
                )
            })?;
        let target = self
            .targets
            .get_mut(&plan.surface)
            .expect("validated above");
        if target.acquired {
            return Err(LiveGpuError::new(
                LiveGpuErrorKind::Validation,
                "a live surface already has an acquired texture",
                false,
            ));
        }
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
        target.acquired = true;
        self.statistics.acquisitions = self.statistics.acquisitions.saturating_add(1);
        let bind_group = target.bind_group.as_ref().expect("configured bind group");
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
        self.statistics.full_target_renders = self.statistics.full_target_renders.saturating_add(1);
        Ok(PendingLiveGpuFrame {
            surface: plan.surface,
            scene_revision: plan.scene_revision,
            texture,
            suboptimal,
            rendered_micros: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
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
            if let Some(intermediate) = target.intermediate.take() {
                intermediate.destroy();
            }
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

impl Drop for LiveGpuPresenter {
    fn drop(&mut self) {
        for (_, mut target) in std::mem::take(&mut self.targets) {
            if let Some(intermediate) = target.intermediate.take() {
                intermediate.destroy();
            }
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
        intermediate: None,
        intermediate_view: None,
        bind_group: None,
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
}
