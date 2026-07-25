use std::collections::BTreeMap;
use std::fmt;

pub const MAX_REPEAT_DECLARATIONS_PER_DOCUMENT: usize = 32;
pub const MAX_UPOWER_DEVICES_PER_PROCESS: usize = 128;
pub const MAX_POWER_PROFILE_HOLDS_PER_PROCESS: usize = 128;
pub const MAX_PIPEWIRE_NODES_PER_PROCESS: usize = 4_096;
pub const MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT: usize = 16;
pub const MAX_PIPEWIRE_LINKS_PER_PROCESS: usize = 16_384;
pub const MAX_PIPEWIRE_LINK_GROUPS_PER_PROCESS: usize = 4_096;
pub const MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT: usize = 16;
pub const MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT: usize = 16;
pub const MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT: usize = 32;
pub const MAX_CONTEXTUAL_LINK_REPEATS_PER_GROUP_TEMPLATE: usize = 8;
pub const MAX_CONTEXTUAL_LINK_GROUP_REPEATS_PER_NODE_TEMPLATE: usize = 8;
pub const MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM: usize = 64;
pub const MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM: usize = 64;
pub const MAX_PIPEWIRE_BINDINGS_PER_ITEM: usize = 64;
pub const MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM: usize = 32;
pub const MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT: usize = 64;
pub const MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS: usize = 256;
pub const MAX_PIPEWIRE_PROPERTY_KEY_BYTES: usize = 128;
pub const MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT: usize = 128;
pub const MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM: usize = 16;
pub const MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT: usize = 128;
pub const MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM: usize = 8;
pub const MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT: usize = 64;
pub const MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM: usize = 4;
pub const MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT: usize = 32;
pub const MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR: usize = 8;
pub const MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR: usize = 4;
pub const MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR: usize = 128;
pub const MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM: usize = 64;
pub const MAX_PIPEWIRE_PEAK_CHANNELS_PER_STREAM: usize = 64;
pub const MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS: usize = 256;
pub const MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET: usize = 256;
pub const MAX_RANGE_CONTROLS_PER_DOCUMENT: usize = 64;
pub const MAX_RANGE_CONTROLS_PER_ITEM: usize = 8;
pub const MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE: usize = 8;
pub const MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT: usize = 32;
pub const MAX_PIPEWIRE_CHANNELS_PER_NODE: usize = 64;
pub const MAX_PIPEWIRE_CHANNEL_BINDINGS_PER_ITEM: usize = 64;
pub const MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM: usize = 8;
pub const MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT: usize = 256;
pub const MAX_RANGE_NUMBER_BYTES: usize = 32;
pub const MAX_PIPEWIRE_PERCEPTUAL_VOLUME: f64 = 2.0;
pub const MAX_ITEMS_PER_REPEAT: usize = MAX_PIPEWIRE_LINKS_PER_PROCESS;
pub const MAX_CLONED_NODES_PER_REPEAT: usize = 4_096;
pub const MAX_CLONED_NODES_PER_DOCUMENT: usize = 16_384;
pub const MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE: usize = 64;
pub const MAX_REPEAT_TEMPLATE_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepeatSource {
    UPowerDevices,
    PowerProfileHolds,
    PipeWireNodes,
    PipeWireLinks,
    PipeWireLinkGroups,
}

impl RepeatSource {
    pub const ALL: [Self; 5] = [
        Self::UPowerDevices,
        Self::PowerProfileHolds,
        Self::PipeWireNodes,
        Self::PipeWireLinks,
        Self::PipeWireLinkGroups,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UPowerDevices => "upower.devices",
            Self::PowerProfileHolds => "power_profile.holds",
            Self::PipeWireNodes => "pipewire.nodes",
            Self::PipeWireLinks => "pipewire.links",
            Self::PipeWireLinkGroups => "pipewire.link_groups",
        }
    }
}

