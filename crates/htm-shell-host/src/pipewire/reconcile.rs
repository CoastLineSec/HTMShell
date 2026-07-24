use super::model::{
    MAX_LINK_GROUPS, MAX_LINKS, MAX_METADATA_VALUE_BYTES, MAX_NODES, PipeWireAvailability,
    PipeWireDefaultTarget, PipeWireDefaultsSnapshot, PipeWireDelta, PipeWireLinkGroupId,
    PipeWireLinkGroupSnapshot, PipeWireLinkId, PipeWireLinkSnapshot, PipeWireLinkState,
    PipeWireModelError, PipeWireNodeClassification, PipeWireNodeId, PipeWireNodeSnapshot,
    PipeWireNodeState, PipeWireResourceCounters, PipeWireSnapshot, RawLinkInfo, RawNodeInfo,
    bounded_text,
};
use super::public::PipeWireNodeType;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_AUDIO_SINK: &str = "default.audio.sink";
const DEFAULT_AUDIO_SOURCE: &str = "default.audio.source";
const DEFAULT_CONFIGURED_AUDIO_SINK: &str = "default.configured.audio.sink";
const DEFAULT_CONFIGURED_AUDIO_SOURCE: &str = "default.configured.audio.source";
const METADATA_CORE_SUBJECT: u32 = 0;

#[derive(Debug, Clone)]
struct NodeRecord {
    raw_id: u32,
    registry_properties: BTreeMap<String, String>,
    detail_properties: Option<BTreeMap<String, String>>,
    state: PipeWireNodeState,
    raw_state: i32,
    state_error: Option<String>,
    input_ports: u32,
    output_ports: u32,
    ready: bool,
}

#[derive(Debug, Clone)]
struct LinkRecord {
    raw_id: u32,
    source_node: Option<u32>,
    target_node: Option<u32>,
    source_port: Option<u32>,
    target_port: Option<u32>,
    state: PipeWireLinkState,
    raw_state: i32,
    ready: bool,
}

#[derive(Debug, Clone, Default)]
struct MetadataValue {
    name: Option<String>,
    unresolved_value: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct PipeWireReconciler {
    generation: u64,
    availability: PipeWireAvailability,
    nodes: BTreeMap<u32, NodeRecord>,
    links: BTreeMap<u32, LinkRecord>,
    group_representatives: BTreeMap<(u32, u32), u32>,
    active_metadata: Option<u32>,
    metadata: BTreeMap<String, MetadataValue>,
    sequence: u64,
    resources: PipeWireResourceCounters,
    current: PipeWireSnapshot,
}

impl PipeWireReconciler {
    pub(crate) fn current(&self) -> &PipeWireSnapshot {
        &self.current
    }

    pub(crate) fn begin_generation(
        &mut self,
        generation: u64,
    ) -> Result<Option<PipeWireSnapshot>, PipeWireModelError> {
        self.generation = generation;
        self.availability = PipeWireAvailability::Synchronizing;
        self.nodes.clear();
        self.links.clear();
        self.group_representatives.clear();
        self.active_metadata = None;
        self.metadata.clear();
        self.publish_if_changed()
    }

    pub(crate) fn mark_ready(&mut self) -> Result<Option<PipeWireSnapshot>, PipeWireModelError> {
        self.availability = PipeWireAvailability::Ready;
        self.publish_if_changed()
    }

    pub(crate) fn mark_unavailable(
        &mut self,
    ) -> Result<Option<PipeWireSnapshot>, PipeWireModelError> {
        self.availability = PipeWireAvailability::Unavailable;
        self.nodes.clear();
        self.links.clear();
        self.group_representatives.clear();
        self.active_metadata = None;
        self.metadata.clear();
        self.publish_if_changed()
    }

    pub(crate) fn update_transport_counters(&mut self, resources: &PipeWireResourceCounters) {
        self.resources.node_proxy_count = resources.node_proxy_count;
        self.resources.link_proxy_count = resources.link_proxy_count;
        self.resources.metadata_proxy_count = resources.metadata_proxy_count;
        self.resources.staged_delta_peak = self
            .resources
            .staged_delta_peak
            .max(resources.staged_delta_peak);
        self.resources.dispatch_iterations = resources.dispatch_iterations;
        self.resources.callbacks_staged = resources.callbacks_staged;
        self.resources.reconnect_attempts = resources.reconnect_attempts;
        self.current.resources = self.resources.clone();
    }

