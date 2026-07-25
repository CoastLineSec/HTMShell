use super::model::{
    FinitePeakVector, MAX_AUDIO_CHANNELS, MAX_LINKS, MAX_METADATA_VALUE_BYTES, MAX_NODE_PROPERTIES,
    MAX_NODE_TEXT_BYTES, MAX_NODES, MAX_PROPERTY_KEY_BYTES, MAX_PROPERTY_VALUE_BYTES,
    MAX_STAGED_DELTAS, PipeWireDelta, PipeWireLinkState, PipeWireNodeState, PipeWirePeakEvent,
    PipeWirePeakSamples, PipeWireResourceCounters, RawLinkInfo, RawNodeAudioInfo, RawNodeInfo,
};
use super::public::PipeWireDemand;
use htm_runtime::MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS;
use pipewire::context::ContextRc;
use pipewire::core::{CoreRc, PW_ID_CORE};
use pipewire::link::{Link, LinkListener};
use pipewire::main_loop::MainLoopRc;
use pipewire::metadata::{Metadata, MetadataListener};
use pipewire::node::{Node, NodeListener};
use pipewire::permissions::PermissionFlags;
use pipewire::properties::PropertiesBox;
use pipewire::registry::{GlobalObject, RegistryRc};
use pipewire::spa::param::ParamType;
use pipewire::spa::param::audio::{AudioFormat, AudioInfoRaw, AudioInfoRawFlags};
use pipewire::spa::pod::deserialize::PodDeserializer;
use pipewire::spa::pod::serialize::PodSerializer;
use pipewire::spa::pod::{Object, Pod, Property, Value, ValueArray};
use pipewire::spa::utils::Direction;
use pipewire::spa::utils::dict::DictRef;
use pipewire::stream::{StreamFlags, StreamListener, StreamRc, StreamState};
use pipewire::types::ObjectType;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::convert::TryInto;
use std::ffi::CStr;
use std::io::Cursor;
use std::os::fd::AsRawFd;
use std::rc::Rc;

pub(crate) const MAX_PIPEWIRE_ITERATIONS_PER_DISPATCH: usize = 8;
const CONFIGURED_DEFAULT_METADATA_TYPE: &str = "Spa:String:JSON";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfiguredDefaultProperty<'a> {
    subject: u32,
    key: &'a str,
    type_name: &'static str,
    value: Option<&'a str>,
}

fn configured_default_property<'a>(
    key: &'a str,
    value: Option<&'a str>,
) -> Result<ConfiguredDefaultProperty<'a>, String> {
    if !matches!(
        key,
        "default.configured.audio.sink" | "default.configured.audio.source"
    ) {
        return Err("unsupported PipeWire configured-default metadata key".into());
    }
    if value.is_some_and(|value| value.len() > MAX_METADATA_VALUE_BYTES || value.contains('\0')) {
        return Err("PipeWire configured-default metadata value is invalid".into());
    }
    Ok(ConfiguredDefaultProperty {
        subject: PW_ID_CORE,
        key,
        type_name: CONFIGURED_DEFAULT_METADATA_TYPE,
        value,
    })
}

fn dispatch_configured_default_property<'a>(
    key: &'a str,
    value: Option<&'a str>,
    dispatch: impl FnOnce(ConfiguredDefaultProperty<'a>),
) -> Result<(), String> {
    dispatch(configured_default_property(key, value)?);
    Ok(())
}

#[derive(Debug, Default)]
struct CallbackStaging {
    deltas: Vec<PipeWireDelta>,
    peak: usize,
    total: u64,
    overflowed: bool,
}

impl CallbackStaging {
    fn push(&mut self, delta: PipeWireDelta) {
        self.total = self.total.saturating_add(1);
        if self.deltas.len() >= MAX_STAGED_DELTAS {
            self.overflowed = true;
            return;
        }
        self.deltas.push(delta);
        self.peak = self.peak.max(self.deltas.len());
    }

    fn take(&mut self) -> Vec<PipeWireDelta> {
        let mut deltas = std::mem::take(&mut self.deltas);
        if std::mem::take(&mut self.overflowed) {
            deltas.push(PipeWireDelta::SourceError(format!(
                "callback staging exceeds {MAX_STAGED_DELTAS} deltas"
            )));
        }
        deltas
    }
}

struct BoundNode {
    _listener: NodeListener,
    _proxy: Node,
}

struct BoundLink {
    _listener: LinkListener,
    _proxy: Link,
}

struct BoundMetadata {
    _listener: MetadataListener,
    proxy: Metadata,
}

#[derive(Debug)]
struct PeakCallbackStaging {
    events: Vec<PipeWirePeakEvent>,
    latest: Vec<(u32, PipeWirePeakSamples)>,
    callbacks: u64,
    coalesced: u64,
}

impl Default for PeakCallbackStaging {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            latest: Vec::with_capacity(MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS),
            callbacks: 0,
            coalesced: 0,
        }
    }
}

impl PeakCallbackStaging {
    fn push_event(&mut self, event: PipeWirePeakEvent) {
        if self.events.len() < MAX_STAGED_DELTAS {
            self.events.push(event);
        }
    }

    fn push_samples(&mut self, raw_id: u32, samples: PipeWirePeakSamples) {
        self.callbacks = self.callbacks.saturating_add(1);
        if let Some((_, current)) = self.latest.iter_mut().find(|(id, _)| *id == raw_id) {
            *current = samples;
            self.coalesced = self.coalesced.saturating_add(1);
        } else if self.latest.len() < MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS {
            self.latest.push((raw_id, samples));
        }
    }

    fn take(&mut self) -> (Vec<PipeWirePeakEvent>, Vec<PipeWirePeakSamples>) {
        (
            std::mem::take(&mut self.events),
            self.latest.drain(..).map(|(_, samples)| samples).collect(),
        )
    }
}

struct PeakStreamUserData {
    raw_id: u32,
    stream_generation: u64,
    layout_generation: u64,
    format: AudioInfoRaw,
    staging: Rc<RefCell<PeakCallbackStaging>>,
}

struct BoundPeakStream {
    _listener: StreamListener<PeakStreamUserData>,
    _stream: StreamRc,
}