impl std::str::FromStr for RepeatSource {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "upower.devices" => Ok(Self::UPowerDevices),
            "power_profile.holds" => Ok(Self::PowerProfileHolds),
            "pipewire.nodes" => Ok(Self::PipeWireNodes),
            "pipewire.links" => Ok(Self::PipeWireLinks),
            "pipewire.link_groups" => Ok(Self::PipeWireLinkGroups),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextualRepeatSource {
    Channels,
    GroupLinks,
    NodeLinkGroups,
}

impl ContextualRepeatSource {
    pub const ALL: [Self; 3] = [Self::Channels, Self::GroupLinks, Self::NodeLinkGroups];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Channels => "item.channels",
            Self::GroupLinks => "item.links",
            Self::NodeLinkGroups => "item.link_groups",
        }
    }

    pub const fn parent(self) -> RepeatSource {
        match self {
            Self::Channels | Self::NodeLinkGroups => RepeatSource::PipeWireNodes,
            Self::GroupLinks => RepeatSource::PipeWireLinkGroups,
        }
    }

    pub const fn item_limit(self) -> usize {
        match self {
            Self::Channels => MAX_PIPEWIRE_CHANNELS_PER_NODE,
            Self::GroupLinks => MAX_PIPEWIRE_LINKS_PER_PROCESS,
            Self::NodeLinkGroups => MAX_PIPEWIRE_LINK_GROUPS_PER_PROCESS,
        }
    }
}