    pub(crate) fn record_diagnostics(&mut self, count: usize) {
        self.resources.diagnostics_contained = self
            .resources
            .diagnostics_contained
            .saturating_add(count as u64);
        self.current.resources.diagnostics_contained = self.resources.diagnostics_contained;
    }

    pub(crate) fn apply(
        &mut self,
        deltas: impl IntoIterator<Item = PipeWireDelta>,
    ) -> Result<Option<PipeWireSnapshot>, PipeWireModelError> {
        self.apply_inner(deltas)?;
        self.publish_if_changed()
    }

    pub(crate) fn apply_unpublished(
        &mut self,
        deltas: impl IntoIterator<Item = PipeWireDelta>,
    ) -> Result<(), PipeWireModelError> {
        self.apply_inner(deltas)
    }

    fn apply_inner(
        &mut self,
        deltas: impl IntoIterator<Item = PipeWireDelta>,
    ) -> Result<(), PipeWireModelError> {
        for delta in deltas {
            match delta {
                PipeWireDelta::NodeAdded { raw_id, properties } => {
                    if !self.nodes.contains_key(&raw_id) && self.nodes.len() >= MAX_NODES {
                        return Err(PipeWireModelError::ResourceLimit(format!(
                            "node count exceeds {MAX_NODES}"
                        )));
                    }
                    self.nodes
                        .entry(raw_id)
                        .and_modify(|node| node.registry_properties.clone_from(&properties))
                        .or_insert(NodeRecord {
                            raw_id,
                            registry_properties: properties,
                            detail_properties: None,
                            state: PipeWireNodeState::Unknown,
                            raw_state: i32::MIN,
                            state_error: None,
                            input_ports: 0,
                            output_ports: 0,
                            ready: false,
                        });
                }
                PipeWireDelta::NodeInfo(info) => self.apply_node_info(info)?,
                PipeWireDelta::NodeTracking { raw_id, tracked } => {
                    if let Some(node) = self.nodes.get_mut(&raw_id)
                        && !tracked
                    {
                        node.detail_properties = None;
                        node.state = PipeWireNodeState::Unknown;
                        node.raw_state = i32::MIN;
                        node.state_error = None;
                        node.input_ports = 0;
                        node.output_ports = 0;
                        node.ready = false;
                    }
                }
                PipeWireDelta::NodeRemoved(raw_id) => {
                    self.nodes.remove(&raw_id);
                    self.links.retain(|_, link| {
                        link.source_node != Some(raw_id) && link.target_node != Some(raw_id)
                    });
                }
                PipeWireDelta::LinkAdded {
                    raw_id,
                    source_node,
                    target_node,
                    source_port,
                    target_port,
                } => {
                    if !self.links.contains_key(&raw_id) && self.links.len() >= MAX_LINKS {
                        return Err(PipeWireModelError::ResourceLimit(format!(
                            "link count exceeds {MAX_LINKS}"
                        )));
                    }
                    self.links.entry(raw_id).or_insert(LinkRecord {
                        raw_id,
                        source_node,
                        target_node,
                        source_port,
                        target_port,
                        state: PipeWireLinkState::Unknown,
                        raw_state: i32::MIN,
                        ready: false,
                    });
                }
                PipeWireDelta::LinkInfo(info) => self.apply_link_info(info),
                PipeWireDelta::LinkRemoved(raw_id) => {
                    self.links.remove(&raw_id);
                }
                PipeWireDelta::MetadataAdded { raw_id } => {
                    self.active_metadata = Some(raw_id);
                    self.metadata.clear();
                }
                PipeWireDelta::MetadataProperty {
                    raw_id,
                    subject,
                    key,
                    type_name,
                    value,
                } => self.apply_metadata(raw_id, subject, key, type_name, value)?,
                PipeWireDelta::MetadataRemoved(raw_id) => {
                    if self.active_metadata == Some(raw_id) {
                        self.active_metadata = None;
                        self.metadata.clear();
                    }
                }
                PipeWireDelta::CoreDone(_)
                | PipeWireDelta::CoreError(_)
                | PipeWireDelta::SourceError(_)
                | PipeWireDelta::Diagnostic(_) => {}
            }
        }
        Ok(())
    }

    fn apply_node_info(&mut self, info: RawNodeInfo) -> Result<(), PipeWireModelError> {
        let Some(node) = self.nodes.get_mut(&info.raw_id) else {
            return Ok(());
        };
        if let Some(properties) = info.properties {
            node.detail_properties = Some(properties);
        }
        node.state = info.state;
        node.raw_state = info.raw_state;
        node.state_error = info
            .state_error
            .filter(|value| value.len() <= super::model::MAX_NODE_TEXT_BYTES);
        node.input_ports = info.input_ports;
        node.output_ports = info.output_ports;
        node.ready = true;
        Ok(())
    }

