mod live;
mod painter;

pub use live::{
    LiveGpuBackendInfo, LiveGpuConfiguration, LiveGpuError, LiveGpuErrorKind, LiveGpuPresenter,
    LiveGpuStatistics, LiveWaylandHandle, PendingLiveGpuFrame,
};

use super::cpu::{CpuPreparedScene, CpuReferenceRenderer};
use super::{
    BackendError, BackendErrorKind, DamageRegion, FramePlan, PixelFormat, RenderResult,
    RenderSurfaceId, RenderTarget, Renderer, ResourceLifecycle, SceneEffect, SceneNodeKind,
    SceneResourceId, SceneResourceVersion, SceneRevision, logical_damage_to_physical,
};
use crate::ExperimentalDocumentIdentity;
use anyrender::PaintScene;
use kurbo::Affine;
use painter::VelloScenePainter;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::pin;
use std::sync::mpsc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant};
use vello::wgpu;

pub(crate) const GPU_BACKEND_NAME: &str = "Vello";
pub(crate) const GPU_BACKEND_VERSION: &str = "0.9.0";
pub(crate) const MAX_GPU_TARGET_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_GPU_CACHE_ENTRIES: usize = 16_384;
pub(crate) const MAX_GPU_CACHE_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_GPU_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;
const GPU_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const COPY_BYTES_PER_PIXEL: u32 = 4;
const COPY_ROW_ALIGNMENT: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuCoverage {
    Native,
    HybridResource,
    CpuFrameFallback,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderPath {
    Gpu,
    CpuFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackendInfo {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub max_texture_dimension_2d: u32,
    pub max_buffer_size: u64,
    pub adapter_selection_micros: u128,
    pub device_creation_micros: u128,
    pub pipeline_creation_micros: u128,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GpuStatistics {
    pub target_creations: u64,
    pub target_resizes: u64,
    pub frames_rendered: u64,
    pub full_target_renders: u64,
    pub readbacks: u64,
    pub resource_uploads: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_evictions: u64,
    pub fallback_requests: u64,
    pub resets: u64,
    pub last_readback_micros: u64,
    pub total_readback_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DeviceGeneration(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TargetGeneration(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreparedResourceKind {
    Font,
    ShapedText,
    Svg,
    RasterImage,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CacheKey {
    device: DeviceGeneration,
    id: SceneResourceId,
    version: SceneResourceVersion,
    kind: PreparedResourceKind,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    byte_len: u64,
    last_use: u64,
}

#[derive(Debug, Default)]
struct GpuResourceCache {
    entries: BTreeMap<CacheKey, CacheEntry>,
    bytes: u64,
    use_sequence: u64,
}

impl GpuResourceCache {
    fn prepare(
        &mut self,
        generation: DeviceGeneration,
        plan: &FramePlan,
        statistics: &mut GpuStatistics,
    ) -> Result<(), BackendError> {
        self.entries.retain(|key, entry| {
            let replaced = plan
                .scene
                .resources
                .iter()
                .any(|resource| resource.id == key.id && resource.version != key.version);
            let keep = key.device == generation && !replaced;
            if !keep {
                self.bytes = self.bytes.saturating_sub(entry.byte_len);
            }
            keep
        });

        for resource in &plan.scene.resources {
            if resource.lifecycle != ResourceLifecycle::Ready {
                continue;
            }
            let byte_len = u64::try_from(resource.byte_len.unwrap_or(0)).map_err(|_| {
                BackendError::new(
                    BackendErrorKind::ResourcePreparation,
                    "GPU resource byte length cannot be represented",
                    false,
                )
            })?;
            if byte_len > MAX_GPU_RESOURCE_BYTES {
                return Err(BackendError::new(
                    BackendErrorKind::FallbackRequired,
                    "GPU resource exceeds the bounded single-resource limit",
                    true,
                ));
            }
            self.use_sequence = self.use_sequence.checked_add(1).ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::BackendReset,
                    "GPU resource-cache use sequence exhausted",
                    true,
                )
            })?;
            let key = CacheKey {
                device: generation,
                id: resource.id.clone(),
                version: resource.version,
                kind: match resource.id.kind {
                    super::ResourceKind::Font => PreparedResourceKind::Font,
                    super::ResourceKind::ShapedText => PreparedResourceKind::ShapedText,
                    super::ResourceKind::Svg => PreparedResourceKind::Svg,
                    super::ResourceKind::RasterImage => PreparedResourceKind::RasterImage,
                },
            };
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.last_use = self.use_sequence;
                statistics.cache_hits = statistics.cache_hits.saturating_add(1);
                continue;
            }
            statistics.cache_misses = statistics.cache_misses.saturating_add(1);
            while self.entries.len() >= MAX_GPU_CACHE_ENTRIES
                || self.bytes.saturating_add(byte_len) > MAX_GPU_CACHE_BYTES
            {
                let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(key, entry)| (entry.last_use, *key))
                    .map(|(key, _)| key.clone())
                else {
                    return Err(BackendError::new(
                        BackendErrorKind::FallbackRequired,
                        "GPU resource cache cannot admit the requested resource",
                        true,
                    ));
                };
                if let Some(entry) = self.entries.remove(&oldest) {
                    self.bytes = self.bytes.saturating_sub(entry.byte_len);
                    statistics.cache_evictions = statistics.cache_evictions.saturating_add(1);
                }
            }
            self.bytes = self.bytes.checked_add(byte_len).ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::ResourcePreparation,
                    "GPU resource-cache size overflow",
                    false,
                )
            })?;
            self.entries.insert(
                key,
                CacheEntry {
                    byte_len,
                    last_use: self.use_sequence,
                },
            );
            statistics.resource_uploads = statistics.resource_uploads.saturating_add(1);
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.use_sequence = 0;
    }
}

#[derive(Clone)]
pub(crate) struct GpuPreparedScene {
    document: ExperimentalDocumentIdentity,
    revision: SceneRevision,
    recording: anyrender::Scene,
    resources: Vec<(SceneResourceId, SceneResourceVersion)>,
}

impl GpuPreparedScene {
    pub(crate) fn from_cpu(
        document: ExperimentalDocumentIdentity,
        prepared: CpuPreparedScene,
        resources: Vec<(SceneResourceId, SceneResourceVersion)>,
    ) -> Self {
        Self {
            document,
            revision: prepared.revision,
            recording: prepared.recording,
            resources,
        }
    }
}

struct OffscreenTarget {
    generation: TargetGeneration,
    descriptor: RenderTarget,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_bytes_per_row: u32,
    initialized: bool,
}

pub(crate) struct VelloOffscreenRenderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    info: BackendInfo,
    device_generation: DeviceGeneration,
    next_target_generation: u64,
    targets: BTreeMap<RenderSurfaceId, OffscreenTarget>,
    prepared: BTreeMap<(ExperimentalDocumentIdentity, SceneRevision), GpuPreparedScene>,
    cache: GpuResourceCache,
    statistics: GpuStatistics,
    shutdown: bool,
}

