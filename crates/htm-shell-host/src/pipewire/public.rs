use super::model::{
    PipeWireAvailability, PipeWireDefaultTarget, PipeWireNodeSnapshot, PipeWireNodeState,
    PipeWireSnapshot,
};
use htm_runtime::{
    ItemBindingKey, NumericValue, PipeWireDocumentDemand, RepeatItemSnapshot, RepeatSource,
    RepeatSourceSnapshot, StateBindingKey, StateToken,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PipeWireProjections {
    pub text: Vec<(StateBindingKey, String)>,
    pub tokens: Vec<(StateBindingKey, StateToken)>,
    pub values: Vec<(StateBindingKey, NumericValue)>,
    pub booleans: Vec<(StateBindingKey, Option<bool>)>,
    pub repeats: Vec<RepeatSourceSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PipeWireDemand {
    pub documents: usize,
    pub service: bool,
    pub nodes: bool,
    pub node_details: bool,
    pub defaults: bool,
    pub links: bool,
    pub property_keys: std::collections::BTreeSet<String>,
}

impl PipeWireDemand {
    pub(crate) fn add_document(&mut self, demand: &PipeWireDocumentDemand) {
        if demand.is_empty() {
            return;
        }
        self.documents = self.documents.saturating_add(1);
        self.service |= demand.service;
        self.nodes |= demand.nodes;
        self.node_details |= demand.node_details;
        self.defaults |= demand.defaults;
        self.property_keys
            .extend(demand.property_keys.iter().cloned());
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.documents == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipeWireNodeType {
    Untracked,
    Audio,
    Video,
    Stream,
    Source,
    Sink,
    AudioSink,
    AudioSource,
    AudioDuplex,
    AudioOutputStream,
    AudioInputStream,
    VideoSource,
    VideoSink,
    Unknown,
}

impl PipeWireNodeType {
    pub const ALL: [Self; 14] = [
        Self::Untracked,
        Self::Audio,
        Self::Video,
        Self::Stream,
        Self::Source,
        Self::Sink,
        Self::AudioSink,
        Self::AudioSource,
        Self::AudioDuplex,
        Self::AudioOutputStream,
        Self::AudioInputStream,
        Self::VideoSource,
        Self::VideoSink,
        Self::Unknown,
    ];

    pub const fn text(self) -> &'static str {
        match self {
            Self::Untracked => "Untracked",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Stream => "Stream",
            Self::Source => "Source",
            Self::Sink => "Sink",
            Self::AudioSink => "Audio sink",
            Self::AudioSource => "Audio source",
            Self::AudioDuplex => "Audio duplex",
            Self::AudioOutputStream => "Audio output stream",
            Self::AudioInputStream => "Audio input stream",
            Self::VideoSource => "Video source",
            Self::VideoSink => "Video sink",
            Self::Unknown => "Unknown",
        }
    }

    pub const fn token(self) -> StateToken {
        match self {
            Self::Untracked => StateToken::Untracked,
            Self::Audio => StateToken::Audio,
            Self::Video => StateToken::Video,
            Self::Stream => StateToken::Stream,
            Self::Source => StateToken::Source,
            Self::Sink => StateToken::Sink,
            Self::AudioSink => StateToken::AudioSink,
            Self::AudioSource => StateToken::AudioSource,
            Self::AudioDuplex => StateToken::AudioDuplex,
            Self::AudioOutputStream => StateToken::AudioOutputStream,
            Self::AudioInputStream => StateToken::AudioInputStream,
            Self::VideoSource => StateToken::VideoSource,
            Self::VideoSink => StateToken::VideoSink,
            Self::Unknown => StateToken::Unknown,
        }
    }

    pub(crate) fn from_node(node: &PipeWireNodeSnapshot) -> Self {
        match node.media_class.as_deref() {
            Some("Audio/Sink") => Self::AudioSink,
            Some("Audio/Source") => Self::AudioSource,
            Some("Audio/Duplex") => Self::AudioDuplex,
            Some("Stream/Output/Audio") => Self::AudioOutputStream,
            Some("Stream/Input/Audio") => Self::AudioInputStream,
            Some("Video/Source") => Self::VideoSource,
            Some("Video/Sink") => Self::VideoSink,
            None if node.classification.audio
                && !node.classification.video
                && !node.classification.stream
                && !node.classification.source
                && !node.classification.sink =>
            {
                Self::Audio
            }
            None if node.classification.video
                && !node.classification.audio
                && !node.classification.stream
                && !node.classification.source
                && !node.classification.sink =>
            {
                Self::Video
            }
            None if !node.classification.audio
                && !node.classification.video
                && !node.classification.stream
                && !node.classification.source
                && !node.classification.sink =>
            {
                Self::Untracked
            }
            _ if node.classification.stream
                && !node.classification.audio
                && !node.classification.video
                && !node.classification.source
                && !node.classification.sink =>
            {
                Self::Stream
            }
            _ if node.classification.source
                && !node.classification.audio
                && !node.classification.video
                && !node.classification.stream
                && !node.classification.sink =>
            {
                Self::Source
            }
            _ if node.classification.sink
                && !node.classification.audio
                && !node.classification.video
                && !node.classification.stream
                && !node.classification.source =>
            {
                Self::Sink
            }
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWireNodeDirection {
    Sink,
    Source,
    Bidirectional,
    Absent,
    Unknown,
}

impl PipeWireNodeDirection {
    pub const ALL: [Self; 5] = [
        Self::Sink,
        Self::Source,
        Self::Bidirectional,
        Self::Absent,
        Self::Unknown,
    ];

    pub const fn text(self) -> &'static str {
        match self {
            Self::Sink => "Sink",
            Self::Source => "Source",
            Self::Bidirectional => "Bidirectional",
            Self::Absent => "No direction",
            Self::Unknown => "Unknown",
        }
    }

    pub const fn token(self) -> StateToken {
        match self {
            Self::Sink => StateToken::Sink,
            Self::Source => StateToken::Source,
            Self::Bidirectional => StateToken::Bidirectional,
            Self::Absent => StateToken::Absent,
            Self::Unknown => StateToken::Unknown,
        }
    }

    fn from_node(node: &PipeWireNodeSnapshot) -> Self {
        let (sink, source) = if node.classification.sink || node.classification.source {
            (node.classification.sink, node.classification.source)
        } else {
            (
                node.classification.accepts_input,
                node.classification.produces_output,
            )
        };
        match (sink, source, node.ready) {
            (true, true, _) => Self::Bidirectional,
            (true, false, _) => Self::Sink,
            (false, true, _) => Self::Source,
            (false, false, true) => Self::Absent,
            (false, false, false) if node.media_class.is_none() => Self::Absent,
            (false, false, false) => Self::Unknown,
        }
    }
}

impl PipeWireSnapshot {
    pub(crate) fn public_projections(&self, demand: &PipeWireDemand) -> PipeWireProjections {
        let mut projections = PipeWireProjections::default();
        projections.text.push((
            StateBindingKey::PipeWireAvailability,
            availability_text(self.availability).into(),
        ));
        projections.tokens.push((
            StateBindingKey::PipeWireAvailability,
            availability_token(self.availability),
        ));
        projections.text.push((
            StateBindingKey::PipeWireReady,
            if self.ready { "true" } else { "false" }.into(),
        ));
        projections
            .tokens
            .push((StateBindingKey::PipeWireReady, bool_token(self.ready)));
        projections
            .booleans
            .push((StateBindingKey::PipeWireReady, Some(self.ready)));
        projections.values.push((
            StateBindingKey::PipeWireNodeCount,
            NumericValue::Integer(if self.ready {
                self.nodes.len() as i64
            } else {
                0
            }),
        ));

        project_default(
            self,
            &self.defaults.actual_sink,
            DefaultKeys::ACTUAL_SINK,
            &mut projections,
        );
        project_default(
            self,
            &self.defaults.actual_source,
            DefaultKeys::ACTUAL_SOURCE,
            &mut projections,
        );
        project_default(
            self,
            &self.defaults.configured_sink,
            DefaultKeys::CONFIGURED_SINK,
            &mut projections,
        );
        project_default(
            self,
            &self.defaults.configured_source,
            DefaultKeys::CONFIGURED_SOURCE,
            &mut projections,
        );

        projections.repeats.push(RepeatSourceSnapshot {
            source: RepeatSource::PipeWireNodes,
            source_generation: self.connection_generation,
            items: if self.ready {
                self.nodes
                    .iter()
                    .map(|node| project_node(self, node, demand))
                    .collect()
            } else {
                Vec::new()
            },
        });
        projections
    }
}

#[derive(Clone, Copy)]
struct DefaultKeys {
    status: StateBindingKey,
    name: StateBindingKey,
    nickname: StateBindingKey,
    description: StateBindingKey,
    media_class: StateBindingKey,
    raw_id: StateBindingKey,
}

impl DefaultKeys {
    const ACTUAL_SINK: Self = Self {
        status: StateBindingKey::PipeWireDefaultSinkStatus,
        name: StateBindingKey::PipeWireDefaultSinkName,
        nickname: StateBindingKey::PipeWireDefaultSinkNickname,
        description: StateBindingKey::PipeWireDefaultSinkDescription,
        media_class: StateBindingKey::PipeWireDefaultSinkMediaClass,
        raw_id: StateBindingKey::PipeWireDefaultSinkRawId,
    };
    const ACTUAL_SOURCE: Self = Self {
        status: StateBindingKey::PipeWireDefaultSourceStatus,
        name: StateBindingKey::PipeWireDefaultSourceName,
        nickname: StateBindingKey::PipeWireDefaultSourceNickname,
        description: StateBindingKey::PipeWireDefaultSourceDescription,
        media_class: StateBindingKey::PipeWireDefaultSourceMediaClass,
        raw_id: StateBindingKey::PipeWireDefaultSourceRawId,
    };
    const CONFIGURED_SINK: Self = Self {
        status: StateBindingKey::PipeWireConfiguredSinkStatus,
        name: StateBindingKey::PipeWireConfiguredSinkName,
        nickname: StateBindingKey::PipeWireConfiguredSinkNickname,
        description: StateBindingKey::PipeWireConfiguredSinkDescription,
        media_class: StateBindingKey::PipeWireConfiguredSinkMediaClass,
        raw_id: StateBindingKey::PipeWireConfiguredSinkRawId,
    };
    const CONFIGURED_SOURCE: Self = Self {
        status: StateBindingKey::PipeWireConfiguredSourceStatus,
        name: StateBindingKey::PipeWireConfiguredSourceName,
        nickname: StateBindingKey::PipeWireConfiguredSourceNickname,
        description: StateBindingKey::PipeWireConfiguredSourceDescription,
        media_class: StateBindingKey::PipeWireConfiguredSourceMediaClass,
        raw_id: StateBindingKey::PipeWireConfiguredSourceRawId,
    };
}

fn project_default(
    snapshot: &PipeWireSnapshot,
    target: &PipeWireDefaultTarget,
    keys: DefaultKeys,
    projections: &mut PipeWireProjections,
) {
    let status = if snapshot.availability != PipeWireAvailability::Ready
        || !snapshot.defaults.metadata_available
    {
        DefaultStatus::Unavailable
    } else if target.node.is_some() {
        DefaultStatus::Available
    } else {
        DefaultStatus::Unresolved
    };
    projections.text.push((keys.status, status.text().into()));
    projections.tokens.push((keys.status, status.token()));
    let node = target
        .node
        .and_then(|id| snapshot.nodes.iter().find(|node| node.id == id));
    projections.text.push((
        keys.name,
        option_text(node.and_then(|node| node.name.as_deref())),
    ));
    projections.text.push((
        keys.nickname,
        option_text(node.and_then(|node| node.nickname.as_deref())),
    ));
    projections.text.push((
        keys.description,
        option_text(node.and_then(|node| node.description.as_deref())),
    ));
    projections.text.push((
        keys.media_class,
        option_text(node.and_then(|node| node.media_class.as_deref())),
    ));
    projections.values.push((
        keys.raw_id,
        node.map(|node| NumericValue::Integer(node.raw_global_id as i64))
            .unwrap_or(NumericValue::Unknown),
    ));
}

fn project_node(
    snapshot: &PipeWireSnapshot,
    node: &PipeWireNodeSnapshot,
    demand: &PipeWireDemand,
) -> RepeatItemSnapshot {
    let node_type = PipeWireNodeType::from_node(node);
    let direction = PipeWireNodeDirection::from_node(node);
    let mut text = BTreeMap::from([
        (ItemBindingKey::Ready, bool_text(node.ready)),
        (ItemBindingKey::Name, option_text(node.name.as_deref())),
        (
            ItemBindingKey::Nickname,
            option_text(node.nickname.as_deref()),
        ),
        (
            ItemBindingKey::Description,
            option_text(node.description.as_deref()),
        ),
        (
            ItemBindingKey::MediaClass,
            option_text(node.media_class.as_deref()),
        ),
        (ItemBindingKey::NodeType, node_type.text().into()),
        (
            ItemBindingKey::NodeState,
            node_state_text(node.state).into(),
        ),
        (ItemBindingKey::Direction, direction.text().into()),
        (
            ItemBindingKey::IsAudio,
            bool_text(node.classification.audio),
        ),
        (
            ItemBindingKey::IsVideo,
            bool_text(node.classification.video),
        ),
        (
            ItemBindingKey::IsStream,
            bool_text(node.classification.stream),
        ),
        (ItemBindingKey::IsSink, bool_text(node.classification.sink)),
        (
            ItemBindingKey::IsSource,
            bool_text(node.classification.source),
        ),
    ]);
    let actual_role = default_role(snapshot, node, false);
    let configured_role = default_role(snapshot, node, true);
    text.insert(ItemBindingKey::DefaultRole, actual_role.text().into());
    text.insert(
        ItemBindingKey::ConfiguredRole,
        configured_role.text().into(),
    );
    let tokens = BTreeMap::from([
        (ItemBindingKey::Ready, bool_token(node.ready)),
        (ItemBindingKey::NodeType, node_type.token()),
        (ItemBindingKey::NodeState, node_state_token(node.state)),
        (ItemBindingKey::Direction, direction.token()),
        (
            ItemBindingKey::IsAudio,
            bool_token(node.classification.audio),
        ),
        (
            ItemBindingKey::IsVideo,
            bool_token(node.classification.video),
        ),
        (
            ItemBindingKey::IsStream,
            bool_token(node.classification.stream),
        ),
        (ItemBindingKey::IsSink, bool_token(node.classification.sink)),
        (
            ItemBindingKey::IsSource,
            bool_token(node.classification.source),
        ),
        (ItemBindingKey::DefaultRole, actual_role.token(false)),
        (ItemBindingKey::ConfiguredRole, configured_role.token(true)),
    ]);
    let properties = demand
        .property_keys
        .iter()
        .filter_map(|key| {
            node.properties
                .get(key)
                .map(|value| (key.clone(), value.clone()))
        })
        .collect();
    RepeatItemSnapshot {
        key: format!("{}:{}", node.id.connection_generation, node.id.global_id),
        text,
        tokens,
        values: BTreeMap::from([(
            ItemBindingKey::RawId,
            NumericValue::Integer(node.raw_global_id as i64),
        )]),
        properties,
    }
}

#[derive(Clone, Copy)]
enum DefaultStatus {
    Unavailable,
    Unresolved,
    Available,
}

impl DefaultStatus {
    const fn text(self) -> &'static str {
        match self {
            Self::Unavailable => "Unavailable",
            Self::Unresolved => "Unresolved",
            Self::Available => "Available",
        }
    }

    const fn token(self) -> StateToken {
        match self {
            Self::Unavailable => StateToken::Unavailable,
            Self::Unresolved => StateToken::Unresolved,
            Self::Available => StateToken::Available,
        }
    }
}

#[derive(Clone, Copy)]
enum NodeRole {
    None,
    Sink,
    Source,
    SinkAndSource,
}

impl NodeRole {
    const fn text(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Sink => "Sink",
            Self::Source => "Source",
            Self::SinkAndSource => "Sink and source",
        }
    }

    const fn token(self, configured: bool) -> StateToken {
        match (self, configured) {
            (Self::None, _) => StateToken::None,
            (Self::Sink, false) => StateToken::DefaultSink,
            (Self::Source, false) => StateToken::DefaultSource,
            (Self::SinkAndSource, false) => StateToken::DefaultSinkAndSource,
            (Self::Sink, true) => StateToken::ConfiguredSink,
            (Self::Source, true) => StateToken::ConfiguredSource,
            (Self::SinkAndSource, true) => StateToken::ConfiguredSinkAndSource,
        }
    }
}

fn default_role(
    snapshot: &PipeWireSnapshot,
    node: &PipeWireNodeSnapshot,
    configured: bool,
) -> NodeRole {
    let (sink, source) = if configured {
        (
            snapshot.defaults.configured_sink.node,
            snapshot.defaults.configured_source.node,
        )
    } else {
        (
            snapshot.defaults.actual_sink.node,
            snapshot.defaults.actual_source.node,
        )
    };
    match (sink == Some(node.id), source == Some(node.id)) {
        (true, true) => NodeRole::SinkAndSource,
        (true, false) => NodeRole::Sink,
        (false, true) => NodeRole::Source,
        (false, false) => NodeRole::None,
    }
}

const fn availability_text(availability: PipeWireAvailability) -> &'static str {
    match availability {
        PipeWireAvailability::Unavailable => "unavailable",
        PipeWireAvailability::Synchronizing => "synchronizing",
        PipeWireAvailability::Ready => "ready",
    }
}

const fn availability_token(availability: PipeWireAvailability) -> StateToken {
    match availability {
        PipeWireAvailability::Unavailable => StateToken::Unavailable,
        PipeWireAvailability::Synchronizing => StateToken::Synchronizing,
        PipeWireAvailability::Ready => StateToken::Ready,
    }
}

const fn node_state_text(state: PipeWireNodeState) -> &'static str {
    match state {
        PipeWireNodeState::Unknown => "Unknown",
        PipeWireNodeState::Error => "Error",
        PipeWireNodeState::Creating => "Creating",
        PipeWireNodeState::Suspended => "Suspended",
        PipeWireNodeState::Idle => "Idle",
        PipeWireNodeState::Running => "Running",
    }
}

const fn node_state_token(state: PipeWireNodeState) -> StateToken {
    match state {
        PipeWireNodeState::Unknown => StateToken::Unknown,
        PipeWireNodeState::Error => StateToken::Error,
        PipeWireNodeState::Creating => StateToken::Creating,
        PipeWireNodeState::Suspended => StateToken::Suspended,
        PipeWireNodeState::Idle => StateToken::Idle,
        PipeWireNodeState::Running => StateToken::Running,
    }
}

fn option_text(value: Option<&str>) -> String {
    value.unwrap_or("—").to_owned()
}

fn bool_text(value: bool) -> String {
    if value { "true" } else { "false" }.into()
}

const fn bool_token(value: bool) -> StateToken {
    if value {
        StateToken::True
    } else {
        StateToken::False
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipewire::model::{
        PipeWireDefaultsSnapshot, PipeWireNodeClassification, PipeWireNodeId,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn node(raw_id: u32, media_class: &str) -> PipeWireNodeSnapshot {
        PipeWireNodeSnapshot {
            id: PipeWireNodeId {
                connection_generation: 7,
                global_id: raw_id,
            },
            raw_global_id: raw_id,
            name: Some(format!("node-{raw_id}")),
            nickname: Some("Short name".into()),
            description: Some("Description".into()),
            media_class: Some(media_class.into()),
            classification: PipeWireNodeClassification::from_properties(
                Some(media_class),
                &BTreeMap::new(),
                1,
                1,
            ),
            state: PipeWireNodeState::Running,
            raw_state: 3,
            state_error: None,
            input_ports: 1,
            output_ports: 1,
            properties: BTreeMap::from([
                ("application.name".into(), "Player".into()),
                ("media.title".into(), "Track".into()),
            ]),
            audio_capable: media_class.contains("Audio"),
            ready: true,
        }
    }

    #[test]
    fn public_projection_is_typed_bounded_and_generation_safe() {
        let sink = node(42, "Audio/Sink");
        let mut snapshot = PipeWireSnapshot {
            availability: PipeWireAvailability::Ready,
            connection_generation: 7,
            ready: true,
            node_count: 1,
            nodes: vec![sink.clone()],
            defaults: PipeWireDefaultsSnapshot {
                metadata_available: true,
                actual_sink: PipeWireDefaultTarget {
                    metadata_name: sink.name.clone(),
                    node: Some(sink.id),
                    ..PipeWireDefaultTarget::default()
                },
                ..PipeWireDefaultsSnapshot::default()
            },
            ..PipeWireSnapshot::default()
        };
        let demand = PipeWireDemand {
            documents: 2,
            service: true,
            nodes: true,
            node_details: true,
            defaults: true,
            property_keys: BTreeSet::from(["application.name".into()]),
            ..PipeWireDemand::default()
        };
        let projections = snapshot.public_projections(&demand);
        let repeat = &projections.repeats[0];
        assert_eq!(repeat.source, RepeatSource::PipeWireNodes);
        assert_eq!(repeat.source_generation, 7);
        assert_eq!(repeat.items[0].key, "7:42");
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::NodeType],
            StateToken::AudioSink
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::DefaultRole],
            StateToken::DefaultSink
        );
        assert_eq!(
            repeat.items[0].properties,
            BTreeMap::from([("application.name".into(), "Player".into())])
        );
        assert!(projections.tokens.contains(&(
            StateBindingKey::PipeWireDefaultSinkStatus,
            StateToken::Available
        )));

        snapshot.connection_generation = 8;
        snapshot.nodes[0].id.connection_generation = 8;
        assert_eq!(
            snapshot.public_projections(&demand).repeats[0].items[0].key,
            "8:42"
        );
    }

    #[test]
    fn node_type_profile_preserves_every_audited_composite() {
        for (media_class, expected) in [
            ("Audio/Sink", PipeWireNodeType::AudioSink),
            ("Audio/Source", PipeWireNodeType::AudioSource),
            ("Audio/Duplex", PipeWireNodeType::AudioDuplex),
            ("Stream/Output/Audio", PipeWireNodeType::AudioOutputStream),
            ("Stream/Input/Audio", PipeWireNodeType::AudioInputStream),
            ("Video/Source", PipeWireNodeType::VideoSource),
            ("Video/Sink", PipeWireNodeType::VideoSink),
        ] {
            assert_eq!(PipeWireNodeType::from_node(&node(1, media_class)), expected);
        }
        assert_eq!(PipeWireNodeType::ALL.len(), 14);
        assert_eq!(PipeWireNodeDirection::ALL.len(), 5);
    }

    #[test]
    fn node_type_leaf_values_states_and_directions_are_finite() {
        let mut modeled = node(1, "Other");
        modeled.media_class = None;
        for (classification, expected) in [
            (
                PipeWireNodeClassification::default(),
                PipeWireNodeType::Untracked,
            ),
            (
                PipeWireNodeClassification {
                    audio: true,
                    ..PipeWireNodeClassification::default()
                },
                PipeWireNodeType::Audio,
            ),
            (
                PipeWireNodeClassification {
                    video: true,
                    ..PipeWireNodeClassification::default()
                },
                PipeWireNodeType::Video,
            ),
            (
                PipeWireNodeClassification {
                    stream: true,
                    ..PipeWireNodeClassification::default()
                },
                PipeWireNodeType::Stream,
            ),
            (
                PipeWireNodeClassification {
                    source: true,
                    ..PipeWireNodeClassification::default()
                },
                PipeWireNodeType::Source,
            ),
            (
                PipeWireNodeClassification {
                    sink: true,
                    ..PipeWireNodeClassification::default()
                },
                PipeWireNodeType::Sink,
            ),
            (
                PipeWireNodeClassification {
                    audio: true,
                    video: true,
                    ..PipeWireNodeClassification::default()
                },
                PipeWireNodeType::Unknown,
            ),
        ] {
            modeled.classification = classification;
            assert_eq!(PipeWireNodeType::from_node(&modeled), expected);
        }

        for state in [
            PipeWireNodeState::Unknown,
            PipeWireNodeState::Error,
            PipeWireNodeState::Creating,
            PipeWireNodeState::Suspended,
            PipeWireNodeState::Idle,
            PipeWireNodeState::Running,
        ] {
            assert!(!node_state_text(state).is_empty());
            assert!(!node_state_token(state).as_str().is_empty());
        }

        for direction in PipeWireNodeDirection::ALL {
            assert!(!direction.text().is_empty());
            assert!(!direction.token().as_str().is_empty());
        }
    }

    #[test]
    fn document_demand_is_counted_once_and_property_keys_are_deduplicated() {
        let document = PipeWireDocumentDemand {
            service: true,
            nodes: true,
            node_details: true,
            defaults: false,
            property_keys: BTreeSet::from(["media.title".into()]),
        };
        let mut demand = PipeWireDemand::default();
        demand.add_document(&document);
        demand.add_document(&document);
        assert_eq!(demand.documents, 2);
        assert_eq!(demand.property_keys.len(), 1);
        assert!(demand.node_details);
        assert!(!demand.defaults);
    }
}