    fn apply_link_info(&mut self, info: RawLinkInfo) {
        let Some(link) = self.links.get_mut(&info.raw_id) else {
            return;
        };
        link.source_node = info.source_node;
        link.target_node = info.target_node;
        link.source_port = info.source_port;
        link.target_port = info.target_port;
        link.state = info.state;
        link.raw_state = info.raw_state;
        link.ready = true;
    }

    fn apply_metadata(
        &mut self,
        raw_id: u32,
        subject: u32,
        key: Option<String>,
        type_name: Option<String>,
        value: Option<String>,
    ) -> Result<(), PipeWireModelError> {
        if self.active_metadata != Some(raw_id) || subject != METADATA_CORE_SUBJECT {
            return Ok(());
        }
        let Some(key) = key else {
            self.metadata.clear();
            return Ok(());
        };
        if !is_default_key(&key) {
            return Ok(());
        }
        let Some(value) = value else {
            self.metadata.remove(&key);
            return Ok(());
        };
        if value.len() > MAX_METADATA_VALUE_BYTES {
            return Err(PipeWireModelError::InvalidData(format!(
                "metadata `{key}` exceeds {MAX_METADATA_VALUE_BYTES} bytes"
            )));
        }
        let name = if type_name.as_deref() == Some("Spa:String:JSON") {
            parse_metadata_name(&value)
        } else {
            None
        };
        self.metadata.insert(
            key,
            MetadataValue {
                unresolved_value: name.is_none().then_some(value),
                name,
            },
        );
        Ok(())
    }

    fn publish_if_changed(&mut self) -> Result<Option<PipeWireSnapshot>, PipeWireModelError> {
        let mut snapshot = self.build_snapshot()?;
        if snapshot.same_content(&self.current) {
            self.resources.duplicate_publications_suppressed = self
                .resources
                .duplicate_publications_suppressed
                .saturating_add(1);
            self.current.resources.duplicate_publications_suppressed =
                self.resources.duplicate_publications_suppressed;
            return Ok(None);
        }
        self.sequence = self.sequence.saturating_add(1);
        self.resources.publications = self.resources.publications.saturating_add(1);
        snapshot.sequence = self.sequence;
        snapshot.resources = self.resources.clone();
        self.current = snapshot.clone();
        Ok(Some(snapshot))
    }

    fn build_snapshot(&mut self) -> Result<PipeWireSnapshot, PipeWireModelError> {
        let mut nodes = self
            .nodes
            .values()
            .map(|node| self.node_snapshot(node))
            .collect::<Result<Vec<_>, _>>()?;
        nodes.sort_by(|left, right| {
            node_sort_key(left)
                .cmp(&node_sort_key(right))
                .then_with(|| left.raw_global_id.cmp(&right.raw_global_id))
        });
        let node_ids = self.nodes.keys().copied().collect::<BTreeSet<_>>();
        let mut links = self
            .links
            .values()
            .map(|link| self.link_snapshot(link, &node_ids))
            .collect::<Vec<_>>();
        links.sort_by_key(|link| {
            (
                link.source_node.map(|id| id.global_id).unwrap_or(u32::MAX),
                link.target_node.map(|id| id.global_id).unwrap_or(u32::MAX),
                link.raw_global_id,
            )
        });
        let link_groups = self.build_link_groups(&node_ids)?;
        let defaults = PipeWireDefaultsSnapshot {
            metadata_available: self.active_metadata.is_some(),
            actual_sink: self.default_target(DEFAULT_AUDIO_SINK),
            actual_source: self.default_target(DEFAULT_AUDIO_SOURCE),
            configured_sink: self.default_target(DEFAULT_CONFIGURED_AUDIO_SINK),
            configured_source: self.default_target(DEFAULT_CONFIGURED_AUDIO_SOURCE),
        };
        self.resources.node_count = nodes.len();
        self.resources.link_count = links.len();
        self.resources.link_group_count = link_groups.len();
        Ok(PipeWireSnapshot {
            schema_version: 1,
            availability: self.availability,
            connection_generation: self.generation,
            ready: self.availability == PipeWireAvailability::Ready,
            node_count: nodes.len(),
            link_count: links.len(),
            link_group_count: link_groups.len(),
            defaults,
            nodes,
            links,
            link_groups,
            sequence: 0,
            resources: self.resources.clone(),
        })
    }