fn calculate_interleaved_peaks(
    bytes: &[u8],
    offset: usize,
    size: usize,
    stride: i32,
    channels: usize,
) -> Option<FinitePeakVector> {
    if channels == 0 || channels > MAX_AUDIO_CHANNELS {
        return None;
    }
    let expected_stride = channels.checked_mul(std::mem::size_of::<f32>())?;
    if stride > 0 && usize::try_from(stride).ok()? != expected_stride {
        return None;
    }
    let end = offset.checked_add(size)?;
    if end > bytes.len() || !size.is_multiple_of(std::mem::size_of::<f32>()) {
        return None;
    }
    let samples = &bytes[offset..end];
    let sample_count = samples.len() / std::mem::size_of::<f32>();
    if sample_count == 0 || !sample_count.is_multiple_of(channels) {
        return None;
    }
    let mut maxima = [0.0f32; MAX_AUDIO_CHANNELS];
    for (index, sample) in samples.chunks_exact(4).enumerate() {
        let value = f32::from_le_bytes(sample.try_into().expect("four-byte chunk"));
        if value.is_finite() {
            let channel = index % channels;
            maxima[channel] = maxima[channel].max(value.abs());
        }
    }
    FinitePeakVector::from_maxima(&maxima[..channels])
}

pub(crate) fn is_permission_denial(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("permission")
        || message.contains("access denied")
        || message.contains("not permitted")
}

#[derive(Clone)]
struct KnownNode {
    global: Rc<GlobalObject<PropertiesBox>>,
    properties: BTreeMap<String, String>,
    writable: bool,
}

#[derive(Clone)]
struct KnownLink {
    global: Rc<GlobalObject<PropertiesBox>>,
    source_node: Option<u32>,
    target_node: Option<u32>,
    source_port: Option<u32>,
    target_port: Option<u32>,
}

#[derive(Default)]
struct BoundObjects {
    known_nodes: HashMap<u32, KnownNode>,
    known_links: HashMap<u32, KnownLink>,
    known_metadata: HashMap<u32, Rc<GlobalObject<PropertiesBox>>>,
    nodes: HashMap<u32, BoundNode>,
    links: HashMap<u32, BoundLink>,
    metadata: HashMap<u32, BoundMetadata>,
    active_metadata: Option<u32>,
}

impl BoundObjects {
    fn update_counters(&self, counters: &mut PipeWireResourceCounters) {
        counters.node_proxy_count = self.nodes.len();
        counters.link_proxy_count = self.links.len();
        counters.metadata_proxy_count = self.metadata.len();
    }
}

pub(crate) struct PipeWireTransport {
    objects: Rc<RefCell<BoundObjects>>,
    _registry_listener: pipewire::registry::Listener,
    _core_listener: pipewire::core::Listener,
    _registry: RegistryRc,
    core: CoreRc,
    _context: ContextRc,
    main_loop: MainLoopRc,
    staging: Rc<RefCell<CallbackStaging>>,
    peak_staging: Rc<RefCell<PeakCallbackStaging>>,
    peak_streams: HashMap<u32, BoundPeakStream>,
    demand: Rc<RefCell<PipeWireDemand>>,
    resources: PipeWireResourceCounters,
}

impl PipeWireTransport {
    pub(crate) fn connect(generation: u64, demand: PipeWireDemand) -> Result<Self, String> {
        let main_loop =
            MainLoopRc::new(None).map_err(|error| format!("create PipeWire loop: {error}"))?;
        let context_properties = pipewire::properties::properties! {
            "module.rt" => "false"
        };
        let context = ContextRc::new(&main_loop, Some(context_properties))
            .map_err(|error| format!("create PipeWire context: {error}"))?;
        let core = context
            .connect_rc(None)
            .map_err(|error| format!("connect PipeWire core: {error}"))?;
        let registry = core
            .get_registry_rc()
            .map_err(|error| format!("get PipeWire registry: {error}"))?;
        let staging = Rc::new(RefCell::new(CallbackStaging::default()));
        let peak_staging = Rc::new(RefCell::new(PeakCallbackStaging::default()));
        let objects = Rc::new(RefCell::new(BoundObjects::default()));
        let demand = Rc::new(RefCell::new(demand));

        let core_staging = staging.clone();
        let core_listener = core
            .add_listener_local()
            .done(move |id, sequence| {
                if id == PW_ID_CORE {
                    core_staging
                        .borrow_mut()
                        .push(PipeWireDelta::CoreDone(sequence.seq()));
                }
            })
            .error({
                let staging = staging.clone();
                move |id, sequence, result, message| {
                    let message = format!(
                        "PipeWire object {id} sequence {sequence} failed with {result}: {message}"
                    );
                    if id == PW_ID_CORE {
                        staging.borrow_mut().push(PipeWireDelta::CoreError(message));
                    } else {
                        staging
                            .borrow_mut()
                            .push(PipeWireDelta::Diagnostic(message));
                    }
                }
            })
            .register();

        let registry_for_global = registry.clone();
        let objects_for_global = objects.clone();
        let staging_for_global = staging.clone();
        let demand_for_global = demand.clone();
        let registry_listener = registry
            .add_listener_local()
            .global(move |global| match &global.type_ {
                ObjectType::Node => register_node(
                    generation,
                    &registry_for_global,
                    global,
                    &objects_for_global,
                    &staging_for_global,
                    &demand_for_global,
                ),
                ObjectType::Link => register_link(
                    generation,
                    &registry_for_global,
                    global,
                    &objects_for_global,
                    &staging_for_global,
                    &demand_for_global,
                ),
                ObjectType::Metadata => register_metadata(
                    &registry_for_global,
                    global,
                    &objects_for_global,
                    &staging_for_global,
                    &demand_for_global,
                ),
                _ => {}
            })
            .global_remove({
                let objects = objects.clone();
                let staging = staging.clone();
                let demand = demand.clone();
                move |raw_id| {
                    let mut objects = objects.borrow_mut();
                    let node_known = objects.known_nodes.remove(&raw_id).is_some();
                    objects.nodes.remove(&raw_id);
                    if node_known && demand.borrow().nodes {
                        staging
                            .borrow_mut()
                            .push(PipeWireDelta::NodeRemoved(raw_id));
                    }
                    let link_known = objects.known_links.remove(&raw_id).is_some();
                    objects.links.remove(&raw_id);
                    if link_known && demand.borrow().links {
                        staging
                            .borrow_mut()
                            .push(PipeWireDelta::LinkRemoved(raw_id));
                    }
                    let metadata_known = objects.known_metadata.remove(&raw_id).is_some();
                    objects.metadata.remove(&raw_id);
                    if objects.active_metadata == Some(raw_id) {
                        objects.active_metadata = None;
                    }
                    if metadata_known && demand.borrow().defaults {
                        staging
                            .borrow_mut()
                            .push(PipeWireDelta::MetadataRemoved(raw_id));
                    }
                }
            })
            .register();

        Ok(Self {
            objects,
            _registry_listener: registry_listener,
            _core_listener: core_listener,
            _registry: registry,
            core,
            _context: context,
            main_loop,
            staging,
            peak_staging,
            peak_streams: HashMap::new(),
            demand,
            resources: PipeWireResourceCounters::default(),
        })
    }

