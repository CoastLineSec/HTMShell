use serde::Serialize;
use std::collections::BTreeMap;

pub const MAX_NODES: usize = 4_096;
pub const MAX_LINKS: usize = 16_384;
pub const MAX_LINK_GROUPS: usize = 4_096;
pub const MAX_NODE_PROPERTIES: usize = 256;
pub const MAX_PROPERTY_KEY_BYTES: usize = 128;
pub const MAX_PROPERTY_VALUE_BYTES: usize = 1_024;
pub const MAX_NODE_TEXT_BYTES: usize = 256;
pub const MAX_METADATA_VALUE_BYTES: usize = 1_024;
pub const MAX_STAGED_DELTAS: usize = MAX_NODES + MAX_LINKS + 4_096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipeWireAvailability {
    #[default]
    Unavailable,
    Synchronizing,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PipeWireNodeId {
    pub connection_generation: u64,
    pub global_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PipeWireLinkId {
    pub connection_generation: u64,
    pub global_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PipeWireLinkGroupId {
    pub connection_generation: u64,
    pub source_node: u32,
    pub target_node: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PipeWireNodeState {
    Unknown,
    Error,
    Creating,
    Suspended,
    Idle,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PipeWireLinkState {
    Unknown,
    Error,
    Unlinked,
    Init,
    Negotiating,
    Allocating,
    Paused,
    Active,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PipeWireNodeClassification {
    pub audio: bool,
    pub video: bool,
    pub sink: bool,
    pub source: bool,
    pub stream: bool,
    pub device_related: bool,
    pub accepts_input: bool,
    pub produces_output: bool,
}

impl PipeWireNodeClassification {
    pub(crate) fn from_properties(
        media_class: Option<&str>,
        properties: &BTreeMap<String, String>,
        input_ports: u32,
        output_ports: u32,
    ) -> Self {
        let mut classification = Self {
            accepts_input: input_ports > 0,
            produces_output: output_ports > 0,
            device_related: properties.contains_key("device.id")
                || media_class.is_some_and(|class| class.starts_with("Device/")),
            ..Self::default()
        };
        match properties.get("media.type").map(String::as_str) {
            Some("Audio") => classification.audio = true,
            Some("Video") => classification.video = true,
            _ => {}
        }
        let Some(media_class) = media_class else {
            return classification;
        };
        let segments = media_class.split('/').collect::<Vec<_>>();
        classification.audio = segments.contains(&"Audio");
        classification.video = segments.contains(&"Video");
        classification.stream = segments.first() == Some(&"Stream");
        classification.sink = segments.contains(&"Sink")
            || media_class == "Stream/Output/Audio"
            || media_class == "Stream/Output/Video";
        classification.source = segments.contains(&"Source")
            || media_class == "Stream/Input/Audio"
            || media_class == "Stream/Input/Video";
        if segments.contains(&"Duplex") {
            classification.sink = true;
            classification.source = true;
        }
        classification
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipeWireNodeSnapshot {
    pub id: PipeWireNodeId,
    #[serde(rename = "session_local_global_id")]
    pub raw_global_id: u32,
    pub name: Option<String>,
    pub nickname: Option<String>,
    pub description: Option<String>,
    pub media_class: Option<String>,
    pub classification: PipeWireNodeClassification,
    pub state: PipeWireNodeState,
    pub raw_state: i32,
    pub state_error: Option<String>,
    pub input_ports: u32,
    pub output_ports: u32,
    pub properties: BTreeMap<String, String>,
    pub audio_capable: bool,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipeWireLinkSnapshot {
    pub id: PipeWireLinkId,
    #[serde(rename = "session_local_global_id")]
    pub raw_global_id: u32,
    pub source_node: Option<PipeWireNodeId>,
    pub target_node: Option<PipeWireNodeId>,
    pub source_node_present: bool,
    pub target_node_present: bool,
    pub source_port_id: Option<u32>,
    pub target_port_id: Option<u32>,
    pub state: PipeWireLinkState,
    pub raw_state: i32,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipeWireLinkGroupSnapshot {
    pub id: PipeWireLinkGroupId,
    pub source_node: Option<PipeWireNodeId>,
    pub target_node: Option<PipeWireNodeId>,
    pub source_node_present: bool,
    pub target_node_present: bool,
    pub members: Vec<PipeWireLinkId>,
    pub representative: PipeWireLinkId,
    pub state: PipeWireLinkState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipeWireDefaultTarget {
    pub metadata_name: Option<String>,
    pub unresolved_value: Option<String>,
    pub node: Option<PipeWireNodeId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipeWireDefaultsSnapshot {
    pub actual_sink: PipeWireDefaultTarget,
    pub actual_source: PipeWireDefaultTarget,
    pub configured_sink: PipeWireDefaultTarget,
    pub configured_source: PipeWireDefaultTarget,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipeWireResourceCounters {
    pub node_count: usize,
    pub link_count: usize,
    pub link_group_count: usize,
    pub node_proxy_count: usize,
    pub link_proxy_count: usize,
    pub metadata_proxy_count: usize,
    pub staged_delta_peak: usize,
    pub dispatch_iterations: u64,
    pub callbacks_staged: u64,
    pub publications: u64,
    pub duplicate_publications_suppressed: u64,
    pub reconnect_attempts: u64,
    pub diagnostics_contained: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipeWireSnapshot {
    pub schema_version: u32,
    pub availability: PipeWireAvailability,
    pub connection_generation: u64,
    pub ready: bool,
    pub node_count: usize,
    pub link_count: usize,
    pub link_group_count: usize,
    pub defaults: PipeWireDefaultsSnapshot,
    pub nodes: Vec<PipeWireNodeSnapshot>,
    pub links: Vec<PipeWireLinkSnapshot>,
    pub link_groups: Vec<PipeWireLinkGroupSnapshot>,
    pub sequence: u64,
    pub resources: PipeWireResourceCounters,
}

impl Default for PipeWireSnapshot {
    fn default() -> Self {
        Self {
            schema_version: 1,
            availability: PipeWireAvailability::Unavailable,
            connection_generation: 0,
            ready: false,
            node_count: 0,
            link_count: 0,
            link_group_count: 0,
            defaults: PipeWireDefaultsSnapshot::default(),
            nodes: Vec::new(),
            links: Vec::new(),
            link_groups: Vec::new(),
            sequence: 0,
            resources: PipeWireResourceCounters::default(),
        }
    }
}

impl PipeWireSnapshot {
    pub(crate) fn same_content(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.availability == other.availability
            && self.connection_generation == other.connection_generation
            && self.ready == other.ready
            && self.node_count == other.node_count
            && self.link_count == other.link_count
            && self.link_group_count == other.link_group_count
            && self.defaults == other.defaults
            && self.nodes == other.nodes
            && self.links == other.links
            && self.link_groups == other.link_groups
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawNodeInfo {
    pub raw_id: u32,
    pub state: PipeWireNodeState,
    pub raw_state: i32,
    pub state_error: Option<String>,
    pub input_ports: u32,
    pub output_ports: u32,
    pub properties: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawLinkInfo {
    pub raw_id: u32,
    pub source_node: Option<u32>,
    pub target_node: Option<u32>,
    pub source_port: Option<u32>,
    pub target_port: Option<u32>,
    pub state: PipeWireLinkState,
    pub raw_state: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PipeWireDelta {
    NodeAdded {
        raw_id: u32,
        properties: BTreeMap<String, String>,
    },
    NodeInfo(RawNodeInfo),
    NodeRemoved(u32),
    LinkAdded {
        raw_id: u32,
        source_node: Option<u32>,
        target_node: Option<u32>,
        source_port: Option<u32>,
        target_port: Option<u32>,
    },
    LinkInfo(RawLinkInfo),
    LinkRemoved(u32),
    MetadataAdded {
        raw_id: u32,
    },
    MetadataProperty {
        raw_id: u32,
        subject: u32,
        key: Option<String>,
        type_name: Option<String>,
        value: Option<String>,
    },
    MetadataRemoved(u32),
    CoreDone(i32),
    CoreError(String),
    SourceError(String),
    Diagnostic(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PipeWireModelError {
    ResourceLimit(String),
    InvalidData(String),
}

impl std::fmt::Display for PipeWireModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceLimit(message) => write!(formatter, "PipeWire resource limit: {message}"),
            Self::InvalidData(message) => write!(formatter, "invalid PipeWire data: {message}"),
        }
    }
}

impl std::error::Error for PipeWireModelError {}

pub(crate) fn bounded_text(
    value: Option<&str>,
    field: &'static str,
    maximum: usize,
) -> Result<Option<String>, PipeWireModelError> {
    value
        .map(|value| {
            if value.len() > maximum {
                Err(PipeWireModelError::InvalidData(format!(
                    "{field} exceeds {maximum} bytes"
                )))
            } else {
                Ok(value.to_owned())
            }
        })
        .transpose()
}

#[cfg(test)]
pub(crate) fn bounded_properties<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<BTreeMap<String, String>, PipeWireModelError> {
    let mut properties = BTreeMap::new();
    for (key, value) in entries {
        if key.len() > MAX_PROPERTY_KEY_BYTES {
            return Err(PipeWireModelError::InvalidData(format!(
                "property key exceeds {MAX_PROPERTY_KEY_BYTES} bytes"
            )));
        }
        if value.len() > MAX_PROPERTY_VALUE_BYTES {
            return Err(PipeWireModelError::InvalidData(format!(
                "property `{key}` exceeds {MAX_PROPERTY_VALUE_BYTES} bytes"
            )));
        }
        properties
            .entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
        if properties.len() > MAX_NODE_PROPERTIES {
            return Err(PipeWireModelError::ResourceLimit(format!(
                "node property count exceeds {MAX_NODE_PROPERTIES}"
            )));
        }
    }
    Ok(properties)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(media_class: Option<&str>) -> PipeWireNodeClassification {
        let properties = media_class
            .map(|value| BTreeMap::from([("media.class".into(), value.into())]))
            .unwrap_or_default();
        PipeWireNodeClassification::from_properties(media_class, &properties, 1, 1)
    }

    #[test]
    fn audited_node_type_flags_remain_composable() {
        let sink = classify(Some("Audio/Sink"));
        assert!(sink.audio && sink.sink && !sink.source && !sink.stream);
        let source = classify(Some("Audio/Source"));
        assert!(source.audio && source.source && !source.sink);
        let duplex = classify(Some("Audio/Duplex"));
        assert!(duplex.audio && duplex.source && duplex.sink);
        let output_stream = classify(Some("Stream/Output/Audio"));
        assert!(output_stream.audio && output_stream.sink && output_stream.stream);
        let input_stream = classify(Some("Stream/Input/Audio"));
        assert!(input_stream.audio && input_stream.source && input_stream.stream);
        let video_sink = classify(Some("Video/Sink"));
        assert!(video_sink.video && video_sink.sink);
        let video_source = classify(Some("Video/Source"));
        assert!(video_source.video && video_source.source);
        let unknown = classify(Some("Midi/Bridge"));
        assert!(!unknown.audio && !unknown.video && !unknown.sink && !unknown.source);
    }

    #[test]
    fn media_type_retains_audio_for_graph_nodes_without_media_class() {
        let properties = BTreeMap::from([("media.type".into(), "Audio".into())]);
        let classification = PipeWireNodeClassification::from_properties(None, &properties, 2, 2);
        assert!(classification.audio);
        assert!(!classification.video);
    }

    #[test]
    fn property_map_is_bounded_and_first_duplicate_wins() {
        let properties = bounded_properties([("key", "first"), ("key", "second")]).unwrap();
        assert_eq!(properties["key"], "first");
        let long_key = "k".repeat(MAX_PROPERTY_KEY_BYTES + 1);
        assert!(bounded_properties([(long_key.as_str(), "value")]).is_err());
        let long_value = "v".repeat(MAX_PROPERTY_VALUE_BYTES + 1);
        assert!(bounded_properties([("key", long_value.as_str())]).is_err());
    }

    #[test]
    fn snapshot_equality_ignores_sequences_and_resource_diagnostics() {
        let mut left = PipeWireSnapshot::default();
        let mut right = PipeWireSnapshot::default();
        left.sequence = 1;
        right.sequence = 99;
        right.resources.callbacks_staged = 50;
        assert!(left.same_content(&right));
    }
}