    fn node_snapshot(&self, node: &NodeRecord) -> Result<PipeWireNodeSnapshot, PipeWireModelError> {
        let properties = node
            .detail_properties
            .as_ref()
            .unwrap_or(&node.registry_properties);
        let name = property_text(properties, "node.name", "node name")?;
        let nickname = property_text(properties, "node.nick", "node nickname")?;
        let description = property_text(properties, "node.description", "node description")?;
        let media_class = property_text(properties, "media.class", "media class")?;
        let classification = PipeWireNodeClassification::from_properties(
            media_class.as_deref(),
            properties,
            node.input_ports,
            node.output_ports,
        );
        Ok(PipeWireNodeSnapshot {
            id: PipeWireNodeId {
                connection_generation: self.generation,
                global_id: node.raw_id,
            },
            raw_global_id: node.raw_id,
            name,
            nickname,
            description,
            media_class,
            classification,
            state: node.state,
            raw_state: node.raw_state,
            state_error: node.state_error.clone(),
            input_ports: node.input_ports,
            output_ports: node.output_ports,
            properties: properties.clone(),
            audio_capable: classification.audio,
            ready: node.ready,
        })
    }

    fn link_snapshot(&self, link: &LinkRecord, node_ids: &BTreeSet<u32>) -> PipeWireLinkSnapshot {
        PipeWireLinkSnapshot {
            id: PipeWireLinkId {
                connection_generation: self.generation,
                global_id: link.raw_id,
            },
            raw_global_id: link.raw_id,
            source_node: link.source_node.map(|global_id| PipeWireNodeId {
                connection_generation: self.generation,
                global_id,
            }),
            target_node: link.target_node.map(|global_id| PipeWireNodeId {
                connection_generation: self.generation,
                global_id,
            }),
            source_node_present: link
                .source_node
                .is_some_and(|global_id| node_ids.contains(&global_id)),
            target_node_present: link
                .target_node
                .is_some_and(|global_id| node_ids.contains(&global_id)),
            source_port_id: link.source_port,
            target_port_id: link.target_port,
            state: link.state,
            raw_state: link.raw_state,
            ready: link.ready,
        }
    }

    fn build_link_groups(
        &mut self,
        node_ids: &BTreeSet<u32>,
    ) -> Result<Vec<PipeWireLinkGroupSnapshot>, PipeWireModelError> {
        let mut members = BTreeMap::<(u32, u32), Vec<u32>>::new();
        for link in self.links.values() {
            let (Some(source), Some(target)) = (link.source_node, link.target_node) else {
                continue;
            };
            members
                .entry((source, target))
                .or_default()
                .push(link.raw_id);
        }
        if members.len() > MAX_LINK_GROUPS {
            return Err(PipeWireModelError::ResourceLimit(format!(
                "link-group count exceeds {MAX_LINK_GROUPS}"
            )));
        }
        self.group_representatives
            .retain(|key, _| members.contains_key(key));
        let mut groups = Vec::with_capacity(members.len());
        for ((source, target), mut member_ids) in members {
            member_ids.sort_unstable();
            let representative = self
                .group_representatives
                .get(&(source, target))
                .copied()
                .filter(|representative| member_ids.binary_search(representative).is_ok())
                .unwrap_or(member_ids[0]);
            self.group_representatives
                .insert((source, target), representative);
            let representative_link = &self.links[&representative];
            groups.push(PipeWireLinkGroupSnapshot {
                id: PipeWireLinkGroupId {
                    connection_generation: self.generation,
                    source_node: source,
                    target_node: target,
                },
                source_node: Some(PipeWireNodeId {
                    connection_generation: self.generation,
                    global_id: source,
                }),
                target_node: Some(PipeWireNodeId {
                    connection_generation: self.generation,
                    global_id: target,
                }),
                source_node_present: node_ids.contains(&source),
                target_node_present: node_ids.contains(&target),
                members: member_ids
                    .into_iter()
                    .map(|global_id| PipeWireLinkId {
                        connection_generation: self.generation,
                        global_id,
                    })
                    .collect(),
                representative: PipeWireLinkId {
                    connection_generation: self.generation,
                    global_id: representative,
                },
                state: representative_link.state,
            });
        }
        Ok(groups)
    }