    pub(crate) fn raw_fd(&self) -> i32 {
        self.main_loop.loop_().fd().as_raw_fd()
    }

    pub(crate) fn request_sync(&self, sequence: i32) -> Result<i32, String> {
        self.core
            .sync(sequence)
            .map(|sequence| sequence.seq())
            .map_err(|error| format!("request PipeWire synchronization: {error}"))
    }

    pub(crate) fn set_demand(&mut self, demand: PipeWireDemand) {
        let previous = self.demand.replace(demand.clone());
        let previous_needs_node_proxy =
            previous.node_details || previous.audio_state || previous.audio_writes;
        let needs_node_proxy = demand.node_details || demand.audio_state || demand.audio_writes;

        if previous.nodes != demand.nodes {
            let known = self
                .objects
                .borrow()
                .known_nodes
                .values()
                .cloned()
                .collect::<Vec<_>>();
            if demand.nodes {
                for node in known {
                    self.staging.borrow_mut().push(PipeWireDelta::NodeAdded {
                        raw_id: node.global.id,
                        properties: node.properties,
                    });
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::NodePermissions {
                            raw_id: node.global.id,
                            writable: node.writable,
                        });
                }
            } else {
                for node in known {
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::NodeRemoved(node.global.id));
                }
            }
        }

        if previous_needs_node_proxy != needs_node_proxy {
            if needs_node_proxy {
                let known = self
                    .objects
                    .borrow()
                    .known_nodes
                    .values()
                    .map(|node| node.global.clone())
                    .collect::<Vec<_>>();
                for global in known {
                    bind_node_proxy(
                        &self._registry,
                        &global,
                        &self.objects,
                        &self.staging,
                        demand.audio_state || demand.audio_writes,
                    );
                }
            } else {
                let ids = self
                    .objects
                    .borrow()
                    .nodes
                    .keys()
                    .copied()
                    .collect::<Vec<_>>();
                self.objects.borrow_mut().nodes.clear();
                for raw_id in ids {
                    self.staging.borrow_mut().push(PipeWireDelta::NodeTracking {
                        raw_id,
                        tracked: false,
                    });
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::NodeAudioTracking {
                            raw_id,
                            tracked: false,
                        });
                }
            }
        }

        let previous_audio = previous.audio_state || previous.audio_writes;
        let audio = demand.audio_state || demand.audio_writes;
        if previous_audio != audio {
            let objects = self.objects.borrow();
            for (raw_id, bound) in &objects.nodes {
                if audio {
                    bound._proxy.subscribe_params(&[ParamType::Props]);
                    bound
                        ._proxy
                        .enum_params(0, Some(ParamType::Props), 0, u32::MAX);
                } else {
                    bound._proxy.subscribe_params(&[]);
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::NodeAudioTracking {
                            raw_id: *raw_id,
                            tracked: false,
                        });
                }
            }
        }

        if previous.links != demand.links {
            let known = self
                .objects
                .borrow()
                .known_links
                .values()
                .cloned()
                .collect::<Vec<_>>();
            if demand.links {
                for link in known {
                    self.staging.borrow_mut().push(PipeWireDelta::LinkAdded {
                        raw_id: link.global.id,
                        source_node: link.source_node,
                        target_node: link.target_node,
                        source_port: link.source_port,
                        target_port: link.target_port,
                    });
                    bind_link_proxy(&self._registry, &link.global, &self.objects, &self.staging);
                }
            } else {
                self.objects.borrow_mut().links.clear();
                for link in known {
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::LinkRemoved(link.global.id));
                }
            }
        }

        if previous.defaults != demand.defaults {
            let mut known = self
                .objects
                .borrow()
                .known_metadata
                .values()
                .cloned()
                .collect::<Vec<_>>();
            known.sort_by_key(|global| global.id);
            if demand.defaults {
                for global in known {
                    bind_metadata_proxy(&self._registry, &global, &self.objects, &self.staging);
                }
            } else {
                let ids = self
                    .objects
                    .borrow()
                    .metadata
                    .keys()
                    .copied()
                    .collect::<Vec<_>>();
                let mut objects = self.objects.borrow_mut();
                objects.metadata.clear();
                objects.active_metadata = None;
                drop(objects);
                for raw_id in ids {
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::MetadataRemoved(raw_id));
                }
            }
        }
    }

    pub(crate) fn dispatch_nonblocking(&mut self) -> Result<usize, String> {
        let mut iterations = 0usize;
        for _ in 0..MAX_PIPEWIRE_ITERATIONS_PER_DISPATCH {
            let dispatched = self
                .main_loop
                .loop_()
                .iterate(pipewire::loop_::Timeout::None);
            if dispatched < 0 {
                return Err(format!(
                    "PipeWire loop iteration failed with result {dispatched}"
                ));
            }
            if dispatched == 0 {
                break;
            }
            iterations += 1;
        }
        self.resources.dispatch_iterations = self
            .resources
            .dispatch_iterations
            .saturating_add(iterations as u64);
        Ok(iterations)
    }

    pub(crate) fn take_staged(&mut self) -> Vec<PipeWireDelta> {
        let mut staging = self.staging.borrow_mut();
        self.resources.callbacks_staged = staging.total;
        self.resources.staged_delta_peak = self.resources.staged_delta_peak.max(staging.peak);
        staging.take()
    }

    pub(crate) fn take_peak_staged(
        &mut self,
    ) -> (Vec<PipeWirePeakEvent>, Vec<PipeWirePeakSamples>) {
        let staging = self.peak_staging.borrow();
        self.resources.peak_process_callbacks = staging.callbacks;
        self.resources.peak_callbacks_coalesced = staging.coalesced;
        drop(staging);
        self.peak_staging.borrow_mut().take()
    }

    pub(crate) fn resources(&self) -> PipeWireResourceCounters {
        let mut resources = self.resources.clone();
        self.objects.borrow().update_counters(&mut resources);
        resources.peak_stream_count = self.peak_streams.len();
        resources
    }

    pub(crate) fn record_peak_publications(&mut self, count: u64) {
        self.resources.peak_vectors_published =
            self.resources.peak_vectors_published.saturating_add(count);
    }

    pub(crate) fn record_peak_duplicate(&mut self) {
        self.resources.peak_duplicate_vectors_suppressed = self
            .resources
            .peak_duplicate_vectors_suppressed
            .saturating_add(1);
    }

    pub(crate) fn start_peak_stream(
        &mut self,
        raw_id: u32,
        stream_generation: u64,
        target: &str,
        capture_sink: bool,
    ) -> Result<(), String> {
        if self.peak_streams.contains_key(&raw_id) {
            return Ok(());
        }
        if target.is_empty() || target.len() > MAX_NODE_TEXT_BYTES || target.contains('\0') {
            return Err("PipeWire peak target is invalid".into());
        }
        let mut properties = pipewire::properties::properties! {
            "media.type" => "Audio",
            "media.category" => "Monitor",
            "media.name" => "Peak detect",
            "application.name" => "HTMShell Peak Detect",
            "stream.monitor" => "true",
            "target.object" => target,
        };
        if capture_sink {
            properties.insert("stream.capture.sink", "true");
        }
        let stream = StreamRc::new(self.core.clone(), "htmshell-peak-monitor", properties)
            .map_err(|error| format!("create PipeWire peak stream: {error}"))?;
        let user_data = PeakStreamUserData {
            raw_id,
            stream_generation,
            layout_generation: 0,
            format: AudioInfoRaw::new(),
            staging: self.peak_staging.clone(),
        };
        let listener = stream
            .add_local_listener_with_user_data(user_data)
            .state_changed(|_, data, _, new| match new {
                StreamState::Connecting => {
                    data.staging
                        .borrow_mut()
                        .push_event(PipeWirePeakEvent::Starting {
                            raw_id: data.raw_id,
                            stream_generation: data.stream_generation,
                        })
                }
                StreamState::Error(message) => {
                    data.staging
                        .borrow_mut()
                        .push_event(PipeWirePeakEvent::Failed {
                            raw_id: data.raw_id,
                            stream_generation: data.stream_generation,
                            denied: is_permission_denial(&message),
                        });
                }
                StreamState::Unconnected => {
                    data.staging
                        .borrow_mut()
                        .push_event(PipeWirePeakEvent::Failed {
                            raw_id: data.raw_id,
                            stream_generation: data.stream_generation,
                            denied: false,
                        });
                }
                StreamState::Paused | StreamState::Streaming => {}
            })
            .param_changed(|_, data, id, param| {
                if id != ParamType::Format.as_raw() {
                    return;
                }
                let Some(param) = param else {
                    data.format = AudioInfoRaw::new();
                    return;
                };
                let mut format = AudioInfoRaw::new();
                if format.parse(param).is_err()
                    || format.format() != AudioFormat::F32LE
                    || format.channels() == 0
                    || format.channels() as usize > MAX_AUDIO_CHANNELS
                {
                    data.staging
                        .borrow_mut()
                        .push_event(PipeWirePeakEvent::Failed {
                            raw_id: data.raw_id,
                            stream_generation: data.stream_generation,
                            denied: false,
                        });
                    return;
                }
                data.format = format;
                data.layout_generation = data.layout_generation.saturating_add(1);
                let channels = format.channels() as usize;
                let positions = if format.flags().contains(AudioInfoRawFlags::UNPOSITIONED) {
                    vec![0; channels]
                } else {
                    format.position()[..channels].to_vec()
                };
                data.staging
                    .borrow_mut()
                    .push_event(PipeWirePeakEvent::Format {
                        raw_id: data.raw_id,
                        stream_generation: data.stream_generation,
                        layout_generation: data.layout_generation,
                        positions,
                    });
            })
            .process(|stream, data| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let channels = data.format.channels() as usize;
                if data.format.format() != AudioFormat::F32LE
                    || channels == 0
                    || channels > MAX_AUDIO_CHANNELS
                {
                    return;
                }
                let Some(spa_data) = buffer.datas_mut().first_mut() else {
                    return;
                };
                let chunk = spa_data.chunk();
                let offset = chunk.offset() as usize;
                let size = chunk.size() as usize;
                let stride = chunk.stride();
                let Some(bytes) = spa_data.data() else {
                    return;
                };
                let Some(peaks) =
                    calculate_interleaved_peaks(bytes, offset, size, stride, channels)
                else {
                    return;
                };
                data.staging.borrow_mut().push_samples(
                    data.raw_id,
                    PipeWirePeakSamples {
                        raw_id: data.raw_id,
                        stream_generation: data.stream_generation,
                        layout_generation: data.layout_generation,
                        peaks,
                    },
                );
            })
            .register()
            .map_err(|error| format!("listen to PipeWire peak stream: {error}"))?;

        let mut audio_info = AudioInfoRaw::new();
        audio_info.set_format(AudioFormat::F32LE);
        let value = Value::Object(Object {
            type_: pipewire::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        });
        let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &value)
            .map_err(|error| format!("serialize peak stream format: {error:?}"))?
            .0
            .into_inner();
        let pod = Pod::from_bytes(&bytes)
            .ok_or_else(|| "serialized peak stream format is invalid".to_owned())?;
        let flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS;
        stream
            .connect(Direction::Input, None, flags, &mut [pod])
            .map_err(|error| format!("connect PipeWire peak stream: {error}"))?;
        self.peak_streams.insert(
            raw_id,
            BoundPeakStream {
                _listener: listener,
                _stream: stream,
            },
        );
        self.resources.peak_stream_starts = self.resources.peak_stream_starts.saturating_add(1);
        self.peak_staging
            .borrow_mut()
            .push_event(PipeWirePeakEvent::Starting {
                raw_id,
                stream_generation,
            });
        Ok(())
    }

    pub(crate) fn stop_peak_stream(&mut self, raw_id: u32) -> bool {
        if self.peak_streams.remove(&raw_id).is_some() {
            self.resources.peak_stream_stops = self.resources.peak_stream_stops.saturating_add(1);
            true
        } else {
            false
        }
    }

    pub(crate) fn stop_all_peak_streams(&mut self) {
        let count = self.peak_streams.len() as u64;
        self.peak_streams.clear();
        self.resources.peak_stream_stops = self.resources.peak_stream_stops.saturating_add(count);
    }

    pub(crate) fn set_node_mute(&self, raw_id: u32, muted: bool) -> Result<(), String> {
        self.set_node_properties(
            raw_id,
            vec![Property::new(
                pipewire::spa::sys::SPA_PROP_mute,
                Value::Bool(muted),
            )],
        )
    }

    pub(crate) fn set_node_channel_volumes(
        &self,
        raw_id: u32,
        volumes: Vec<f32>,
    ) -> Result<(), String> {
        if volumes.is_empty()
            || volumes.len() > MAX_AUDIO_CHANNELS
            || volumes
                .iter()
                .any(|volume| !volume.is_finite() || *volume < 0.0)
        {
            return Err("invalid PipeWire channel-volume vector".into());
        }
        self.set_node_properties(
            raw_id,
            vec![Property::new(
                pipewire::spa::sys::SPA_PROP_channelVolumes,
                Value::ValueArray(ValueArray::Float(volumes)),
            )],
        )
    }

    pub(crate) fn set_configured_default(
        &self,
        key: &str,
        value: Option<&str>,
    ) -> Result<(), String> {
        let objects = self.objects.borrow();
        let raw_id = objects
            .active_metadata
            .ok_or_else(|| "PipeWire default metadata is unavailable".to_owned())?;
        let metadata = objects
            .metadata
            .get(&raw_id)
            .ok_or_else(|| "PipeWire default metadata proxy is unavailable".to_owned())?;
        let writable = objects.known_metadata.get(&raw_id).is_some_and(|global| {
            global
                .permissions
                .contains(PermissionFlags::W | PermissionFlags::X)
        });
        if !writable {
            return Err("PipeWire default metadata is read-only".into());
        }
        dispatch_configured_default_property(key, value, |property| {
            metadata.proxy.set_property(
                property.subject,
                property.key,
                Some(property.type_name),
                property.value,
            );
        })
    }

    fn set_node_properties(&self, raw_id: u32, properties: Vec<Property>) -> Result<(), String> {
        let objects = self.objects.borrow();
        let known = objects
            .known_nodes
            .get(&raw_id)
            .ok_or_else(|| format!("PipeWire node {raw_id} is no longer present"))?;
        if !known.writable {
            return Err(format!("PipeWire node {raw_id} is not writable"));
        }
        let node = objects
            .nodes
            .get(&raw_id)
            .ok_or_else(|| format!("PipeWire node {raw_id} is not bound"))?;
        let bytes = serialize_node_properties(properties)?;
        let pod = Pod::from_bytes(&bytes)
            .ok_or_else(|| "serialized PipeWire property pod is invalid".to_owned())?;
        node._proxy.set_param(ParamType::Props, 0, pod);
        Ok(())
    }
}