impl std::str::FromStr for ContextualRepeatSource {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
            .ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemBindingKey {
    Ready,
    Type,
    PowerSupply,
    Energy,
    EnergyCapacity,
    ChangeRate,
    TimeToEmpty,
    TimeToFull,
    Percentage,
    IsPresent,
    State,
    HealthPercentage,
    HealthSupported,
    IconName,
    IsLaptopBattery,
    NativePath,
    Model,
    Profile,
    ApplicationId,
    Reason,
    Name,
    Nickname,
    Description,
    MediaClass,
    NodeType,
    NodeState,
    Direction,
    RawId,
    IsAudio,
    IsVideo,
    IsStream,
    IsSink,
    IsSource,
    DefaultRole,
    ConfiguredRole,
    Property,
    AudioStatus,
    Volume,
    MuteState,
    CanSetVolume,
    CanSetMute,
    CanSetPreferredSink,
    CanSetPreferredSource,
    CanMonitorPeaks,
    ChannelCount,
    ChannelStatus,
    PositionName,
    Position,
    Index,
    Status,
    IsAuxiliary,
    IsCustom,
    Peak,
    SourcePortId,
    TargetPortId,
    IsMonitor,
    SourceStatus,
    SourceName,
    SourceNickname,
    SourceDescription,
    SourceMediaClass,
    SourceNodeType,
    SourceNodeState,
    SourceDirection,
    SourceRawId,
    TargetStatus,
    TargetName,
    TargetNickname,
    TargetDescription,
    TargetMediaClass,
    TargetNodeType,
    TargetNodeState,
    TargetDirection,
    TargetRawId,
    MemberCount,
    RepresentativeLinkRawId,
    LinkGroupCount,
    LinkGroupStatus,
    ConnectionDirection,
    PeerStatus,
    PeerName,
    PeerNickname,
    PeerDescription,
    PeerMediaClass,
    PeerNodeType,
    PeerNodeState,
    PeerDirection,
    PeerRawId,
}

impl ItemBindingKey {
    pub const ALL: [Self; 88] = [
        Self::Ready,
        Self::Type,
        Self::PowerSupply,
        Self::Energy,
        Self::EnergyCapacity,
        Self::ChangeRate,
        Self::TimeToEmpty,
        Self::TimeToFull,
        Self::Percentage,
        Self::IsPresent,
        Self::State,
        Self::HealthPercentage,
        Self::HealthSupported,
        Self::IconName,
        Self::IsLaptopBattery,
        Self::NativePath,
        Self::Model,
        Self::Profile,
        Self::ApplicationId,
        Self::Reason,
        Self::Name,
        Self::Nickname,
        Self::Description,
        Self::MediaClass,
        Self::NodeType,
        Self::NodeState,
        Self::Direction,
        Self::RawId,
        Self::IsAudio,
        Self::IsVideo,
        Self::IsStream,
        Self::IsSink,
        Self::IsSource,
        Self::DefaultRole,
        Self::ConfiguredRole,
        Self::Property,
        Self::AudioStatus,
        Self::Volume,
        Self::MuteState,
        Self::CanSetVolume,
        Self::CanSetMute,
        Self::CanSetPreferredSink,
        Self::CanSetPreferredSource,
        Self::CanMonitorPeaks,
        Self::ChannelCount,
        Self::ChannelStatus,
        Self::PositionName,
        Self::Position,
        Self::Index,
        Self::Status,
        Self::IsAuxiliary,
        Self::IsCustom,
        Self::Peak,
        Self::SourcePortId,
        Self::TargetPortId,
        Self::IsMonitor,
        Self::SourceStatus,
        Self::SourceName,
        Self::SourceNickname,
        Self::SourceDescription,
        Self::SourceMediaClass,
        Self::SourceNodeType,
        Self::SourceNodeState,
        Self::SourceDirection,
        Self::SourceRawId,
        Self::TargetStatus,
        Self::TargetName,
        Self::TargetNickname,
        Self::TargetDescription,
        Self::TargetMediaClass,
        Self::TargetNodeType,
        Self::TargetNodeState,
        Self::TargetDirection,
        Self::TargetRawId,
        Self::MemberCount,
        Self::RepresentativeLinkRawId,
        Self::LinkGroupCount,
        Self::LinkGroupStatus,
        Self::ConnectionDirection,
        Self::PeerStatus,
        Self::PeerName,
        Self::PeerNickname,
        Self::PeerDescription,
        Self::PeerMediaClass,
        Self::PeerNodeType,
        Self::PeerNodeState,
        Self::PeerDirection,
        Self::PeerRawId,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "item.ready",
            Self::Type => "item.type",
            Self::PowerSupply => "item.power_supply",
            Self::Energy => "item.energy",
            Self::EnergyCapacity => "item.energy_capacity",
            Self::ChangeRate => "item.change_rate",
            Self::TimeToEmpty => "item.time_to_empty",
            Self::TimeToFull => "item.time_to_full",
            Self::Percentage => "item.percentage",
            Self::IsPresent => "item.is_present",
            Self::State => "item.state",
            Self::HealthPercentage => "item.health_percentage",
            Self::HealthSupported => "item.health_supported",
            Self::IconName => "item.icon_name",
            Self::IsLaptopBattery => "item.is_laptop_battery",
            Self::NativePath => "item.native_path",
            Self::Model => "item.model",
            Self::Profile => "item.profile",
            Self::ApplicationId => "item.application_id",
            Self::Reason => "item.reason",
            Self::Name => "item.name",
            Self::Nickname => "item.nickname",
            Self::Description => "item.description",
            Self::MediaClass => "item.media_class",
            Self::NodeType => "item.node_type",
            Self::NodeState => "item.node_state",
            Self::Direction => "item.direction",
            Self::RawId => "item.raw_id",
            Self::IsAudio => "item.is_audio",
            Self::IsVideo => "item.is_video",
            Self::IsStream => "item.is_stream",
            Self::IsSink => "item.is_sink",
            Self::IsSource => "item.is_source",
            Self::DefaultRole => "item.default_role",
            Self::ConfiguredRole => "item.configured_role",
            Self::Property => "item.property",
            Self::AudioStatus => "item.audio_status",
            Self::Volume => "item.volume",
            Self::MuteState => "item.mute_state",
            Self::CanSetVolume => "item.can_set_volume",
            Self::CanSetMute => "item.can_set_mute",
            Self::CanSetPreferredSink => "item.can_set_preferred_sink",
            Self::CanSetPreferredSource => "item.can_set_preferred_source",
            Self::CanMonitorPeaks => "item.can_monitor_peaks",
            Self::ChannelCount => "item.channel_count",
            Self::ChannelStatus => "item.channel_status",
            Self::PositionName => "item.position_name",
            Self::Position => "item.position",
            Self::Index => "item.index",
            Self::Status => "item.status",
            Self::IsAuxiliary => "item.is_auxiliary",
            Self::IsCustom => "item.is_custom",
            Self::Peak => "item.peak",
            Self::SourcePortId => "item.source_port_id",
            Self::TargetPortId => "item.target_port_id",
            Self::IsMonitor => "item.is_monitor",
            Self::SourceStatus => "item.source.status",
            Self::SourceName => "item.source.name",
            Self::SourceNickname => "item.source.nickname",
            Self::SourceDescription => "item.source.description",
            Self::SourceMediaClass => "item.source.media_class",
            Self::SourceNodeType => "item.source.node_type",
            Self::SourceNodeState => "item.source.node_state",
            Self::SourceDirection => "item.source.direction",
            Self::SourceRawId => "item.source.raw_id",
            Self::TargetStatus => "item.target.status",
            Self::TargetName => "item.target.name",
            Self::TargetNickname => "item.target.nickname",
            Self::TargetDescription => "item.target.description",
            Self::TargetMediaClass => "item.target.media_class",
            Self::TargetNodeType => "item.target.node_type",
            Self::TargetNodeState => "item.target.node_state",
            Self::TargetDirection => "item.target.direction",
            Self::TargetRawId => "item.target.raw_id",
            Self::MemberCount => "item.member_count",
            Self::RepresentativeLinkRawId => "item.representative_link_raw_id",
            Self::LinkGroupCount => "item.link_group_count",
            Self::LinkGroupStatus => "item.link_group_status",
            Self::ConnectionDirection => "item.connection_direction",
            Self::PeerStatus => "item.peer.status",
            Self::PeerName => "item.peer.name",
            Self::PeerNickname => "item.peer.nickname",
            Self::PeerDescription => "item.peer.description",
            Self::PeerMediaClass => "item.peer.media_class",
            Self::PeerNodeType => "item.peer.node_type",
            Self::PeerNodeState => "item.peer.node_state",
            Self::PeerDirection => "item.peer.direction",
            Self::PeerRawId => "item.peer.raw_id",
        }
    }