    fn default_target(&self, key: &str) -> PipeWireDefaultTarget {
        let Some(value) = self.metadata.get(key) else {
            return PipeWireDefaultTarget::default();
        };
        PipeWireDefaultTarget {
            metadata_name: value.name.clone(),
            unresolved_value: value.unresolved_value.clone(),
            node: value.name.as_ref().and_then(|name| {
                self.nodes
                    .values()
                    .find(|node| {
                        node.detail_properties
                            .as_ref()
                            .unwrap_or(&node.registry_properties)
                            .get("node.name")
                            == Some(name)
                    })
                    .map(|node| PipeWireNodeId {
                        connection_generation: self.generation,
                        global_id: node.raw_id,
                    })
            }),
        }
    }
}

fn node_sort_key(node: &PipeWireNodeSnapshot) -> (PipeWireNodeType, &str, &str, &str) {
    (
        PipeWireNodeType::from_node(node),
        node.media_class.as_deref().unwrap_or(""),
        node.description.as_deref().unwrap_or(""),
        node.name.as_deref().unwrap_or(""),
    )
}

fn property_text(
    properties: &BTreeMap<String, String>,
    key: &'static str,
    field: &'static str,
) -> Result<Option<String>, PipeWireModelError> {
    let value = properties.get(key).map(String::as_str);
    if value.is_some_and(|value| value.len() > super::model::MAX_NODE_TEXT_BYTES) {
        return Ok(None);
    }
    bounded_text(value, field, super::model::MAX_NODE_TEXT_BYTES)
}

fn is_default_key(key: &str) -> bool {
    matches!(
        key,
        DEFAULT_AUDIO_SINK
            | DEFAULT_AUDIO_SOURCE
            | DEFAULT_CONFIGURED_AUDIO_SINK
            | DEFAULT_CONFIGURED_AUDIO_SOURCE
    )
}

#[derive(Deserialize)]
struct MetadataName<'a> {
    name: &'a str,
}