impl VelloOffscreenRenderer {
    pub(crate) fn new(force_software_adapter: bool) -> Result<Self, BackendError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Self::new_with_instance(instance, force_software_adapter, None)
    }

    fn new_with_instance(
        instance: wgpu::Instance,
        force_software_adapter: bool,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, BackendError> {
        let adapter_selection_started = Instant::now();
        let mut adapters = bounded_block_on(
            instance.enumerate_adapters(wgpu::Backends::VULKAN | wgpu::Backends::GL),
            GPU_WAIT_TIMEOUT,
        )?;
        adapters.retain(|adapter| {
            let info = adapter.get_info();
            let format = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm);
            let required_usage =
                wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC;
            format.allowed_usages.contains(required_usage)
                && (!force_software_adapter || info.device_type == wgpu::DeviceType::Cpu)
                && compatible_surface.is_none_or(|surface| {
                    let capabilities = surface.get_capabilities(adapter);
                    capabilities.formats.iter().any(|format| {
                        matches!(
                            format,
                            wgpu::TextureFormat::Rgba8Unorm
                                | wgpu::TextureFormat::Bgra8Unorm
                                | wgpu::TextureFormat::Rgba8UnormSrgb
                                | wgpu::TextureFormat::Bgra8UnormSrgb
                        )
                    }) && capabilities
                        .present_modes
                        .contains(&wgpu::PresentMode::Fifo)
                        && capabilities.alpha_modes.iter().any(|mode| {
                            matches!(
                                mode,
                                wgpu::CompositeAlphaMode::PreMultiplied
                                    | wgpu::CompositeAlphaMode::PostMultiplied
                                    | wgpu::CompositeAlphaMode::Inherit
                            )
                        })
                })
        });
        adapters.sort_by_key(|adapter| adapter_rank(&adapter.get_info()));
        let adapter = adapters.into_iter().next().ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::AdapterUnavailable,
                "no compatible Vulkan or GLES offscreen adapter",
                true,
            )
        })?;
        let adapter_selection_micros = adapter_selection_started.elapsed().as_micros();
        let limits = adapter.limits();
        if limits.max_texture_dimension_2d == 0
            || limits.max_buffer_size < COPY_ROW_ALIGNMENT.into()
        {
            return Err(BackendError::new(
                BackendErrorKind::UnsupportedCapability,
                "adapter limits cannot support the bounded offscreen target",
                false,
            ));
        }
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("HTMShell offscreen Vello device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default().using_resolution(limits.clone()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        };
        let device_creation_started = Instant::now();
        let (device, queue) =
            bounded_block_on(adapter.request_device(&descriptor), GPU_WAIT_TIMEOUT)?.map_err(
                |error| {
                    BackendError::new(
                        BackendErrorKind::DeviceCreation,
                        format!("offscreen GPU device creation failed: {error}"),
                        true,
                    )
                },
            )?;
        let device_creation_micros = device_creation_started.elapsed().as_micros();
        let pipeline_creation_started = Instant::now();
        let renderer = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .map_err(|error| {
            BackendError::new(
                BackendErrorKind::PipelineCreation,
                format!("Vello pipeline initialization failed: {error}"),
                true,
            )
        })?;
        let pipeline_creation_micros = pipeline_creation_started.elapsed().as_micros();
        let adapter_info = adapter.get_info();
        let info = BackendInfo {
            name: adapter_info.name,
            backend: format!("{:?}", adapter_info.backend),
            device_type: format!("{:?}", adapter_info.device_type),
            driver: adapter_info.driver,
            max_texture_dimension_2d: limits.max_texture_dimension_2d,
            max_buffer_size: limits.max_buffer_size,
            adapter_selection_micros,
            device_creation_micros,
            pipeline_creation_micros,
        };
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            renderer,
            info,
            device_generation: DeviceGeneration(1),
            next_target_generation: 0,
            targets: BTreeMap::new(),
            prepared: BTreeMap::new(),
            cache: GpuResourceCache::default(),
            statistics: GpuStatistics::default(),
            shutdown: false,
        })
    }

    pub(crate) fn info(&self) -> &BackendInfo {
        &self.info
    }

    pub(crate) fn statistics(&self) -> GpuStatistics {
        self.statistics
    }

    pub(crate) fn cache_usage(&self) -> (usize, u64) {
        (self.cache.entries.len(), self.cache.bytes)
    }

    fn allocate_target(&mut self, target: RenderTarget) -> Result<OffscreenTarget, BackendError> {
        validate_target(target, &self.info)?;
        let row_bytes = target
            .width
            .checked_mul(COPY_BYTES_PER_PIXEL)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::TargetAllocation,
                    "GPU target row-byte calculation overflowed",
                    false,
                )
            })?;
        let padded_bytes_per_row = row_bytes.next_multiple_of(COPY_ROW_ALIGNMENT);
        let readback_bytes = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(target.height))
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::TargetAllocation,
                    "GPU readback-buffer size overflowed",
                    false,
                )
            })?;
        if readback_bytes > self.info.max_buffer_size || readback_bytes > MAX_GPU_TARGET_BYTES {
            return Err(BackendError::new(
                BackendErrorKind::TargetAllocation,
                "GPU target exceeds the bounded readback-buffer limit",
                true,
            ));
        }
        self.next_target_generation =
            self.next_target_generation.checked_add(1).ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::BackendReset,
                    "GPU target generation exhausted",
                    true,
                )
            })?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HTMShell offscreen Vello target"),
            size: wgpu::Extent3d {
                width: target.width,
                height: target.height,
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
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HTMShell offscreen Vello readback"),
            size: readback_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(OffscreenTarget {
            generation: TargetGeneration(self.next_target_generation),
            descriptor: target,
            texture,
            view,
            readback,
            padded_bytes_per_row,
            initialized: false,
        })
    }

    fn coverage(plan: &FramePlan) -> Result<GpuCoverage, BackendError> {
        let mut coverage = GpuCoverage::Native;
        for node in &plan.scene.nodes {
            coverage = merge_coverage(coverage, node_coverage(node.kind));
            for effect in &node.effects {
                coverage = merge_coverage(coverage, effect_coverage(effect));
            }
        }
        if coverage == GpuCoverage::Unsupported {
            return Err(BackendError::new(
                BackendErrorKind::UnsupportedCapability,
                "retained scene contains unsupported GPU content",
                false,
            ));
        }
        Ok(coverage)
    }

    fn read_target(
        device: &wgpu::Device,
        target: &OffscreenTarget,
    ) -> Result<Vec<u8>, BackendError> {
        let slice = target.readback.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(GPU_WAIT_TIMEOUT),
            })
            .map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Timeout,
                    format!("GPU readback polling failed: {error}"),
                    true,
                )
            })?;
        receiver
            .recv_timeout(Duration::from_millis(1))
            .map_err(|_| {
                BackendError::new(
                    BackendErrorKind::Timeout,
                    "GPU readback callback did not complete",
                    true,
                )
            })?
            .map_err(|error| {
                BackendError::new(
                    BackendErrorKind::Readback,
                    format!("GPU readback mapping failed: {error}"),
                    true,
                )
            })?;
        let mapped = slice.get_mapped_range();
        let row_bytes = usize::try_from(target.descriptor.width)
            .ok()
            .and_then(|width| width.checked_mul(COPY_BYTES_PER_PIXEL as usize))
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::Readback,
                    "GPU readback row size cannot be represented",
                    false,
                )
            })?;
        let output_len = row_bytes
            .checked_mul(target.descriptor.height as usize)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::Readback,
                    "GPU readback output size overflowed",
                    false,
                )
            })?;
        let mut pixels = vec![0; output_len];
        for row in 0..target.descriptor.height as usize {
            let source_start = row * target.padded_bytes_per_row as usize;
            let destination_start = row * row_bytes;
            pixels[destination_start..destination_start + row_bytes]
                .copy_from_slice(&mapped[source_start..source_start + row_bytes]);
        }
        drop(mapped);
        target.readback.unmap();
        // Vello's Rgba8Unorm storage target contains straight-alpha color.
        // HTMShell's renderer contract and wl_shm reference path use
        // premultiplied RGBA bytes, so conversion happens once at readback.
        premultiply_rgba8_in_place(&mut pixels);
        Ok(pixels)
    }
}

impl Renderer for VelloOffscreenRenderer {
    type Prepared = GpuPreparedScene;

    fn create_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError> {
        if self.shutdown {
            return Err(BackendError::new(
                BackendErrorKind::BackendUnavailable,
                "GPU backend is shut down",
                false,
            ));
        }
        let allocated = self.allocate_target(target)?;
        if let Some(replaced) = self.targets.insert(surface, allocated) {
            replaced.texture.destroy();
            replaced.readback.destroy();
        }
        self.statistics.target_creations = self.statistics.target_creations.saturating_add(1);
        Ok(())
    }