    pub const fn supports_source(self, source: RepeatSource) -> bool {
        match source {
            RepeatSource::UPowerDevices => matches!(
                self,
                Self::Ready
                    | Self::Type
                    | Self::PowerSupply
                    | Self::Energy
                    | Self::EnergyCapacity
                    | Self::ChangeRate
                    | Self::TimeToEmpty
                    | Self::TimeToFull
                    | Self::Percentage
                    | Self::IsPresent
                    | Self::State
                    | Self::HealthPercentage
                    | Self::HealthSupported
                    | Self::IconName
                    | Self::IsLaptopBattery
                    | Self::NativePath
                    | Self::Model
            ),
            RepeatSource::PowerProfileHolds => {
                matches!(self, Self::Profile | Self::ApplicationId | Self::Reason)
            }
            RepeatSource::PipeWireNodes => matches!(
                self,
                Self::Ready
                    | Self::Name
                    | Self::Nickname
                    | Self::Description
                    | Self::MediaClass
                    | Self::NodeType
                    | Self::NodeState
                    | Self::Direction
                    | Self::RawId
                    | Self::IsAudio
                    | Self::IsVideo
                    | Self::IsStream
                    | Self::IsSink
                    | Self::IsSource
                    | Self::DefaultRole
                    | Self::ConfiguredRole
                    | Self::Property
                    | Self::AudioStatus
                    | Self::Volume
                    | Self::MuteState
                    | Self::CanSetVolume
                    | Self::CanSetMute
                    | Self::CanSetPreferredSink
                    | Self::CanSetPreferredSource
                    | Self::CanMonitorPeaks
                    | Self::ChannelCount
                    | Self::ChannelStatus
                    | Self::LinkGroupCount
                    | Self::LinkGroupStatus
            ),
            RepeatSource::PipeWireLinks => self.supports_link(),
            RepeatSource::PipeWireLinkGroups => self.supports_link_group(),
        }
    }

    pub const fn supports_channel(self) -> bool {
        matches!(
            self,
            Self::PositionName
                | Self::Position
                | Self::Index
                | Self::Volume
                | Self::Status
                | Self::CanSetVolume
                | Self::IsAuxiliary
                | Self::IsCustom
        )
    }