fn serialize_node_properties(properties: Vec<Property>) -> Result<Vec<u8>, String> {
    let value = Value::Object(Object {
        type_: pipewire::spa::sys::SPA_TYPE_OBJECT_Props,
        id: pipewire::spa::sys::SPA_PARAM_Props,
        properties,
    });
    PodSerializer::serialize(Cursor::new(Vec::new()), &value)
        .map_err(|error| format!("serialize PipeWire node properties: {error:?}"))
        .map(|serialized| serialized.0.into_inner())
}

fn register_node(
    _generation: u64,
    registry: &RegistryRc,
    global: &pipewire::registry::GlobalObject<&DictRef>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
    demand: &Rc<RefCell<PipeWireDemand>>,
) {
    if objects.borrow().known_nodes.len() >= MAX_NODES {
        staging
            .borrow_mut()
            .push(PipeWireDelta::SourceError(format!(
                "node count exceeds {MAX_NODES}"
            )));
        return;
    }
    let properties = match bounded_dictionary(global.props.as_ref().copied()) {
        Ok(properties) => properties,
        Err(error) => {
            staging.borrow_mut().push(PipeWireDelta::SourceError(error));
            return;
        }
    };
    if properties.skipped > 0 {
        staging.borrow_mut().push(PipeWireDelta::Diagnostic(format!(
            "node {} skipped {} overlong properties",
            global.id, properties.skipped
        )));
    }
    let properties = properties.values;
    let global = Rc::new(global.to_owned());
    objects.borrow_mut().known_nodes.insert(
        global.id,
        KnownNode {
            global: global.clone(),
            properties: properties.clone(),
            writable: global
                .permissions
                .contains(PermissionFlags::W | PermissionFlags::X),
        },
    );
    if demand.borrow().nodes {
        staging.borrow_mut().push(PipeWireDelta::NodeAdded {
            raw_id: global.id,
            properties,
        });
        staging.borrow_mut().push(PipeWireDelta::NodePermissions {
            raw_id: global.id,
            writable: global
                .permissions
                .contains(PermissionFlags::W | PermissionFlags::X),
        });
    }
    if demand.borrow().node_details || demand.borrow().audio_state || demand.borrow().audio_writes {
        bind_node_proxy(
            registry,
            &global,
            objects,
            staging,
            demand.borrow().audio_state || demand.borrow().audio_writes,
        );
    }
}

