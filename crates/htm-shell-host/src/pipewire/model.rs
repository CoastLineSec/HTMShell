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
pub const MAX_AUDIO_CHANNELS: usize = 64;
pub const MAX_PERCEPTUAL_VOLUME: f32 = 2.0;
pub const SPA_AUDIO_CHANNEL_AUX_START: u32 = 0x1000;
pub const SPA_AUDIO_CHANNEL_AUX_END: u32 = 0x1fff;
pub const SPA_AUDIO_CHANNEL_CUSTOM_START: u32 = 0x10000;

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
    pub monitor: bool,
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
            monitor: matches!(
                properties.get("media.category").map(String::as_str),
                Some("Monitor" | "Manager")
            ),
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
    pub audio: PipeWireNodeAudioSnapshot,
    pub ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiniteVolume(u32);

impl FiniteVolume {
    pub(crate) fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then(|| Self(value.to_bits()))
    }

    pub(crate) fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

impl Serialize for FiniteVolume {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f32(self.get())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipeWireNodeAudioSnapshot {
    pub channels: Vec<FiniteVolume>,
    pub channel_positions: Vec<PipeWireAudioChannelPosition>,
    pub channel_layout_generation: u64,
    pub average_volume: Option<FiniteVolume>,
    pub muted: Option<bool>,
    pub ready: bool,
    pub can_set_volume: bool,
    pub can_set_mute: bool,
}

impl PipeWireNodeAudioSnapshot {
    #[cfg(test)]
    pub(crate) fn from_linear_channels(
        channels: &[f32],
        muted: Option<bool>,
        writable: bool,
    ) -> Option<Self> {
        if channels.len() > MAX_AUDIO_CHANNELS {
            return None;
        }
        let channels = channels
            .iter()
            .map(|value| FiniteVolume::new(value.cbrt()))
            .collect::<Option<Vec<_>>>()?;
        let average_volume = perceptual_average(&channels);
        let channel_positions = normalize_channel_positions(channels.len(), &[]);
        Some(Self {
            ready: average_volume.is_some() && muted.is_some(),
            can_set_volume: writable && average_volume.is_some(),
            can_set_mute: writable && muted.is_some(),
            channels,
            channel_positions,
            channel_layout_generation: 1,
            average_volume,
            muted,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PipeWireAudioChannelPosition {
    pub raw: u32,
}

impl PipeWireAudioChannelPosition {
    pub const NAMED: [Self; 38] = [
        Self::new(0),
        Self::new(1),
        Self::new(2),
        Self::new(3),
        Self::new(4),
        Self::new(5),
        Self::new(6),
        Self::new(7),
        Self::new(8),
        Self::new(9),
        Self::new(10),
        Self::new(11),
        Self::new(12),
        Self::new(13),
        Self::new(14),
        Self::new(15),
        Self::new(16),
        Self::new(17),
        Self::new(18),
        Self::new(19),
        Self::new(20),
        Self::new(21),
        Self::new(22),
        Self::new(23),
        Self::new(24),
        Self::new(25),
        Self::new(26),
        Self::new(27),
        Self::new(28),
        Self::new(29),
        Self::new(30),
        Self::new(31),
        Self::new(32),
        Self::new(33),
        Self::new(34),
        Self::new(35),
        Self::new(36),
        Self::new(37),
    ];
    pub const AUXILIARY_FIRST: Self = Self::new(SPA_AUDIO_CHANNEL_AUX_START);
    pub const AUXILIARY_LAST: Self = Self::new(SPA_AUDIO_CHANNEL_AUX_END);
    pub const CUSTOM_FIRST: Self = Self::new(SPA_AUDIO_CHANNEL_CUSTOM_START);
    pub const CUSTOM_LAST: Self = Self::new(u32::MAX);

    pub const fn new(raw: u32) -> Self {
        Self { raw }
    }

    pub fn token(self) -> String {
        match self.raw {
            0 => "unknown".into(),
            1 => "na".into(),
            2 => "mono".into(),
            3 => "front-left".into(),
            4 => "front-right".into(),
            5 => "front-center".into(),
            6 => "lfe".into(),
            7 => "side-left".into(),
            8 => "side-right".into(),
            9 => "front-left-center".into(),
            10 => "front-right-center".into(),
            11 => "rear-center".into(),
            12 => "rear-left".into(),
            13 => "rear-right".into(),
            14 => "top-center".into(),
            15 => "top-front-left".into(),
            16 => "top-front-center".into(),
            17 => "top-front-right".into(),
            18 => "top-rear-left".into(),
            19 => "top-rear-center".into(),
            20 => "top-rear-right".into(),
            21 => "rear-left-center".into(),
            22 => "rear-right-center".into(),
            23 => "front-left-wide".into(),
            24 => "front-right-wide".into(),
            25 => "lfe-2".into(),
            26 => "front-left-high".into(),
            27 => "front-center-high".into(),
            28 => "front-right-high".into(),
            29 => "top-front-left-center".into(),
            30 => "top-front-right-center".into(),
            31 => "top-side-left".into(),
            32 => "top-side-right".into(),
            33 => "lfe-left".into(),
            34 => "lfe-right".into(),
            35 => "bottom-center".into(),
            36 => "bottom-left-center".into(),
            37 => "bottom-right-center".into(),
            raw if (SPA_AUDIO_CHANNEL_AUX_START..=SPA_AUDIO_CHANNEL_AUX_END).contains(&raw) => {
                format!("aux-{}", raw - SPA_AUDIO_CHANNEL_AUX_START + 1)
            }
            raw if raw >= SPA_AUDIO_CHANNEL_CUSTOM_START => {
                format!("custom-{}", raw - SPA_AUDIO_CHANNEL_CUSTOM_START + 1)
            }
            _ => "unknown".into(),
        }
    }

    pub fn name(self) -> String {
        match self.raw {
            0 => "Unknown".into(),
            1 => "N/A".into(),
            2 => "Mono".into(),
            3 => "Front Left".into(),
            4 => "Front Right".into(),
            5 => "Front Center".into(),
            6 => "Low Frequency Effects".into(),
            7 => "Side Left".into(),
            8 => "Side Right".into(),
            9 => "Front Left Center".into(),
            10 => "Front Right Center".into(),
            11 => "Rear Center".into(),
            12 => "Rear Left".into(),
            13 => "Rear Right".into(),
            14 => "Top Center".into(),
            15 => "Top Front Left".into(),
            16 => "Top Front Center".into(),
            17 => "Top Front Right".into(),
            18 => "Top Rear Left".into(),
            19 => "Top Rear Center".into(),
            20 => "Top Rear Right".into(),
            21 => "Rear Left Center".into(),
            22 => "Rear Right Center".into(),
            23 => "Front Left Wide".into(),
            24 => "Front Right Wide".into(),
            25 => "Low Frequency Effects 2".into(),
            26 => "Front Left High".into(),
            27 => "Front Center High".into(),
            28 => "Front Right High".into(),
            29 => "Top Front Left Center".into(),
            30 => "Top Front Right Center".into(),
            31 => "Top Side Left".into(),
            32 => "Top Side Right".into(),
            33 => "Low Frequency Effects Left".into(),
            34 => "Low Frequency Effects Right".into(),
            35 => "Bottom Center".into(),
            36 => "Bottom Left Center".into(),
            37 => "Bottom Right Center".into(),
            raw if (SPA_AUDIO_CHANNEL_AUX_START..=SPA_AUDIO_CHANNEL_AUX_END).contains(&raw) => {
                format!("Aux {}", raw - SPA_AUDIO_CHANNEL_AUX_START + 1)
            }
            raw if raw >= SPA_AUDIO_CHANNEL_CUSTOM_START => {
                format!("Custom {}", raw - SPA_AUDIO_CHANNEL_CUSTOM_START + 1)
            }
            _ => "Unknown".into(),
        }
    }

    pub const fn is_auxiliary(self) -> bool {
        self.raw >= SPA_AUDIO_CHANNEL_AUX_START && self.raw <= SPA_AUDIO_CHANNEL_AUX_END
    }

    pub const fn is_custom(self) -> bool {
        self.raw >= SPA_AUDIO_CHANNEL_CUSTOM_START
    }
}

pub(crate) fn normalize_channel_positions(
    volume_count: usize,
    positions: &[u32],
) -> Vec<PipeWireAudioChannelPosition> {
    if volume_count == 0 {
        return Vec::new();
    }
    if positions.is_empty() {
        let fallback: &[u32] = match volume_count {
            1 => &[2],
            2 => &[3, 4],
            3 => &[3, 4, 6],
            4 => &[3, 4, 12, 13],
            5 => &[3, 4, 5, 7, 8],
            6 => &[3, 4, 5, 6, 7, 8],
            7 => &[3, 4, 5, 12, 13, 7, 8],
            8 => &[3, 4, 5, 6, 12, 13, 7, 8],
            _ => &[],
        };
        if !fallback.is_empty() {
            return fallback
                .iter()
                .copied()
                .map(PipeWireAudioChannelPosition::new)
                .collect();
        }
    }
    (0..volume_count)
        .map(|index| PipeWireAudioChannelPosition::new(*positions.get(index).unwrap_or(&0)))
        .collect()
}

pub(crate) fn perceptual_average(channels: &[FiniteVolume]) -> Option<FiniteVolume> {
    if channels.is_empty() {
        return None;
    }
    let sum = channels.iter().try_fold(0.0_f32, |sum, value| {
        let next = sum + value.get();
        next.is_finite().then_some(next)
    })?;
    FiniteVolume::new(sum / channels.len() as f32)
}

pub(crate) fn scaled_perceptual_channels(
    channels: &[FiniteVolume],
    desired_average: f32,
) -> Option<Vec<FiniteVolume>> {
    if !desired_average.is_finite()
        || !(0.0..=MAX_PERCEPTUAL_VOLUME).contains(&desired_average)
        || channels.is_empty()
        || channels.len() > MAX_AUDIO_CHANNELS
    {
        return None;
    }
    let current_average = perceptual_average(channels)?.get();
    let perceptual = if current_average > 0.0 {
        let scale = desired_average / current_average;
        channels
            .iter()
            .map(|channel| channel.get() * scale)
            .collect::<Vec<_>>()
    } else {
        vec![desired_average; channels.len()]
    };
    perceptual.into_iter().map(FiniteVolume::new).collect()
}

pub(crate) fn perceptual_channels_to_linear(channels: &[FiniteVolume]) -> Option<Vec<f32>> {
    if channels.is_empty() || channels.len() > MAX_AUDIO_CHANNELS {
        return None;
    }
    channels
        .iter()
        .map(|value| {
            let value = value.get();
            let linear = value * value * value;
            (linear.is_finite() && linear >= 0.0).then_some(linear)
        })
        .collect()
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
    pub metadata_available: bool,
    pub actual_sink: PipeWireDefaultTarget,
    pub actual_source: PipeWireDefaultTarget,
    pub configured_sink: PipeWireDefaultTarget,
    pub configured_source: PipeWireDefaultTarget,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipeWireControlCounters {
    pub audio_state_activations: u64,
    pub audio_state_releases: u64,
    pub audio_parameter_updates: u64,
    pub mute_writes_sent: u64,
    pub volume_writes_sent: u64,
    pub writes_coalesced: u64,
    pub writes_confirmed: u64,
    pub writes_failed: u64,
    pub writes_timed_out: u64,
    pub stale_writes_rejected: u64,
    pub duplicate_writes_suppressed: u64,
    pub average_intents: u64,
    pub channel_intents: u64,
    pub vectors_sent: u64,
    pub vectors_coalesced: u64,
    pub duplicate_vectors_suppressed: u64,
    pub vectors_confirmed: u64,
    pub vectors_failed: u64,
    pub vectors_timed_out: u64,
    pub stale_vectors_rejected: u64,
    pub layout_invalidated_intents: u64,
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
    pub controls: PipeWireControlCounters,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawNodeAudioInfo {
    pub raw_id: u32,
    pub channel_volumes: Option<Vec<f32>>,
    pub channel_positions: Option<Vec<u32>>,
    pub muted: Option<bool>,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PipeWireDelta {
    NodeAdded {
        raw_id: u32,
        properties: BTreeMap<String, String>,
    },
    NodePermissions {
        raw_id: u32,
        writable: bool,
    },
    NodeInfo(RawNodeInfo),
    NodeTracking {
        raw_id: u32,
        tracked: bool,
    },
    NodeAudioInfo(RawNodeAudioInfo),
    NodeAudioTracking {
        raw_id: u32,
        tracked: bool,
    },
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
    fn perceptual_volume_matches_the_audited_channel_rule() {
        let audio =
            PipeWireNodeAudioSnapshot::from_linear_channels(&[1.0, 0.125], Some(false), true)
                .unwrap();
        assert!(audio.ready);
        assert!(audio.can_set_volume);
        assert!(audio.can_set_mute);
        assert!((audio.channels[0].get() - 1.0).abs() < 0.000_001);
        assert!((audio.channels[1].get() - 0.5).abs() < 0.000_001);
        assert!((audio.average_volume.unwrap().get() - 0.75).abs() < 0.000_001);

        let scaled = perceptual_channels_to_linear(
            &scaled_perceptual_channels(&audio.channels, 1.5).unwrap(),
        )
        .unwrap();
        assert!((scaled[0] - 8.0).abs() < 0.000_01);
        assert!((scaled[1] - 1.0).abs() < 0.000_01);
    }

    #[test]
    fn zero_balance_and_invalid_vectors_are_contained() {
        let audio =
            PipeWireNodeAudioSnapshot::from_linear_channels(&[0.0, 0.0], Some(true), true).unwrap();
        let scaled = perceptual_channels_to_linear(
            &scaled_perceptual_channels(&audio.channels, 0.5).unwrap(),
        )
        .unwrap();
        assert_eq!(scaled, vec![0.125, 0.125]);
        assert!(
            PipeWireNodeAudioSnapshot::from_linear_channels(&[f32::NAN], Some(false), true)
                .is_none()
        );
        assert!(scaled_perceptual_channels(&audio.channels, MAX_PERCEPTUAL_VOLUME + 0.1).is_none());
        assert!(scaled_perceptual_channels(&[], 0.5).is_none());
    }

    #[test]
    fn channel_positions_cover_named_auxiliary_custom_and_unknown_values() {
        let named = [
            (0, "unknown", "Unknown"),
            (1, "na", "N/A"),
            (2, "mono", "Mono"),
            (3, "front-left", "Front Left"),
            (4, "front-right", "Front Right"),
            (5, "front-center", "Front Center"),
            (6, "lfe", "Low Frequency Effects"),
            (7, "side-left", "Side Left"),
            (8, "side-right", "Side Right"),
            (9, "front-left-center", "Front Left Center"),
            (10, "front-right-center", "Front Right Center"),
            (11, "rear-center", "Rear Center"),
            (12, "rear-left", "Rear Left"),
            (13, "rear-right", "Rear Right"),
            (14, "top-center", "Top Center"),
            (15, "top-front-left", "Top Front Left"),
            (16, "top-front-center", "Top Front Center"),
            (17, "top-front-right", "Top Front Right"),
            (18, "top-rear-left", "Top Rear Left"),
            (19, "top-rear-center", "Top Rear Center"),
            (20, "top-rear-right", "Top Rear Right"),
            (21, "rear-left-center", "Rear Left Center"),
            (22, "rear-right-center", "Rear Right Center"),
            (23, "front-left-wide", "Front Left Wide"),
            (24, "front-right-wide", "Front Right Wide"),
            (25, "lfe-2", "Low Frequency Effects 2"),
            (26, "front-left-high", "Front Left High"),
            (27, "front-center-high", "Front Center High"),
            (28, "front-right-high", "Front Right High"),
            (29, "top-front-left-center", "Top Front Left Center"),
            (30, "top-front-right-center", "Top Front Right Center"),
            (31, "top-side-left", "Top Side Left"),
            (32, "top-side-right", "Top Side Right"),
            (33, "lfe-left", "Low Frequency Effects Left"),
            (34, "lfe-right", "Low Frequency Effects Right"),
            (35, "bottom-center", "Bottom Center"),
            (36, "bottom-left-center", "Bottom Left Center"),
            (37, "bottom-right-center", "Bottom Right Center"),
        ];
        for (raw, token, name) in named {
            let position = PipeWireAudioChannelPosition::new(raw);
            assert_eq!(position.token(), token);
            assert_eq!(position.name(), name);
        }
        let auxiliary = PipeWireAudioChannelPosition::new(SPA_AUDIO_CHANNEL_AUX_START + 63);
        assert_eq!(auxiliary.token(), "aux-64");
        assert_eq!(auxiliary.name(), "Aux 64");
        assert!(auxiliary.is_auxiliary());
        let custom = PipeWireAudioChannelPosition::new(SPA_AUDIO_CHANNEL_CUSTOM_START + 4);
        assert_eq!(custom.token(), "custom-5");
        assert_eq!(custom.name(), "Custom 5");
        assert!(custom.is_custom());
        assert_eq!(PipeWireAudioChannelPosition::new(999).token(), "unknown");
    }

    #[test]
    fn fallback_channel_layouts_match_the_reference_order() {
        assert_eq!(
            normalize_channel_positions(6, &[])
                .into_iter()
                .map(PipeWireAudioChannelPosition::token)
                .collect::<Vec<_>>(),
            [
                "front-left",
                "front-right",
                "front-center",
                "lfe",
                "side-left",
                "side-right"
            ]
        );
        assert_eq!(
            normalize_channel_positions(3, &[3])
                .into_iter()
                .map(PipeWireAudioChannelPosition::token)
                .collect::<Vec<_>>(),
            ["front-left", "unknown", "unknown"]
        );
        assert_eq!(normalize_channel_positions(2, &[3, 4, 5]).len(), 2);
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