    pub const fn supports_peak_channel(self) -> bool {
        matches!(
            self,
            Self::PositionName
                | Self::Position
                | Self::Index
                | Self::Peak
                | Self::Status
                | Self::IsAuxiliary
                | Self::IsCustom
        )
    }

    pub const fn supports_link(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::State
                | Self::RawId
                | Self::SourcePortId
                | Self::TargetPortId
                | Self::IsMonitor
                | Self::SourceStatus
                | Self::SourceName
                | Self::SourceNickname
                | Self::SourceDescription
                | Self::SourceMediaClass
                | Self::SourceNodeType
                | Self::SourceNodeState
                | Self::SourceDirection
                | Self::SourceRawId
                | Self::TargetStatus
                | Self::TargetName
                | Self::TargetNickname
                | Self::TargetDescription
                | Self::TargetMediaClass
                | Self::TargetNodeType
                | Self::TargetNodeState
                | Self::TargetDirection
                | Self::TargetRawId
        )
    }

    pub const fn supports_link_group(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::State
                | Self::IsMonitor
                | Self::MemberCount
                | Self::RepresentativeLinkRawId
                | Self::SourceStatus
                | Self::SourceName
                | Self::SourceNickname
                | Self::SourceDescription
                | Self::SourceMediaClass
                | Self::SourceNodeType
                | Self::SourceNodeState
                | Self::SourceDirection
                | Self::SourceRawId
                | Self::TargetStatus
                | Self::TargetName
                | Self::TargetNickname
                | Self::TargetDescription
                | Self::TargetMediaClass
                | Self::TargetNodeType
                | Self::TargetNodeState
                | Self::TargetDirection
                | Self::TargetRawId
        )
    }

    pub const fn supports_node_link_group(self) -> bool {
        self.supports_link_group()
            || matches!(
                self,
                Self::ConnectionDirection
                    | Self::PeerStatus
                    | Self::PeerName
                    | Self::PeerNickname
                    | Self::PeerDescription
                    | Self::PeerMediaClass
                    | Self::PeerNodeType
                    | Self::PeerNodeState
                    | Self::PeerDirection
                    | Self::PeerRawId
            )
    }

    pub const fn supports_contextual(self, source: ContextualRepeatSource) -> bool {
        match source {
            ContextualRepeatSource::Channels => self.supports_channel(),
            ContextualRepeatSource::GroupLinks => self.supports_link(),
            ContextualRepeatSource::NodeLinkGroups => self.supports_node_link_group(),
        }
    }

    pub const fn is_relation(self) -> bool {
        matches!(
            self,
            Self::SourceStatus
                | Self::SourceName
                | Self::SourceNickname
                | Self::SourceDescription
                | Self::SourceMediaClass
                | Self::SourceNodeType
                | Self::SourceNodeState
                | Self::SourceDirection
                | Self::SourceRawId
                | Self::TargetStatus
                | Self::TargetName
                | Self::TargetNickname
                | Self::TargetDescription
                | Self::TargetMediaClass
                | Self::TargetNodeType
                | Self::TargetNodeState
                | Self::TargetDirection
                | Self::TargetRawId
                | Self::PeerStatus
                | Self::PeerName
                | Self::PeerNickname
                | Self::PeerDescription
                | Self::PeerMediaClass
                | Self::PeerNodeType
                | Self::PeerNodeState
                | Self::PeerDirection
                | Self::PeerRawId
        )
    }

    pub const fn supports_text(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::Type
                | Self::PowerSupply
                | Self::IsPresent
                | Self::State
                | Self::HealthSupported
                | Self::IconName
                | Self::IsLaptopBattery
                | Self::NativePath
                | Self::Model
                | Self::Profile
                | Self::ApplicationId
                | Self::Reason
                | Self::Name
                | Self::Nickname
                | Self::Description
                | Self::MediaClass
                | Self::NodeType
                | Self::NodeState
                | Self::Direction
                | Self::IsAudio
                | Self::IsVideo
                | Self::IsStream
                | Self::IsSink
                | Self::IsSource
                | Self::DefaultRole
                | Self::ConfiguredRole
                | Self::Property
                | Self::AudioStatus
                | Self::MuteState
                | Self::CanSetVolume
                | Self::CanSetMute
                | Self::CanSetPreferredSink
                | Self::CanSetPreferredSource
                | Self::CanMonitorPeaks
                | Self::ChannelStatus
                | Self::PositionName
                | Self::Position
                | Self::Status
                | Self::IsAuxiliary
                | Self::IsCustom
                | Self::IsMonitor
                | Self::SourceStatus
                | Self::SourceName
                | Self::SourceNickname
                | Self::SourceDescription
                | Self::SourceMediaClass
                | Self::SourceNodeType
                | Self::SourceNodeState
                | Self::SourceDirection
                | Self::TargetStatus
                | Self::TargetName
                | Self::TargetNickname
                | Self::TargetDescription
                | Self::TargetMediaClass
                | Self::TargetNodeType
                | Self::TargetNodeState
                | Self::TargetDirection
                | Self::LinkGroupStatus
                | Self::ConnectionDirection
                | Self::PeerStatus
                | Self::PeerName
                | Self::PeerNickname
                | Self::PeerDescription
                | Self::PeerMediaClass
                | Self::PeerNodeType
                | Self::PeerNodeState
                | Self::PeerDirection
        )
    }

    pub const fn supports_token(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::Type
                | Self::PowerSupply
                | Self::IsPresent
                | Self::State
                | Self::HealthSupported
                | Self::IsLaptopBattery
                | Self::Profile
                | Self::NodeType
                | Self::NodeState
                | Self::Direction
                | Self::IsAudio
                | Self::IsVideo
                | Self::IsStream
                | Self::IsSink
                | Self::IsSource
                | Self::DefaultRole
                | Self::ConfiguredRole
                | Self::Property
                | Self::AudioStatus
                | Self::MuteState
                | Self::CanSetVolume
                | Self::CanSetMute
                | Self::CanSetPreferredSink
                | Self::CanSetPreferredSource
                | Self::CanMonitorPeaks
                | Self::ChannelStatus
                | Self::Position
                | Self::Status
                | Self::IsAuxiliary
                | Self::IsCustom
                | Self::IsMonitor
                | Self::SourceStatus
                | Self::SourceNodeType
                | Self::SourceNodeState
                | Self::SourceDirection
                | Self::TargetStatus
                | Self::TargetNodeType
                | Self::TargetNodeState
                | Self::TargetDirection
                | Self::LinkGroupStatus
                | Self::ConnectionDirection
                | Self::PeerStatus
                | Self::PeerNodeType
                | Self::PeerNodeState
                | Self::PeerDirection
        )
    }

    pub const fn supports_value(self) -> bool {
        matches!(
            self,
            Self::Energy
                | Self::EnergyCapacity
                | Self::ChangeRate
                | Self::TimeToEmpty
                | Self::TimeToFull
                | Self::Percentage
                | Self::HealthPercentage
                | Self::RawId
                | Self::Volume
                | Self::Peak
                | Self::ChannelCount
                | Self::Index
                | Self::SourcePortId
                | Self::TargetPortId
                | Self::SourceRawId
                | Self::TargetRawId
                | Self::MemberCount
                | Self::RepresentativeLinkRawId
                | Self::LinkGroupCount
                | Self::PeerRawId
        )
    }
}