fn bind_node_proxy(
    registry: &RegistryRc,
    global: &GlobalObject<PropertiesBox>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
    audio: bool,
) {
    if objects.borrow().nodes.contains_key(&global.id) {
        return;
    }
    if objects.borrow().nodes.len() >= MAX_NODES {
        staging
            .borrow_mut()
            .push(PipeWireDelta::SourceError(format!(
                "node proxy count exceeds {MAX_NODES}"
            )));
        return;
    }
    let node = match registry.bind::<Node, _>(global) {
        Ok(node) => node,
        Err(error) => {
            staging.borrow_mut().push(PipeWireDelta::Diagnostic(format!(
                "bind node {}: {error}",
                global.id
            )));
            return;
        }
    };
    let raw_id = global.id;
    let node_staging = staging.clone();
    let audio_staging = staging.clone();
    let listener = node
        .add_listener_local()
        .info(move |info| {
            let raw = info.as_raw();
            let state_error = if raw.state == pipewire::sys::pw_node_state_PW_NODE_STATE_ERROR
                && !raw.error.is_null()
            {
                // PipeWire owns a NUL-terminated error string for the duration
                // of this info callback.
                let value = unsafe { CStr::from_ptr(raw.error) }.to_string_lossy();
                (value.len() <= MAX_NODE_TEXT_BYTES).then(|| value.into_owned())
            } else {
                None
            };
            let properties = info
                .props()
                .map(|properties| bounded_dictionary(Some(properties)));
            match properties.transpose() {
                Ok(properties) => {
                    let skipped = properties
                        .as_ref()
                        .map(|properties| properties.skipped)
                        .unwrap_or_default();
                    if skipped > 0 {
                        node_staging
                            .borrow_mut()
                            .push(PipeWireDelta::Diagnostic(format!(
                                "node {raw_id} skipped {skipped} overlong properties"
                            )));
                    }
                    node_staging
                        .borrow_mut()
                        .push(PipeWireDelta::NodeInfo(RawNodeInfo {
                            raw_id,
                            state: node_state(raw.state),
                            raw_state: raw.state,
                            state_error,
                            input_ports: raw.n_input_ports,
                            output_ports: raw.n_output_ports,
                            properties: properties.map(|properties| properties.values),
                        }))
                }
                Err(error) => node_staging
                    .borrow_mut()
                    .push(PipeWireDelta::SourceError(error)),
            }
        })
        .param(move |_sequence, param_type, _index, _next, parameter| {
            if param_type != ParamType::Props {
                return;
            }
            let Some(parameter) = parameter else {
                audio_staging
                    .borrow_mut()
                    .push(PipeWireDelta::NodeAudioTracking {
                        raw_id,
                        tracked: false,
                    });
                return;
            };
            match parse_audio_properties(raw_id, parameter) {
                Ok(Some(info)) => audio_staging
                    .borrow_mut()
                    .push(PipeWireDelta::NodeAudioInfo(info)),
                Ok(None) => {}
                Err(message) => audio_staging
                    .borrow_mut()
                    .push(PipeWireDelta::Diagnostic(message)),
            }
        })
        .register();
    objects.borrow_mut().nodes.insert(
        global.id,
        BoundNode {
            _listener: listener,
            _proxy: node,
        },
    );
    if audio {
        let objects = objects.borrow();
        let node = &objects.nodes[&global.id]._proxy;
        node.subscribe_params(&[ParamType::Props]);
        node.enum_params(0, Some(ParamType::Props), 0, u32::MAX);
    }
}