fn parse_metadata_name(value: &str) -> Option<String> {
    serde_json::from_str::<MetadataName<'_>>(value)
        .ok()
        .and_then(|metadata| {
            (metadata.name.len() <= super::model::MAX_NODE_TEXT_BYTES)
                .then(|| metadata.name.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn add_node(raw_id: u32, name: &str, media_class: &str) -> Vec<PipeWireDelta> {
        vec![
            PipeWireDelta::NodeAdded {
                raw_id,
                properties: properties(&[("node.name", name), ("media.class", media_class)]),
            },
            PipeWireDelta::NodeInfo(RawNodeInfo {
                raw_id,
                state: PipeWireNodeState::Running,
                raw_state: 4,
                state_error: None,
                input_ports: 2,
                output_ports: 2,
                properties: None,
            }),
        ]
    }

    fn add_link(
        raw_id: u32,
        source: u32,
        target: u32,
        state: PipeWireLinkState,
    ) -> Vec<PipeWireDelta> {
        vec![
            PipeWireDelta::LinkAdded {
                raw_id,
                source_node: Some(source),
                target_node: Some(target),
                source_port: Some(raw_id + 10),
                target_port: Some(raw_id + 20),
            },
            PipeWireDelta::LinkInfo(RawLinkInfo {
                raw_id,
                source_node: Some(source),
                target_node: Some(target),
                source_port: Some(raw_id + 10),
                target_port: Some(raw_id + 20),
                state,
                raw_state: 5,
            }),
        ]
    }

    #[test]
    fn generation_replacement_clears_graph_and_changes_identity() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        reconciler.apply(add_node(7, "sink", "Audio/Sink")).unwrap();
        let first = reconciler.mark_ready().unwrap().unwrap();
        assert_eq!(first.nodes[0].id.connection_generation, 1);
        let cleared = reconciler.begin_generation(2).unwrap().unwrap();
        assert!(cleared.nodes.is_empty());
        reconciler.apply(add_node(7, "sink", "Audio/Sink")).unwrap();
        let second = reconciler.mark_ready().unwrap().unwrap();
        assert_eq!(second.nodes[0].id.connection_generation, 2);
        assert_ne!(first.nodes[0].id, second.nodes[0].id);
    }

    #[test]
    fn initial_graph_stays_unpublished_until_ready_barrier() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        reconciler
            .apply_unpublished(add_node(4, "sink", "Audio/Sink"))
            .unwrap();
        assert_eq!(
            reconciler.current().availability,
            PipeWireAvailability::Synchronizing
        );
        assert!(reconciler.current().nodes.is_empty());
        let ready = reconciler.mark_ready().unwrap().unwrap();
        assert_eq!(ready.availability, PipeWireAvailability::Ready);
        assert_eq!(ready.node_count, 1);
    }

    #[test]
    fn node_updates_preserve_identity_and_suppress_duplicates() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(3).unwrap();
        let deltas = add_node(9, "source", "Audio/Source");
        let first = reconciler.apply(deltas.clone()).unwrap().unwrap();
        let id = first.nodes[0].id;
        assert!(reconciler.apply(deltas).unwrap().is_none());
        assert_eq!(reconciler.current().nodes[0].id, id);
        assert!(
            reconciler
                .current()
                .resources
                .duplicate_publications_suppressed
                > 0
        );
    }

    #[test]
    fn node_removal_removes_related_links_and_groups() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        let mut deltas = add_node(1, "source", "Audio/Source");
        deltas.extend(add_node(2, "sink", "Audio/Sink"));
        deltas.extend(add_link(10, 1, 2, PipeWireLinkState::Active));
        let snapshot = reconciler.apply(deltas).unwrap().unwrap();
        assert_eq!(snapshot.link_group_count, 1);
        let removed = reconciler
            .apply([PipeWireDelta::NodeRemoved(1)])
            .unwrap()
            .unwrap();
        assert!(removed.links.is_empty());
        assert!(removed.link_groups.is_empty());
    }

    #[test]
    fn link_group_retains_representative_until_it_is_removed() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        let mut deltas = add_link(20, 1, 2, PipeWireLinkState::Paused);
        deltas.extend(add_link(30, 1, 2, PipeWireLinkState::Active));
        let first = reconciler.apply(deltas).unwrap().unwrap();
        assert_eq!(first.link_groups[0].representative.global_id, 20);
        assert_eq!(first.link_groups[0].state, PipeWireLinkState::Paused);
        reconciler
            .apply(add_link(10, 1, 2, PipeWireLinkState::Active))
            .unwrap();
        assert_eq!(
            reconciler.current().link_groups[0].representative.global_id,
            20
        );
        let replacement = reconciler
            .apply([PipeWireDelta::LinkRemoved(20)])
            .unwrap()
            .unwrap();
        assert_eq!(replacement.link_groups[0].representative.global_id, 10);
    }

    #[test]
    fn link_endpoint_change_moves_group_without_replacing_link_identity() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(4).unwrap();
        reconciler
            .apply(add_link(5, 1, 2, PipeWireLinkState::Active))
            .unwrap();
        let id = reconciler.current().links[0].id;
        let snapshot = reconciler
            .apply([PipeWireDelta::LinkInfo(RawLinkInfo {
                raw_id: 5,
                source_node: Some(1),
                target_node: Some(3),
                source_port: Some(10),
                target_port: Some(11),
                state: PipeWireLinkState::Active,
                raw_state: 5,
            })])
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.links[0].id, id);
        assert_eq!(snapshot.link_groups[0].id.target_node, 3);
    }

    #[test]
    fn all_link_states_and_unknown_are_retained() {
        let states = [
            PipeWireLinkState::Unknown,
            PipeWireLinkState::Error,
            PipeWireLinkState::Unlinked,
            PipeWireLinkState::Init,
            PipeWireLinkState::Negotiating,
            PipeWireLinkState::Allocating,
            PipeWireLinkState::Paused,
            PipeWireLinkState::Active,
        ];
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        for (index, state) in states.into_iter().enumerate() {
            reconciler
                .apply(add_link(index as u32 + 1, 1, index as u32 + 2, state))
                .unwrap();
        }
        let actual = reconciler
            .current()
            .links
            .iter()
            .map(|link| link.state)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual.len(), states.len());
    }

    #[test]
    fn all_node_states_and_unknown_are_retained() {
        let states = [
            PipeWireNodeState::Unknown,
            PipeWireNodeState::Error,
            PipeWireNodeState::Creating,
            PipeWireNodeState::Suspended,
            PipeWireNodeState::Idle,
            PipeWireNodeState::Running,
        ];
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        for (index, state) in states.into_iter().enumerate() {
            let raw_id = index as u32 + 1;
            reconciler
                .apply([
                    PipeWireDelta::NodeAdded {
                        raw_id,
                        properties: properties(&[("node.name", "node")]),
                    },
                    PipeWireDelta::NodeInfo(RawNodeInfo {
                        raw_id,
                        state,
                        raw_state: index as i32,
                        state_error: (state == PipeWireNodeState::Error)
                            .then(|| "modeled error".into()),
                        input_ports: 0,
                        output_ports: 0,
                        properties: None,
                    }),
                ])
                .unwrap();
        }
        let actual = reconciler
            .current()
            .nodes
            .iter()
            .map(|node| node.state)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual.len(), states.len());
    }

    #[test]
    fn default_metadata_resolves_late_node_and_clears_on_removal() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(2).unwrap();
        reconciler
            .apply([
                PipeWireDelta::MetadataAdded { raw_id: 80 },
                PipeWireDelta::MetadataProperty {
                    raw_id: 80,
                    subject: 0,
                    key: Some(DEFAULT_AUDIO_SINK.into()),
                    type_name: Some("Spa:String:JSON".into()),
                    value: Some(r#"{"name":"sink"}"#.into()),
                },
            ])
            .unwrap();
        assert_eq!(
            reconciler
                .current()
                .defaults
                .actual_sink
                .metadata_name
                .as_deref(),
            Some("sink")
        );
        assert!(reconciler.current().defaults.actual_sink.node.is_none());
        reconciler
            .apply(add_node(12, "sink", "Audio/Sink"))
            .unwrap();
        assert_eq!(
            reconciler.current().defaults.actual_sink.node,
            Some(PipeWireNodeId {
                connection_generation: 2,
                global_id: 12
            })
        );
        reconciler
            .apply([PipeWireDelta::MetadataRemoved(80)])
            .unwrap();
        assert_eq!(
            reconciler.current().defaults.actual_sink,
            PipeWireDefaultTarget::default()
        );
    }

    #[test]
    fn malformed_metadata_is_retained_only_as_diagnostic_text() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        reconciler
            .apply([
                PipeWireDelta::MetadataAdded { raw_id: 1 },
                PipeWireDelta::MetadataProperty {
                    raw_id: 1,
                    subject: 0,
                    key: Some(DEFAULT_AUDIO_SOURCE.into()),
                    type_name: Some("Spa:String:JSON".into()),
                    value: Some("not-json".into()),
                },
            ])
            .unwrap();
        let target = &reconciler.current().defaults.actual_source;
        assert!(target.metadata_name.is_none());
        assert_eq!(target.unresolved_value.as_deref(), Some("not-json"));
    }

    #[test]
    fn metadata_replacement_does_not_replace_the_graph_generation() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(7).unwrap();
        reconciler
            .apply([
                PipeWireDelta::MetadataAdded { raw_id: 10 },
                PipeWireDelta::MetadataProperty {
                    raw_id: 10,
                    subject: 0,
                    key: Some(DEFAULT_AUDIO_SINK.into()),
                    type_name: Some("Spa:String:JSON".into()),
                    value: Some(r#"{"name":"old"}"#.into()),
                },
                PipeWireDelta::MetadataAdded { raw_id: 11 },
                PipeWireDelta::MetadataProperty {
                    raw_id: 10,
                    subject: 0,
                    key: Some(DEFAULT_AUDIO_SINK.into()),
                    type_name: Some("Spa:String:JSON".into()),
                    value: Some(r#"{"name":"stale"}"#.into()),
                },
                PipeWireDelta::MetadataProperty {
                    raw_id: 11,
                    subject: 0,
                    key: Some(DEFAULT_AUDIO_SINK.into()),
                    type_name: Some("Spa:String:JSON".into()),
                    value: Some(r#"{"name":"new"}"#.into()),
                },
                PipeWireDelta::MetadataRemoved(10),
            ])
            .unwrap();
        assert_eq!(reconciler.current().connection_generation, 7);
        assert_eq!(
            reconciler
                .current()
                .defaults
                .actual_sink
                .metadata_name
                .as_deref(),
            Some("new")
        );
    }

    #[test]
    fn all_four_default_keys_are_distinct() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        let keys = [
            DEFAULT_AUDIO_SINK,
            DEFAULT_AUDIO_SOURCE,
            DEFAULT_CONFIGURED_AUDIO_SINK,
            DEFAULT_CONFIGURED_AUDIO_SOURCE,
        ];
        let mut deltas = vec![PipeWireDelta::MetadataAdded { raw_id: 1 }];
        for (index, key) in keys.into_iter().enumerate() {
            deltas.push(PipeWireDelta::MetadataProperty {
                raw_id: 1,
                subject: 0,
                key: Some(key.into()),
                type_name: Some("Spa:String:JSON".into()),
                value: Some(format!(r#"{{"name":"node-{index}"}}"#)),
            });
        }
        reconciler.apply(deltas).unwrap();
        let defaults = &reconciler.current().defaults;
        assert_eq!(
            defaults.actual_sink.metadata_name.as_deref(),
            Some("node-0")
        );
        assert_eq!(
            defaults.actual_source.metadata_name.as_deref(),
            Some("node-1")
        );
        assert_eq!(
            defaults.configured_sink.metadata_name.as_deref(),
            Some("node-2")
        );
        assert_eq!(
            defaults.configured_source.metadata_name.as_deref(),
            Some("node-3")
        );
    }

    #[test]
    fn link_endpoint_presence_tracks_late_nodes_without_changing_link_identity() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(3).unwrap();
        let first = reconciler
            .apply(add_link(8, 40, 41, PipeWireLinkState::Negotiating))
            .unwrap()
            .unwrap();
        let link_id = first.links[0].id;
        assert!(!first.links[0].source_node_present);
        assert!(!first.links[0].target_node_present);
        let mut nodes = add_node(40, "source", "Audio/Source");
        nodes.extend(add_node(41, "sink", "Audio/Sink"));
        let resolved = reconciler.apply(nodes).unwrap().unwrap();
        assert_eq!(resolved.links[0].id, link_id);
        assert!(resolved.links[0].source_node_present);
        assert!(resolved.links[0].target_node_present);
    }

    #[test]
    fn unavailable_state_clears_stale_graph_and_defaults() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        let mut deltas = add_node(1, "source", "Audio/Source");
        deltas.extend(add_link(2, 1, 3, PipeWireLinkState::Active));
        deltas.push(PipeWireDelta::MetadataAdded { raw_id: 9 });
        deltas.push(PipeWireDelta::MetadataProperty {
            raw_id: 9,
            subject: 0,
            key: Some(DEFAULT_AUDIO_SOURCE.into()),
            type_name: Some("Spa:String:JSON".into()),
            value: Some(r#"{"name":"source"}"#.into()),
        });
        reconciler.apply(deltas).unwrap();
        let unavailable = reconciler.mark_unavailable().unwrap().unwrap();
        assert_eq!(unavailable.availability, PipeWireAvailability::Unavailable);
        assert!(unavailable.nodes.is_empty());
        assert!(unavailable.links.is_empty());
        assert_eq!(
            unavailable.defaults.actual_source,
            PipeWireDefaultTarget::default()
        );
    }

    #[test]
    fn one_callback_burst_produces_one_logical_publication() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        let before = reconciler.current().sequence;
        let mut deltas = Vec::new();
        for raw_id in 0..1_000u32 {
            deltas.extend(add_node(raw_id, &format!("node-{raw_id}"), "Audio/Sink"));
        }
        let snapshot = reconciler.apply(deltas).unwrap().unwrap();
        assert_eq!(snapshot.sequence, before + 1);
        assert_eq!(snapshot.node_count, 1_000);
    }

    #[test]
    fn repeated_generations_do_not_alias_or_accumulate_graph_state() {
        let mut reconciler = PipeWireReconciler::default();
        let mut prior = None;
        for generation in 1..=100 {
            reconciler.begin_generation(generation).unwrap();
            reconciler.apply(add_node(5, "sink", "Audio/Sink")).unwrap();
            let snapshot = reconciler.mark_ready().unwrap().unwrap();
            let id = snapshot.nodes[0].id;
            assert_ne!(Some(id), prior);
            assert_eq!(snapshot.node_count, 1);
            prior = Some(id);
        }
    }

    #[test]
    fn deterministic_node_order_is_not_identity() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        let mut deltas = add_node(30, "video", "Video/Source");
        deltas.extend(add_node(20, "z-source", "Audio/Source"));
        deltas.extend(add_node(10, "a-sink", "Audio/Sink"));
        let snapshot = reconciler.apply(deltas).unwrap().unwrap();
        let ids = snapshot
            .nodes
            .iter()
            .map(|node| node.raw_global_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn resource_limits_are_checked_before_growth() {
        let mut reconciler = PipeWireReconciler::default();
        reconciler.begin_generation(1).unwrap();
        for raw_id in 0..MAX_NODES as u32 {
            reconciler
                .apply([PipeWireDelta::NodeAdded {
                    raw_id,
                    properties: BTreeMap::new(),
                }])
                .unwrap();
        }
        let error = reconciler
            .apply([PipeWireDelta::NodeAdded {
                raw_id: MAX_NODES as u32,
                properties: BTreeMap::new(),
            }])
            .unwrap_err();
        assert!(error.to_string().contains("node count"));
    }
}