impl std::str::FromStr for ItemBindingKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|key| key.as_str() == value)
            .ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateValueFormat {
    Raw,
    Percent,
    Energy,
    Power,
    Duration,
}

impl StateValueFormat {
    pub const ALL: [Self; 5] = [
        Self::Raw,
        Self::Percent,
        Self::Energy,
        Self::Power,
        Self::Duration,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Percent => "percent",
            Self::Energy => "energy",
            Self::Power => "power",
            Self::Duration => "duration",
        }
    }
}

impl std::str::FromStr for StateValueFormat {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "raw" => Ok(Self::Raw),
            "percent" => Ok(Self::Percent),
            "energy" => Ok(Self::Energy),
            "power" => Ok(Self::Power),
            "duration" => Ok(Self::Duration),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumericValue {
    Unknown,
    Integer(i64),
    Decimal(f64),
}

impl NumericValue {
    pub fn finite_decimal(value: f64) -> Self {
        if value.is_finite() {
            Self::Decimal(value)
        } else {
            Self::Unknown
        }
    }

    pub fn as_f64(self) -> Option<f64> {
        match self {
            Self::Unknown => None,
            Self::Integer(value) => Some(value as f64),
            Self::Decimal(value) if value.is_finite() => Some(value),
            Self::Decimal(_) => None,
        }
    }