fn register_link(
    _generation: u64,
    registry: &RegistryRc,
    global: &pipewire::registry::GlobalObject<&DictRef>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
    demand: &Rc<RefCell<PipeWireDemand>>,
) {
    if objects.borrow().known_links.len() >= MAX_LINKS {
        staging
            .borrow_mut()
            .push(PipeWireDelta::SourceError(format!(
                "link count exceeds {MAX_LINKS}"
            )));
        return;
    }
    let properties = match bounded_dictionary(global.props.as_ref().copied()) {
        Ok(properties) => properties,
        Err(error) => {
            staging.borrow_mut().push(PipeWireDelta::SourceError(error));
            return;
        }
    };
    if properties.skipped > 0 {
        staging.borrow_mut().push(PipeWireDelta::Diagnostic(format!(
            "link {} skipped {} overlong properties",
            global.id, properties.skipped
        )));
    }
    let source_node = parse_id(properties.values.get("link.output.node"));
    let target_node = parse_id(properties.values.get("link.input.node"));
    let source_port = parse_id(properties.values.get("link.output.port"));
    let target_port = parse_id(properties.values.get("link.input.port"));
    let global = Rc::new(global.to_owned());
    objects.borrow_mut().known_links.insert(
        global.id,
        KnownLink {
            global: global.clone(),
            source_node,
            target_node,
            source_port,
            target_port,
        },
    );
    if demand.borrow().links {
        staging.borrow_mut().push(PipeWireDelta::LinkAdded {
            raw_id: global.id,
            source_node,
            target_node,
            source_port,
            target_port,
        });
        bind_link_proxy(registry, &global, objects, staging);
    }
}

fn bind_link_proxy(
    registry: &RegistryRc,
    global: &GlobalObject<PropertiesBox>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
) {
    if objects.borrow().links.contains_key(&global.id) {
        return;
    }
    if objects.borrow().links.len() >= MAX_LINKS {
        staging
            .borrow_mut()
            .push(PipeWireDelta::SourceError(format!(
                "link proxy count exceeds {MAX_LINKS}"
            )));
        return;
    }
    let link = match registry.bind::<Link, _>(global) {
        Ok(link) => link,
        Err(error) => {
            staging.borrow_mut().push(PipeWireDelta::Diagnostic(format!(
                "bind link {}: {error}",
                global.id
            )));
            return;
        }
    };
    let raw_id = global.id;
    let link_staging = staging.clone();
    let listener = link
        .add_listener_local()
        .info(move |info| {
            let raw = info.as_raw();
            link_staging
                .borrow_mut()
                .push(PipeWireDelta::LinkInfo(RawLinkInfo {
                    raw_id,
                    source_node: valid_id(raw.output_node_id),
                    target_node: valid_id(raw.input_node_id),
                    source_port: valid_id(raw.output_port_id),
                    target_port: valid_id(raw.input_port_id),
                    state: link_state(raw.state),
                    raw_state: raw.state,
                }));
        })
        .register();
    objects.borrow_mut().links.insert(
        global.id,
        BoundLink {
            _listener: listener,
            _proxy: link,
        },
    );
}

fn register_metadata(
    registry: &RegistryRc,
    global: &pipewire::registry::GlobalObject<&DictRef>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
    demand: &Rc<RefCell<PipeWireDemand>>,
) {
    let metadata_name = global
        .props
        .as_ref()
        .and_then(|properties| properties.get("metadata.name"));
    if metadata_name != Some("default") {
        return;
    }
    let global = Rc::new(global.to_owned());
    objects
        .borrow_mut()
        .known_metadata
        .insert(global.id, global.clone());
    if demand.borrow().defaults {
        bind_metadata_proxy(registry, &global, objects, staging);
    }
}

fn bind_metadata_proxy(
    registry: &RegistryRc,
    global: &GlobalObject<PropertiesBox>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
) {
    if objects.borrow().active_metadata == Some(global.id)
        && objects.borrow().metadata.contains_key(&global.id)
    {
        return;
    }
    let metadata = match registry.bind::<Metadata, _>(global) {
        Ok(metadata) => metadata,
        Err(error) => {
            staging.borrow_mut().push(PipeWireDelta::Diagnostic(format!(
                "bind metadata {}: {error}",
                global.id
            )));
            return;
        }
    };
    staging.borrow_mut().push(PipeWireDelta::MetadataAdded {
        raw_id: global.id,
        writable: global
            .permissions
            .contains(PermissionFlags::W | PermissionFlags::X),
    });
    let raw_id = global.id;
    let metadata_staging = staging.clone();
    let listener = metadata
        .add_listener_local()
        .property(move |subject, key, type_name, value| {
            let key = bounded_optional(key, MAX_PROPERTY_KEY_BYTES);
            let type_name = bounded_optional(type_name, MAX_PROPERTY_KEY_BYTES);
            let value = bounded_optional(value, MAX_METADATA_VALUE_BYTES);
            match (key, type_name, value) {
                (Ok(key), Ok(type_name), Ok(value)) => {
                    metadata_staging
                        .borrow_mut()
                        .push(PipeWireDelta::MetadataProperty {
                            raw_id,
                            subject,
                            key,
                            type_name,
                            value,
                        });
                }
                _ => metadata_staging
                    .borrow_mut()
                    .push(PipeWireDelta::Diagnostic(
                        "default metadata field exceeds its bound".into(),
                    )),
            }
            0
        })
        .register();
    let mut objects = objects.borrow_mut();
    objects.metadata.clear();
    objects.active_metadata = Some(global.id);
    objects.metadata.insert(
        global.id,
        BoundMetadata {
            _listener: listener,
            proxy: metadata,
        },
    );
}