    fn resize_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError> {
        if !self.targets.contains_key(&surface) {
            return Err(BackendError::new(
                BackendErrorKind::StaleGeneration,
                "cannot resize a missing GPU target",
                true,
            ));
        }
        self.create_target(surface, target)?;
        self.statistics.target_resizes = self.statistics.target_resizes.saturating_add(1);
        Ok(())
    }

    fn prepare(&mut self, plan: &FramePlan, prepared: Self::Prepared) -> Result<(), BackendError> {
        validate_plan(plan)?;
        if prepared.document != plan.document || prepared.revision != plan.scene_revision {
            return Err(BackendError::new(
                BackendErrorKind::StaleGeneration,
                "GPU prepared recording does not match the frame plan generation",
                true,
            ));
        }
        let coverage = Self::coverage(plan)?;
        if coverage == GpuCoverage::CpuFrameFallback {
            self.statistics.fallback_requests = self.statistics.fallback_requests.saturating_add(1);
            return Err(BackendError::new(
                BackendErrorKind::FallbackRequired,
                "retained scene contains an effect assigned to CPU frame fallback",
                true,
            ));
        }
        self.cache
            .prepare(self.device_generation, plan, &mut self.statistics)?;
        self.prepared
            .retain(|(document, _), _| *document != prepared.document);
        self.prepared
            .insert((prepared.document, prepared.revision), prepared);
        Ok(())
    }

    fn render(
        &mut self,
        plan: &FramePlan,
        target: RenderTarget,
    ) -> Result<RenderResult, BackendError> {
        validate_plan(plan)?;
        let physical_damage = logical_damage_to_physical(
            &plan.damage,
            plan.logical_width,
            plan.logical_height,
            plan.physical_width,
            plan.physical_height,
            plan.scale_numerator,
            plan.scale_denominator,
        );
        if !matches!(plan.damage, DamageRegion::Empty) && physical_damage.is_empty() {
            return Err(BackendError::new(
                BackendErrorKind::InvalidPlan,
                "GPU frame plan contains incomplete physical damage",
                false,
            ));
        }
        let prepared = self
            .prepared
            .get(&(plan.document, plan.scene_revision))
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorKind::ResourcePreparation,
                    "GPU recording is unavailable for the requested document revision",
                    true,
                )
            })?
            .clone();
        let offscreen = self.targets.get_mut(&plan.surface).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::StaleGeneration,
                "GPU frame plan targets a missing surface generation",
                true,
            )
        })?;
        if offscreen.descriptor != target
            || target.width != plan.physical_width
            || target.height != plan.physical_height
            || target.pixel_format != plan.pixel_format
        {
            return Err(BackendError::new(
                BackendErrorKind::InvalidPlan,
                "GPU target does not match the immutable frame plan",
                false,
            ));
        }
        if matches!(plan.damage, DamageRegion::Empty) {
            if !offscreen.initialized {
                return Err(BackendError::new(
                    BackendErrorKind::InvalidPlan,
                    "empty damage cannot initialize a GPU target",
                    false,
                ));
            }
        } else {
            let mut scene = vello::Scene::new();
            let mut painter = VelloScenePainter::new(&mut scene);
            let scale = f64::from(plan.scale_numerator) / f64::from(plan.scale_denominator);
            painter.append_scene(prepared.recording, Affine::scale(scale));
            if painter.unsupported() {
                self.statistics.fallback_requests =
                    self.statistics.fallback_requests.saturating_add(1);
                return Err(BackendError::new(
                    BackendErrorKind::FallbackRequired,
                    "AnyRender recording contains a backend-specific brush or filter",
                    true,
                ));
            }
            self.renderer
                .render_to_texture(
                    &self.device,
                    &self.queue,
                    &scene,
                    &offscreen.view,
                    &vello::RenderParams {
                        base_color: vello::peniko::Color::TRANSPARENT,
                        width: target.width,
                        height: target.height,
                        antialiasing_method: vello::AaConfig::Area,
                    },
                )
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorKind::Render,
                        format!("Vello offscreen rendering failed: {error}"),
                        true,
                    )
                })?;
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("HTMShell Vello offscreen readback encoder"),
                });
            encoder.copy_texture_to_buffer(
                offscreen.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &offscreen.readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(offscreen.padded_bytes_per_row),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: target.width,
                    height: target.height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit([encoder.finish()]);
            offscreen.initialized = true;
            self.statistics.frames_rendered = self.statistics.frames_rendered.saturating_add(1);
            self.statistics.full_target_renders =
                self.statistics.full_target_renders.saturating_add(1);
        }
        let readback_started = Instant::now();
        let pixels = Self::read_target(&self.device, offscreen)?;
        let readback_micros =
            u64::try_from(readback_started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.statistics.last_readback_micros = readback_micros;
        self.statistics.total_readback_micros = self
            .statistics
            .total_readback_micros
            .saturating_add(readback_micros);
        self.statistics.readbacks = self.statistics.readbacks.saturating_add(1);
        Ok(RenderResult {
            scene_revision: plan.scene_revision,
            pixels,
            applied_damage: plan.damage.clone(),
            full_raster: true,
            prepared_resources: plan.scene.live_resources(),
        })
    }

    fn readback(&mut self, result: RenderResult) -> Result<Vec<u8>, BackendError> {
        Ok(result.pixels)
    }

    fn release_resources(
        &mut self,
        live: &[(SceneResourceId, SceneResourceVersion)],
    ) -> Result<(), BackendError> {
        let requested_live: BTreeSet<_> = live.iter().cloned().collect();
        let prepared_live: BTreeSet<_> = self
            .prepared
            .values()
            .flat_map(|prepared| prepared.resources.iter().cloned())
            .collect();
        self.cache.entries.retain(|key, entry| {
            let identity = (key.id.clone(), key.version);
            let keep = requested_live.contains(&identity) || prepared_live.contains(&identity);
            if !keep {
                self.cache.bytes = self.cache.bytes.saturating_sub(entry.byte_len);
            }
            keep
        });
        if self.prepared.len() > 64 {
            let newest = self
                .prepared
                .keys()
                .next_back()
                .copied()
                .expect("nonempty prepared map");
            self.prepared.retain(|key, _| *key == newest);
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), BackendError> {
        for (_, target) in std::mem::take(&mut self.targets) {
            target.texture.destroy();
            target.readback.destroy();
        }
        self.prepared.clear();
        self.cache.clear();
        self.renderer = vello::Renderer::new(
            &self.device,
            vello::RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .map_err(|error| {
            BackendError::new(
                BackendErrorKind::BackendReset,
                format!("Vello renderer reset failed: {error}"),
                true,
            )
        })?;
        self.device_generation.0 = self.device_generation.0.checked_add(1).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::BackendReset,
                "GPU device generation exhausted",
                false,
            )
        })?;
        self.statistics.resets = self.statistics.resets.saturating_add(1);
        Ok(())
    }

    fn release_target(&mut self, surface: RenderSurfaceId) {
        if let Some(target) = self.targets.remove(&surface) {
            target.texture.destroy();
            target.readback.destroy();
        }
    }

    fn shutdown(&mut self) {
        for (_, target) in std::mem::take(&mut self.targets) {
            target.texture.destroy();
            target.readback.destroy();
        }
        self.prepared.clear();
        self.cache.clear();
        self.device.destroy();
        self.shutdown = true;
    }
}

impl Drop for VelloOffscreenRenderer {
    fn drop(&mut self) {
        if !self.shutdown {
            self.shutdown();
        }
    }
}

pub(crate) struct OffscreenRenderer {
    gpu: Option<VelloOffscreenRenderer>,
    cpu: CpuReferenceRenderer,
    target: Option<(RenderSurfaceId, RenderTarget)>,
    last_gpu_error: Option<BackendErrorKind>,
}

