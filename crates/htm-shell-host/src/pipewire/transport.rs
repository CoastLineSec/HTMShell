use super::model::{
    MAX_LINKS, MAX_METADATA_VALUE_BYTES, MAX_NODE_PROPERTIES, MAX_NODE_TEXT_BYTES, MAX_NODES,
    MAX_PROPERTY_KEY_BYTES, MAX_PROPERTY_VALUE_BYTES, MAX_STAGED_DELTAS, PipeWireDelta,
    PipeWireLinkState, PipeWireNodeState, PipeWireResourceCounters, RawLinkInfo, RawNodeInfo,
};
use super::public::PipeWireDemand;
use pipewire::context::ContextRc;
use pipewire::core::{CoreRc, PW_ID_CORE};
use pipewire::link::{Link, LinkListener};
use pipewire::main_loop::MainLoopRc;
use pipewire::metadata::{Metadata, MetadataListener};
use pipewire::node::{Node, NodeListener};
use pipewire::properties::PropertiesBox;
use pipewire::registry::{GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ffi::CStr;
use std::os::fd::AsRawFd;
use std::rc::Rc;

pub(crate) const MAX_PIPEWIRE_ITERATIONS_PER_DISPATCH: usize = 8;

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
    _proxy: Metadata,
}

#[derive(Clone)]
struct KnownNode {
    global: Rc<GlobalObject<PropertiesBox>>,
    properties: BTreeMap<String, String>,
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
                    staging.borrow_mut().push(PipeWireDelta::CoreError(format!(
                        "core object {id} sequence {sequence} failed with {result}: {message}"
                    )));
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
                }
            } else {
                for node in known {
                    self.staging
                        .borrow_mut()
                        .push(PipeWireDelta::NodeRemoved(node.global.id));
                }
            }
        }

        if previous.node_details != demand.node_details {
            if demand.node_details {
                let known = self
                    .objects
                    .borrow()
                    .known_nodes
                    .values()
                    .map(|node| node.global.clone())
                    .collect::<Vec<_>>();
                for global in known {
                    bind_node_proxy(&self._registry, &global, &self.objects, &self.staging);
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
            let known = self
                .objects
                .borrow()
                .known_metadata
                .values()
                .cloned()
                .collect::<Vec<_>>();
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
                self.objects.borrow_mut().metadata.clear();
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

    pub(crate) fn resources(&self) -> PipeWireResourceCounters {
        let mut resources = self.resources.clone();
        self.objects.borrow().update_counters(&mut resources);
        resources
    }
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
        },
    );
    if demand.borrow().nodes {
        staging.borrow_mut().push(PipeWireDelta::NodeAdded {
            raw_id: global.id,
            properties,
        });
    }
    if demand.borrow().node_details {
        bind_node_proxy(registry, &global, objects, staging);
    }
}

fn bind_node_proxy(
    registry: &RegistryRc,
    global: &GlobalObject<PropertiesBox>,
    objects: &Rc<RefCell<BoundObjects>>,
    staging: &Rc<RefCell<CallbackStaging>>,
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
        .register();
    objects.borrow_mut().nodes.insert(
        global.id,
        BoundNode {
            _listener: listener,
            _proxy: node,
        },
    );
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
    if objects.borrow().metadata.contains_key(&global.id) {
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
    staging
        .borrow_mut()
        .push(PipeWireDelta::MetadataAdded { raw_id: global.id });
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
    objects.borrow_mut().metadata.insert(
        global.id,
        BoundMetadata {
            _listener: listener,
            _proxy: metadata,
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
}