struct BoundedDictionary {
    values: BTreeMap<String, String>,
    skipped: usize,
}

fn bounded_dictionary(dict: Option<&DictRef>) -> Result<BoundedDictionary, String> {
    let mut values = BTreeMap::new();
    let mut skipped = 0usize;
    let Some(dict) = dict else {
        return Ok(BoundedDictionary { values, skipped });
    };
    for (key, value) in dict.iter() {
        if key.len() > MAX_PROPERTY_KEY_BYTES
            || value.len() > MAX_PROPERTY_VALUE_BYTES
            || !key.chars().all(|character| !character.is_control())
            || !value.chars().all(|character| !character.is_control())
        {
            skipped = skipped.saturating_add(1);
            continue;
        }
        values
            .entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
        if values.len() > MAX_NODE_PROPERTIES {
            return Err(format!(
                "property count exceeds {MAX_NODE_PROPERTIES} entries"
            ));
        }
    }
    Ok(BoundedDictionary { values, skipped })
}

fn bounded_optional(value: Option<&str>, maximum: usize) -> Result<Option<String>, ()> {
    value
        .map(|value| {
            if value.len() > maximum {
                Err(())
            } else {
                Ok(value.to_owned())
            }
        })
        .transpose()
}

fn parse_audio_properties(
    raw_id: u32,
    parameter: &Pod,
) -> Result<Option<RawNodeAudioInfo>, String> {
    let (_, value) = PodDeserializer::deserialize_any_from(parameter.as_bytes())
        .map_err(|_| format!("node {raw_id} returned malformed audio properties"))?;
    let Value::Object(object) = value else {
        return Err(format!(
            "node {raw_id} returned non-object audio properties"
        ));
    };
    if object.id != pipewire::spa::sys::SPA_PARAM_Props {
        return Ok(None);
    }
    let mut muted = None;
    let mut channel_volumes = None;
    let mut channel_positions = None;
    for property in object.properties {
        match (property.key, property.value) {
            (pipewire::spa::sys::SPA_PROP_mute, Value::Bool(value)) => muted = Some(value),
            (
                pipewire::spa::sys::SPA_PROP_channelVolumes,
                Value::ValueArray(ValueArray::Float(values)),
            ) => {
                if values.len() > MAX_AUDIO_CHANNELS
                    || values
                        .iter()
                        .any(|value| !value.is_finite() || *value < 0.0)
                {
                    return Err(format!("node {raw_id} returned invalid channel volumes"));
                }
                channel_volumes = Some(values);
            }
            (
                pipewire::spa::sys::SPA_PROP_channelMap,
                Value::ValueArray(ValueArray::Id(values)),
            ) => {
                if values.len() > MAX_AUDIO_CHANNELS {
                    return Err(format!("node {raw_id} returned an excessive channel map"));
                }
                channel_positions = Some(values.into_iter().map(|value| value.0).collect());
            }
            (pipewire::spa::sys::SPA_PROP_mute, _)
            | (pipewire::spa::sys::SPA_PROP_channelVolumes, _)
            | (pipewire::spa::sys::SPA_PROP_channelMap, _) => {
                return Err(format!(
                    "node {raw_id} returned an invalid audio property type"
                ));
            }
            _ => {}
        }
    }
    if muted.is_none() && channel_volumes.is_none() && channel_positions.is_none() {
        return Ok(None);
    }
    Ok(Some(RawNodeAudioInfo {
        raw_id,
        channel_volumes,
        channel_positions,
        muted,
    }))
}

fn parse_id(value: Option<&String>) -> Option<u32> {
    value
        .and_then(|value| value.parse().ok())
        .and_then(valid_id)
}

fn valid_id(value: u32) -> Option<u32> {
    (value != pipewire::constants::ID_ANY).then_some(value)
}

fn node_state(raw: i32) -> PipeWireNodeState {
    match raw {
        pipewire::sys::pw_node_state_PW_NODE_STATE_ERROR => PipeWireNodeState::Error,
        pipewire::sys::pw_node_state_PW_NODE_STATE_CREATING => PipeWireNodeState::Creating,
        pipewire::sys::pw_node_state_PW_NODE_STATE_SUSPENDED => PipeWireNodeState::Suspended,
        pipewire::sys::pw_node_state_PW_NODE_STATE_IDLE => PipeWireNodeState::Idle,
        pipewire::sys::pw_node_state_PW_NODE_STATE_RUNNING => PipeWireNodeState::Running,
        _ => PipeWireNodeState::Unknown,
    }
}