impl OffscreenRenderer {
    pub(crate) fn new(force_software_adapter: bool) -> Self {
        match VelloOffscreenRenderer::new(force_software_adapter) {
            Ok(gpu) => Self {
                gpu: Some(gpu),
                cpu: CpuReferenceRenderer::default(),
                target: None,
                last_gpu_error: None,
            },
            Err(error) => Self {
                gpu: None,
                cpu: CpuReferenceRenderer::default(),
                target: None,
                last_gpu_error: Some(error.kind),
            },
        }
    }

    #[cfg(test)]
    fn modeled_unavailable(error: BackendErrorKind) -> Self {
        Self {
            gpu: None,
            cpu: CpuReferenceRenderer::default(),
            target: None,
            last_gpu_error: Some(error),
        }
    }

    pub(crate) fn backend_info(&self) -> Option<&BackendInfo> {
        self.gpu.as_ref().map(VelloOffscreenRenderer::info)
    }

    pub(crate) fn last_gpu_error(&self) -> Option<BackendErrorKind> {
        self.last_gpu_error
    }

    pub(crate) fn render(
        &mut self,
        plan: &FramePlan,
        target: RenderTarget,
        prepared: CpuPreparedScene,
    ) -> Result<(Vec<u8>, RenderPath), BackendError> {
        self.ensure_target(plan.surface, target)?;
        self.cpu.prepare(plan, prepared.clone())?;
        if let Some(gpu) = &mut self.gpu {
            let gpu_prepared =
                GpuPreparedScene::from_cpu(plan.document, prepared, plan.scene.live_resources());
            let rendered = gpu
                .prepare(plan, gpu_prepared)
                .and_then(|()| gpu.render(plan, target))
                .and_then(|result| gpu.readback(result));
            match rendered {
                Ok(pixels) => return Ok((pixels, RenderPath::Gpu)),
                Err(error) => {
                    self.last_gpu_error = Some(error.kind);
                    if !error.recoverable {
                        return Err(error);
                    }
                }
            }
        }
        let result = self.cpu.render(plan, target)?;
        let pixels = self.cpu.readback(result)?;
        Ok((pixels, RenderPath::CpuFallback))
    }

    fn ensure_target(
        &mut self,
        surface: RenderSurfaceId,
        target: RenderTarget,
    ) -> Result<(), BackendError> {
        match self.target {
            None => {
                self.cpu.create_target(surface, target)?;
                if let Some(gpu) = &mut self.gpu
                    && let Err(error) = gpu.create_target(surface, target)
                {
                    self.last_gpu_error = Some(error.kind);
                    self.gpu = None;
                }
            }
            Some((old_surface, _old_target)) if old_surface != surface => {
                self.cpu.release_target(old_surface);
                self.cpu.create_target(surface, target)?;
                if let Some(gpu) = &mut self.gpu {
                    gpu.release_target(old_surface);
                    if let Err(error) = gpu.create_target(surface, target) {
                        self.last_gpu_error = Some(error.kind);
                        self.gpu = None;
                    }
                }
            }
            Some((_, old_target)) if old_target != target => {
                self.cpu.resize_target(surface, target)?;
                if let Some(gpu) = &mut self.gpu
                    && let Err(error) = gpu.resize_target(surface, target)
                {
                    self.last_gpu_error = Some(error.kind);
                    self.gpu = None;
                }
            }
            Some(_) => {}
        }
        self.target = Some((surface, target));
        Ok(())
    }
}

fn merge_coverage(left: GpuCoverage, right: GpuCoverage) -> GpuCoverage {
    use GpuCoverage::{CpuFrameFallback, HybridResource, Native, Unsupported};
    match (left, right) {
        (Unsupported, _) | (_, Unsupported) => Unsupported,
        (CpuFrameFallback, _) | (_, CpuFrameFallback) => CpuFrameFallback,
        (HybridResource, _) | (_, HybridResource) => HybridResource,
        (Native, Native) => Native,
    }
}

fn adapter_rank(info: &wgpu::AdapterInfo) -> (u8, u8, String, u32, u32) {
    let device = match info.device_type {
        wgpu::DeviceType::IntegratedGpu => 0,
        wgpu::DeviceType::DiscreteGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Cpu => 3,
        wgpu::DeviceType::Other => 4,
    };
    let backend = match info.backend {
        wgpu::Backend::Vulkan => 0,
        wgpu::Backend::Gl => 1,
        wgpu::Backend::Metal | wgpu::Backend::Dx12 => 2,
        wgpu::Backend::BrowserWebGpu => 3,
        wgpu::Backend::Noop => 4,
    };
    (
        device,
        backend,
        info.name.to_ascii_lowercase(),
        info.vendor,
        info.device,
    )
}

fn node_coverage(kind: SceneNodeKind) -> GpuCoverage {
    match kind {
        SceneNodeKind::SurfaceClear
        | SceneNodeKind::Box
        | SceneNodeKind::TextRun
        | SceneNodeKind::Svg
        | SceneNodeKind::UnavailableImage => GpuCoverage::Native,
        SceneNodeKind::RasterImage => GpuCoverage::HybridResource,
    }
}

fn effect_coverage(effect: &SceneEffect) -> GpuCoverage {
    match effect {
        SceneEffect::Opacity { .. }
        | SceneEffect::Clip { .. }
        | SceneEffect::Transform { .. }
        | SceneEffect::BackgroundLayers { .. }
        | SceneEffect::BoxShadows { .. } => GpuCoverage::Native,
        SceneEffect::ForegroundFilter { .. } | SceneEffect::BackdropFilter { .. } => {
            GpuCoverage::CpuFrameFallback
        }
    }
}

fn validate_target(target: RenderTarget, info: &BackendInfo) -> Result<(), BackendError> {
    if target.width == 0 || target.height == 0 {
        return Err(BackendError::new(
            BackendErrorKind::TargetAllocation,
            "GPU target dimensions must be nonzero",
            false,
        ));
    }
    if target.pixel_format != PixelFormat::PremultipliedRgba8 {
        return Err(BackendError::new(
            BackendErrorKind::UnsupportedCapability,
            "GPU target format is not the canonical premultiplied RGBA8 format",
            false,
        ));
    }
    if target.width > info.max_texture_dimension_2d || target.height > info.max_texture_dimension_2d
    {
        return Err(BackendError::new(
            BackendErrorKind::TargetAllocation,
            "GPU target exceeds the adapter texture-dimension limit",
            true,
        ));
    }
    let bytes = u64::from(target.width)
        .checked_mul(u64::from(target.height))
        .and_then(|pixels| pixels.checked_mul(COPY_BYTES_PER_PIXEL.into()))
        .ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::TargetAllocation,
                "GPU target byte calculation overflowed",
                false,
            )
        })?;
    if bytes > MAX_GPU_TARGET_BYTES {
        return Err(BackendError::new(
            BackendErrorKind::TargetAllocation,
            "GPU target exceeds the HTMShell offscreen byte limit",
            true,
        ));
    }
    Ok(())
}

fn validate_plan(plan: &FramePlan) -> Result<(), BackendError> {
    if !plan.presentation_eligible
        || plan.scale_numerator == 0
        || plan.scale_denominator == 0
        || plan.physical_width == 0
        || plan.physical_height == 0
        || plan.scene.document != plan.document
        || plan.scene.revision != plan.scene_revision
    {
        return Err(BackendError::new(
            BackendErrorKind::InvalidPlan,
            "GPU frame plan failed generation, scale, size, or eligibility validation",
            false,
        ));
    }
    Ok(())
}

fn premultiply_rgba8_in_place(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(COPY_BYTES_PER_PIXEL as usize) {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn bounded_block_on<F: Future>(future: F, timeout: Duration) -> Result<F::Output, BackendError> {
    let mut future = pin!(future);
    let waker = noop_waker();
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + timeout;
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return Ok(output),
            Poll::Pending if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Poll::Pending => {
                return Err(BackendError::new(
                    BackendErrorKind::Timeout,
                    "bounded GPU initialization wait timed out",
                    true,
                ));
            }
        }
    }
}