    pub fn format(self, format: StateValueFormat) -> Result<FormattedValue, ValueFormatError> {
        match self {
            Self::Unknown => Ok(FormattedValue {
                display: "—".into(),
                value: None,
            }),
            Self::Integer(value) => format_number(value as f64, Some(value), format),
            Self::Decimal(value) if value.is_finite() => format_number(value, None, format),
            Self::Decimal(_) => Err(ValueFormatError::NonFinite),
        }
    }

    pub fn format_volume(
        self,
        format: StateValueFormat,
    ) -> Result<FormattedValue, ValueFormatError> {
        if format != StateValueFormat::Percent {
            return self.format(format);
        }
        let value = match self {
            Self::Unknown => {
                return Ok(FormattedValue {
                    display: "—".into(),
                    value: None,
                });
            }
            Self::Integer(value) => value as f64,
            Self::Decimal(value) => value,
        };
        if !value.is_finite() || value < 0.0 {
            return Err(ValueFormatError::OutOfRange);
        }
        Ok(FormattedValue {
            display: format!("{:.0}%", value * 100.0),
            value: Some(canonical_decimal(value)),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedValue {
    pub display: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueFormatError {
    NonFinite,
    OutOfRange,
}

impl fmt::Display for ValueFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => formatter.write_str("numeric value is not finite"),
            Self::OutOfRange => formatter.write_str("numeric value is outside the format domain"),
        }
    }
}

fn format_number(
    value: f64,
    integer: Option<i64>,
    format: StateValueFormat,
) -> Result<FormattedValue, ValueFormatError> {
    if !value.is_finite() {
        return Err(ValueFormatError::NonFinite);
    }
    let raw = integer
        .map(|value| value.to_string())
        .unwrap_or_else(|| canonical_decimal(value));
    let display = match format {
        StateValueFormat::Raw => raw.clone(),
        StateValueFormat::Percent => {
            if !(0.0..=100.0).contains(&value) {
                return Err(ValueFormatError::OutOfRange);
            }
            format!("{}%", value.round() as u8)
        }
        StateValueFormat::Energy => {
            if value < 0.0 {
                return Err(ValueFormatError::OutOfRange);
            }
            format!("{value:.1} Wh")
        }
        StateValueFormat::Power => format!("{value:.1} W"),
        StateValueFormat::Duration => {
            if value < 0.0 || value > u64::MAX as f64 {
                return Err(ValueFormatError::OutOfRange);
            }
            format_duration(value.round() as u64)
        }
    };
    Ok(FormattedValue {
        display,
        value: Some(raw),
    })
}

fn canonical_decimal(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3_600 {
        return format!("{}m", seconds / 60);
    }
    if seconds < 86_400 {
        return format!("{}h {:02}m", seconds / 3_600, (seconds % 3_600) / 60);
    }
    format!("{}d {:02}h", seconds / 86_400, (seconds % 86_400) / 3_600)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepeatItemSnapshot {
    pub key: String,
    pub text: BTreeMap<ItemBindingKey, String>,
    pub tokens: BTreeMap<ItemBindingKey, String>,
    pub values: BTreeMap<ItemBindingKey, NumericValue>,
    pub properties: BTreeMap<String, String>,
    pub channels: Option<ContextualRepeatSnapshot>,
    pub links: Option<ContextualRepeatSnapshot>,
    pub link_groups: Option<ContextualRepeatSnapshot>,
}

impl RepeatItemSnapshot {
    pub fn contextual(&self, source: ContextualRepeatSource) -> Option<&ContextualRepeatSnapshot> {
        match source {
            ContextualRepeatSource::Channels => self.channels.as_ref(),
            ContextualRepeatSource::GroupLinks => self.links.as_ref(),
            ContextualRepeatSource::NodeLinkGroups => self.link_groups.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextualRepeatSnapshot {
    pub source_generation: u64,
    pub items: Vec<RepeatItemSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepeatSourceSnapshot {
    pub source: RepeatSource,
    pub source_generation: u64,
    pub items: Vec<RepeatItemSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PipeWireDocumentDemand {
    pub service: bool,
    pub nodes: bool,
    pub node_details: bool,
    pub defaults: bool,
    pub audio_state: bool,
    pub audio_writes: bool,
    pub configured_default_writes: bool,
    pub preferred_sink_writes: bool,
    pub preferred_source_writes: bool,
    pub channel_projection: bool,
    pub channel_writes: bool,
    pub link_collection: bool,
    pub link_details: bool,
    pub link_group_collection: bool,
    pub group_members: bool,
    pub node_link_tracking: bool,
    pub relation_projection: bool,
    pub peak_monitor_declarations: bool,
    pub peak_maximum_projection: bool,
    pub peak_channel_projection: bool,
    pub property_keys: std::collections::BTreeSet<String>,
}

impl PipeWireDocumentDemand {
    pub fn is_empty(&self) -> bool {
        !self.service
            && !self.nodes
            && !self.node_details
            && !self.defaults
            && !self.audio_state
            && !self.audio_writes
            && !self.configured_default_writes
            && !self.preferred_sink_writes
            && !self.preferred_source_writes
            && !self.channel_projection
            && !self.channel_writes
            && !self.link_collection
            && !self.link_details
            && !self.link_group_collection
            && !self.group_members
            && !self.node_link_tracking
            && !self.relation_projection
            && !self.peak_monitor_declarations
            && !self.peak_maximum_projection
            && !self.peak_channel_projection
            && self.property_keys.is_empty()
    }

    pub fn merge(&mut self, other: &Self) {
        self.service |= other.service;
        self.nodes |= other.nodes;
        self.node_details |= other.node_details;
        self.defaults |= other.defaults;
        self.audio_state |= other.audio_state;
        self.audio_writes |= other.audio_writes;
        self.configured_default_writes |= other.configured_default_writes;
        self.preferred_sink_writes |= other.preferred_sink_writes;
        self.preferred_source_writes |= other.preferred_source_writes;
        self.channel_projection |= other.channel_projection;
        self.channel_writes |= other.channel_writes;
        self.link_collection |= other.link_collection;
        self.link_details |= other.link_details;
        self.link_group_collection |= other.link_group_collection;
        self.group_members |= other.group_members;
        self.node_link_tracking |= other.node_link_tracking;
        self.relation_projection |= other.relation_projection;
        self.peak_monitor_declarations |= other.peak_monitor_declarations;
        self.peak_maximum_projection |= other.peak_maximum_projection;
        self.peak_channel_projection |= other.peak_channel_projection;
        self.property_keys
            .extend(other.property_keys.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_formats_are_finite_and_deterministic() {
        assert_eq!(
            NumericValue::Decimal(42.5)
                .format(StateValueFormat::Percent)
                .unwrap()
                .display,
            "43%"
        );
        assert_eq!(
            NumericValue::Decimal(12.25)
                .format(StateValueFormat::Raw)
                .unwrap()
                .display,
            "12.25"
        );
        assert_eq!(
            NumericValue::Integer(3_900)
                .format(StateValueFormat::Duration)
                .unwrap()
                .display,
            "1h 05m"
        );
        assert_eq!(
            NumericValue::Integer(183_600)
                .format(StateValueFormat::Duration)
                .unwrap()
                .display,
            "2d 03h"
        );
        assert!(
            NumericValue::Decimal(f64::NAN)
                .format(StateValueFormat::Raw)
                .is_err()
        );
    }
}