fn link_state(raw: i32) -> PipeWireLinkState {
    match raw {
        pipewire::sys::pw_link_state_PW_LINK_STATE_ERROR => PipeWireLinkState::Error,
        pipewire::sys::pw_link_state_PW_LINK_STATE_UNLINKED => PipeWireLinkState::Unlinked,
        pipewire::sys::pw_link_state_PW_LINK_STATE_INIT => PipeWireLinkState::Init,
        pipewire::sys::pw_link_state_PW_LINK_STATE_NEGOTIATING => PipeWireLinkState::Negotiating,
        pipewire::sys::pw_link_state_PW_LINK_STATE_ALLOCATING => PipeWireLinkState::Allocating,
        pipewire::sys::pw_link_state_PW_LINK_STATE_PAUSED => PipeWireLinkState::Paused,
        pipewire::sys::pw_link_state_PW_LINK_STATE_ACTIVE => PipeWireLinkState::Active,
        _ => PipeWireLinkState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_mappings_contain_unknown_future_values() {
        assert_eq!(node_state(i32::MAX), PipeWireNodeState::Unknown);
        assert_eq!(link_state(i32::MAX), PipeWireLinkState::Unknown);
    }

    #[test]
    fn callback_staging_is_bounded() {
        let mut staging = CallbackStaging::default();
        for _ in 0..=MAX_STAGED_DELTAS {
            staging.push(PipeWireDelta::CoreDone(1));
        }
        let deltas = staging.take();
        assert_eq!(deltas.len(), MAX_STAGED_DELTAS + 1);
        assert!(matches!(deltas.last(), Some(PipeWireDelta::SourceError(_))));
    }

    #[test]
    fn duplicate_dictionary_keys_use_pipewire_lookup_order() {
        let properties =
            super::super::model::bounded_properties([("key", "first"), ("key", "second")]).unwrap();
        assert_eq!(properties["key"], "first");
    }

    #[test]
    fn dispatch_iteration_bound_is_finite() {
        assert_eq!(MAX_PIPEWIRE_ITERATIONS_PER_DISPATCH, 8);
    }

    #[test]
    fn configured_default_write_spec_is_exact_and_bounded() {
        let mut sink = None;
        dispatch_configured_default_property(
            "default.configured.audio.sink",
            Some(r#"{"name":"alsa_output.test"}"#),
            |property| sink = Some(property),
        )
        .unwrap();
        let sink = sink.unwrap();
        assert_eq!(sink.subject, PW_ID_CORE);
        assert_eq!(sink.key, "default.configured.audio.sink");
        assert_eq!(sink.type_name, "Spa:String:JSON");
        assert_eq!(sink.value, Some(r#"{"name":"alsa_output.test"}"#));

        let mut source = None;
        dispatch_configured_default_property("default.configured.audio.source", None, |property| {
            source = Some(property)
        })
        .unwrap();
        let source = source.unwrap();
        assert_eq!(source.subject, PW_ID_CORE);
        assert_eq!(source.key, "default.configured.audio.source");
        assert_eq!(source.type_name, "Spa:String:JSON");
        assert_eq!(source.value, None);

        assert!(configured_default_property("default.audio.sink", None).is_err());
        assert!(
            configured_default_property("default.configured.audio.sink", Some("bad\0value"))
                .is_err()
        );
        assert!(
            configured_default_property(
                "default.configured.audio.sink",
                Some(&"x".repeat(MAX_METADATA_VALUE_BYTES + 1)),
            )
            .is_err()
        );
    }

    #[test]
    fn exact_mute_and_channel_volume_pods_round_trip_through_the_transport_parser() {
        let bytes = serialize_node_properties(vec![
            Property::new(pipewire::spa::sys::SPA_PROP_mute, Value::Bool(true)),
            Property::new(
                pipewire::spa::sys::SPA_PROP_channelVolumes,
                Value::ValueArray(ValueArray::Float(vec![1.0, 0.125])),
            ),
            Property::new(
                pipewire::spa::sys::SPA_PROP_channelMap,
                Value::ValueArray(ValueArray::Id(vec![
                    pipewire::spa::utils::Id(3),
                    pipewire::spa::utils::Id(4),
                ])),
            ),
        ])
        .unwrap();
        let pod = Pod::from_bytes(&bytes).unwrap();
        let info = parse_audio_properties(42, pod).unwrap().unwrap();
        assert_eq!(info.raw_id, 42);
        assert_eq!(info.muted, Some(true));
        assert_eq!(info.channel_volumes, Some(vec![1.0, 0.125]));
        assert_eq!(info.channel_positions, Some(vec![3, 4]));
    }

    #[test]
    fn malformed_audio_parameters_are_contained() {
        let bytes = serialize_node_properties(vec![Property::new(
            pipewire::spa::sys::SPA_PROP_channelVolumes,
            Value::ValueArray(ValueArray::Float(vec![f32::NAN])),
        )])
        .unwrap();
        let pod = Pod::from_bytes(&bytes).unwrap();
        assert!(parse_audio_properties(7, pod).is_err());
    }

    #[test]
    fn peak_callback_calculation_is_ordered_perceptual_and_bounded() {
        let samples = [0.0f32, -0.125, 1.0, 0.064, f32::NAN, -8.0];
        let bytes = samples
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let peaks = calculate_interleaved_peaks(&bytes, 0, bytes.len(), 8, 2).unwrap();
        let peaks = peaks.as_slice();
        assert_eq!(peaks.len(), 2);
        assert!((peaks[0].get() - 1.0).abs() < f32::EPSILON);
        assert!((peaks[1].get() - 2.0).abs() < f32::EPSILON);

        assert!(calculate_interleaved_peaks(&[], 0, 0, 8, 2).is_none());
        assert!(calculate_interleaved_peaks(&bytes, 0, bytes.len(), 4, 2).is_none());
        assert!(calculate_interleaved_peaks(&bytes, 1, bytes.len(), 8, 2).is_none());
        assert!(calculate_interleaved_peaks(&bytes, 0, bytes.len() - 4, 8, 2).is_none());
        assert!(
            calculate_interleaved_peaks(&bytes, 0, bytes.len(), 8, MAX_AUDIO_CHANNELS + 1)
                .is_none()
        );
    }

    #[test]
    fn peak_callback_staging_retains_only_the_latest_vector_per_node() {
        let mut staging = PeakCallbackStaging::default();
        for value in 0..1_000u32 {
            staging.push_samples(
                42,
                PipeWirePeakSamples {
                    raw_id: 42,
                    stream_generation: 7,
                    layout_generation: 3,
                    peaks: FinitePeakVector::from_perceptual(&[value as f32]).unwrap(),
                },
            );
        }
        staging.push_samples(
            43,
            PipeWirePeakSamples {
                raw_id: 43,
                stream_generation: 8,
                layout_generation: 1,
                peaks: FinitePeakVector::from_perceptual(&[0.5]).unwrap(),
            },
        );
        let (events, samples) = staging.take();
        assert!(events.is_empty());
        assert_eq!(samples.len(), 2);
        assert_eq!(staging.callbacks, 1_001);
        assert_eq!(staging.coalesced, 999);
        assert!(
            samples
                .iter()
                .any(|sample| { sample.raw_id == 42 && sample.peaks.as_slice()[0].get() == 999.0 })
        );
    }

    #[test]
    fn peak_permission_errors_are_classified_without_guessing_other_failures() {
        assert!(is_permission_denial("Permission denied"));
        assert!(is_permission_denial("access denied"));
        assert!(is_permission_denial("operation not permitted"));
        assert!(!is_permission_denial("format negotiation failed"));
        assert!(!is_permission_denial("node was removed"));
    }
}
