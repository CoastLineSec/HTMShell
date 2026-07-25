use super::model::{
    PipeWireAvailability, PipeWireDefaultTarget, PipeWireLinkGroupSnapshot, PipeWireLinkSnapshot,
    PipeWireLinkState, PipeWireNodeId, PipeWireNodeSnapshot, PipeWireNodeState, PipeWireSnapshot,
};
use htm_runtime::{
    ContextualRepeatSnapshot, ItemBindingKey, NumericValue, PipeWireDocumentDemand,
    RepeatItemSnapshot, RepeatSource, RepeatSourceSnapshot, StateBindingKey, StateToken,
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
    pub audio_state: bool,
    pub audio_writes: bool,
    pub channel_projection: bool,
    pub channel_writes: bool,
    pub link_collection: bool,
    pub link_details: bool,
    pub link_group_collection: bool,
    pub group_members: bool,
    pub node_link_tracking: bool,
    pub relation_projection: bool,
    pub configured_default_writes: bool,
    pub preferred_sink_writes: bool,
    pub preferred_source_writes: bool,
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
        self.audio_state |= demand.audio_state;
        self.audio_writes |= demand.audio_writes;
        self.channel_projection |= demand.channel_projection;
        self.channel_writes |= demand.channel_writes;
        self.link_collection |= demand.link_collection;
        self.link_details |= demand.link_details;
        self.link_group_collection |= demand.link_group_collection;
        self.group_members |= demand.group_members;
        self.node_link_tracking |= demand.node_link_tracking;
        self.relation_projection |= demand.relation_projection;
        self.configured_default_writes |= demand.configured_default_writes;
        self.preferred_sink_writes |= demand.preferred_sink_writes;
        self.preferred_source_writes |= demand.preferred_source_writes;
        self.links |= demand.link_collection
            || demand.link_group_collection
            || demand.node_link_tracking
            || demand.group_members;
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
        projections.values.push((
            StateBindingKey::PipeWireLinkCount,
            NumericValue::Integer(if self.ready {
                self.links.len() as i64
            } else {
                0
            }),
        ));
        projections.values.push((
            StateBindingKey::PipeWireLinkGroupCount,
            NumericValue::Integer(if self.ready {
                self.link_groups.len() as i64
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
        projections.repeats.push(RepeatSourceSnapshot {
            source: RepeatSource::PipeWireLinks,
            source_generation: self.connection_generation,
            items: if self.ready && demand.link_collection {
                self.links
                    .iter()
                    .map(|link| project_link(self, link))
                    .collect()
            } else {
                Vec::new()
            },
        });
        projections.repeats.push(RepeatSourceSnapshot {
            source: RepeatSource::PipeWireLinkGroups,
            source_generation: self.connection_generation,
            items: if self.ready && demand.link_group_collection {
                self.link_groups
                    .iter()
                    .map(|group| project_group(self, group, demand, None))
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
    audio_status: StateBindingKey,
    volume: StateBindingKey,
    mute_state: StateBindingKey,
    can_set_volume: StateBindingKey,
    can_set_mute: StateBindingKey,
    can_clear: Option<StateBindingKey>,
}

impl DefaultKeys {
    const ACTUAL_SINK: Self = Self {
        status: StateBindingKey::PipeWireDefaultSinkStatus,
        name: StateBindingKey::PipeWireDefaultSinkName,
        nickname: StateBindingKey::PipeWireDefaultSinkNickname,
        description: StateBindingKey::PipeWireDefaultSinkDescription,
        media_class: StateBindingKey::PipeWireDefaultSinkMediaClass,
        raw_id: StateBindingKey::PipeWireDefaultSinkRawId,
        audio_status: StateBindingKey::PipeWireDefaultSinkAudioStatus,
        volume: StateBindingKey::PipeWireDefaultSinkVolume,
        mute_state: StateBindingKey::PipeWireDefaultSinkMuteState,
        can_set_volume: StateBindingKey::PipeWireDefaultSinkCanSetVolume,
        can_set_mute: StateBindingKey::PipeWireDefaultSinkCanSetMute,
        can_clear: None,
    };
    const ACTUAL_SOURCE: Self = Self {
        status: StateBindingKey::PipeWireDefaultSourceStatus,
        name: StateBindingKey::PipeWireDefaultSourceName,
        nickname: StateBindingKey::PipeWireDefaultSourceNickname,
        description: StateBindingKey::PipeWireDefaultSourceDescription,
        media_class: StateBindingKey::PipeWireDefaultSourceMediaClass,
        raw_id: StateBindingKey::PipeWireDefaultSourceRawId,
        audio_status: StateBindingKey::PipeWireDefaultSourceAudioStatus,
        volume: StateBindingKey::PipeWireDefaultSourceVolume,
        mute_state: StateBindingKey::PipeWireDefaultSourceMuteState,
        can_set_volume: StateBindingKey::PipeWireDefaultSourceCanSetVolume,
        can_set_mute: StateBindingKey::PipeWireDefaultSourceCanSetMute,
        can_clear: None,
    };
    const CONFIGURED_SINK: Self = Self {
        status: StateBindingKey::PipeWireConfiguredSinkStatus,
        name: StateBindingKey::PipeWireConfiguredSinkName,
        nickname: StateBindingKey::PipeWireConfiguredSinkNickname,
        description: StateBindingKey::PipeWireConfiguredSinkDescription,
        media_class: StateBindingKey::PipeWireConfiguredSinkMediaClass,
        raw_id: StateBindingKey::PipeWireConfiguredSinkRawId,
        audio_status: StateBindingKey::PipeWireConfiguredSinkAudioStatus,
        volume: StateBindingKey::PipeWireConfiguredSinkVolume,
        mute_state: StateBindingKey::PipeWireConfiguredSinkMuteState,
        can_set_volume: StateBindingKey::PipeWireConfiguredSinkCanSetVolume,
        can_set_mute: StateBindingKey::PipeWireConfiguredSinkCanSetMute,
        can_clear: Some(StateBindingKey::PipeWireConfiguredSinkCanClear),
    };
    const CONFIGURED_SOURCE: Self = Self {
        status: StateBindingKey::PipeWireConfiguredSourceStatus,
        name: StateBindingKey::PipeWireConfiguredSourceName,
        nickname: StateBindingKey::PipeWireConfiguredSourceNickname,
        description: StateBindingKey::PipeWireConfiguredSourceDescription,
        media_class: StateBindingKey::PipeWireConfiguredSourceMediaClass,
        raw_id: StateBindingKey::PipeWireConfiguredSourceRawId,
        audio_status: StateBindingKey::PipeWireConfiguredSourceAudioStatus,
        volume: StateBindingKey::PipeWireConfiguredSourceVolume,
        mute_state: StateBindingKey::PipeWireConfiguredSourceMuteState,
        can_set_volume: StateBindingKey::PipeWireConfiguredSourceCanSetVolume,
        can_set_mute: StateBindingKey::PipeWireConfiguredSourceCanSetMute,
        can_clear: Some(StateBindingKey::PipeWireConfiguredSourceCanClear),
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
    let audio_status = node.map(audio_status).unwrap_or(AudioStatus::Unavailable);
    projections
        .text
        .push((keys.audio_status, audio_status.text().into()));
    projections
        .tokens
        .push((keys.audio_status, audio_status.token()));
    let volume = node
        .and_then(|node| node.audio.average_volume)
        .map(|volume| NumericValue::Decimal(f64::from(volume.get())))
        .unwrap_or(NumericValue::Unknown);
    projections.values.push((keys.volume, volume));
    let mute = node.and_then(|node| node.audio.muted);
    projections
        .text
        .push((keys.mute_state, mute_text(mute).into()));
    projections.tokens.push((keys.mute_state, mute_token(mute)));
    let can_set_volume = node.is_some_and(|node| node.audio.can_set_volume);
    let can_set_mute = node.is_some_and(|node| node.audio.can_set_mute);
    for (key, value) in [
        (keys.can_set_volume, can_set_volume),
        (keys.can_set_mute, can_set_mute),
    ] {
        projections.text.push((key, bool_text(value)));
        projections.tokens.push((key, bool_token(value)));
        projections.booleans.push((key, Some(value)));
    }
    if let Some(key) = keys.can_clear {
        let value = snapshot.ready
            && snapshot.defaults.metadata_writable
            && (target.metadata_name.is_some() || target.unresolved_value.is_some());
        projections.text.push((key, bool_text(value)));
        projections.tokens.push((key, bool_token(value)));
        projections.booleans.push((key, Some(value)));
    }
}

fn project_node(
    snapshot: &PipeWireSnapshot,
    node: &PipeWireNodeSnapshot,
    demand: &PipeWireDemand,
) -> RepeatItemSnapshot {
    let node_type = PipeWireNodeType::from_node(node);
    let direction = PipeWireNodeDirection::from_node(node);
    let tracked_groups = if demand.node_link_tracking {
        tracked_groups(snapshot, node)
    } else {
        Vec::new()
    };
    let (can_set_preferred_sink, can_set_preferred_source) =
        super::preferred_node_capabilities(snapshot, node);
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
        (
            ItemBindingKey::AudioStatus,
            audio_status(node).text().into(),
        ),
        (
            ItemBindingKey::MuteState,
            mute_text(node.audio.muted).into(),
        ),
        (
            ItemBindingKey::CanSetVolume,
            bool_text(node.audio.can_set_volume),
        ),
        (
            ItemBindingKey::CanSetMute,
            bool_text(node.audio.can_set_mute),
        ),
        (
            ItemBindingKey::CanSetPreferredSink,
            bool_text(can_set_preferred_sink),
        ),
        (
            ItemBindingKey::CanSetPreferredSource,
            bool_text(can_set_preferred_source),
        ),
        (
            ItemBindingKey::ChannelStatus,
            channel_status(node).text().into(),
        ),
        (
            ItemBindingKey::LinkGroupStatus,
            if snapshot.ready {
                "Ready"
            } else {
                "Unavailable"
            }
            .into(),
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
        (
            ItemBindingKey::Ready,
            bool_token(node.ready).as_str().into(),
        ),
        (ItemBindingKey::NodeType, node_type.token().as_str().into()),
        (
            ItemBindingKey::NodeState,
            node_state_token(node.state).as_str().into(),
        ),
        (ItemBindingKey::Direction, direction.token().as_str().into()),
        (
            ItemBindingKey::IsAudio,
            bool_token(node.classification.audio).as_str().into(),
        ),
        (
            ItemBindingKey::IsVideo,
            bool_token(node.classification.video).as_str().into(),
        ),
        (
            ItemBindingKey::IsStream,
            bool_token(node.classification.stream).as_str().into(),
        ),
        (
            ItemBindingKey::IsSink,
            bool_token(node.classification.sink).as_str().into(),
        ),
        (
            ItemBindingKey::IsSource,
            bool_token(node.classification.source).as_str().into(),
        ),
        (
            ItemBindingKey::AudioStatus,
            audio_status(node).token().as_str().into(),
        ),
        (
            ItemBindingKey::MuteState,
            mute_token(node.audio.muted).as_str().into(),
        ),
        (
            ItemBindingKey::CanSetVolume,
            bool_token(node.audio.can_set_volume).as_str().into(),
        ),
        (
            ItemBindingKey::CanSetMute,
            bool_token(node.audio.can_set_mute).as_str().into(),
        ),
        (
            ItemBindingKey::CanSetPreferredSink,
            bool_token(can_set_preferred_sink).as_str().into(),
        ),
        (
            ItemBindingKey::CanSetPreferredSource,
            bool_token(can_set_preferred_source).as_str().into(),
        ),
        (
            ItemBindingKey::ChannelStatus,
            channel_status(node).token().as_str().into(),
        ),
        (
            ItemBindingKey::LinkGroupStatus,
            if snapshot.ready {
                StateToken::Ready
            } else {
                StateToken::Unavailable
            }
            .as_str()
            .into(),
        ),
        (
            ItemBindingKey::DefaultRole,
            actual_role.token(false).as_str().into(),
        ),
        (
            ItemBindingKey::ConfiguredRole,
            configured_role.token(true).as_str().into(),
        ),
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
        values: BTreeMap::from([
            (
                ItemBindingKey::RawId,
                NumericValue::Integer(node.raw_global_id as i64),
            ),
            (
                ItemBindingKey::Volume,
                node.audio
                    .average_volume
                    .map(|value| NumericValue::Decimal(f64::from(value.get())))
                    .unwrap_or(NumericValue::Unknown),
            ),
            (
                ItemBindingKey::ChannelCount,
                NumericValue::Integer(node.audio.channels.len() as i64),
            ),
            (
                ItemBindingKey::LinkGroupCount,
                NumericValue::Integer(tracked_groups.len() as i64),
            ),
        ]),
        properties,
        channels: demand.channel_projection.then(|| project_channels(node)),
        links: None,
        link_groups: demand.node_link_tracking.then(|| ContextualRepeatSnapshot {
            source_generation: snapshot.connection_generation,
            items: tracked_groups
                .into_iter()
                .map(|group| project_group(snapshot, group, demand, Some(node.id)))
                .collect(),
        }),
    }
}

fn project_channels(node: &PipeWireNodeSnapshot) -> ContextualRepeatSnapshot {
    let items = node
        .audio
        .channels
        .iter()
        .zip(&node.audio.channel_positions)
        .enumerate()
        .map(|(index, (volume, position))| {
            let token = position.token();
            let can_set = node.audio.can_set_volume;
            RepeatItemSnapshot {
                key: format!(
                    "{}:{index}:{}",
                    node.audio.channel_layout_generation, position.raw
                ),
                text: BTreeMap::from([
                    (ItemBindingKey::PositionName, position.name()),
                    (ItemBindingKey::Position, token.clone()),
                    (ItemBindingKey::Status, "Ready".into()),
                    (ItemBindingKey::CanSetVolume, bool_text(can_set)),
                    (
                        ItemBindingKey::IsAuxiliary,
                        bool_text(position.is_auxiliary()),
                    ),
                    (ItemBindingKey::IsCustom, bool_text(position.is_custom())),
                ]),
                tokens: BTreeMap::from([
                    (ItemBindingKey::Position, token),
                    (ItemBindingKey::Status, StateToken::Ready.as_str().into()),
                    (
                        ItemBindingKey::CanSetVolume,
                        bool_token(can_set).as_str().into(),
                    ),
                    (
                        ItemBindingKey::IsAuxiliary,
                        bool_token(position.is_auxiliary()).as_str().into(),
                    ),
                    (
                        ItemBindingKey::IsCustom,
                        bool_token(position.is_custom()).as_str().into(),
                    ),
                ]),
                values: BTreeMap::from([
                    (ItemBindingKey::Index, NumericValue::Integer(index as i64)),
                    (
                        ItemBindingKey::Volume,
                        NumericValue::Decimal(f64::from(volume.get())),
                    ),
                ]),
                properties: BTreeMap::new(),
                channels: None,
                links: None,
                link_groups: None,
            }
        })
        .collect();
    ContextualRepeatSnapshot {
        source_generation: node.audio.channel_layout_generation,
        items,
    }
}

fn project_link(snapshot: &PipeWireSnapshot, link: &PipeWireLinkSnapshot) -> RepeatItemSnapshot {
    let readiness = link_readiness(snapshot, link);
    let is_monitor =
        relation_node(snapshot, link.target_node).is_some_and(|node| node.classification.monitor);
    let mut text = BTreeMap::from([
        (ItemBindingKey::Ready, readiness.text().into()),
        (ItemBindingKey::State, link_state_text(link.state).into()),
        (ItemBindingKey::IsMonitor, bool_text(is_monitor)),
    ]);
    let mut tokens = BTreeMap::from([
        (ItemBindingKey::Ready, readiness.token().as_str().into()),
        (
            ItemBindingKey::State,
            link_state_token(link.state).as_str().into(),
        ),
        (
            ItemBindingKey::IsMonitor,
            bool_token(is_monitor).as_str().into(),
        ),
    ]);
    let mut values = BTreeMap::from([
        (
            ItemBindingKey::RawId,
            NumericValue::Integer(link.raw_global_id as i64),
        ),
        (
            ItemBindingKey::SourcePortId,
            optional_id(link.source_port_id),
        ),
        (
            ItemBindingKey::TargetPortId,
            optional_id(link.target_port_id),
        ),
    ]);
    project_relation(
        snapshot,
        link.source_node,
        RelationKeys::SOURCE,
        &mut text,
        &mut tokens,
        &mut values,
    );
    project_relation(
        snapshot,
        link.target_node,
        RelationKeys::TARGET,
        &mut text,
        &mut tokens,
        &mut values,
    );
    RepeatItemSnapshot {
        key: format!("{}:{}", link.id.connection_generation, link.id.global_id),
        text,
        tokens,
        values,
        properties: BTreeMap::new(),
        channels: None,
        links: None,
        link_groups: None,
    }
}

fn project_group(
    snapshot: &PipeWireSnapshot,
    group: &PipeWireLinkGroupSnapshot,
    demand: &PipeWireDemand,
    tracked_node: Option<PipeWireNodeId>,
) -> RepeatItemSnapshot {
    let readiness = group_readiness(snapshot, group);
    let is_monitor =
        relation_node(snapshot, group.target_node).is_some_and(|node| node.classification.monitor);
    let mut text = BTreeMap::from([
        (ItemBindingKey::Ready, readiness.text().into()),
        (ItemBindingKey::State, link_state_text(group.state).into()),
        (ItemBindingKey::IsMonitor, bool_text(is_monitor)),
    ]);
    let mut tokens = BTreeMap::from([
        (ItemBindingKey::Ready, readiness.token().as_str().into()),
        (
            ItemBindingKey::State,
            link_state_token(group.state).as_str().into(),
        ),
        (
            ItemBindingKey::IsMonitor,
            bool_token(is_monitor).as_str().into(),
        ),
    ]);
    let mut values = BTreeMap::from([
        (
            ItemBindingKey::MemberCount,
            NumericValue::Integer(group.members.len() as i64),
        ),
        (
            ItemBindingKey::RepresentativeLinkRawId,
            NumericValue::Integer(group.representative.global_id as i64),
        ),
    ]);
    project_relation(
        snapshot,
        group.source_node,
        RelationKeys::SOURCE,
        &mut text,
        &mut tokens,
        &mut values,
    );
    project_relation(
        snapshot,
        group.target_node,
        RelationKeys::TARGET,
        &mut text,
        &mut tokens,
        &mut values,
    );
    if let Some(node_id) = tracked_node {
        let connection = connection_direction(group, node_id);
        text.insert(
            ItemBindingKey::ConnectionDirection,
            connection.text().into(),
        );
        tokens.insert(
            ItemBindingKey::ConnectionDirection,
            connection.token().as_str().into(),
        );
        let peer = match connection {
            ConnectionDirection::Incoming => group.source_node,
            ConnectionDirection::Outgoing => group.target_node,
            ConnectionDirection::SelfConnection => Some(node_id),
            ConnectionDirection::Unknown => None,
        };
        project_relation(
            snapshot,
            peer,
            RelationKeys::PEER,
            &mut text,
            &mut tokens,
            &mut values,
        );
    }
    RepeatItemSnapshot {
        key: format!(
            "{}:{}:{}",
            group.id.connection_generation, group.id.source_node, group.id.target_node
        ),
        text,
        tokens,
        values,
        properties: BTreeMap::new(),
        channels: None,
        links: (tracked_node.is_none() && demand.group_members).then(|| ContextualRepeatSnapshot {
            source_generation: snapshot.connection_generation,
            items: group
                .members
                .iter()
                .filter_map(|member| {
                    snapshot
                        .links
                        .iter()
                        .find(|link| link.id == *member)
                        .map(|link| project_link(snapshot, link))
                })
                .collect(),
        }),
        link_groups: None,
    }
}

fn tracked_groups<'a>(
    snapshot: &'a PipeWireSnapshot,
    node: &PipeWireNodeSnapshot,
) -> Vec<&'a PipeWireLinkGroupSnapshot> {
    snapshot
        .link_groups
        .iter()
        .filter(|group| {
            let selected = if node.classification.sink {
                group.target_node == Some(node.id)
            } else {
                group.source_node == Some(node.id)
            };
            selected
                && !relation_node(snapshot, group.target_node)
                    .is_some_and(|target| target.classification.monitor)
        })
        .collect()
}

#[derive(Clone, Copy)]
struct RelationKeys {
    status: ItemBindingKey,
    name: ItemBindingKey,
    nickname: ItemBindingKey,
    description: ItemBindingKey,
    media_class: ItemBindingKey,
    node_type: ItemBindingKey,
    node_state: ItemBindingKey,
    direction: ItemBindingKey,
    raw_id: ItemBindingKey,
}

impl RelationKeys {
    const SOURCE: Self = Self {
        status: ItemBindingKey::SourceStatus,
        name: ItemBindingKey::SourceName,
        nickname: ItemBindingKey::SourceNickname,
        description: ItemBindingKey::SourceDescription,
        media_class: ItemBindingKey::SourceMediaClass,
        node_type: ItemBindingKey::SourceNodeType,
        node_state: ItemBindingKey::SourceNodeState,
        direction: ItemBindingKey::SourceDirection,
        raw_id: ItemBindingKey::SourceRawId,
    };
    const TARGET: Self = Self {
        status: ItemBindingKey::TargetStatus,
        name: ItemBindingKey::TargetName,
        nickname: ItemBindingKey::TargetNickname,
        description: ItemBindingKey::TargetDescription,
        media_class: ItemBindingKey::TargetMediaClass,
        node_type: ItemBindingKey::TargetNodeType,
        node_state: ItemBindingKey::TargetNodeState,
        direction: ItemBindingKey::TargetDirection,
        raw_id: ItemBindingKey::TargetRawId,
    };
    const PEER: Self = Self {
        status: ItemBindingKey::PeerStatus,
        name: ItemBindingKey::PeerName,
        nickname: ItemBindingKey::PeerNickname,
        description: ItemBindingKey::PeerDescription,
        media_class: ItemBindingKey::PeerMediaClass,
        node_type: ItemBindingKey::PeerNodeType,
        node_state: ItemBindingKey::PeerNodeState,
        direction: ItemBindingKey::PeerDirection,
        raw_id: ItemBindingKey::PeerRawId,
    };
}

fn project_relation(
    snapshot: &PipeWireSnapshot,
    id: Option<PipeWireNodeId>,
    keys: RelationKeys,
    text: &mut BTreeMap<ItemBindingKey, String>,
    tokens: &mut BTreeMap<ItemBindingKey, String>,
    values: &mut BTreeMap<ItemBindingKey, NumericValue>,
) {
    let node = relation_node(snapshot, id);
    let status = match (id, node) {
        (None, _) => RelationStatus::Unavailable,
        (Some(_), None) => RelationStatus::Unresolved,
        (Some(_), Some(_)) => RelationStatus::Available,
    };
    text.insert(keys.status, status.text().into());
    tokens.insert(keys.status, status.token().as_str().into());
    text.insert(
        keys.name,
        option_text(node.and_then(|node| node.name.as_deref())),
    );
    text.insert(
        keys.nickname,
        option_text(node.and_then(|node| node.nickname.as_deref())),
    );
    text.insert(
        keys.description,
        option_text(node.and_then(|node| node.description.as_deref())),
    );
    text.insert(
        keys.media_class,
        option_text(node.and_then(|node| node.media_class.as_deref())),
    );
    let node_type = node.map(PipeWireNodeType::from_node);
    text.insert(
        keys.node_type,
        node_type.map_or("Unknown", PipeWireNodeType::text).into(),
    );
    tokens.insert(
        keys.node_type,
        node_type
            .map_or(StateToken::Unknown, PipeWireNodeType::token)
            .as_str()
            .into(),
    );
    text.insert(
        keys.node_state,
        node.map_or("Unknown", |node| node_state_text(node.state))
            .into(),
    );
    tokens.insert(
        keys.node_state,
        node.map_or(StateToken::Unknown, |node| node_state_token(node.state))
            .as_str()
            .into(),
    );
    let direction = node.map(PipeWireNodeDirection::from_node);
    text.insert(
        keys.direction,
        direction
            .map_or("Unknown", PipeWireNodeDirection::text)
            .into(),
    );
    tokens.insert(
        keys.direction,
        direction
            .map_or(StateToken::Unknown, PipeWireNodeDirection::token)
            .as_str()
            .into(),
    );
    values.insert(
        keys.raw_id,
        node.map(|node| NumericValue::Integer(node.raw_global_id as i64))
            .unwrap_or(NumericValue::Unknown),
    );
}

fn relation_node(
    snapshot: &PipeWireSnapshot,
    id: Option<PipeWireNodeId>,
) -> Option<&PipeWireNodeSnapshot> {
    id.and_then(|id| snapshot.nodes.iter().find(|node| node.id == id))
}

#[derive(Clone, Copy)]
enum RelationStatus {
    Unavailable,
    Unresolved,
    Available,
}

impl RelationStatus {
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
enum GraphReadiness {
    Unavailable,
    Partial,
    Ready,
}

impl GraphReadiness {
    const fn text(self) -> &'static str {
        match self {
            Self::Unavailable => "Unavailable",
            Self::Partial => "Partial",
            Self::Ready => "Ready",
        }
    }

    const fn token(self) -> StateToken {
        match self {
            Self::Unavailable => StateToken::Unavailable,
            Self::Partial => StateToken::Partial,
            Self::Ready => StateToken::Ready,
        }
    }
}

fn link_readiness(snapshot: &PipeWireSnapshot, link: &PipeWireLinkSnapshot) -> GraphReadiness {
    if !link.ready {
        GraphReadiness::Unavailable
    } else if relation_node(snapshot, link.source_node).is_some()
        && relation_node(snapshot, link.target_node).is_some()
    {
        GraphReadiness::Ready
    } else {
        GraphReadiness::Partial
    }
}

fn group_readiness(
    snapshot: &PipeWireSnapshot,
    group: &PipeWireLinkGroupSnapshot,
) -> GraphReadiness {
    if group.members.is_empty() {
        GraphReadiness::Unavailable
    } else if relation_node(snapshot, group.source_node).is_some()
        && relation_node(snapshot, group.target_node).is_some()
    {
        GraphReadiness::Ready
    } else {
        GraphReadiness::Partial
    }
}

#[derive(Clone, Copy)]
enum ConnectionDirection {
    Incoming,
    Outgoing,
    SelfConnection,
    Unknown,
}

impl ConnectionDirection {
    const fn text(self) -> &'static str {
        match self {
            Self::Incoming => "Incoming",
            Self::Outgoing => "Outgoing",
            Self::SelfConnection => "Self",
            Self::Unknown => "Unknown",
        }
    }

    const fn token(self) -> StateToken {
        match self {
            Self::Incoming => StateToken::Incoming,
            Self::Outgoing => StateToken::Outgoing,
            Self::SelfConnection => StateToken::SelfConnection,
            Self::Unknown => StateToken::Unknown,
        }
    }
}

fn connection_direction(
    group: &PipeWireLinkGroupSnapshot,
    node: PipeWireNodeId,
) -> ConnectionDirection {
    match (
        group.source_node == Some(node),
        group.target_node == Some(node),
    ) {
        (true, true) => ConnectionDirection::SelfConnection,
        (true, false) => ConnectionDirection::Outgoing,
        (false, true) => ConnectionDirection::Incoming,
        (false, false) => ConnectionDirection::Unknown,
    }
}

const fn link_state_text(state: PipeWireLinkState) -> &'static str {
    match state {
        PipeWireLinkState::Unknown => "Unknown",
        PipeWireLinkState::Error => "Error",
        PipeWireLinkState::Unlinked => "Unlinked",
        PipeWireLinkState::Init => "Init",
        PipeWireLinkState::Negotiating => "Negotiating",
        PipeWireLinkState::Allocating => "Allocating",
        PipeWireLinkState::Paused => "Paused",
        PipeWireLinkState::Active => "Active",
    }
}

const fn link_state_token(state: PipeWireLinkState) -> StateToken {
    match state {
        PipeWireLinkState::Unknown => StateToken::Unknown,
        PipeWireLinkState::Error => StateToken::Error,
        PipeWireLinkState::Unlinked => StateToken::Unlinked,
        PipeWireLinkState::Init => StateToken::Init,
        PipeWireLinkState::Negotiating => StateToken::Negotiating,
        PipeWireLinkState::Allocating => StateToken::Allocating,
        PipeWireLinkState::Paused => StateToken::Paused,
        PipeWireLinkState::Active => StateToken::Active,
    }
}

fn optional_id(id: Option<u32>) -> NumericValue {
    id.map(|id| NumericValue::Integer(id as i64))
        .unwrap_or(NumericValue::Unknown)
}

#[derive(Clone, Copy)]
enum AudioStatus {
    Unsupported,
    Unavailable,
    Ready,
}

impl AudioStatus {
    const fn text(self) -> &'static str {
        match self {
            Self::Unsupported => "Unsupported",
            Self::Unavailable => "Unavailable",
            Self::Ready => "Ready",
        }
    }

    const fn token(self) -> StateToken {
        match self {
            Self::Unsupported => StateToken::Unsupported,
            Self::Unavailable => StateToken::Unavailable,
            Self::Ready => StateToken::Ready,
        }
    }
}

fn audio_status(node: &PipeWireNodeSnapshot) -> AudioStatus {
    if !node.audio_capable {
        AudioStatus::Unsupported
    } else if node.audio.ready {
        AudioStatus::Ready
    } else {
        AudioStatus::Unavailable
    }
}

fn channel_status(node: &PipeWireNodeSnapshot) -> AudioStatus {
    if !node.audio_capable {
        AudioStatus::Unsupported
    } else if !node.audio.channels.is_empty()
        && node.audio.channels.len() == node.audio.channel_positions.len()
    {
        AudioStatus::Ready
    } else {
        AudioStatus::Unavailable
    }
}

const fn mute_text(muted: Option<bool>) -> &'static str {
    match muted {
        Some(true) => "Muted",
        Some(false) => "Unmuted",
        None => "Unavailable",
    }
}

const fn mute_token(muted: Option<bool>) -> StateToken {
    match muted {
        Some(true) => StateToken::Muted,
        Some(false) => StateToken::Unmuted,
        None => StateToken::Unavailable,
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
        PipeWireDefaultsSnapshot, PipeWireLinkGroupId, PipeWireLinkId, PipeWireNodeAudioSnapshot,
        PipeWireNodeClassification, PipeWireNodeId,
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
            audio: Default::default(),
            ready: true,
        }
    }

    #[test]
    fn public_projection_is_typed_bounded_and_generation_safe() {
        let mut sink = node(42, "Audio/Sink");
        sink.audio =
            PipeWireNodeAudioSnapshot::from_linear_channels(&[1.0, 0.125], Some(false), true)
                .unwrap();
        let mut snapshot = PipeWireSnapshot {
            availability: PipeWireAvailability::Ready,
            connection_generation: 7,
            ready: true,
            node_count: 1,
            nodes: vec![sink.clone()],
            defaults: PipeWireDefaultsSnapshot {
                metadata_available: true,
                metadata_writable: true,
                metadata_generation: 3,
                actual_sink: PipeWireDefaultTarget {
                    metadata_name: sink.name.clone(),
                    node: Some(sink.id),
                    ..PipeWireDefaultTarget::default()
                },
                configured_sink: PipeWireDefaultTarget {
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
            channel_projection: true,
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
            StateToken::AudioSink.as_str()
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::DefaultRole],
            StateToken::DefaultSink.as_str()
        );
        assert_eq!(
            repeat.items[0].properties,
            BTreeMap::from([("application.name".into(), "Player".into())])
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::AudioStatus],
            StateToken::Ready.as_str()
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::MuteState],
            StateToken::Unmuted.as_str()
        );
        assert_eq!(
            repeat.items[0].values[&ItemBindingKey::Volume],
            NumericValue::Decimal(0.75)
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::CanSetVolume],
            StateToken::True.as_str()
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::CanSetPreferredSink],
            StateToken::True.as_str()
        );
        assert_eq!(
            repeat.items[0].tokens[&ItemBindingKey::CanSetPreferredSource],
            StateToken::False.as_str()
        );
        let channels = repeat.items[0].channels.as_ref().unwrap();
        assert_eq!(channels.source_generation, 1);
        assert_eq!(channels.items.len(), 2);
        assert_eq!(channels.items[0].key, "1:0:3");
        assert_eq!(
            channels.items[0].tokens[&ItemBindingKey::Position],
            "front-left"
        );
        assert_eq!(
            channels.items[1].values[&ItemBindingKey::Volume],
            NumericValue::Decimal(0.5)
        );
        assert!(projections.tokens.contains(&(
            StateBindingKey::PipeWireDefaultSinkStatus,
            StateToken::Available
        )));
        assert!(projections.tokens.contains(&(
            StateBindingKey::PipeWireDefaultSinkAudioStatus,
            StateToken::Ready
        )));
        assert!(projections.values.contains(&(
            StateBindingKey::PipeWireDefaultSinkVolume,
            NumericValue::Decimal(0.75)
        )));
        assert!(
            projections
                .booleans
                .contains(&(StateBindingKey::PipeWireDefaultSinkCanSetMute, Some(true)))
        );
        assert!(
            projections
                .booleans
                .contains(&(StateBindingKey::PipeWireConfiguredSinkCanClear, Some(true)))
        );

        snapshot.connection_generation = 8;
        snapshot.nodes[0].id.connection_generation = 8;
        assert_eq!(
            snapshot.public_projections(&demand).repeats[0].items[0].key,
            "8:42"
        );
    }

    #[test]
    fn unsupported_and_incomplete_audio_nodes_remain_explicit() {
        let video = node(1, "Video/Source");
        assert_eq!(audio_status(&video).token(), StateToken::Unsupported);
        assert_eq!(channel_status(&video).token(), StateToken::Unsupported);
        assert_eq!(mute_token(video.audio.muted), StateToken::Unavailable);

        let mut audio = node(2, "Audio/Source");
        assert_eq!(audio_status(&audio).token(), StateToken::Unavailable);
        assert_eq!(channel_status(&audio).token(), StateToken::Unavailable);
        assert!(!audio.audio.can_set_volume);
        assert!(!audio.audio.can_set_mute);

        audio.audio =
            PipeWireNodeAudioSnapshot::from_linear_channels(&[1.0], Some(false), false).unwrap();
        assert_eq!(channel_status(&audio).token(), StateToken::Ready);
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
            audio_state: false,
            audio_writes: false,
            channel_projection: true,
            channel_writes: true,
            property_keys: BTreeSet::from(["media.title".into()]),
            ..PipeWireDocumentDemand::default()
        };
        let mut demand = PipeWireDemand::default();
        demand.add_document(&document);
        demand.add_document(&document);
        assert_eq!(demand.documents, 2);
        assert_eq!(demand.property_keys.len(), 1);
        assert!(demand.node_details);
        assert!(!demand.defaults);
        assert!(demand.channel_projection);
        assert!(demand.channel_writes);
    }

    #[test]
    fn link_states_and_missing_endpoint_statuses_are_finite() {
        for (state, text, token) in [
            (PipeWireLinkState::Unknown, "Unknown", StateToken::Unknown),
            (PipeWireLinkState::Error, "Error", StateToken::Error),
            (
                PipeWireLinkState::Unlinked,
                "Unlinked",
                StateToken::Unlinked,
            ),
            (PipeWireLinkState::Init, "Init", StateToken::Init),
            (
                PipeWireLinkState::Negotiating,
                "Negotiating",
                StateToken::Negotiating,
            ),
            (
                PipeWireLinkState::Allocating,
                "Allocating",
                StateToken::Allocating,
            ),
            (PipeWireLinkState::Paused, "Paused", StateToken::Paused),
            (PipeWireLinkState::Active, "Active", StateToken::Active),
        ] {
            assert_eq!(link_state_text(state), text);
            assert_eq!(link_state_token(state), token);
        }

        let sink = node(2, "Audio/Sink");
        let missing_source = PipeWireNodeId {
            connection_generation: 7,
            global_id: 1,
        };
        let mut link = PipeWireLinkSnapshot {
            id: PipeWireLinkId {
                connection_generation: 7,
                global_id: 20,
            },
            raw_global_id: 20,
            source_node: Some(missing_source),
            target_node: Some(sink.id),
            source_node_present: false,
            target_node_present: true,
            source_port_id: None,
            target_port_id: Some(40),
            state: PipeWireLinkState::Active,
            raw_state: 5,
            ready: true,
        };
        let snapshot = PipeWireSnapshot {
            availability: PipeWireAvailability::Ready,
            connection_generation: 7,
            ready: true,
            nodes: vec![sink],
            links: vec![link.clone()],
            ..PipeWireSnapshot::default()
        };
        let projected = project_link(&snapshot, &link);
        assert_eq!(
            projected.tokens[&ItemBindingKey::Ready],
            StateToken::Partial.as_str()
        );
        assert_eq!(
            projected.tokens[&ItemBindingKey::SourceStatus],
            StateToken::Unresolved.as_str()
        );
        assert_eq!(
            projected.tokens[&ItemBindingKey::TargetStatus],
            StateToken::Available.as_str()
        );
        assert_eq!(
            projected.values[&ItemBindingKey::SourcePortId],
            NumericValue::Unknown
        );

        link.source_node = None;
        let projected = project_link(&snapshot, &link);
        assert_eq!(
            projected.tokens[&ItemBindingKey::SourceStatus],
            StateToken::Unavailable.as_str()
        );
    }

    #[test]
    fn monitor_groups_remain_top_level_but_are_excluded_from_node_tracking() {
        let source = node(1, "Audio/Source");
        let sink = node(2, "Audio/Sink");
        let mut monitor = node(3, "Audio/Sink");
        monitor
            .properties
            .insert("media.category".into(), "Monitor".into());
        monitor.classification = PipeWireNodeClassification::from_properties(
            monitor.media_class.as_deref(),
            &monitor.properties,
            monitor.input_ports,
            monitor.output_ports,
        );
        assert!(monitor.classification.monitor);

        let links = [(20, sink.id), (21, monitor.id)]
            .into_iter()
            .map(|(raw_id, target)| PipeWireLinkSnapshot {
                id: PipeWireLinkId {
                    connection_generation: 7,
                    global_id: raw_id,
                },
                raw_global_id: raw_id,
                source_node: Some(source.id),
                target_node: Some(target),
                source_node_present: true,
                target_node_present: true,
                source_port_id: Some(raw_id + 10),
                target_port_id: Some(raw_id + 20),
                state: PipeWireLinkState::Active,
                raw_state: 5,
                ready: true,
            })
            .collect::<Vec<_>>();
        let groups = links
            .iter()
            .map(|link| PipeWireLinkGroupSnapshot {
                id: PipeWireLinkGroupId {
                    connection_generation: 7,
                    source_node: source.id.global_id,
                    target_node: link.target_node.unwrap().global_id,
                },
                source_node: Some(source.id),
                target_node: link.target_node,
                source_node_present: true,
                target_node_present: true,
                members: vec![link.id],
                representative: link.id,
                state: link.state,
            })
            .collect::<Vec<_>>();
        let snapshot = PipeWireSnapshot {
            availability: PipeWireAvailability::Ready,
            connection_generation: 7,
            ready: true,
            node_count: 3,
            link_count: 2,
            link_group_count: 2,
            nodes: vec![source, sink, monitor],
            links,
            link_groups: groups,
            ..PipeWireSnapshot::default()
        };
        let demand = PipeWireDemand {
            documents: 1,
            service: true,
            nodes: true,
            links: true,
            link_collection: true,
            link_group_collection: true,
            node_link_tracking: true,
            ..PipeWireDemand::default()
        };
        let projections = snapshot.public_projections(&demand);
        let top_links = projections
            .repeats
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireLinks)
            .unwrap();
        assert_eq!(top_links.items.len(), 2);
        assert_eq!(
            top_links.items[1].tokens[&ItemBindingKey::IsMonitor],
            StateToken::True.as_str()
        );
        let top_groups = projections
            .repeats
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireLinkGroups)
            .unwrap();
        assert_eq!(top_groups.items.len(), 2);

        let nodes = projections
            .repeats
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireNodes)
            .unwrap();
        let source = nodes
            .items
            .iter()
            .find(|item| item.key.ends_with(":1"))
            .unwrap();
        assert_eq!(
            source.values[&ItemBindingKey::LinkGroupCount],
            NumericValue::Integer(1)
        );
        let monitor = nodes
            .items
            .iter()
            .find(|item| item.key.ends_with(":3"))
            .unwrap();
        assert_eq!(
            monitor.values[&ItemBindingKey::LinkGroupCount],
            NumericValue::Integer(0)
        );
    }

    #[test]
    fn node_tracking_uses_sink_incoming_and_otherwise_outgoing_selection() {
        let source = node(1, "Audio/Source");
        let sink = node(2, "Audio/Sink");
        let duplex = node(3, "Audio/Duplex");
        let unknown = node(4, "Other");
        let pairs = [
            (10, source.id, sink.id),
            (11, source.id, duplex.id),
            (12, duplex.id, sink.id),
            (13, unknown.id, sink.id),
            (14, source.id, source.id),
        ];
        let groups = pairs
            .into_iter()
            .map(
                |(raw_id, source_node, target_node)| PipeWireLinkGroupSnapshot {
                    id: PipeWireLinkGroupId {
                        connection_generation: 7,
                        source_node: source_node.global_id,
                        target_node: target_node.global_id,
                    },
                    source_node: Some(source_node),
                    target_node: Some(target_node),
                    source_node_present: true,
                    target_node_present: true,
                    members: vec![PipeWireLinkId {
                        connection_generation: 7,
                        global_id: raw_id,
                    }],
                    representative: PipeWireLinkId {
                        connection_generation: 7,
                        global_id: raw_id,
                    },
                    state: PipeWireLinkState::Active,
                },
            )
            .collect();
        let snapshot = PipeWireSnapshot {
            availability: PipeWireAvailability::Ready,
            connection_generation: 7,
            ready: true,
            nodes: vec![
                source.clone(),
                sink.clone(),
                duplex.clone(),
                unknown.clone(),
            ],
            link_groups: groups,
            ..PipeWireSnapshot::default()
        };

        assert_eq!(tracked_groups(&snapshot, &source).len(), 3);
        assert_eq!(tracked_groups(&snapshot, &sink).len(), 3);
        assert_eq!(tracked_groups(&snapshot, &duplex).len(), 1);
        assert_eq!(tracked_groups(&snapshot, &unknown).len(), 1);
        let self_group = tracked_groups(&snapshot, &source)
            .into_iter()
            .find(|group| group.source_node == group.target_node)
            .unwrap();
        assert!(matches!(
            connection_direction(self_group, source.id),
            ConnectionDirection::SelfConnection
        ));
    }

    #[test]
    fn links_groups_relations_members_and_node_trackers_are_typed() {
        let source = node(1, "Audio/Source");
        let sink = node(2, "Audio/Sink");
        let link = PipeWireLinkSnapshot {
            id: PipeWireLinkId {
                connection_generation: 7,
                global_id: 20,
            },
            raw_global_id: 20,
            source_node: Some(source.id),
            target_node: Some(sink.id),
            source_node_present: true,
            target_node_present: true,
            source_port_id: Some(30),
            target_port_id: Some(40),
            state: PipeWireLinkState::Negotiating,
            raw_state: 3,
            ready: true,
        };
        let group = PipeWireLinkGroupSnapshot {
            id: PipeWireLinkGroupId {
                connection_generation: 7,
                source_node: 1,
                target_node: 2,
            },
            source_node: Some(source.id),
            target_node: Some(sink.id),
            source_node_present: true,
            target_node_present: true,
            members: vec![link.id],
            representative: link.id,
            state: link.state,
        };
        let snapshot = PipeWireSnapshot {
            availability: PipeWireAvailability::Ready,
            connection_generation: 7,
            ready: true,
            node_count: 2,
            link_count: 1,
            link_group_count: 1,
            nodes: vec![source, sink],
            links: vec![link],
            link_groups: vec![group],
            ..PipeWireSnapshot::default()
        };
        let demand = PipeWireDemand {
            documents: 1,
            service: true,
            nodes: true,
            node_details: true,
            links: true,
            link_collection: true,
            link_details: true,
            link_group_collection: true,
            group_members: true,
            node_link_tracking: true,
            relation_projection: true,
            ..PipeWireDemand::default()
        };
        let projections = snapshot.public_projections(&demand);
        let links = projections
            .repeats
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireLinks)
            .unwrap();
        assert_eq!(links.items.len(), 1);
        assert_eq!(
            links.items[0].tokens[&ItemBindingKey::State],
            StateToken::Negotiating.as_str()
        );
        assert_eq!(links.items[0].text[&ItemBindingKey::SourceName], "node-1");
        let groups = projections
            .repeats
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireLinkGroups)
            .unwrap();
        assert_eq!(
            groups.items[0].values[&ItemBindingKey::MemberCount],
            NumericValue::Integer(1)
        );
        assert_eq!(groups.items[0].links.as_ref().unwrap().items.len(), 1);
        let nodes = projections
            .repeats
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireNodes)
            .unwrap();
        for item in &nodes.items {
            assert_eq!(
                item.values[&ItemBindingKey::LinkGroupCount],
                NumericValue::Integer(1)
            );
            let tracked = &item.link_groups.as_ref().unwrap().items[0];
            let expected = if item.key.ends_with(":1") {
                StateToken::Outgoing
            } else {
                StateToken::Incoming
            };
            assert_eq!(
                tracked.tokens[&ItemBindingKey::ConnectionDirection],
                expected.as_str()
            );
        }
    }
}