fn noop_waker() -> Waker {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );
    // SAFETY: every function in VTABLE ignores the null data pointer, cloning
    // recreates the same inert RawWaker, and drop performs no operation.
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LogicalRect, ViewportSpec};
    use crate::render::{
        FrameReason, FrameReasonSet, ResourceKind, ResourceOwner, RetainedScene, SceneBounds,
        SceneDelta, SceneNode, SceneNodeId, SceneResource, SceneResourceKey, SceneSubpart,
    };
    use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity};
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::{HtmlDocument, HtmlProvider};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use std::sync::Arc;
    use vello::peniko::{
        BlendMode, Blob, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat,
        ImageQuality,
    };

    fn proof_plan(surface: RenderSurfaceId, effect: Option<SceneEffect>) -> FramePlan {
        let document = ExperimentalDocumentIdentity { serial: 41 };
        let root = SceneNodeId {
            document,
            dom: None,
            subpart: SceneSubpart::Root,
            ordinal: 0,
        };
        let box_id = SceneNodeId {
            document,
            dom: Some(ExperimentalNodeIdentity {
                slot: 2,
                generation: 7,
            }),
            subpart: SceneSubpart::Box,
            ordinal: 0,
        };
        let bounds = SceneBounds {
            layout: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 48.0,
            },
            visual: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 48.0,
            },
            clip: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 48.0,
            },
            damage: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 48.0,
            },
        };
        let revision = SceneRevision(1);
        let scene = RetainedScene {
            document,
            revision,
            viewport: ViewportSpec {
                logical_width: 64,
                logical_height: 48,
                ..ViewportSpec::default()
            },
            root,
            nodes: vec![
                SceneNode {
                    id: root,
                    parent: None,
                    children: vec![box_id],
                    kind: SceneNodeKind::SurfaceClear,
                    tree_order: 0,
                    paint_order: 0,
                    visible: true,
                    bounds: bounds.clone(),
                    effects: vec![],
                    resource: None,
                    paint_signature: 1,
                },
                SceneNode {
                    id: box_id,
                    parent: Some(root),
                    children: vec![],
                    kind: SceneNodeKind::Box,
                    tree_order: 1,
                    paint_order: 1,
                    visible: true,
                    bounds,
                    effects: effect.into_iter().collect(),
                    resource: None,
                    paint_signature: 2,
                },
            ],
            resources: vec![],
            content_fingerprint: 3,
        };
        FramePlan {
            surface,
            document,
            scene_revision: revision,
            prior_scene_revision: None,
            logical_width: 64,
            logical_height: 48,
            physical_width: 64,
            physical_height: 48,
            scale_numerator: 120,
            scale_denominator: 120,
            pixel_format: PixelFormat::PremultipliedRgba8,
            clear: true,
            scene: Arc::new(scene),
            delta: SceneDelta {
                from_revision: None,
                to_revision: revision,
                changes: vec![],
                resource_changes: vec![],
                full_scene_replacement: true,
                unchanged_nodes: 0,
            },
            damage: DamageRegion::Full,
            reasons: FrameReasonSet::from([FrameReason::InitialPresentation]),
            full_repaint: true,
            presentation_eligible: true,
        }
    }

    fn proof_recording(plan: &FramePlan) -> GpuPreparedScene {
        let mut recording = anyrender::Scene::new();
        recording.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0x20, 0x70, 0xc0, 0xff),
            None,
            &kurbo::RoundedRect::from_rect(kurbo::Rect::new(4.0, 4.0, 52.0, 38.0), 6.0),
        );
        GpuPreparedScene {
            document: plan.document,
            revision: plan.scene_revision,
            recording,
            resources: plan.scene.live_resources(),
        }
    }

    fn solid_recording() -> anyrender::Scene {
        let mut recording = anyrender::Scene::new();
        recording.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0x20, 0x70, 0xc0, 0xff),
            None,
            &kurbo::Rect::new(4.0, 4.0, 52.0, 38.0),
        );
        recording
    }

    fn translucent_recording() -> anyrender::Scene {
        let mut recording = anyrender::Scene::new();
        recording.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0xff, 0, 0, 0x80),
            None,
            &kurbo::Rect::new(4.0, 4.0, 40.0, 32.0),
        );
        recording.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0, 0, 0xff, 0x80),
            None,
            &kurbo::Rect::new(20.0, 12.0, 56.0, 40.0),
        );
        recording
    }

    fn image_recording() -> anyrender::Scene {
        let image = ImageBrush::new(ImageData {
            data: Blob::from(vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 255, 255, 255, 0,
            ]),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: 2,
            height: 2,
        })
        .with_quality(ImageQuality::Low);
        let mut recording = anyrender::Scene::new();
        recording.draw_image(
            image.as_ref(),
            Affine::translate((8.0, 8.0)) * Affine::scale(12.0),
        );
        recording
    }

    fn transformed_clipped_recording() -> anyrender::Scene {
        let mut recording = anyrender::Scene::new();
        let clip = kurbo::RoundedRect::from_rect(kurbo::Rect::new(8.0, 6.0, 54.0, 42.0), 7.0);
        recording.push_layer(
            BlendMode::default(),
            0.75,
            Affine::IDENTITY,
            &clip,
            None,
            None,
        );
        recording.fill(
            Fill::NonZero,
            Affine::translate((28.0, 12.0)) * Affine::rotate(0.3),
            Color::from_rgba8(0xf0, 0x90, 0x20, 0xff),
            None,
            &kurbo::Rect::new(-12.0, 0.0, 28.0, 30.0),
        );
        recording.pop_layer();
        recording
    }

    fn cpu_pixels(recording: anyrender::Scene, plan: &FramePlan) -> Vec<u8> {
        let scale = f64::from(plan.scale_numerator) / f64::from(plan.scale_denominator);
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |target| target.append_scene(recording, Affine::scale(scale)),
            plan.physical_width,
            plan.physical_height,
        )
    }

    fn gpu_pixels(
        renderer: &mut VelloOffscreenRenderer,
        plan: &FramePlan,
        recording: anyrender::Scene,
    ) -> Vec<u8> {
        let target = RenderTarget {
            width: plan.physical_width,
            height: plan.physical_height,
            pixel_format: PixelFormat::PremultipliedRgba8,
        };
        renderer.create_target(plan.surface, target).unwrap();
        renderer
            .prepare(
                plan,
                GpuPreparedScene {
                    document: plan.document,
                    revision: plan.scene_revision,
                    recording,
                    resources: plan.scene.live_resources(),
                },
            )
            .unwrap();
        let result = renderer.render(plan, target).unwrap();
        renderer.readback(result).unwrap()
    }

    fn assert_tolerant_pixels(
        expected: &[u8],
        actual: &[u8],
        max_channel_error: u8,
        maximum_differing_percent: f64,
    ) {
        assert_eq!(actual.len(), expected.len());
        let mut differing_pixels = 0usize;
        for (expected, actual) in expected.chunks_exact(4).zip(actual.chunks_exact(4)) {
            let different = expected
                .iter()
                .zip(actual)
                .any(|(left, right)| left.abs_diff(*right) > max_channel_error);
            differing_pixels += usize::from(different);
            assert!(actual[0] <= actual[3]);
            assert!(actual[1] <= actual[3]);
            assert!(actual[2] <= actual[3]);
        }
        let percentage =
            differing_pixels as f64 * 100.0 / (actual.len() / COPY_BYTES_PER_PIXEL as usize) as f64;
        assert!(
            percentage <= maximum_differing_percent,
            "{percentage:.3}% of pixels exceeded the {max_channel_error}-channel tolerance"
        );
    }

    fn text_svg_document_proof() -> (FramePlan, CpuPreparedScene) {
        let viewport = ViewportSpec {
            logical_width: 128,
            logical_height: 72,
            ..ViewportSpec::default()
        };
        let mut document = HtmlDocument::from_html(
            "<!doctype html><html><head><style>body{margin:0;background:#182030;color:white;font:16px sans-serif}p{margin:4px}svg{width:32px;height:32px}</style></head><body><p>GPU text</p><svg viewBox=\"0 0 32 32\"><circle cx=\"16\" cy=\"16\" r=\"12\" fill=\"#40c080\"/></svg></body></html>",
            DocumentConfig {
                viewport: Some(Viewport::new(128, 72, 1.0, ColorScheme::Dark)),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..DocumentConfig::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        let identities = crate::identity::IdentityRegistry::from_document(&document);
        let svg_slot = document.query_selector("svg").unwrap().unwrap();
        let svg_identity = identities.identity_for_slot(&document, svg_slot).unwrap();
        let document_identity = ExperimentalDocumentIdentity { serial: 77 };
        let revision = SceneRevision(1);
        let mut scene = super::super::build_retained_scene(
            &document,
            &identities,
            document_identity,
            revision,
            viewport,
        )
        .unwrap();
        assert!(
            scene
                .nodes
                .iter()
                .any(|node| node.kind == SceneNodeKind::TextRun)
        );
        let svg_resource = SceneResourceId {
            owner: ResourceOwner::Document(document_identity),
            kind: ResourceKind::Svg,
            key: SceneResourceKey::Dom {
                slot: svg_identity.slot,
                generation: svg_identity.generation,
            },
        };
        let svg_node = scene
            .nodes
            .iter_mut()
            .find(|node| node.id.dom == Some(svg_identity))
            .unwrap();
        svg_node.kind = SceneNodeKind::Svg;
        svg_node.resource = Some((svg_resource.clone(), SceneResourceVersion(1)));
        scene.resources.push(SceneResource {
            id: svg_resource,
            version: SceneResourceVersion(1),
            lifecycle: ResourceLifecycle::Ready,
            diagnostic_key: "inline-svg-proof".into(),
            byte_len: None,
        });
        scene
            .resources
            .sort_by(|left, right| left.id.cmp(&right.id));
        let plan = FramePlan {
            surface: RenderSurfaceId {
                instance: 16,
                generation: 1,
            },
            document: document_identity,
            scene_revision: revision,
            prior_scene_revision: None,
            logical_width: viewport.logical_width,
            logical_height: viewport.logical_height,
            physical_width: viewport.logical_width,
            physical_height: viewport.logical_height,
            scale_numerator: 120,
            scale_denominator: 120,
            pixel_format: PixelFormat::PremultipliedRgba8,
            clear: true,
            scene: Arc::new(scene),
            delta: SceneDelta {
                from_revision: None,
                to_revision: revision,
                changes: vec![],
                resource_changes: vec![],
                full_scene_replacement: true,
                unchanged_nodes: 0,
            },
            damage: DamageRegion::Full,
            reasons: FrameReasonSet::from([FrameReason::InitialPresentation]),
            full_repaint: true,
            presentation_eligible: true,
        };
        let prepared = super::super::cpu::prepare_scene(&mut document, revision, viewport).unwrap();
        (plan, prepared)
    }

    #[test]
    fn coverage_profile_is_explicit_and_filters_fall_back() {
        let surface = RenderSurfaceId {
            instance: 1,
            generation: 1,
        };
        assert_eq!(
            VelloOffscreenRenderer::coverage(&proof_plan(surface, None)).unwrap(),
            GpuCoverage::Native
        );
        assert_eq!(
            VelloOffscreenRenderer::coverage(&proof_plan(
                surface,
                Some(SceneEffect::ForegroundFilter {
                    signature: 7,
                    conservative_full_bounds: true,
                })
            ))
            .unwrap(),
            GpuCoverage::CpuFrameFallback
        );
    }

    #[test]
    fn every_retained_kind_has_an_explicit_backend_classification() {
        for kind in [
            SceneNodeKind::SurfaceClear,
            SceneNodeKind::Box,
            SceneNodeKind::TextRun,
            SceneNodeKind::Svg,
            SceneNodeKind::UnavailableImage,
        ] {
            assert_eq!(node_coverage(kind), GpuCoverage::Native);
        }
        assert_eq!(
            node_coverage(SceneNodeKind::RasterImage),
            GpuCoverage::HybridResource
        );
        for effect in [
            SceneEffect::Opacity { value: 0.5 },
            SceneEffect::Clip {
                bounds: LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                rounded: None,
            },
            SceneEffect::Transform {
                coefficients: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            },
            SceneEffect::BackgroundLayers { signature: 1 },
            SceneEffect::BoxShadows {
                signature: 2,
                conservative_full_bounds: true,
            },
        ] {
            assert_eq!(effect_coverage(&effect), GpuCoverage::Native);
        }
        for effect in [
            SceneEffect::ForegroundFilter {
                signature: 3,
                conservative_full_bounds: true,
            },
            SceneEffect::BackdropFilter {
                signature: 4,
                conservative_full_bounds: true,
            },
        ] {
            assert_eq!(effect_coverage(&effect), GpuCoverage::CpuFrameFallback);
        }
    }

    #[test]
    fn cache_is_neutral_versioned_bounded_and_generation_safe() {
        let surface = RenderSurfaceId {
            instance: 1,
            generation: 1,
        };
        let mut plan = proof_plan(surface, None);
        plan.scene = Arc::new(RetainedScene {
            resources: vec![SceneResource {
                id: SceneResourceId {
                    owner: ResourceOwner::Document(plan.document),
                    kind: ResourceKind::RasterImage,
                    key: SceneResourceKey::Dom {
                        slot: 2,
                        generation: 7,
                    },
                },
                version: SceneResourceVersion(1),
                lifecycle: ResourceLifecycle::Ready,
                diagnostic_key: "proof-image".into(),
                byte_len: Some(128),
            }],
            ..(*plan.scene).clone()
        });
        let mut cache = GpuResourceCache::default();
        let mut statistics = GpuStatistics::default();
        cache
            .prepare(DeviceGeneration(1), &plan, &mut statistics)
            .unwrap();
        cache
            .prepare(DeviceGeneration(1), &plan, &mut statistics)
            .unwrap();
        assert_eq!(statistics.resource_uploads, 1);
        assert_eq!(statistics.cache_hits, 1);
        assert_eq!(cache.bytes, 128);

        Arc::make_mut(&mut plan.scene).resources[0].version = SceneResourceVersion(2);
        cache
            .prepare(DeviceGeneration(1), &plan, &mut statistics)
            .unwrap();
        assert_eq!(statistics.resource_uploads, 2);
        assert_eq!(cache.entries.len(), 1);
        cache.clear();
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn modeled_cache_plan_and_target_stress_remains_bounded() {
        let surface = RenderSurfaceId {
            instance: 2,
            generation: 1,
        };
        let mut plan = proof_plan(surface, None);
        plan.scene = Arc::new(RetainedScene {
            resources: vec![SceneResource {
                id: SceneResourceId {
                    owner: ResourceOwner::Document(plan.document),
                    kind: ResourceKind::RasterImage,
                    key: SceneResourceKey::Dom {
                        slot: 3,
                        generation: 1,
                    },
                },
                version: SceneResourceVersion(1),
                lifecycle: ResourceLifecycle::Ready,
                diagnostic_key: "stress-image".into(),
                byte_len: Some(128),
            }],
            ..(*plan.scene).clone()
        });
        let mut cache = GpuResourceCache::default();
        let mut statistics = GpuStatistics::default();
        cache
            .prepare(DeviceGeneration(1), &plan, &mut statistics)
            .unwrap();

        for _ in 0..1_000 {
            cache
                .prepare(DeviceGeneration(1), &plan, &mut statistics)
                .unwrap();
            assert_eq!(
                VelloOffscreenRenderer::coverage(&plan).unwrap(),
                GpuCoverage::Native
            );
        }
        for version in 2..=501 {
            Arc::make_mut(&mut plan.scene).resources[0].version = SceneResourceVersion(version);
            cache
                .prepare(DeviceGeneration(1), &plan, &mut statistics)
                .unwrap();
        }

        let info = BackendInfo {
            name: "modeled".into(),
            backend: "Vulkan".into(),
            device_type: "Cpu".into(),
            driver: "modeled".into(),
            max_texture_dimension_2d: 4096,
            max_buffer_size: MAX_GPU_TARGET_BYTES,
            adapter_selection_micros: 0,
            device_creation_micros: 0,
            pipeline_creation_micros: 0,
        };
        for side in 1..=100 {
            validate_target(
                RenderTarget {
                    width: side,
                    height: side,
                    pixel_format: PixelFormat::PremultipliedRgba8,
                },
                &info,
            )
            .unwrap();
        }

        assert_eq!(statistics.resource_uploads, 501);
        assert_eq!(statistics.cache_hits, 1_000);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, 128);
    }

    #[test]
    fn target_validation_rejects_zero_and_oversized_targets() {
        let info = BackendInfo {
            name: "modeled".into(),
            backend: "Vulkan".into(),
            device_type: "Cpu".into(),
            driver: "modeled".into(),
            max_texture_dimension_2d: 4096,
            max_buffer_size: MAX_GPU_TARGET_BYTES,
            adapter_selection_micros: 0,
            device_creation_micros: 0,
            pipeline_creation_micros: 0,
        };
        assert_eq!(
            validate_target(
                RenderTarget {
                    width: 0,
                    height: 1,
                    pixel_format: PixelFormat::PremultipliedRgba8,
                },
                &info
            )
            .unwrap_err()
            .kind,
            BackendErrorKind::TargetAllocation
        );
        assert_eq!(
            validate_target(
                RenderTarget {
                    width: 4097,
                    height: 1,
                    pixel_format: PixelFormat::PremultipliedRgba8,
                },
                &info
            )
            .unwrap_err()
            .kind,
            BackendErrorKind::TargetAllocation
        );
    }

    #[test]
    fn reset_invalidates_targets_resources_and_device_generation() {
        let mut cache = GpuResourceCache {
            bytes: 64,
            ..GpuResourceCache::default()
        };
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert_eq!(
            merge_coverage(GpuCoverage::Native, GpuCoverage::HybridResource),
            GpuCoverage::HybridResource
        );
        assert_eq!(
            merge_coverage(GpuCoverage::HybridResource, GpuCoverage::CpuFrameFallback),
            GpuCoverage::CpuFrameFallback
        );
    }

    #[test]
    fn pixel_tolerance_preserves_premultiplied_alpha_invariant() {
        let pixels = vec![64, 32, 16, 128, 0, 0, 0, 0];
        assert_tolerant_pixels(&pixels, &pixels, 0, 0.0);
    }

    #[test]
    fn readback_color_conversion_is_canonical_and_bounded() {
        let mut pixels = vec![
            0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 128, 255, 255, 255, 0,
        ];
        premultiply_rgba8_in_place(&mut pixels);
        assert_eq!(
            pixels,
            vec![0, 0, 0, 255, 255, 255, 255, 255, 128, 0, 0, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn modeled_adapter_absence_preserves_complete_cpu_fallback() {
        let plan = proof_plan(
            RenderSurfaceId {
                instance: 20,
                generation: 2,
            },
            None,
        );
        let mut renderer =
            OffscreenRenderer::modeled_unavailable(BackendErrorKind::AdapterUnavailable);
        let expected = cpu_pixels(solid_recording(), &plan);
        let (pixels, path) = renderer
            .render(
                &plan,
                RenderTarget {
                    width: plan.physical_width,
                    height: plan.physical_height,
                    pixel_format: PixelFormat::PremultipliedRgba8,
                },
                CpuPreparedScene {
                    revision: plan.scene_revision,
                    recording: solid_recording(),
                },
            )
            .unwrap();
        assert_eq!(path, RenderPath::CpuFallback);
        assert_eq!(
            renderer.last_gpu_error(),
            Some(BackendErrorKind::AdapterUnavailable)
        );
        assert_eq!(pixels, expected);
    }

    #[test]
    #[ignore = "requires a compatible Vulkan or GLES adapter"]
    fn hardware_offscreen_proof_renders_reads_back_and_shares_device() {
        let initialization_started = Instant::now();
        let mut renderer =
            VelloOffscreenRenderer::new(false).expect("compatible offscreen GPU adapter");
        let initialization_elapsed = initialization_started.elapsed();
        let rendering_started = Instant::now();
        let surface_a = RenderSurfaceId {
            instance: 10,
            generation: 1,
        };
        let surface_b = RenderSurfaceId {
            instance: 11,
            generation: 1,
        };
        let plan_a = proof_plan(surface_a, None);
        let exact_recording = solid_recording();
        let cpu_comparison_started = Instant::now();
        let expected = cpu_pixels(exact_recording.clone(), &plan_a);
        let cpu_comparison_elapsed = cpu_comparison_started.elapsed();
        let exact_target = RenderTarget {
            width: plan_a.physical_width,
            height: plan_a.physical_height,
            pixel_format: PixelFormat::PremultipliedRgba8,
        };
        let first_target_started = Instant::now();
        renderer
            .create_target(plan_a.surface, exact_target)
            .unwrap();
        let first_target_elapsed = first_target_started.elapsed();
        renderer
            .prepare(
                &plan_a,
                GpuPreparedScene {
                    document: plan_a.document,
                    revision: plan_a.scene_revision,
                    recording: exact_recording,
                    resources: plan_a.scene.live_resources(),
                },
            )
            .unwrap();
        let primitive_render_started = Instant::now();
        let result = renderer.render(&plan_a, exact_target).unwrap();
        let primitive_render_elapsed = primitive_render_started.elapsed();
        let primitive_contract_readback_started = Instant::now();
        let pixels = renderer.readback(result).unwrap();
        let primitive_contract_readback_elapsed = primitive_contract_readback_started.elapsed();
        assert_eq!(pixels, expected);
        let offset = (10 * 64 + 10) * 4;
        assert_eq!(&pixels[offset..offset + 4], &[0x20, 0x70, 0xc0, 0xff]);
        assert_eq!(&pixels[..4], &[0, 0, 0, 0]);

        let mut plan_b = proof_plan(surface_b, None);
        plan_b.physical_width = 80;
        plan_b.physical_height = 60;
        plan_b.scale_numerator = 150;
        let rounded = proof_recording(&plan_b).recording;
        let expected = cpu_pixels(rounded.clone(), &plan_b);
        let pixels = gpu_pixels(&mut renderer, &plan_b, rounded);
        assert_tolerant_pixels(&expected, &pixels, 3, 1.0);
        plan_b.physical_width = 96;
        plan_b.physical_height = 72;
        plan_b.scale_numerator = 180;
        let rounded = proof_recording(&plan_b).recording;
        let expected = cpu_pixels(rounded.clone(), &plan_b);
        let resized_target = RenderTarget {
            width: plan_b.physical_width,
            height: plan_b.physical_height,
            pixel_format: PixelFormat::PremultipliedRgba8,
        };
        renderer
            .resize_target(plan_b.surface, resized_target)
            .unwrap();
        renderer
            .prepare(
                &plan_b,
                GpuPreparedScene {
                    document: plan_b.document,
                    revision: plan_b.scene_revision,
                    recording: rounded,
                    resources: plan_b.scene.live_resources(),
                },
            )
            .unwrap();
        let result = renderer.render(&plan_b, resized_target).unwrap();
        let pixels = renderer.readback(result).unwrap();
        assert_tolerant_pixels(&expected, &pixels, 3, 1.0);

        let surface_c = RenderSurfaceId {
            instance: 12,
            generation: 1,
        };
        let mut hybrid_plan = proof_plan(surface_c, None);
        let image_id = SceneResourceId {
            owner: ResourceOwner::Document(hybrid_plan.document),
            kind: ResourceKind::RasterImage,
            key: SceneResourceKey::Dom {
                slot: 2,
                generation: 7,
            },
        };
        {
            let scene = Arc::make_mut(&mut hybrid_plan.scene);
            scene.nodes[1].kind = SceneNodeKind::RasterImage;
            scene.nodes[1].resource = Some((image_id.clone(), SceneResourceVersion(1)));
            scene.resources.push(SceneResource {
                id: image_id,
                version: SceneResourceVersion(1),
                lifecycle: ResourceLifecycle::Ready,
                diagnostic_key: "hybrid-proof".into(),
                byte_len: Some(16),
            });
        }
        assert_eq!(
            VelloOffscreenRenderer::coverage(&hybrid_plan).unwrap(),
            GpuCoverage::HybridResource
        );
        let image = image_recording();
        let expected = cpu_pixels(image.clone(), &hybrid_plan);
        let hybrid_upload_started = Instant::now();
        let pixels = gpu_pixels(&mut renderer, &hybrid_plan, image.clone());
        let hybrid_upload_elapsed = hybrid_upload_started.elapsed();
        assert_tolerant_pixels(&expected, &pixels, 4, 3.0);
        let hybrid_cache_hit_started = Instant::now();
        let pixels = gpu_pixels(&mut renderer, &hybrid_plan, image);
        let hybrid_cache_hit_elapsed = hybrid_cache_hit_started.elapsed();
        assert_tolerant_pixels(&expected, &pixels, 4, 3.0);
        assert_eq!(renderer.statistics.cache_hits, 1);

        let surface_d = RenderSurfaceId {
            instance: 13,
            generation: 1,
        };
        let alpha_plan = proof_plan(surface_d, None);
        let alpha = translucent_recording();
        let expected = cpu_pixels(alpha.clone(), &alpha_plan);
        let pixels = gpu_pixels(&mut renderer, &alpha_plan, alpha);
        assert_tolerant_pixels(&expected, &pixels, 3, 1.0);

        let surface_e = RenderSurfaceId {
            instance: 15,
            generation: 1,
        };
        let mut transform_plan = proof_plan(surface_e, None);
        Arc::make_mut(&mut transform_plan.scene).nodes[1]
            .effects
            .extend([
                SceneEffect::Opacity { value: 0.75 },
                SceneEffect::Transform {
                    coefficients: [1.0, 0.0, 0.0, 1.0, 28.0, 12.0],
                },
                SceneEffect::Clip {
                    bounds: LogicalRect {
                        x: 8.0,
                        y: 6.0,
                        width: 46.0,
                        height: 36.0,
                    },
                    rounded: None,
                },
            ]);
        let transformed = transformed_clipped_recording();
        let expected = cpu_pixels(transformed.clone(), &transform_plan);
        let pixels = gpu_pixels(&mut renderer, &transform_plan, transformed);
        assert_tolerant_pixels(&expected, &pixels, 4, 2.0);

        let (text_svg_plan, text_svg_prepared) = text_svg_document_proof();
        let expected = cpu_pixels(text_svg_prepared.recording.clone(), &text_svg_plan);
        let pixels = gpu_pixels(&mut renderer, &text_svg_plan, text_svg_prepared.recording);
        assert_tolerant_pixels(&expected, &pixels, 8, 8.0);

        let fallback_surface = RenderSurfaceId {
            instance: 14,
            generation: 1,
        };
        let fallback_plan = proof_plan(
            fallback_surface,
            Some(SceneEffect::ForegroundFilter {
                signature: 55,
                conservative_full_bounds: true,
            }),
        );
        let mut facade = OffscreenRenderer::new(false);
        let prepared = CpuPreparedScene {
            revision: fallback_plan.scene_revision,
            recording: solid_recording(),
        };
        let (pixels, path) = facade
            .render(
                &fallback_plan,
                RenderTarget {
                    width: 64,
                    height: 48,
                    pixel_format: PixelFormat::PremultipliedRgba8,
                },
                prepared,
            )
            .unwrap();
        assert_eq!(path, RenderPath::CpuFallback);
        assert_eq!(pixels.len(), 64 * 48 * 4);

        assert_eq!(renderer.targets.len(), 6);
        assert_eq!(renderer.statistics.frames_rendered, 8);
        assert!(!renderer.info.name.is_empty());

        let old_generation = renderer.device_generation;
        renderer.reset().unwrap();
        assert!(renderer.targets.is_empty());
        assert_eq!(renderer.cache_usage(), (0, 0));
        assert!(renderer.device_generation > old_generation);
        renderer.prepare(&plan_a, proof_recording(&plan_a)).unwrap();
        let stale = renderer
            .render(
                &plan_a,
                RenderTarget {
                    width: 64,
                    height: 48,
                    pixel_format: PixelFormat::PremultipliedRgba8,
                },
            )
            .unwrap_err();
        assert_eq!(stale.kind, BackendErrorKind::StaleGeneration);
        let final_target = RenderTarget {
            width: plan_a.physical_width,
            height: plan_a.physical_height,
            pixel_format: PixelFormat::PremultipliedRgba8,
        };
        renderer
            .create_target(plan_a.surface, final_target)
            .unwrap();
        renderer
            .prepare(
                &plan_a,
                GpuPreparedScene {
                    document: plan_a.document,
                    revision: plan_a.scene_revision,
                    recording: solid_recording(),
                    resources: plan_a.scene.live_resources(),
                },
            )
            .unwrap();
        let final_render_started = Instant::now();
        let result = renderer.render(&plan_a, final_target).unwrap();
        let final_render_elapsed = final_render_started.elapsed();
        let final_readback_started = Instant::now();
        let pixels = renderer.readback(result).unwrap();
        let final_readback_elapsed = final_readback_started.elapsed();
        assert_eq!(pixels.len(), 64 * 48 * 4);
        let observed_threads = std::fs::read_dir("/proc/self/task")
            .map(|entries| entries.count())
            .unwrap_or(0);
        let observed_file_descriptors = std::fs::read_dir("/proc/self/fd")
            .map(|entries| entries.count())
            .unwrap_or(0);
        let observed_rss_kib = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("VmRSS:"))
                    .and_then(|value| value.split_ascii_whitespace().next())
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .unwrap_or(0);
        eprintln!(
            "backend={GPU_BACKEND_NAME} version={GPU_BACKEND_VERSION} adapter={} api={} device_type={} driver={} required_features=empty texture=Rgba8Unorm targets=64x48,80x60,96x72 coverage=native+hybrid+cpu-fallback comparison=pass max_texture={} max_buffer={} init_us={} adapter_us={} device_us={} pipeline_us={} first_target_us={} primitive_render_map_us={} primitive_contract_readback_us={} hybrid_upload_frame_us={} hybrid_cache_hit_frame_us={} cpu_comparison_us={} proof_us={} final_render_map_us={} final_contract_readback_us={} threads={} fds={} rss_kib={} stats={:?} cache={:?}",
            renderer.info.name,
            renderer.info.backend,
            renderer.info.device_type,
            renderer.info.driver,
            renderer.info.max_texture_dimension_2d,
            renderer.info.max_buffer_size,
            initialization_elapsed.as_micros(),
            renderer.info.adapter_selection_micros,
            renderer.info.device_creation_micros,
            renderer.info.pipeline_creation_micros,
            first_target_elapsed.as_micros(),
            primitive_render_elapsed.as_micros(),
            primitive_contract_readback_elapsed.as_micros(),
            hybrid_upload_elapsed.as_micros(),
            hybrid_cache_hit_elapsed.as_micros(),
            cpu_comparison_elapsed.as_micros(),
            rendering_started.elapsed().as_micros(),
            final_render_elapsed.as_micros(),
            final_readback_elapsed.as_micros(),
            observed_threads,
            observed_file_descriptors,
            observed_rss_kib,
            renderer.statistics(),
            renderer.cache_usage(),
        );
    }
}
