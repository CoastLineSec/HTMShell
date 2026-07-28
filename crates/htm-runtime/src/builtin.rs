use crate::identity::{IdentityRegistry, author_slots};
use crate::{
    ClockFormat, ClockTimeZone, ContextualRepeatSource, ExperimentalDocumentIdentity,
    ExperimentalNodeIdentity, ItemBindingKey, MAX_CLOCK_DECLARATIONS_PER_DOCUMENT,
    MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT, MAX_CONTEXTUAL_LINK_GROUP_REPEATS_PER_NODE_TEMPLATE,
    MAX_CONTEXTUAL_LINK_REPEATS_PER_GROUP_TEMPLATE, MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT,
    MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE, MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT,
    MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM, MAX_PIPEWIRE_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_CHANNEL_BINDINGS_PER_ITEM, MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT,
    MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM, MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT,
    MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM, MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT,
    MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM, MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT,
    MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR,
    MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR, MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR, MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT,
    MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM, MAX_PIPEWIRE_PERCEPTUAL_VOLUME,
    MAX_PIPEWIRE_PROPERTY_KEY_BYTES, MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT,
    MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM, MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_RANGE_CONTROLS_PER_DOCUMENT,
    MAX_RANGE_CONTROLS_PER_ITEM, MAX_RANGE_NUMBER_BYTES, MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE,
    MAX_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_REPEAT_TEMPLATE_DEPTH, RepeatSource, RuntimeError,
    StateValueFormat,
};
use blitz_dom::node::{ElementData, NodeData};
use blitz_dom::{LocalName, local_name};
use blitz_html::HtmlDocument;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::OnceLock;

const ELEMENT_ATTRIBUTE: &str = "data-htm-element";
const BIND_ATTRIBUTE: &str = "data-htm-bind";
const ACTION_ATTRIBUTE: &str = "data-htm-action";
const TARGET_ATTRIBUTE: &str = "data-htm-target";
const FORMAT_ATTRIBUTE: &str = "data-htm-format";
const SOURCE_ATTRIBUTE: &str = "data-htm-source";
const LOCAL_ID_ATTRIBUTE: &str = "data-htm-local-id";
const ENABLED_BIND_ATTRIBUTE: &str = "data-htm-enabled-bind";
pub const PROPERTY_KEY_ATTRIBUTE: &str = "data-htm-property-key";
const TIME_ZONE_ATTRIBUTE: &str = "data-htm-time-zone";
const ENABLED_ATTRIBUTE: &str = "data-htm-enabled";
pub(crate) const DATETIME_ATTRIBUTE: &str = "datetime";
pub(crate) const STATE_ATTRIBUTE: &str = "data-htm-state";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuiltInElementKind {
    StateText,
    ActionButton,
    StateToken,
    ClockText,
    StateValue,
    Repeat,
    RangeControl,
    PeakMonitor,
}

impl BuiltInElementKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StateText => "state-text",
            Self::ActionButton => "action-button",
            Self::StateToken => "state-token",
            Self::ClockText => "clock-text",
            Self::StateValue => "state-value",
            Self::Repeat => "repeat",
            Self::RangeControl => "range-control",
            Self::PeakMonitor => "peak-monitor",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "state-text" => Some(Self::StateText),
            "action-button" => Some(Self::ActionButton),
            "state-token" => Some(Self::StateToken),
            "clock-text" => Some(Self::ClockText),
            "state-value" => Some(Self::StateValue),
            "repeat" => Some(Self::Repeat),
            "range-control" => Some(Self::RangeControl),
            "peak-monitor" => Some(Self::PeakMonitor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateBindingKey {
    ClockTime,
    UPowerAvailability,
    UPowerOnBattery,
    UPowerDeviceCount,
    BatteryPercentage,
    BatteryStatus,
    BatteryWarning,
    BatteryReady,
    BatteryType,
    BatteryEnergy,
    BatteryEnergyCapacity,
    BatteryChangeRate,
    BatteryTimeToEmpty,
    BatteryTimeToFull,
    BatteryIsPresent,
    BatteryHealthPercentage,
    BatteryHealthSupported,
    BatteryIconName,
    BatteryIsLaptopBattery,
    BatteryPowerSupply,
    BatteryNativePath,
    BatteryModel,
    PowerProfileAvailability,
    PowerProfileCurrent,
    PowerProfilePerformanceAvailable,
    PowerProfileDegradation,
    PowerProfileHoldCount,
    PipeWireAvailability,
    PipeWireReady,
    PipeWireNodeCount,
    PipeWireLinkCount,
    PipeWireLinkGroupCount,
    PipeWireDefaultSinkStatus,
    PipeWireDefaultSinkName,
    PipeWireDefaultSinkNickname,
    PipeWireDefaultSinkDescription,
    PipeWireDefaultSinkMediaClass,
    PipeWireDefaultSinkRawId,
    PipeWireDefaultSourceStatus,
    PipeWireDefaultSourceName,
    PipeWireDefaultSourceNickname,
    PipeWireDefaultSourceDescription,
    PipeWireDefaultSourceMediaClass,
    PipeWireDefaultSourceRawId,
    PipeWireConfiguredSinkStatus,
    PipeWireConfiguredSinkName,
    PipeWireConfiguredSinkNickname,
    PipeWireConfiguredSinkDescription,
    PipeWireConfiguredSinkMediaClass,
    PipeWireConfiguredSinkRawId,
    PipeWireConfiguredSourceStatus,
    PipeWireConfiguredSourceName,
    PipeWireConfiguredSourceNickname,
    PipeWireConfiguredSourceDescription,
    PipeWireConfiguredSourceMediaClass,
    PipeWireConfiguredSourceRawId,
    PipeWireDefaultSinkAudioStatus,
    PipeWireDefaultSinkVolume,
    PipeWireDefaultSinkMuteState,
    PipeWireDefaultSinkCanSetVolume,
    PipeWireDefaultSinkCanSetMute,
    PipeWireDefaultSinkCanMonitorPeaks,
    PipeWireDefaultSourceAudioStatus,
    PipeWireDefaultSourceVolume,
    PipeWireDefaultSourceMuteState,
    PipeWireDefaultSourceCanSetVolume,
    PipeWireDefaultSourceCanSetMute,
    PipeWireDefaultSourceCanMonitorPeaks,
    PipeWireConfiguredSinkAudioStatus,
    PipeWireConfiguredSinkVolume,
    PipeWireConfiguredSinkMuteState,
    PipeWireConfiguredSinkCanSetVolume,
    PipeWireConfiguredSinkCanSetMute,
    PipeWireConfiguredSinkCanClear,
    PipeWireConfiguredSourceAudioStatus,
    PipeWireConfiguredSourceVolume,
    PipeWireConfiguredSourceMuteState,
    PipeWireConfiguredSourceCanSetVolume,
    PipeWireConfiguredSourceCanSetMute,
    PipeWireConfiguredSourceCanClear,
    OutputLabel,
    OutputScale,
    SurfaceTemplateId,
    SurfaceScaleProfile,
    OverlayStatus,
    OverlayActivationCount,
    ShellLastAction,
}

impl StateBindingKey {
    pub const ALL: [Self; 87] = [
        Self::ClockTime,
        Self::UPowerAvailability,
        Self::UPowerOnBattery,
        Self::UPowerDeviceCount,
        Self::BatteryPercentage,
        Self::BatteryStatus,
        Self::BatteryWarning,
        Self::BatteryReady,
        Self::BatteryType,
        Self::BatteryEnergy,
        Self::BatteryEnergyCapacity,
        Self::BatteryChangeRate,
        Self::BatteryTimeToEmpty,
        Self::BatteryTimeToFull,
        Self::BatteryIsPresent,
        Self::BatteryHealthPercentage,
        Self::BatteryHealthSupported,
        Self::BatteryIconName,
        Self::BatteryIsLaptopBattery,
        Self::BatteryPowerSupply,
        Self::BatteryNativePath,
        Self::BatteryModel,
        Self::PowerProfileAvailability,
        Self::PowerProfileCurrent,
        Self::PowerProfilePerformanceAvailable,
        Self::PowerProfileDegradation,
        Self::PowerProfileHoldCount,
        Self::PipeWireAvailability,
        Self::PipeWireReady,
        Self::PipeWireNodeCount,
        Self::PipeWireLinkCount,
        Self::PipeWireLinkGroupCount,
        Self::PipeWireDefaultSinkStatus,
        Self::PipeWireDefaultSinkName,
        Self::PipeWireDefaultSinkNickname,
        Self::PipeWireDefaultSinkDescription,
        Self::PipeWireDefaultSinkMediaClass,
        Self::PipeWireDefaultSinkRawId,
        Self::PipeWireDefaultSourceStatus,
        Self::PipeWireDefaultSourceName,
        Self::PipeWireDefaultSourceNickname,
        Self::PipeWireDefaultSourceDescription,
        Self::PipeWireDefaultSourceMediaClass,
        Self::PipeWireDefaultSourceRawId,
        Self::PipeWireConfiguredSinkStatus,
        Self::PipeWireConfiguredSinkName,
        Self::PipeWireConfiguredSinkNickname,
        Self::PipeWireConfiguredSinkDescription,
        Self::PipeWireConfiguredSinkMediaClass,
        Self::PipeWireConfiguredSinkRawId,
        Self::PipeWireConfiguredSourceStatus,
        Self::PipeWireConfiguredSourceName,
        Self::PipeWireConfiguredSourceNickname,
        Self::PipeWireConfiguredSourceDescription,
        Self::PipeWireConfiguredSourceMediaClass,
        Self::PipeWireConfiguredSourceRawId,
        Self::PipeWireDefaultSinkAudioStatus,
        Self::PipeWireDefaultSinkVolume,
        Self::PipeWireDefaultSinkMuteState,
        Self::PipeWireDefaultSinkCanSetVolume,
        Self::PipeWireDefaultSinkCanSetMute,
        Self::PipeWireDefaultSinkCanMonitorPeaks,
        Self::PipeWireDefaultSourceAudioStatus,
        Self::PipeWireDefaultSourceVolume,
        Self::PipeWireDefaultSourceMuteState,
        Self::PipeWireDefaultSourceCanSetVolume,
        Self::PipeWireDefaultSourceCanSetMute,
        Self::PipeWireDefaultSourceCanMonitorPeaks,
        Self::PipeWireConfiguredSinkAudioStatus,
        Self::PipeWireConfiguredSinkVolume,
        Self::PipeWireConfiguredSinkMuteState,
        Self::PipeWireConfiguredSinkCanSetVolume,
        Self::PipeWireConfiguredSinkCanSetMute,
        Self::PipeWireConfiguredSinkCanClear,
        Self::PipeWireConfiguredSourceAudioStatus,
        Self::PipeWireConfiguredSourceVolume,
        Self::PipeWireConfiguredSourceMuteState,
        Self::PipeWireConfiguredSourceCanSetVolume,
        Self::PipeWireConfiguredSourceCanSetMute,
        Self::PipeWireConfiguredSourceCanClear,
        Self::OutputLabel,
        Self::OutputScale,
        Self::SurfaceTemplateId,
        Self::SurfaceScaleProfile,
        Self::OverlayStatus,
        Self::OverlayActivationCount,
        Self::ShellLastAction,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClockTime => "clock.time",
            Self::UPowerAvailability => "upower.availability",
            Self::UPowerOnBattery => "upower.on_battery",
            Self::UPowerDeviceCount => "upower.device_count",
            Self::BatteryPercentage => "battery.percentage",
            Self::BatteryStatus => "battery.status",
            Self::BatteryWarning => "battery.warning",
            Self::BatteryReady => "battery.ready",
            Self::BatteryType => "battery.type",
            Self::BatteryEnergy => "battery.energy",
            Self::BatteryEnergyCapacity => "battery.energy_capacity",
            Self::BatteryChangeRate => "battery.change_rate",
            Self::BatteryTimeToEmpty => "battery.time_to_empty",
            Self::BatteryTimeToFull => "battery.time_to_full",
            Self::BatteryIsPresent => "battery.is_present",
            Self::BatteryHealthPercentage => "battery.health_percentage",
            Self::BatteryHealthSupported => "battery.health_supported",
            Self::BatteryIconName => "battery.icon_name",
            Self::BatteryIsLaptopBattery => "battery.is_laptop_battery",
            Self::BatteryPowerSupply => "battery.power_supply",
            Self::BatteryNativePath => "battery.native_path",
            Self::BatteryModel => "battery.model",
            Self::PowerProfileAvailability => "power_profile.availability",
            Self::PowerProfileCurrent => "power_profile.current",
            Self::PowerProfilePerformanceAvailable => "power_profile.performance_available",
            Self::PowerProfileDegradation => "power_profile.degradation",
            Self::PowerProfileHoldCount => "power_profile.hold_count",
            Self::PipeWireAvailability => "pipewire.availability",
            Self::PipeWireReady => "pipewire.ready",
            Self::PipeWireNodeCount => "pipewire.node_count",
            Self::PipeWireLinkCount => "pipewire.link_count",
            Self::PipeWireLinkGroupCount => "pipewire.link_group_count",
            Self::PipeWireDefaultSinkStatus => "pipewire.default_sink.status",
            Self::PipeWireDefaultSinkName => "pipewire.default_sink.name",
            Self::PipeWireDefaultSinkNickname => "pipewire.default_sink.nickname",
            Self::PipeWireDefaultSinkDescription => "pipewire.default_sink.description",
            Self::PipeWireDefaultSinkMediaClass => "pipewire.default_sink.media_class",
            Self::PipeWireDefaultSinkRawId => "pipewire.default_sink.raw_id",
            Self::PipeWireDefaultSourceStatus => "pipewire.default_source.status",
            Self::PipeWireDefaultSourceName => "pipewire.default_source.name",
            Self::PipeWireDefaultSourceNickname => "pipewire.default_source.nickname",
            Self::PipeWireDefaultSourceDescription => "pipewire.default_source.description",
            Self::PipeWireDefaultSourceMediaClass => "pipewire.default_source.media_class",
            Self::PipeWireDefaultSourceRawId => "pipewire.default_source.raw_id",
            Self::PipeWireConfiguredSinkStatus => "pipewire.configured_sink.status",
            Self::PipeWireConfiguredSinkName => "pipewire.configured_sink.name",
            Self::PipeWireConfiguredSinkNickname => "pipewire.configured_sink.nickname",
            Self::PipeWireConfiguredSinkDescription => "pipewire.configured_sink.description",
            Self::PipeWireConfiguredSinkMediaClass => "pipewire.configured_sink.media_class",
            Self::PipeWireConfiguredSinkRawId => "pipewire.configured_sink.raw_id",
            Self::PipeWireConfiguredSourceStatus => "pipewire.configured_source.status",
            Self::PipeWireConfiguredSourceName => "pipewire.configured_source.name",
            Self::PipeWireConfiguredSourceNickname => "pipewire.configured_source.nickname",
            Self::PipeWireConfiguredSourceDescription => "pipewire.configured_source.description",
            Self::PipeWireConfiguredSourceMediaClass => "pipewire.configured_source.media_class",
            Self::PipeWireConfiguredSourceRawId => "pipewire.configured_source.raw_id",
            Self::PipeWireDefaultSinkAudioStatus => "pipewire.default_sink.audio_status",
            Self::PipeWireDefaultSinkVolume => "pipewire.default_sink.volume",
            Self::PipeWireDefaultSinkMuteState => "pipewire.default_sink.mute_state",
            Self::PipeWireDefaultSinkCanSetVolume => "pipewire.default_sink.can_set_volume",
            Self::PipeWireDefaultSinkCanSetMute => "pipewire.default_sink.can_set_mute",
            Self::PipeWireDefaultSinkCanMonitorPeaks => "pipewire.default_sink.can_monitor_peaks",
            Self::PipeWireDefaultSourceAudioStatus => "pipewire.default_source.audio_status",
            Self::PipeWireDefaultSourceVolume => "pipewire.default_source.volume",
            Self::PipeWireDefaultSourceMuteState => "pipewire.default_source.mute_state",
            Self::PipeWireDefaultSourceCanSetVolume => "pipewire.default_source.can_set_volume",
            Self::PipeWireDefaultSourceCanSetMute => "pipewire.default_source.can_set_mute",
            Self::PipeWireDefaultSourceCanMonitorPeaks => {
                "pipewire.default_source.can_monitor_peaks"
            }
            Self::PipeWireConfiguredSinkAudioStatus => "pipewire.configured_sink.audio_status",
            Self::PipeWireConfiguredSinkVolume => "pipewire.configured_sink.volume",
            Self::PipeWireConfiguredSinkMuteState => "pipewire.configured_sink.mute_state",
            Self::PipeWireConfiguredSinkCanSetVolume => "pipewire.configured_sink.can_set_volume",
            Self::PipeWireConfiguredSinkCanSetMute => "pipewire.configured_sink.can_set_mute",
            Self::PipeWireConfiguredSinkCanClear => "pipewire.configured_sink.can_clear",
            Self::PipeWireConfiguredSourceAudioStatus => "pipewire.configured_source.audio_status",
            Self::PipeWireConfiguredSourceVolume => "pipewire.configured_source.volume",
            Self::PipeWireConfiguredSourceMuteState => "pipewire.configured_source.mute_state",
            Self::PipeWireConfiguredSourceCanSetVolume => {
                "pipewire.configured_source.can_set_volume"
            }
            Self::PipeWireConfiguredSourceCanSetMute => "pipewire.configured_source.can_set_mute",
            Self::PipeWireConfiguredSourceCanClear => "pipewire.configured_source.can_clear",
            Self::OutputLabel => "output.label",
            Self::OutputScale => "output.scale",
            Self::SurfaceTemplateId => "surface.template_id",
            Self::SurfaceScaleProfile => "surface.scale_profile",
            Self::OverlayStatus => "overlay.status",
            Self::OverlayActivationCount => "overlay.activation_count",
            Self::ShellLastAction => "shell.last_action",
        }
    }

    pub const fn scope(self) -> StateBindingScope {
        match self {
            Self::ClockTime
            | Self::UPowerAvailability
            | Self::UPowerOnBattery
            | Self::UPowerDeviceCount
            | Self::BatteryPercentage
            | Self::BatteryStatus
            | Self::BatteryWarning
            | Self::BatteryReady
            | Self::BatteryType
            | Self::BatteryEnergy
            | Self::BatteryEnergyCapacity
            | Self::BatteryChangeRate
            | Self::BatteryTimeToEmpty
            | Self::BatteryTimeToFull
            | Self::BatteryIsPresent
            | Self::BatteryHealthPercentage
            | Self::BatteryHealthSupported
            | Self::BatteryIconName
            | Self::BatteryIsLaptopBattery
            | Self::BatteryPowerSupply
            | Self::BatteryNativePath
            | Self::BatteryModel
            | Self::PowerProfileAvailability
            | Self::PowerProfileCurrent
            | Self::PowerProfilePerformanceAvailable
            | Self::PowerProfileDegradation
            | Self::PowerProfileHoldCount
            | Self::PipeWireAvailability
            | Self::PipeWireReady
            | Self::PipeWireNodeCount
            | Self::PipeWireLinkCount
            | Self::PipeWireLinkGroupCount
            | Self::PipeWireDefaultSinkStatus
            | Self::PipeWireDefaultSinkName
            | Self::PipeWireDefaultSinkNickname
            | Self::PipeWireDefaultSinkDescription
            | Self::PipeWireDefaultSinkMediaClass
            | Self::PipeWireDefaultSinkRawId
            | Self::PipeWireDefaultSourceStatus
            | Self::PipeWireDefaultSourceName
            | Self::PipeWireDefaultSourceNickname
            | Self::PipeWireDefaultSourceDescription
            | Self::PipeWireDefaultSourceMediaClass
            | Self::PipeWireDefaultSourceRawId
            | Self::PipeWireConfiguredSinkStatus
            | Self::PipeWireConfiguredSinkName
            | Self::PipeWireConfiguredSinkNickname
            | Self::PipeWireConfiguredSinkDescription
            | Self::PipeWireConfiguredSinkMediaClass
            | Self::PipeWireConfiguredSinkRawId
            | Self::PipeWireConfiguredSourceStatus
            | Self::PipeWireConfiguredSourceName
            | Self::PipeWireConfiguredSourceNickname
            | Self::PipeWireConfiguredSourceDescription
            | Self::PipeWireConfiguredSourceMediaClass
            | Self::PipeWireConfiguredSourceRawId
            | Self::PipeWireDefaultSinkAudioStatus
            | Self::PipeWireDefaultSinkVolume
            | Self::PipeWireDefaultSinkMuteState
            | Self::PipeWireDefaultSinkCanSetVolume
            | Self::PipeWireDefaultSinkCanSetMute
            | Self::PipeWireDefaultSinkCanMonitorPeaks
            | Self::PipeWireDefaultSourceAudioStatus
            | Self::PipeWireDefaultSourceVolume
            | Self::PipeWireDefaultSourceMuteState
            | Self::PipeWireDefaultSourceCanSetVolume
            | Self::PipeWireDefaultSourceCanSetMute
            | Self::PipeWireDefaultSourceCanMonitorPeaks
            | Self::PipeWireConfiguredSinkAudioStatus
            | Self::PipeWireConfiguredSinkVolume
            | Self::PipeWireConfiguredSinkMuteState
            | Self::PipeWireConfiguredSinkCanSetVolume
            | Self::PipeWireConfiguredSinkCanSetMute
            | Self::PipeWireConfiguredSinkCanClear
            | Self::PipeWireConfiguredSourceAudioStatus
            | Self::PipeWireConfiguredSourceVolume
            | Self::PipeWireConfiguredSourceMuteState
            | Self::PipeWireConfiguredSourceCanSetVolume
            | Self::PipeWireConfiguredSourceCanSetMute
            | Self::PipeWireConfiguredSourceCanClear => StateBindingScope::Process,
            Self::OutputLabel
            | Self::OutputScale
            | Self::OverlayStatus
            | Self::OverlayActivationCount
            | Self::ShellLastAction => StateBindingScope::Output,
            Self::SurfaceTemplateId | Self::SurfaceScaleProfile => StateBindingScope::Surface,
        }
    }

    pub const fn supports(self, kind: StateValueKind) -> bool {
        match kind {
            StateValueKind::Text => matches!(
                self,
                Self::ClockTime
                    | Self::UPowerAvailability
                    | Self::UPowerOnBattery
                    | Self::BatteryPercentage
                    | Self::BatteryStatus
                    | Self::BatteryReady
                    | Self::BatteryType
                    | Self::BatteryIsPresent
                    | Self::BatteryHealthSupported
                    | Self::BatteryIconName
                    | Self::BatteryIsLaptopBattery
                    | Self::BatteryPowerSupply
                    | Self::BatteryNativePath
                    | Self::BatteryModel
                    | Self::PowerProfileAvailability
                    | Self::PowerProfileCurrent
                    | Self::PowerProfilePerformanceAvailable
                    | Self::PowerProfileDegradation
                    | Self::PipeWireAvailability
                    | Self::PipeWireReady
                    | Self::PipeWireDefaultSinkStatus
                    | Self::PipeWireDefaultSinkName
                    | Self::PipeWireDefaultSinkNickname
                    | Self::PipeWireDefaultSinkDescription
                    | Self::PipeWireDefaultSinkMediaClass
                    | Self::PipeWireDefaultSourceStatus
                    | Self::PipeWireDefaultSourceName
                    | Self::PipeWireDefaultSourceNickname
                    | Self::PipeWireDefaultSourceDescription
                    | Self::PipeWireDefaultSourceMediaClass
                    | Self::PipeWireConfiguredSinkStatus
                    | Self::PipeWireConfiguredSinkName
                    | Self::PipeWireConfiguredSinkNickname
                    | Self::PipeWireConfiguredSinkDescription
                    | Self::PipeWireConfiguredSinkMediaClass
                    | Self::PipeWireConfiguredSourceStatus
                    | Self::PipeWireConfiguredSourceName
                    | Self::PipeWireConfiguredSourceNickname
                    | Self::PipeWireConfiguredSourceDescription
                    | Self::PipeWireConfiguredSourceMediaClass
                    | Self::PipeWireDefaultSinkAudioStatus
                    | Self::PipeWireDefaultSinkMuteState
                    | Self::PipeWireDefaultSinkCanSetVolume
                    | Self::PipeWireDefaultSinkCanSetMute
                    | Self::PipeWireDefaultSinkCanMonitorPeaks
                    | Self::PipeWireDefaultSourceAudioStatus
                    | Self::PipeWireDefaultSourceMuteState
                    | Self::PipeWireDefaultSourceCanSetVolume
                    | Self::PipeWireDefaultSourceCanSetMute
                    | Self::PipeWireDefaultSourceCanMonitorPeaks
                    | Self::PipeWireConfiguredSinkAudioStatus
                    | Self::PipeWireConfiguredSinkMuteState
                    | Self::PipeWireConfiguredSinkCanSetVolume
                    | Self::PipeWireConfiguredSinkCanSetMute
                    | Self::PipeWireConfiguredSinkCanClear
                    | Self::PipeWireConfiguredSourceAudioStatus
                    | Self::PipeWireConfiguredSourceMuteState
                    | Self::PipeWireConfiguredSourceCanSetVolume
                    | Self::PipeWireConfiguredSourceCanSetMute
                    | Self::PipeWireConfiguredSourceCanClear
                    | Self::OutputLabel
                    | Self::OutputScale
                    | Self::SurfaceTemplateId
                    | Self::OverlayStatus
                    | Self::OverlayActivationCount
                    | Self::ShellLastAction
            ),
            StateValueKind::Token => matches!(
                self,
                Self::UPowerAvailability
                    | Self::UPowerOnBattery
                    | Self::BatteryStatus
                    | Self::BatteryWarning
                    | Self::BatteryReady
                    | Self::BatteryType
                    | Self::BatteryIsPresent
                    | Self::BatteryHealthSupported
                    | Self::BatteryIsLaptopBattery
                    | Self::BatteryPowerSupply
                    | Self::PowerProfileAvailability
                    | Self::PowerProfileCurrent
                    | Self::PowerProfilePerformanceAvailable
                    | Self::PowerProfileDegradation
                    | Self::PipeWireAvailability
                    | Self::PipeWireReady
                    | Self::PipeWireDefaultSinkStatus
                    | Self::PipeWireDefaultSourceStatus
                    | Self::PipeWireConfiguredSinkStatus
                    | Self::PipeWireConfiguredSourceStatus
                    | Self::PipeWireDefaultSinkAudioStatus
                    | Self::PipeWireDefaultSinkMuteState
                    | Self::PipeWireDefaultSinkCanSetVolume
                    | Self::PipeWireDefaultSinkCanSetMute
                    | Self::PipeWireDefaultSinkCanMonitorPeaks
                    | Self::PipeWireDefaultSourceAudioStatus
                    | Self::PipeWireDefaultSourceMuteState
                    | Self::PipeWireDefaultSourceCanSetVolume
                    | Self::PipeWireDefaultSourceCanSetMute
                    | Self::PipeWireDefaultSourceCanMonitorPeaks
                    | Self::PipeWireConfiguredSinkAudioStatus
                    | Self::PipeWireConfiguredSinkMuteState
                    | Self::PipeWireConfiguredSinkCanSetVolume
                    | Self::PipeWireConfiguredSinkCanSetMute
                    | Self::PipeWireConfiguredSinkCanClear
                    | Self::PipeWireConfiguredSourceAudioStatus
                    | Self::PipeWireConfiguredSourceMuteState
                    | Self::PipeWireConfiguredSourceCanSetVolume
                    | Self::PipeWireConfiguredSourceCanSetMute
                    | Self::PipeWireConfiguredSourceCanClear
                    | Self::SurfaceScaleProfile
                    | Self::OverlayStatus
            ),
            StateValueKind::Value => matches!(
                self,
                Self::UPowerDeviceCount
                    | Self::BatteryPercentage
                    | Self::BatteryEnergy
                    | Self::BatteryEnergyCapacity
                    | Self::BatteryChangeRate
                    | Self::BatteryTimeToEmpty
                    | Self::BatteryTimeToFull
                    | Self::BatteryHealthPercentage
                    | Self::PowerProfileHoldCount
                    | Self::PipeWireNodeCount
                    | Self::PipeWireLinkCount
                    | Self::PipeWireLinkGroupCount
                    | Self::PipeWireDefaultSinkRawId
                    | Self::PipeWireDefaultSourceRawId
                    | Self::PipeWireConfiguredSinkRawId
                    | Self::PipeWireConfiguredSourceRawId
                    | Self::PipeWireDefaultSinkVolume
                    | Self::PipeWireDefaultSourceVolume
                    | Self::PipeWireConfiguredSinkVolume
                    | Self::PipeWireConfiguredSourceVolume
            ),
            StateValueKind::Boolean => matches!(
                self,
                Self::PowerProfileAvailability
                    | Self::PowerProfilePerformanceAvailable
                    | Self::PipeWireReady
                    | Self::PipeWireDefaultSinkCanSetVolume
                    | Self::PipeWireDefaultSinkCanSetMute
                    | Self::PipeWireDefaultSinkCanMonitorPeaks
                    | Self::PipeWireDefaultSourceCanSetVolume
                    | Self::PipeWireDefaultSourceCanSetMute
                    | Self::PipeWireDefaultSourceCanMonitorPeaks
                    | Self::PipeWireConfiguredSinkCanSetVolume
                    | Self::PipeWireConfiguredSinkCanSetMute
                    | Self::PipeWireConfiguredSinkCanClear
                    | Self::PipeWireConfiguredSourceCanSetVolume
                    | Self::PipeWireConfiguredSourceCanSetMute
                    | Self::PipeWireConfiguredSourceCanClear
            ),
        }
    }

    pub const fn allowed_value_formats(self) -> &'static [StateValueFormat] {
        match self {
            Self::UPowerDeviceCount
            | Self::PowerProfileHoldCount
            | Self::PipeWireNodeCount
            | Self::PipeWireLinkCount
            | Self::PipeWireLinkGroupCount
            | Self::PipeWireDefaultSinkRawId
            | Self::PipeWireDefaultSourceRawId
            | Self::PipeWireConfiguredSinkRawId
            | Self::PipeWireConfiguredSourceRawId => &[StateValueFormat::Raw],
            Self::BatteryPercentage | Self::BatteryHealthPercentage => {
                &[StateValueFormat::Raw, StateValueFormat::Percent]
            }
            Self::BatteryEnergy | Self::BatteryEnergyCapacity => {
                &[StateValueFormat::Raw, StateValueFormat::Energy]
            }
            Self::BatteryChangeRate => &[StateValueFormat::Raw, StateValueFormat::Power],
            Self::BatteryTimeToEmpty | Self::BatteryTimeToFull => {
                &[StateValueFormat::Raw, StateValueFormat::Duration]
            }
            Self::PipeWireDefaultSinkVolume
            | Self::PipeWireDefaultSourceVolume
            | Self::PipeWireConfiguredSinkVolume
            | Self::PipeWireConfiguredSourceVolume => {
                &[StateValueFormat::Raw, StateValueFormat::Percent]
            }
            _ => &[],
        }
    }

    pub const fn token_values(self) -> &'static [&'static str] {
        match self {
            Self::OverlayStatus => &["open", "closed"],
            Self::SurfaceScaleProfile => &["scale-1", "fractional"],
            Self::UPowerAvailability => &["available", "unavailable"],
            Self::UPowerOnBattery => &["battery", "external", "unavailable"],
            Self::BatteryStatus => &[
                "unavailable",
                "absent",
                "unknown",
                "charging",
                "discharging",
                "empty",
                "full",
                "pending-charge",
                "pending-discharge",
            ],
            Self::BatteryWarning => &[
                "unknown",
                "none",
                "discharging",
                "low",
                "critical",
                "action",
            ],
            Self::BatteryReady
            | Self::BatteryIsPresent
            | Self::BatteryHealthSupported
            | Self::BatteryIsLaptopBattery
            | Self::BatteryPowerSupply => &["true", "false", "unknown"],
            Self::BatteryType => DEVICE_TYPE_TOKENS,
            Self::PowerProfileAvailability => &["available", "unavailable"],
            Self::PowerProfileCurrent => &[
                "power-saver",
                "balanced",
                "performance",
                "unknown",
                "unavailable",
            ],
            Self::PowerProfilePerformanceAvailable => &["true", "false", "unavailable"],
            Self::PowerProfileDegradation => &[
                "none",
                "high-temperature",
                "lap-detected",
                "unknown",
                "unavailable",
            ],
            Self::PipeWireAvailability => &["unavailable", "synchronizing", "ready"],
            Self::PipeWireReady => &["true", "false"],
            Self::PipeWireDefaultSinkStatus
            | Self::PipeWireDefaultSourceStatus
            | Self::PipeWireConfiguredSinkStatus
            | Self::PipeWireConfiguredSourceStatus => &["unavailable", "unresolved", "available"],
            Self::PipeWireDefaultSinkAudioStatus
            | Self::PipeWireDefaultSourceAudioStatus
            | Self::PipeWireConfiguredSinkAudioStatus
            | Self::PipeWireConfiguredSourceAudioStatus => &["unsupported", "unavailable", "ready"],
            Self::PipeWireDefaultSinkMuteState
            | Self::PipeWireDefaultSourceMuteState
            | Self::PipeWireConfiguredSinkMuteState
            | Self::PipeWireConfiguredSourceMuteState => &["unavailable", "muted", "unmuted"],
            Self::PipeWireDefaultSinkCanSetVolume
            | Self::PipeWireDefaultSinkCanSetMute
            | Self::PipeWireDefaultSinkCanMonitorPeaks
            | Self::PipeWireDefaultSourceCanSetVolume
            | Self::PipeWireDefaultSourceCanSetMute
            | Self::PipeWireDefaultSourceCanMonitorPeaks
            | Self::PipeWireConfiguredSinkCanSetVolume
            | Self::PipeWireConfiguredSinkCanSetMute
            | Self::PipeWireConfiguredSinkCanClear
            | Self::PipeWireConfiguredSourceCanSetVolume
            | Self::PipeWireConfiguredSourceCanSetMute
            | Self::PipeWireConfiguredSourceCanClear => &["true", "false"],
            _ => &[],
        }
    }
}

pub const DEVICE_TYPE_TOKENS: &[&str] = &[
    "unknown",
    "line-power",
    "battery",
    "ups",
    "monitor",
    "mouse",
    "keyboard",
    "pda",
    "phone",
    "media-player",
    "tablet",
    "computer",
    "gaming-input",
    "pen",
    "touchpad",
    "modem",
    "network",
    "headset",
    "speakers",
    "headphones",
    "video",
    "other-audio",
    "remote-control",
    "printer",
    "scanner",
    "camera",
    "wearable",
    "toy",
    "bluetooth-generic",
];

pub const DEVICE_STATE_TOKENS: &[&str] = &[
    "unknown",
    "charging",
    "discharging",
    "empty",
    "fully-charged",
    "pending-charge",
    "pending-discharge",
];

impl std::str::FromStr for StateBindingKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "clock.time" => Ok(Self::ClockTime),
            "upower.availability" => Ok(Self::UPowerAvailability),
            "upower.on_battery" => Ok(Self::UPowerOnBattery),
            "upower.device_count" => Ok(Self::UPowerDeviceCount),
            "battery.percentage" => Ok(Self::BatteryPercentage),
            "battery.status" => Ok(Self::BatteryStatus),
            "battery.warning" => Ok(Self::BatteryWarning),
            "battery.ready" => Ok(Self::BatteryReady),
            "battery.type" => Ok(Self::BatteryType),
            "battery.energy" => Ok(Self::BatteryEnergy),
            "battery.energy_capacity" => Ok(Self::BatteryEnergyCapacity),
            "battery.change_rate" => Ok(Self::BatteryChangeRate),
            "battery.time_to_empty" => Ok(Self::BatteryTimeToEmpty),
            "battery.time_to_full" => Ok(Self::BatteryTimeToFull),
            "battery.is_present" => Ok(Self::BatteryIsPresent),
            "battery.health_percentage" => Ok(Self::BatteryHealthPercentage),
            "battery.health_supported" => Ok(Self::BatteryHealthSupported),
            "battery.icon_name" => Ok(Self::BatteryIconName),
            "battery.is_laptop_battery" => Ok(Self::BatteryIsLaptopBattery),
            "battery.power_supply" => Ok(Self::BatteryPowerSupply),
            "battery.native_path" => Ok(Self::BatteryNativePath),
            "battery.model" => Ok(Self::BatteryModel),
            "power_profile.availability" => Ok(Self::PowerProfileAvailability),
            "power_profile.current" => Ok(Self::PowerProfileCurrent),
            "power_profile.performance_available" => Ok(Self::PowerProfilePerformanceAvailable),
            "power_profile.degradation" => Ok(Self::PowerProfileDegradation),
            "power_profile.hold_count" => Ok(Self::PowerProfileHoldCount),
            "pipewire.availability" => Ok(Self::PipeWireAvailability),
            "pipewire.ready" => Ok(Self::PipeWireReady),
            "pipewire.node_count" => Ok(Self::PipeWireNodeCount),
            "pipewire.link_count" => Ok(Self::PipeWireLinkCount),
            "pipewire.link_group_count" => Ok(Self::PipeWireLinkGroupCount),
            "pipewire.default_sink.status" => Ok(Self::PipeWireDefaultSinkStatus),
            "pipewire.default_sink.name" => Ok(Self::PipeWireDefaultSinkName),
            "pipewire.default_sink.nickname" => Ok(Self::PipeWireDefaultSinkNickname),
            "pipewire.default_sink.description" => Ok(Self::PipeWireDefaultSinkDescription),
            "pipewire.default_sink.media_class" => Ok(Self::PipeWireDefaultSinkMediaClass),
            "pipewire.default_sink.raw_id" => Ok(Self::PipeWireDefaultSinkRawId),
            "pipewire.default_source.status" => Ok(Self::PipeWireDefaultSourceStatus),
            "pipewire.default_source.name" => Ok(Self::PipeWireDefaultSourceName),
            "pipewire.default_source.nickname" => Ok(Self::PipeWireDefaultSourceNickname),
            "pipewire.default_source.description" => Ok(Self::PipeWireDefaultSourceDescription),
            "pipewire.default_source.media_class" => Ok(Self::PipeWireDefaultSourceMediaClass),
            "pipewire.default_source.raw_id" => Ok(Self::PipeWireDefaultSourceRawId),
            "pipewire.configured_sink.status" => Ok(Self::PipeWireConfiguredSinkStatus),
            "pipewire.configured_sink.name" => Ok(Self::PipeWireConfiguredSinkName),
            "pipewire.configured_sink.nickname" => Ok(Self::PipeWireConfiguredSinkNickname),
            "pipewire.configured_sink.description" => Ok(Self::PipeWireConfiguredSinkDescription),
            "pipewire.configured_sink.media_class" => Ok(Self::PipeWireConfiguredSinkMediaClass),
            "pipewire.configured_sink.raw_id" => Ok(Self::PipeWireConfiguredSinkRawId),
            "pipewire.configured_source.status" => Ok(Self::PipeWireConfiguredSourceStatus),
            "pipewire.configured_source.name" => Ok(Self::PipeWireConfiguredSourceName),
            "pipewire.configured_source.nickname" => Ok(Self::PipeWireConfiguredSourceNickname),
            "pipewire.configured_source.description" => {
                Ok(Self::PipeWireConfiguredSourceDescription)
            }
            "pipewire.configured_source.media_class" => {
                Ok(Self::PipeWireConfiguredSourceMediaClass)
            }
            "pipewire.configured_source.raw_id" => Ok(Self::PipeWireConfiguredSourceRawId),
            "pipewire.default_sink.audio_status" => Ok(Self::PipeWireDefaultSinkAudioStatus),
            "pipewire.default_sink.volume" => Ok(Self::PipeWireDefaultSinkVolume),
            "pipewire.default_sink.mute_state" => Ok(Self::PipeWireDefaultSinkMuteState),
            "pipewire.default_sink.can_set_volume" => Ok(Self::PipeWireDefaultSinkCanSetVolume),
            "pipewire.default_sink.can_set_mute" => Ok(Self::PipeWireDefaultSinkCanSetMute),
            "pipewire.default_sink.can_monitor_peaks" => {
                Ok(Self::PipeWireDefaultSinkCanMonitorPeaks)
            }
            "pipewire.default_source.audio_status" => Ok(Self::PipeWireDefaultSourceAudioStatus),
            "pipewire.default_source.volume" => Ok(Self::PipeWireDefaultSourceVolume),
            "pipewire.default_source.mute_state" => Ok(Self::PipeWireDefaultSourceMuteState),
            "pipewire.default_source.can_set_volume" => Ok(Self::PipeWireDefaultSourceCanSetVolume),
            "pipewire.default_source.can_set_mute" => Ok(Self::PipeWireDefaultSourceCanSetMute),
            "pipewire.default_source.can_monitor_peaks" => {
                Ok(Self::PipeWireDefaultSourceCanMonitorPeaks)
            }
            "pipewire.configured_sink.audio_status" => Ok(Self::PipeWireConfiguredSinkAudioStatus),
            "pipewire.configured_sink.volume" => Ok(Self::PipeWireConfiguredSinkVolume),
            "pipewire.configured_sink.mute_state" => Ok(Self::PipeWireConfiguredSinkMuteState),
            "pipewire.configured_sink.can_set_volume" => {
                Ok(Self::PipeWireConfiguredSinkCanSetVolume)
            }
            "pipewire.configured_sink.can_set_mute" => Ok(Self::PipeWireConfiguredSinkCanSetMute),
            "pipewire.configured_sink.can_clear" => Ok(Self::PipeWireConfiguredSinkCanClear),
            "pipewire.configured_source.audio_status" => {
                Ok(Self::PipeWireConfiguredSourceAudioStatus)
            }
            "pipewire.configured_source.volume" => Ok(Self::PipeWireConfiguredSourceVolume),
            "pipewire.configured_source.mute_state" => Ok(Self::PipeWireConfiguredSourceMuteState),
            "pipewire.configured_source.can_set_volume" => {
                Ok(Self::PipeWireConfiguredSourceCanSetVolume)
            }
            "pipewire.configured_source.can_set_mute" => {
                Ok(Self::PipeWireConfiguredSourceCanSetMute)
            }
            "pipewire.configured_source.can_clear" => Ok(Self::PipeWireConfiguredSourceCanClear),
            "output.label" => Ok(Self::OutputLabel),
            "output.scale" => Ok(Self::OutputScale),
            "surface.template_id" => Ok(Self::SurfaceTemplateId),
            "surface.scale_profile" => Ok(Self::SurfaceScaleProfile),
            "overlay.status" => Ok(Self::OverlayStatus),
            "overlay.activation_count" => Ok(Self::OverlayActivationCount),
            "shell.last_action" => Ok(Self::ShellLastAction),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateBindingScope {
    Process,
    Output,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateValueKind {
    Text,
    Token,
    Value,
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateToken {
    Open,
    Closed,
    Scale1,
    Fractional,
    Unavailable,
    Absent,
    Unknown,
    Charging,
    Discharging,
    Empty,
    Full,
    PendingCharge,
    PendingDischarge,
    None,
    Low,
    Critical,
    Action,
    Enabled,
    Disabled,
    Available,
    External,
    True,
    False,
    LinePower,
    Battery,
    Ups,
    Monitor,
    Mouse,
    Keyboard,
    Pda,
    Phone,
    MediaPlayer,
    Tablet,
    Computer,
    GamingInput,
    Pen,
    Touchpad,
    Modem,
    Network,
    Headset,
    Speakers,
    Headphones,
    Video,
    OtherAudio,
    RemoteControl,
    Printer,
    Scanner,
    Camera,
    Wearable,
    Toy,
    BluetoothGeneric,
    FullyCharged,
    PowerSaver,
    Balanced,
    Performance,
    HighTemperature,
    LapDetected,
    Synchronizing,
    Ready,
    Unresolved,
    Untracked,
    Audio,
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
    Error,
    Creating,
    Suspended,
    Idle,
    Running,
    Bidirectional,
    DefaultSink,
    DefaultSource,
    DefaultSinkAndSource,
    ConfiguredSink,
    ConfiguredSource,
    ConfiguredSinkAndSource,
    Unsupported,
    Muted,
    Unmuted,
    Pending,
    Failed,
    Partial,
    Unlinked,
    Init,
    Negotiating,
    Allocating,
    Paused,
    Active,
    Incoming,
    Outgoing,
    SelfConnection,
    Starting,
}

impl StateToken {
    pub const ALL: [Self; 100] = [
        Self::Open,
        Self::Closed,
        Self::Scale1,
        Self::Fractional,
        Self::Unavailable,
        Self::Absent,
        Self::Unknown,
        Self::Charging,
        Self::Discharging,
        Self::Empty,
        Self::Full,
        Self::PendingCharge,
        Self::PendingDischarge,
        Self::None,
        Self::Low,
        Self::Critical,
        Self::Action,
        Self::Enabled,
        Self::Disabled,
        Self::Available,
        Self::External,
        Self::True,
        Self::False,
        Self::LinePower,
        Self::Battery,
        Self::Ups,
        Self::Monitor,
        Self::Mouse,
        Self::Keyboard,
        Self::Pda,
        Self::Phone,
        Self::MediaPlayer,
        Self::Tablet,
        Self::Computer,
        Self::GamingInput,
        Self::Pen,
        Self::Touchpad,
        Self::Modem,
        Self::Network,
        Self::Headset,
        Self::Speakers,
        Self::Headphones,
        Self::Video,
        Self::OtherAudio,
        Self::RemoteControl,
        Self::Printer,
        Self::Scanner,
        Self::Camera,
        Self::Wearable,
        Self::Toy,
        Self::BluetoothGeneric,
        Self::FullyCharged,
        Self::PowerSaver,
        Self::Balanced,
        Self::Performance,
        Self::HighTemperature,
        Self::LapDetected,
        Self::Synchronizing,
        Self::Ready,
        Self::Unresolved,
        Self::Untracked,
        Self::Audio,
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
        Self::Error,
        Self::Creating,
        Self::Suspended,
        Self::Idle,
        Self::Running,
        Self::Bidirectional,
        Self::DefaultSink,
        Self::DefaultSource,
        Self::DefaultSinkAndSource,
        Self::ConfiguredSink,
        Self::ConfiguredSource,
        Self::ConfiguredSinkAndSource,
        Self::Unsupported,
        Self::Muted,
        Self::Unmuted,
        Self::Pending,
        Self::Failed,
        Self::Partial,
        Self::Unlinked,
        Self::Init,
        Self::Negotiating,
        Self::Allocating,
        Self::Paused,
        Self::Active,
        Self::Incoming,
        Self::Outgoing,
        Self::SelfConnection,
        Self::Starting,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Scale1 => "scale-1",
            Self::Fractional => "fractional",
            Self::Unavailable => "unavailable",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Empty => "empty",
            Self::Full => "full",
            Self::PendingCharge => "pending-charge",
            Self::PendingDischarge => "pending-discharge",
            Self::None => "none",
            Self::Low => "low",
            Self::Critical => "critical",
            Self::Action => "action",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Available => "available",
            Self::External => "external",
            Self::True => "true",
            Self::False => "false",
            Self::LinePower => "line-power",
            Self::Battery => "battery",
            Self::Ups => "ups",
            Self::Monitor => "monitor",
            Self::Mouse => "mouse",
            Self::Keyboard => "keyboard",
            Self::Pda => "pda",
            Self::Phone => "phone",
            Self::MediaPlayer => "media-player",
            Self::Tablet => "tablet",
            Self::Computer => "computer",
            Self::GamingInput => "gaming-input",
            Self::Pen => "pen",
            Self::Touchpad => "touchpad",
            Self::Modem => "modem",
            Self::Network => "network",
            Self::Headset => "headset",
            Self::Speakers => "speakers",
            Self::Headphones => "headphones",
            Self::Video => "video",
            Self::OtherAudio => "other-audio",
            Self::RemoteControl => "remote-control",
            Self::Printer => "printer",
            Self::Scanner => "scanner",
            Self::Camera => "camera",
            Self::Wearable => "wearable",
            Self::Toy => "toy",
            Self::BluetoothGeneric => "bluetooth-generic",
            Self::FullyCharged => "fully-charged",
            Self::PowerSaver => "power-saver",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::HighTemperature => "high-temperature",
            Self::LapDetected => "lap-detected",
            Self::Synchronizing => "synchronizing",
            Self::Ready => "ready",
            Self::Unresolved => "unresolved",
            Self::Untracked => "untracked",
            Self::Audio => "audio",
            Self::Stream => "stream",
            Self::Source => "source",
            Self::Sink => "sink",
            Self::AudioSink => "audio-sink",
            Self::AudioSource => "audio-source",
            Self::AudioDuplex => "audio-duplex",
            Self::AudioOutputStream => "audio-output-stream",
            Self::AudioInputStream => "audio-input-stream",
            Self::VideoSource => "video-source",
            Self::VideoSink => "video-sink",
            Self::Error => "error",
            Self::Creating => "creating",
            Self::Suspended => "suspended",
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Bidirectional => "bidirectional",
            Self::DefaultSink => "default-sink",
            Self::DefaultSource => "default-source",
            Self::DefaultSinkAndSource => "default-sink-and-source",
            Self::ConfiguredSink => "configured-sink",
            Self::ConfiguredSource => "configured-source",
            Self::ConfiguredSinkAndSource => "configured-sink-and-source",
            Self::Unsupported => "unsupported",
            Self::Muted => "muted",
            Self::Unmuted => "unmuted",
            Self::Pending => "pending",
            Self::Failed => "failed",
            Self::Partial => "partial",
            Self::Unlinked => "unlinked",
            Self::Init => "init",
            Self::Negotiating => "negotiating",
            Self::Allocating => "allocating",
            Self::Paused => "paused",
            Self::Active => "active",
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
            Self::SelfConnection => "self",
            Self::Starting => "starting",
        }
    }

    pub fn valid_for(self, key: StateBindingKey) -> bool {
        key.token_values().contains(&self.as_str())
    }
}

impl std::str::FromStr for StateToken {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|token| token.as_str() == value)
            .ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellAction {
    OverlayToggle,
    OverlayClose,
    OverlayActivate,
    ClockEnable,
    ClockDisable,
    ClockToggle,
    PowerProfileSetPowerSaver,
    PowerProfileSetBalanced,
    PowerProfileSetPerformance,
    PipeWireAudioMute,
    PipeWireAudioUnmute,
    PipeWireAudioToggleMute,
    PipeWireAudioSetVolume,
    PipeWireAudioSetChannelVolume,
    PipeWireDefaultsSetPreferredSink,
    PipeWireDefaultsSetPreferredSource,
    PipeWireDefaultsClearPreferredSink,
    PipeWireDefaultsClearPreferredSource,
    PipeWirePeaksEnable,
    PipeWirePeaksDisable,
    PipeWirePeaksToggle,
}

impl ShellAction {
    pub const ALL: [Self; 21] = [
        Self::OverlayToggle,
        Self::OverlayClose,
        Self::OverlayActivate,
        Self::ClockEnable,
        Self::ClockDisable,
        Self::ClockToggle,
        Self::PowerProfileSetPowerSaver,
        Self::PowerProfileSetBalanced,
        Self::PowerProfileSetPerformance,
        Self::PipeWireAudioMute,
        Self::PipeWireAudioUnmute,
        Self::PipeWireAudioToggleMute,
        Self::PipeWireAudioSetVolume,
        Self::PipeWireAudioSetChannelVolume,
        Self::PipeWireDefaultsSetPreferredSink,
        Self::PipeWireDefaultsSetPreferredSource,
        Self::PipeWireDefaultsClearPreferredSink,
        Self::PipeWireDefaultsClearPreferredSource,
        Self::PipeWirePeaksEnable,
        Self::PipeWirePeaksDisable,
        Self::PipeWirePeaksToggle,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OverlayToggle => "overlay.toggle",
            Self::OverlayClose => "overlay.close",
            Self::OverlayActivate => "overlay.activate",
            Self::ClockEnable => "clock.enable",
            Self::ClockDisable => "clock.disable",
            Self::ClockToggle => "clock.toggle",
            Self::PowerProfileSetPowerSaver => "power_profile.set_power_saver",
            Self::PowerProfileSetBalanced => "power_profile.set_balanced",
            Self::PowerProfileSetPerformance => "power_profile.set_performance",
            Self::PipeWireAudioMute => "pipewire.audio.mute",
            Self::PipeWireAudioUnmute => "pipewire.audio.unmute",
            Self::PipeWireAudioToggleMute => "pipewire.audio.toggle_mute",
            Self::PipeWireAudioSetVolume => "pipewire.audio.set_volume",
            Self::PipeWireAudioSetChannelVolume => "pipewire.audio.set_channel_volume",
            Self::PipeWireDefaultsSetPreferredSink => "pipewire.defaults.set_preferred_sink",
            Self::PipeWireDefaultsSetPreferredSource => "pipewire.defaults.set_preferred_source",
            Self::PipeWireDefaultsClearPreferredSink => "pipewire.defaults.clear_preferred_sink",
            Self::PipeWireDefaultsClearPreferredSource => {
                "pipewire.defaults.clear_preferred_source"
            }
            Self::PipeWirePeaksEnable => "pipewire.peaks.enable",
            Self::PipeWirePeaksDisable => "pipewire.peaks.disable",
            Self::PipeWirePeaksToggle => "pipewire.peaks.toggle",
        }
    }
}

impl std::str::FromStr for ShellAction {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "overlay.toggle" => Ok(Self::OverlayToggle),
            "overlay.close" => Ok(Self::OverlayClose),
            "overlay.activate" => Ok(Self::OverlayActivate),
            "clock.enable" => Ok(Self::ClockEnable),
            "clock.disable" => Ok(Self::ClockDisable),
            "clock.toggle" => Ok(Self::ClockToggle),
            "power_profile.set_power_saver" => Ok(Self::PowerProfileSetPowerSaver),
            "power_profile.set_balanced" => Ok(Self::PowerProfileSetBalanced),
            "power_profile.set_performance" => Ok(Self::PowerProfileSetPerformance),
            "pipewire.audio.mute" => Ok(Self::PipeWireAudioMute),
            "pipewire.audio.unmute" => Ok(Self::PipeWireAudioUnmute),
            "pipewire.audio.toggle_mute" => Ok(Self::PipeWireAudioToggleMute),
            "pipewire.audio.set_volume" => Ok(Self::PipeWireAudioSetVolume),
            "pipewire.audio.set_channel_volume" => Ok(Self::PipeWireAudioSetChannelVolume),
            "pipewire.defaults.set_preferred_sink" => Ok(Self::PipeWireDefaultsSetPreferredSink),
            "pipewire.defaults.set_preferred_source" => {
                Ok(Self::PipeWireDefaultsSetPreferredSource)
            }
            "pipewire.defaults.clear_preferred_sink" => {
                Ok(Self::PipeWireDefaultsClearPreferredSink)
            }
            "pipewire.defaults.clear_preferred_source" => {
                Ok(Self::PipeWireDefaultsClearPreferredSource)
            }
            "pipewire.peaks.enable" => Ok(Self::PipeWirePeaksEnable),
            "pipewire.peaks.disable" => Ok(Self::PipeWirePeaksDisable),
            "pipewire.peaks.toggle" => Ok(Self::PipeWirePeaksToggle),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltInSurfaceKind {
    SingleOverlay,
    Panel,
    Overlay,
}

impl BuiltInSurfaceKind {
    pub(crate) fn permits(self, action: ShellAction) -> bool {
        matches!(
            (self, action),
            (Self::Panel, ShellAction::OverlayToggle)
                | (
                    Self::Panel | Self::Overlay,
                    ShellAction::ClockEnable | ShellAction::ClockDisable | ShellAction::ClockToggle
                )
                | (
                    Self::Overlay,
                    ShellAction::OverlayClose | ShellAction::OverlayActivate
                )
                | (
                    Self::Panel | Self::Overlay,
                    ShellAction::PowerProfileSetPowerSaver
                        | ShellAction::PowerProfileSetBalanced
                        | ShellAction::PowerProfileSetPerformance
                )
                | (
                    Self::Panel | Self::Overlay,
                    ShellAction::PipeWireAudioMute
                        | ShellAction::PipeWireAudioUnmute
                        | ShellAction::PipeWireAudioToggleMute
                        | ShellAction::PipeWireAudioSetVolume
                        | ShellAction::PipeWireAudioSetChannelVolume
                        | ShellAction::PipeWireDefaultsSetPreferredSink
                        | ShellAction::PipeWireDefaultsSetPreferredSource
                        | ShellAction::PipeWireDefaultsClearPreferredSink
                        | ShellAction::PipeWireDefaultsClearPreferredSource
                        | ShellAction::PipeWirePeaksEnable
                        | ShellAction::PipeWirePeaksDisable
                        | ShellAction::PipeWirePeaksToggle
                )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementInstanceId {
    pub document_generation: ExperimentalDocumentIdentity,
    pub html_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDeclaration {
    pub id: ElementInstanceId,
    pub kind: BuiltInElementKind,
    pub binding: Option<StateBindingKey>,
    pub binding_kind: Option<StateValueKind>,
    pub action: Option<ShellAction>,
    pub action_target: Option<ElementInstanceId>,
    pub pipewire_target: Option<PipeWireControlTarget>,
    pub clock: Option<ClockDeclaration>,
    pub disabled: bool,
    pub enabled_binding: Option<StateBindingKey>,
    pub value_format: Option<StateValueFormat>,
    pub repeat: Option<RepeatDeclaration>,
    pub range: Option<RangeControlDeclaration>,
    pub peak_monitor: Option<PeakMonitorDeclaration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PeakBindingKey {
    Status,
    Enabled,
    Active,
    CanEnable,
    CanDisable,
    Maximum,
    ChannelCount,
}

impl PeakBindingKey {
    pub const ALL: [Self; 7] = [
        Self::Status,
        Self::Enabled,
        Self::Active,
        Self::CanEnable,
        Self::CanDisable,
        Self::Maximum,
        Self::ChannelCount,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "peak.status",
            Self::Enabled => "peak.enabled",
            Self::Active => "peak.active",
            Self::CanEnable => "peak.can_enable",
            Self::CanDisable => "peak.can_disable",
            Self::Maximum => "peak.maximum",
            Self::ChannelCount => "peak.channel_count",
        }
    }

    pub const fn supports_text(self) -> bool {
        !matches!(self, Self::Maximum | Self::ChannelCount)
    }

    pub const fn supports_token(self) -> bool {
        !matches!(self, Self::Maximum | Self::ChannelCount)
    }

    pub const fn supports_value(self) -> bool {
        matches!(self, Self::Maximum | Self::ChannelCount)
    }
}

impl std::str::FromStr for PeakBindingKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|key| key.as_str() == value)
            .ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeakMonitorTarget {
    CurrentItem,
    DefaultSink,
    DefaultSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeakScopedElementDeclaration {
    pub local_id: String,
    pub kind: BuiltInElementKind,
    pub binding: Option<PeakBindingKey>,
    pub action: Option<ShellAction>,
    pub value_format: Option<StateValueFormat>,
    pub disabled: bool,
    pub prototype_order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeakChannelRepeatDeclaration {
    pub id: String,
    pub template_prototype_order: usize,
    pub descendants: Vec<RepeatedElementDeclaration>,
    pub prototype_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeakMonitorDeclaration {
    pub id: String,
    pub target: PeakMonitorTarget,
    pub enabled: bool,
    pub root_prototype_order: usize,
    pub descendants: Vec<PeakScopedElementDeclaration>,
    pub channel_repeats: Vec<PeakChannelRepeatDeclaration>,
    pub prototype_nodes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWireControlTarget {
    CurrentItem,
    DefaultSink,
    DefaultSource,
}

impl PipeWireControlTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CurrentItem => "item",
            Self::DefaultSink => "pipewire.default_sink",
            Self::DefaultSource => "pipewire.default_source",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeNumber(u64);

impl RangeNumber {
    fn new(value: f64) -> Self {
        Self(value.to_bits())
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeControlDeclaration {
    pub target: PipeWireControlTarget,
    pub minimum: RangeNumber,
    pub maximum: RangeNumber,
    pub step: RangeNumber,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockDeclaration {
    pub id: ElementInstanceId,
    pub format: ClockFormat,
    pub time_zone: ClockTimeZone,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatedElementDeclaration {
    pub local_id: String,
    pub kind: BuiltInElementKind,
    pub binding: Option<ItemBindingKey>,
    pub action: Option<ShellAction>,
    pub enabled_binding: Option<ItemBindingKey>,
    pub range: Option<RangeControlDeclaration>,
    pub disabled: bool,
    pub property_key: Option<String>,
    pub value_format: Option<StateValueFormat>,
    pub prototype_order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextualRepeatDeclaration {
    pub id: String,
    pub source: ContextualRepeatSource,
    pub template_prototype_order: usize,
    pub descendants: Vec<RepeatedElementDeclaration>,
    pub prototype_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatDeclaration {
    pub id: ElementInstanceId,
    pub source: RepeatSource,
    pub template_node: ExperimentalNodeIdentity,
    pub root_node: ExperimentalNodeIdentity,
    pub descendants: Vec<RepeatedElementDeclaration>,
    pub contextual_repeats: Vec<ContextualRepeatDeclaration>,
    pub peak_monitors: Vec<PeakMonitorDeclaration>,
    pub prototype_nodes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuiltInElementSummary {
    pub registered_elements: usize,
    pub bindings: usize,
    pub text_bindings: usize,
    pub token_bindings: usize,
    pub actions: usize,
    pub clock_declarations: usize,
    pub value_bindings: usize,
    pub boolean_bindings: usize,
    pub repeat_declarations: usize,
    pub discovery_scans: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BindingUpdate {
    pub changed_keys: usize,
    pub changed_elements: usize,
    pub changed_text_elements: usize,
    pub changed_token_elements: usize,
    pub changed_value_elements: usize,
    pub changed_boolean_elements: usize,
    pub suppressed_keys: usize,
}

#[derive(Debug, Clone, Copy)]
struct BuiltInElementDefinition {
    name: &'static str,
    allowed_tags: &'static [&'static str],
    required_attribute: &'static str,
}

const DEFINITIONS: [BuiltInElementDefinition; 8] = [
    BuiltInElementDefinition {
        name: "state-text",
        allowed_tags: &["span", "p", "output"],
        required_attribute: BIND_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "action-button",
        allowed_tags: &["button"],
        required_attribute: ACTION_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "state-token",
        allowed_tags: &["div", "span", "section"],
        required_attribute: BIND_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "clock-text",
        allowed_tags: &["time"],
        required_attribute: FORMAT_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "state-value",
        allowed_tags: &["data"],
        required_attribute: BIND_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "repeat",
        allowed_tags: &["template"],
        required_attribute: SOURCE_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "range-control",
        allowed_tags: &["input"],
        required_attribute: BIND_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "peak-monitor",
        allowed_tags: &["div", "section", "article", "aside"],
        required_attribute: ENABLED_ATTRIBUTE,
    },
];

static REGISTRY_VALIDATION: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone)]
struct IndexedElement {
    declaration: ElementDeclaration,
    node: ExperimentalNodeIdentity,
    depth: usize,
    order: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionTarget {
    pub(crate) id: ElementInstanceId,
    pub(crate) action: ShellAction,
    pub(crate) node: ExperimentalNodeIdentity,
    pub(crate) target: Option<ElementInstanceId>,
    pub(crate) pipewire_target: Option<PipeWireControlTarget>,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltInElementIndex {
    elements: BTreeMap<String, IndexedElement>,
    text_bindings: BTreeMap<StateBindingKey, Vec<String>>,
    token_bindings: BTreeMap<StateBindingKey, Vec<String>>,
    value_bindings: BTreeMap<StateBindingKey, Vec<String>>,
    boolean_bindings: BTreeMap<StateBindingKey, Vec<String>>,
    actions: Vec<String>,
    applied_values: BTreeMap<(StateBindingKey, StateValueKind), String>,
    surface_kind: BuiltInSurfaceKind,
    summary: BuiltInElementSummary,
}

impl BuiltInElementIndex {
    pub(crate) fn discover(
        document: &HtmlDocument,
        identities: &IdentityRegistry,
        document_generation: ExperimentalDocumentIdentity,
        surface_kind: BuiltInSurfaceKind,
        source: &str,
        local_input_consumers: &BTreeSet<usize>,
    ) -> Result<Self, RuntimeError> {
        ensure_registry_valid()?;
        let mut elements = BTreeMap::new();
        let mut text_bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut token_bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut value_bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut boolean_bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut actions = Vec::new();
        let mut unresolved_action_targets = BTreeMap::new();
        let slots = author_slots(document);
        let mut id_counts: BTreeMap<String, usize> = BTreeMap::new();
        for slot in &slots {
            if let Some(id) = document
                .get_node(*slot)
                .and_then(|node| node.element_data())
                .and_then(|element| element.attr(local_name!("id")))
                .filter(|id| !id.is_empty())
            {
                *id_counts.entry(id.to_owned()).or_default() += 1;
            }
        }

        for (order, slot) in slots.into_iter().enumerate() {
            if local_input_consumers.contains(&slot) {
                continue;
            }
            let Some(node) = document.get_node(slot) else {
                continue;
            };
            let Some(element) = node.element_data() else {
                continue;
            };
            if repeat_ancestor(document, slot).is_some() {
                continue;
            }
            if peak_monitor_ancestor(document, slot).is_some() {
                continue;
            }
            if element.has_attr(LocalName::from(LOCAL_ID_ATTRIBUTE)) {
                return Err(invalid_declaration(
                    &declaration_context(source, element.attr(local_name!("id"))),
                    "`data-htm-local-id` is only valid inside a repeat template",
                ));
            }
            let Some(kind_name) = element.attr(LocalName::from(ELEMENT_ATTRIBUTE)) else {
                continue;
            };
            let context = declaration_context(source, element.attr(local_name!("id")));
            let kind = BuiltInElementKind::parse(kind_name).ok_or_else(|| {
                invalid_declaration(&context, format!("unknown built-in element `{kind_name}`"))
            })?;
            let definition = definition(kind);
            let tag = element.name.local.as_ref();
            if !definition.allowed_tags.contains(&tag) {
                return Err(invalid_declaration(
                    &context,
                    format!(
                        "`{}` requires one of [{}], not <{tag}>",
                        definition.name,
                        definition.allowed_tags.join(", ")
                    ),
                ));
            }
            let html_id = element
                .attr(local_name!("id"))
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid_declaration(&context, "registered element requires `id`"))?
                .to_owned();
            if id_counts.get(&html_id).copied().unwrap_or_default() != 1 {
                return Err(invalid_declaration(
                    &context,
                    format!("registered id `{html_id}` is not unique in the document"),
                ));
            }
            for attribute in element.attrs() {
                let name = attribute.name.local.as_ref();
                if name.starts_with("data-htm-")
                    && !allowed_behavior_attributes(kind).contains(&name)
                {
                    return Err(invalid_declaration(
                        &context,
                        format!("unsupported HTMShell behavior attribute `{name}`"),
                    ));
                }
            }
            let required = element.attr(LocalName::from(definition.required_attribute));
            let required = required.filter(|value| !value.is_empty()).ok_or_else(|| {
                invalid_declaration(
                    &context,
                    format!(
                        "`{}` requires `{}`",
                        definition.name, definition.required_attribute
                    ),
                )
            })?;
            if kind == BuiltInElementKind::ClockText
                && (element.has_attr(LocalName::from(DATETIME_ATTRIBUTE))
                    || element.has_attr(LocalName::from(STATE_ATTRIBUTE)))
            {
                return Err(invalid_declaration(
                    &context,
                    "`datetime` and `data-htm-state` are runtime-owned",
                ));
            }
            if kind == BuiltInElementKind::PeakMonitor
                && element.has_attr(LocalName::from(STATE_ATTRIBUTE))
            {
                return Err(invalid_declaration(
                    &context,
                    "`data-htm-state` is runtime-owned for `peak-monitor`",
                ));
            }
            let peak_monitor = if kind == BuiltInElementKind::PeakMonitor {
                let enabled = match required {
                    "true" => true,
                    "false" => false,
                    value => {
                        return Err(invalid_declaration(
                            &context,
                            format!("`data-htm-enabled` must be `true` or `false`, not `{value}`"),
                        ));
                    }
                };
                let target = element
                    .attr(LocalName::from(TARGET_ATTRIBUTE))
                    .ok_or_else(|| {
                        invalid_declaration(
                            &context,
                            "`peak-monitor` requires `data-htm-target` outside a repeat",
                        )
                    })
                    .and_then(|value| parse_peak_default_target(value, &context))?;
                Some(analyze_peak_monitor(
                    document,
                    slot,
                    &html_id,
                    PeakMonitorAnalysis {
                        target,
                        enabled,
                        repeated: false,
                        base_prototype_order: 0,
                    },
                    &context,
                )?)
            } else {
                None
            };
            let (
                binding,
                binding_kind,
                action,
                pipewire_target,
                clock,
                value_format,
                repeat,
                range,
                enabled_binding,
            ) = match kind {
                BuiltInElementKind::StateText => {
                    validate_state_text_children(document, slot, &context)?;
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported state binding `{required}`"),
                        )
                    })?;
                    if !binding.supports(StateValueKind::Text) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "state binding `{required}` does not support text presentation"
                            ),
                        ));
                    }
                    (
                        Some(binding),
                        Some(StateValueKind::Text),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                BuiltInElementKind::ActionButton => {
                    let action = required.parse::<ShellAction>().map_err(|()| {
                        invalid_declaration(&context, format!("unsupported action `{required}`"))
                    })?;
                    if !surface_kind.permits(action) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "action `{}` is not permitted from this surface kind",
                                action.as_str()
                            ),
                        ));
                    }
                    let clock_action = matches!(
                        action,
                        ShellAction::ClockEnable
                            | ShellAction::ClockDisable
                            | ShellAction::ClockToggle
                    );
                    let pipewire_mute_action = matches!(
                        action,
                        ShellAction::PipeWireAudioMute
                            | ShellAction::PipeWireAudioUnmute
                            | ShellAction::PipeWireAudioToggleMute
                    );
                    let pipewire_clear_action = matches!(
                        action,
                        ShellAction::PipeWireDefaultsClearPreferredSink
                            | ShellAction::PipeWireDefaultsClearPreferredSource
                    );
                    if matches!(
                        action,
                        ShellAction::PipeWireAudioSetVolume
                            | ShellAction::PipeWireAudioSetChannelVolume
                    ) {
                        return Err(invalid_declaration(
                            &context,
                            "PipeWire numeric audio actions require `range-control`",
                        ));
                    }
                    if matches!(
                        action,
                        ShellAction::PipeWireDefaultsSetPreferredSink
                            | ShellAction::PipeWireDefaultsSetPreferredSource
                    ) {
                        return Err(invalid_declaration(
                            &context,
                            "preferred-default node actions are valid only inside `pipewire.nodes`",
                        ));
                    }
                    if matches!(
                        action,
                        ShellAction::PipeWirePeaksEnable
                            | ShellAction::PipeWirePeaksDisable
                            | ShellAction::PipeWirePeaksToggle
                    ) {
                        return Err(invalid_declaration(
                            &context,
                            "PipeWire peak actions are valid only inside `peak-monitor`",
                        ));
                    }
                    let target_value = element.attr(LocalName::from(TARGET_ATTRIBUTE));
                    let pipewire_target = match (
                        clock_action,
                        pipewire_mute_action,
                        pipewire_clear_action,
                        target_value,
                    ) {
                        (true, _, _, Some(target)) if !target.is_empty() => {
                            unresolved_action_targets.insert(html_id.clone(), target.to_owned());
                            None
                        }
                        (true, _, _, _) => {
                            return Err(invalid_declaration(
                                &context,
                                "clock action requires nonempty `data-htm-target`",
                            ));
                        }
                        (false, true, false, Some(target)) => {
                            Some(parse_default_pipewire_target(target, &context)?)
                        }
                        (false, true, false, None) => {
                            return Err(invalid_declaration(
                                &context,
                                "PipeWire audio action requires `data-htm-target` outside a repeat",
                            ));
                        }
                        (false, false, _, Some(_)) => {
                            return Err(invalid_declaration(
                                &context,
                                "`data-htm-target` is forbidden for this action",
                            ));
                        }
                        (false, false, _, None) => None,
                        (false, true, true, _) => {
                            unreachable!("one action cannot be both mute and clear")
                        }
                    };
                    let enabled_binding = element
                        .attr(LocalName::from(ENABLED_BIND_ATTRIBUTE))
                        .map(|value| {
                            value.parse::<StateBindingKey>().map_err(|()| {
                                invalid_declaration(
                                    &context,
                                    format!("unsupported enabled binding `{value}`"),
                                )
                            })
                        })
                        .transpose()?;
                    if enabled_binding
                        .is_some_and(|binding| !binding.supports(StateValueKind::Boolean))
                    {
                        return Err(invalid_declaration(
                            &context,
                            "`data-htm-enabled-bind` requires a Boolean state key",
                        ));
                    }
                    if pipewire_mute_action
                        && enabled_binding != pipewire_target.map(pipewire_mute_enabled_key)
                    {
                        return Err(invalid_declaration(
                            &context,
                            "PipeWire mute action requires the matching `can_set_mute` enabled binding",
                        ));
                    }
                    let clear_enabled = match action {
                        ShellAction::PipeWireDefaultsClearPreferredSink => {
                            Some(StateBindingKey::PipeWireConfiguredSinkCanClear)
                        }
                        ShellAction::PipeWireDefaultsClearPreferredSource => {
                            Some(StateBindingKey::PipeWireConfiguredSourceCanClear)
                        }
                        _ => None,
                    };
                    if pipewire_clear_action && enabled_binding != clear_enabled {
                        return Err(invalid_declaration(
                            &context,
                            "preferred-default clear action requires the matching configured-default `can_clear` enabled binding",
                        ));
                    }
                    (
                        None,
                        None,
                        Some(action),
                        pipewire_target,
                        None,
                        None,
                        None,
                        None,
                        enabled_binding,
                    )
                }
                BuiltInElementKind::StateToken => {
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported state binding `{required}`"),
                        )
                    })?;
                    if !binding.supports(StateValueKind::Token) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "state binding `{required}` does not support token presentation"
                            ),
                        ));
                    }
                    (
                        Some(binding),
                        Some(StateValueKind::Token),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                BuiltInElementKind::ClockText => {
                    validate_state_text_children(document, slot, &context)?;
                    let format = ClockFormat::compile(required).map_err(|error| {
                        invalid_declaration(
                            &context,
                            format!("invalid `{FORMAT_ATTRIBUTE}`: {error}"),
                        )
                    })?;
                    let time_zone =
                        ClockTimeZone::parse(element.attr(LocalName::from(TIME_ZONE_ATTRIBUTE)))
                            .map_err(|error| {
                                invalid_declaration(
                                    &context,
                                    format!("invalid `{TIME_ZONE_ATTRIBUTE}`: {error}"),
                                )
                            })?;
                    let enabled = match element.attr(LocalName::from(ENABLED_ATTRIBUTE)) {
                        None | Some("true") => true,
                        Some("false") => false,
                        Some(value) => {
                            return Err(invalid_declaration(
                                &context,
                                format!(
                                    "`{ENABLED_ATTRIBUTE}` must be `true` or `false`, not `{value}`"
                                ),
                            ));
                        }
                    };
                    (
                        None,
                        None,
                        None,
                        None,
                        Some(ClockDeclaration {
                            id: ElementInstanceId {
                                document_generation,
                                html_id: html_id.clone(),
                            },
                            format,
                            time_zone,
                            enabled,
                        }),
                        None,
                        None,
                        None,
                        None,
                    )
                }
                BuiltInElementKind::StateValue => {
                    validate_state_text_children(document, slot, &context)?;
                    if element.has_attr(local_name!("value")) {
                        return Err(invalid_declaration(
                            &context,
                            "`value` is runtime-owned for `state-value`",
                        ));
                    }
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported state binding `{required}`"),
                        )
                    })?;
                    if !binding.supports(StateValueKind::Value) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "state binding `{required}` does not support numeric presentation"
                            ),
                        ));
                    }
                    let format = parse_value_format(
                        element.attr(LocalName::from(FORMAT_ATTRIBUTE)),
                        binding.allowed_value_formats(),
                        &context,
                    )?;
                    (
                        Some(binding),
                        Some(StateValueKind::Value),
                        None,
                        None,
                        None,
                        Some(format),
                        None,
                        None,
                        None,
                    )
                }
                BuiltInElementKind::Repeat => {
                    let repeat = analyze_repeat(
                        document,
                        identities,
                        slot,
                        document_generation,
                        &html_id,
                        required,
                        &context,
                    )?;
                    (None, None, None, None, None, None, Some(repeat), None, None)
                }
                BuiltInElementKind::RangeControl => {
                    if element.attr(local_name!("type")) != Some("range") {
                        return Err(invalid_declaration(
                            &context,
                            "`range-control` requires `<input type=\"range\">`",
                        ));
                    }
                    if !node.children.is_empty() {
                        return Err(invalid_declaration(
                            &context,
                            "`range-control` cannot contain descendants",
                        ));
                    }
                    if element.has_attr(local_name!("value"))
                        || element.has_attr(LocalName::from(STATE_ATTRIBUTE))
                    {
                        return Err(invalid_declaration(
                            &context,
                            "`value` and `data-htm-state` are runtime-owned for `range-control`",
                        ));
                    }
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported range binding `{required}`"),
                        )
                    })?;
                    let target = element
                        .attr(LocalName::from(TARGET_ATTRIBUTE))
                        .ok_or_else(|| {
                            invalid_declaration(
                                &context,
                                "`range-control` requires `data-htm-target` outside a repeat",
                            )
                        })
                        .and_then(|value| parse_default_pipewire_target(value, &context))?;
                    if binding != pipewire_volume_key(target) {
                        return Err(invalid_declaration(
                            &context,
                            "range binding and PipeWire target do not match",
                        ));
                    }
                    let action = element
                            .attr(LocalName::from(ACTION_ATTRIBUTE))
                            .and_then(|value| value.parse::<ShellAction>().ok())
                            .filter(|action| *action == ShellAction::PipeWireAudioSetVolume)
                            .ok_or_else(|| {
                                invalid_declaration(
                                    &context,
                                    "`range-control` requires `data-htm-action=\"pipewire.audio.set_volume\"`",
                                )
                            })?;
                    let enabled_binding = element
                        .attr(LocalName::from(ENABLED_BIND_ATTRIBUTE))
                        .and_then(|value| value.parse::<StateBindingKey>().ok());
                    if enabled_binding != Some(pipewire_volume_enabled_key(target)) {
                        return Err(invalid_declaration(
                            &context,
                            "`range-control` requires the matching `can_set_volume` enabled binding",
                        ));
                    }
                    let range = parse_range_control(element, target, &context)?;
                    (
                        Some(binding),
                        Some(StateValueKind::Value),
                        Some(action),
                        Some(target),
                        None,
                        Some(StateValueFormat::Raw),
                        None,
                        Some(range),
                        enabled_binding,
                    )
                }
                BuiltInElementKind::PeakMonitor => {
                    (None, None, None, None, None, None, None, None, None)
                }
            };
            let instance_id = ElementInstanceId {
                document_generation,
                html_id: html_id.clone(),
            };
            let declaration = ElementDeclaration {
                id: instance_id,
                kind,
                binding,
                binding_kind,
                action,
                action_target: None,
                pipewire_target,
                clock,
                disabled: element.has_attr(local_name!("disabled")),
                enabled_binding,
                value_format,
                repeat,
                range,
                peak_monitor,
            };
            let indexed = IndexedElement {
                declaration,
                node: identities.identity_for_slot(document, slot)?,
                depth: node_depth(document, slot),
                order,
            };
            if let Some(binding) = binding {
                match binding_kind {
                    Some(StateValueKind::Text) => {
                        text_bindings
                            .entry(binding)
                            .or_default()
                            .push(html_id.clone());
                    }
                    Some(StateValueKind::Token) => {
                        token_bindings
                            .entry(binding)
                            .or_default()
                            .push(html_id.clone());
                    }
                    Some(StateValueKind::Value) => {
                        value_bindings
                            .entry(binding)
                            .or_default()
                            .push(html_id.clone());
                    }
                    Some(StateValueKind::Boolean) => {
                        boolean_bindings
                            .entry(binding)
                            .or_default()
                            .push(html_id.clone());
                    }
                    None => {
                        return Err(invalid_declaration(
                            &context,
                            "state binding has no presentation kind",
                        ));
                    }
                }
            }
            if action.is_some() && kind == BuiltInElementKind::ActionButton {
                actions.push(html_id.clone());
            }
            if let Some(enabled_binding) = enabled_binding {
                boolean_bindings
                    .entry(enabled_binding)
                    .or_default()
                    .push(html_id.clone());
            }
            elements.insert(html_id, indexed);
        }

        let clock_declarations = elements
            .values()
            .filter(|element| element.declaration.clock.is_some())
            .count();
        if clock_declarations > MAX_CLOCK_DECLARATIONS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {clock_declarations} clock declarations; the per-document limit is {MAX_CLOCK_DECLARATIONS_PER_DOCUMENT}"
            )));
        }
        let repeat_declarations = elements
            .values()
            .filter(|element| element.declaration.repeat.is_some())
            .count();
        if repeat_declarations > MAX_REPEAT_DECLARATIONS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {repeat_declarations} repeat declarations; the per-document limit is {MAX_REPEAT_DECLARATIONS_PER_DOCUMENT}"
            )));
        }
        let pipewire_repeats = elements
            .values()
            .filter_map(|element| element.declaration.repeat.as_ref())
            .filter(|repeat| repeat.source == RepeatSource::PipeWireNodes)
            .collect::<Vec<_>>();
        if pipewire_repeats.len() > MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {} `pipewire.nodes` repeat declarations; the per-document limit is {MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT}",
                pipewire_repeats.len()
            )));
        }
        let top_peak_monitors = elements
            .values()
            .filter_map(|element| element.declaration.peak_monitor.as_ref())
            .collect::<Vec<_>>();
        let repeated_peak_monitors = pipewire_repeats
            .iter()
            .flat_map(|repeat| repeat.peak_monitors.iter())
            .collect::<Vec<_>>();
        let peak_monitor_count = top_peak_monitors
            .len()
            .saturating_add(repeated_peak_monitors.len());
        if peak_monitor_count > MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {peak_monitor_count} peak monitors; the per-document limit is {MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT}"
            )));
        }
        let enabled_peak_monitors = top_peak_monitors
            .iter()
            .chain(repeated_peak_monitors.iter())
            .filter(|monitor| monitor.enabled)
            .count();
        if enabled_peak_monitors > MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {enabled_peak_monitors} initially enabled peak monitors; the per-document limit is {MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT}"
            )));
        }
        let pipewire_link_repeats = elements
            .values()
            .filter_map(|element| element.declaration.repeat.as_ref())
            .filter(|repeat| repeat.source == RepeatSource::PipeWireLinks)
            .collect::<Vec<_>>();
        if pipewire_link_repeats.len() > MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {} `pipewire.links` repeat declarations; the per-document limit is {MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT}",
                pipewire_link_repeats.len()
            )));
        }
        let pipewire_group_repeats = elements
            .values()
            .filter_map(|element| element.declaration.repeat.as_ref())
            .filter(|repeat| repeat.source == RepeatSource::PipeWireLinkGroups)
            .collect::<Vec<_>>();
        if pipewire_group_repeats.len() > MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {} `pipewire.link_groups` repeat declarations; the per-document limit is {MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT}",
                pipewire_group_repeats.len()
            )));
        }
        let property_keys = pipewire_repeats
            .iter()
            .flat_map(|repeat| repeat.descendants.iter())
            .filter_map(|descendant| descendant.property_key.as_ref())
            .collect::<BTreeSet<_>>();
        if property_keys.len() > MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} requests {} unique PipeWire property keys; the per-document limit is {MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT}",
                property_keys.len()
            )));
        }
        let top_audio_controls = elements
            .values()
            .filter(|element| {
                element
                    .declaration
                    .action
                    .is_some_and(|action| action.as_str().starts_with("pipewire.audio."))
            })
            .count();
        let repeated_audio_controls = pipewire_repeats
            .iter()
            .flat_map(|repeat| repeat.descendants.iter())
            .filter(|descendant| {
                descendant
                    .action
                    .is_some_and(|action| action.as_str().starts_with("pipewire.audio."))
            })
            .count();
        let audio_controls = top_audio_controls.saturating_add(repeated_audio_controls);
        if audio_controls > MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {audio_controls} PipeWire audio controls; the per-document limit is {MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT}"
            )));
        }
        let top_default_controls = elements
            .values()
            .filter(|element| {
                element
                    .declaration
                    .action
                    .is_some_and(|action| action.as_str().starts_with("pipewire.defaults."))
            })
            .count();
        let repeated_default_controls = pipewire_repeats
            .iter()
            .flat_map(|repeat| repeat.descendants.iter())
            .filter(|descendant| {
                descendant
                    .action
                    .is_some_and(|action| action.as_str().starts_with("pipewire.defaults."))
            })
            .count();
        let default_controls = top_default_controls.saturating_add(repeated_default_controls);
        if default_controls > MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {default_controls} PipeWire preferred-default controls; the per-document limit is {MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT}"
            )));
        }
        let contextual_repeats = pipewire_repeats
            .iter()
            .chain(pipewire_group_repeats.iter())
            .flat_map(|repeat| repeat.contextual_repeats.iter())
            .collect::<Vec<_>>();
        if contextual_repeats.len() > MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {} contextual repeats; the per-document limit is {MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT}",
                contextual_repeats.len()
            )));
        }
        let contextual_graph_repeats = contextual_repeats
            .iter()
            .filter(|repeat| repeat.source != ContextualRepeatSource::Channels)
            .count();
        if contextual_graph_repeats > MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {contextual_graph_repeats} contextual graph repeats; the per-document limit is {MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT}"
            )));
        }
        let channel_ranges = contextual_repeats
            .iter()
            .filter(|repeat| repeat.source == ContextualRepeatSource::Channels)
            .flat_map(|repeat| repeat.descendants.iter())
            .filter(|descendant| descendant.range.is_some())
            .count();
        if channel_ranges > MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {channel_ranges} channel range controls; the per-document limit is {MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT}"
            )));
        }
        let range_controls = elements
            .values()
            .filter(|element| element.declaration.range.is_some())
            .count()
            .saturating_add(
                pipewire_repeats
                    .iter()
                    .flat_map(|repeat| repeat.descendants.iter())
                    .filter(|descendant| descendant.range.is_some())
                    .count(),
            );
        if range_controls > MAX_RANGE_CONTROLS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {range_controls} range controls; the per-document limit is {MAX_RANGE_CONTROLS_PER_DOCUMENT}"
            )));
        }
        for (action_id, target_id) in unresolved_action_targets {
            let target = elements.get(&target_id).ok_or_else(|| {
                invalid_declaration(
                    &declaration_context(source, Some(&action_id)),
                    format!("clock target `#{target_id}` does not exist"),
                )
            })?;
            if target.declaration.kind != BuiltInElementKind::ClockText {
                return Err(invalid_declaration(
                    &declaration_context(source, Some(&action_id)),
                    format!("clock target `#{target_id}` is not `clock-text`"),
                ));
            }
            let target_identity = target.declaration.id.clone();
            elements
                .get_mut(&action_id)
                .expect("action was indexed above")
                .declaration
                .action_target = Some(target_identity);
        }

        actions.sort_by_key(|id| {
            let element = &elements[id];
            (
                std::cmp::Reverse(element.depth),
                std::cmp::Reverse(element.order),
            )
        });
        for ids in text_bindings.values_mut() {
            ids.sort();
        }
        for ids in token_bindings.values_mut() {
            ids.sort();
        }
        for ids in value_bindings.values_mut() {
            ids.sort();
        }
        for ids in boolean_bindings.values_mut() {
            ids.sort();
        }
        let text_binding_count = text_bindings.values().map(Vec::len).sum();
        let token_binding_count = token_bindings.values().map(Vec::len).sum();
        let value_binding_count = value_bindings.values().map(Vec::len).sum();
        let boolean_binding_count = boolean_bindings.values().map(Vec::len).sum();
        let summary = BuiltInElementSummary {
            registered_elements: elements.len(),
            bindings: text_binding_count
                + token_binding_count
                + value_binding_count
                + boolean_binding_count,
            text_bindings: text_binding_count,
            token_bindings: token_binding_count,
            actions: actions.len(),
            clock_declarations,
            value_bindings: value_binding_count,
            boolean_bindings: boolean_binding_count,
            repeat_declarations,
            discovery_scans: 1,
        };
        Ok(Self {
            elements,
            text_bindings,
            token_bindings,
            value_bindings,
            boolean_bindings,
            actions,
            applied_values: BTreeMap::new(),
            surface_kind,
            summary,
        })
    }

    pub(crate) fn summary(&self) -> BuiltInElementSummary {
        self.summary
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub(crate) fn declarations(&self) -> Vec<ElementDeclaration> {
        self.elements
            .values()
            .map(|element| element.declaration.clone())
            .collect()
    }

    pub(crate) fn clock_declarations(&self) -> Vec<ClockDeclaration> {
        self.elements
            .values()
            .filter_map(|element| element.declaration.clock.clone())
            .collect()
    }

    pub(crate) fn repeat_declarations(&self) -> Vec<RepeatDeclaration> {
        self.elements
            .values()
            .filter_map(|element| element.declaration.repeat.clone())
            .collect()
    }

    pub(crate) fn peak_monitor_declarations(&self) -> Vec<PeakMonitorDeclaration> {
        self.elements
            .values()
            .filter_map(|element| element.declaration.peak_monitor.clone())
            .collect()
    }

    pub(crate) fn clock_declaration(
        &self,
        identity: &ElementInstanceId,
    ) -> Option<&ClockDeclaration> {
        self.elements
            .get(&identity.html_id)
            .filter(|element| element.declaration.id == *identity)
            .and_then(|element| element.declaration.clock.as_ref())
    }

    pub(crate) fn set_clock_enabled(
        &mut self,
        identity: &ElementInstanceId,
        enabled: bool,
    ) -> Result<bool, RuntimeError> {
        let element = self.elements.get_mut(&identity.html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "clock target `#{}` disappeared",
                identity.html_id
            ))
        })?;
        if element.declaration.id != *identity {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "clock target `#{}` belongs to a stale document generation",
                identity.html_id
            )));
        }
        let clock = element.declaration.clock.as_mut().ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "target `#{}` is not `clock-text`",
                identity.html_id
            ))
        })?;
        if clock.enabled == enabled {
            return Ok(false);
        }
        clock.enabled = enabled;
        Ok(true)
    }

    pub(crate) fn element(&self, html_id: &str) -> Option<&ElementDeclaration> {
        self.elements.get(html_id).map(|entry| &entry.declaration)
    }

    pub(crate) fn binding_targets(&self, key: StateBindingKey, kind: StateValueKind) -> &[String] {
        match kind {
            StateValueKind::Text => &self.text_bindings,
            StateValueKind::Token => &self.token_bindings,
            StateValueKind::Value => &self.value_bindings,
            StateValueKind::Boolean => &self.boolean_bindings,
        }
        .get(&key)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    }

    pub(crate) fn binding_is_unchanged(
        &self,
        key: StateBindingKey,
        kind: StateValueKind,
        value: &str,
    ) -> bool {
        self.applied_values
            .get(&(key, kind))
            .is_some_and(|old| old == value)
    }

    pub(crate) fn record_binding(
        &mut self,
        key: StateBindingKey,
        kind: StateValueKind,
        value: String,
    ) {
        self.applied_values.insert((key, kind), value);
    }

    pub(crate) fn indexed_node(&self, html_id: &str) -> Option<ExperimentalNodeIdentity> {
        self.elements.get(html_id).map(|entry| entry.node)
    }

    pub(crate) fn action_candidates(&self) -> impl Iterator<Item = &str> {
        self.actions.iter().map(String::as_str)
    }

    pub(crate) fn action_target(
        &self,
        html_id: &str,
        document: &HtmlDocument,
        identities: &IdentityRegistry,
    ) -> Result<Option<ActionTarget>, RuntimeError> {
        let Some(entry) = self.elements.get(html_id) else {
            return Ok(None);
        };
        let Some(action) = entry.declaration.action else {
            return Ok(None);
        };
        if !self.surface_kind.permits(action) {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "action `{}` is not permitted for the current surface",
                action.as_str()
            )));
        }
        let slot = identities.resolve(document, entry.node)?;
        let disabled = document
            .get_node(slot)
            .and_then(|node| node.element_data())
            .is_some_and(|element| element.has_attr(local_name!("disabled")));
        if disabled {
            return Ok(None);
        }
        if let Some(target) = &entry.declaration.action_target {
            if target.document_generation != entry.declaration.id.document_generation {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "clock target `#{}` belongs to a stale document generation",
                    target.html_id
                )));
            }
            let target_entry = self.elements.get(&target.html_id).ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "clock target `#{}` disappeared",
                    target.html_id
                ))
            })?;
            if target_entry.declaration.kind != BuiltInElementKind::ClockText
                || target_entry.declaration.id != *target
            {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "clock target `#{}` is stale or has the wrong kind",
                    target.html_id
                )));
            }
            identities.resolve(document, target_entry.node)?;
        }
        Ok(Some(ActionTarget {
            id: entry.declaration.id.clone(),
            action,
            node: entry.node,
            target: entry.declaration.action_target.clone(),
            pipewire_target: entry.declaration.pipewire_target,
        }))
    }
}

pub fn built_in_registry_names() -> &'static [&'static str] {
    &[
        "state-text",
        "action-button",
        "state-token",
        "clock-text",
        "state-value",
        "repeat",
        "range-control",
        "peak-monitor",
    ]
}

pub(crate) fn ensure_registry_valid() -> Result<(), RuntimeError> {
    REGISTRY_VALIDATION
        .get_or_init(|| validate_definitions(&DEFINITIONS))
        .clone()
        .map_err(|message| RuntimeError::InvalidPackage(format!("built-in registry: {message}")))
}

fn validate_definitions(definitions: &[BuiltInElementDefinition]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for definition in definitions {
        if definition.name.is_empty() || !names.insert(definition.name) {
            return Err(format!(
                "registry entry `{}` is empty or duplicated",
                definition.name
            ));
        }
        if definition.allowed_tags.is_empty() {
            return Err(format!(
                "registry entry `{}` has no allowed HTML tags",
                definition.name
            ));
        }
    }
    Ok(())
}

fn definition(kind: BuiltInElementKind) -> &'static BuiltInElementDefinition {
    match kind {
        BuiltInElementKind::StateText => &DEFINITIONS[0],
        BuiltInElementKind::ActionButton => &DEFINITIONS[1],
        BuiltInElementKind::StateToken => &DEFINITIONS[2],
        BuiltInElementKind::ClockText => &DEFINITIONS[3],
        BuiltInElementKind::StateValue => &DEFINITIONS[4],
        BuiltInElementKind::Repeat => &DEFINITIONS[5],
        BuiltInElementKind::RangeControl => &DEFINITIONS[6],
        BuiltInElementKind::PeakMonitor => &DEFINITIONS[7],
    }
}

fn allowed_behavior_attributes(kind: BuiltInElementKind) -> &'static [&'static str] {
    match kind {
        BuiltInElementKind::StateText => &[ELEMENT_ATTRIBUTE, BIND_ATTRIBUTE],
        BuiltInElementKind::ActionButton => &[
            ELEMENT_ATTRIBUTE,
            ACTION_ATTRIBUTE,
            TARGET_ATTRIBUTE,
            ENABLED_BIND_ATTRIBUTE,
        ],
        BuiltInElementKind::StateToken => &[ELEMENT_ATTRIBUTE, BIND_ATTRIBUTE],
        BuiltInElementKind::ClockText => &[
            ELEMENT_ATTRIBUTE,
            FORMAT_ATTRIBUTE,
            TIME_ZONE_ATTRIBUTE,
            ENABLED_ATTRIBUTE,
        ],
        BuiltInElementKind::StateValue => &[ELEMENT_ATTRIBUTE, BIND_ATTRIBUTE, FORMAT_ATTRIBUTE],
        BuiltInElementKind::Repeat => &[ELEMENT_ATTRIBUTE, SOURCE_ATTRIBUTE],
        BuiltInElementKind::RangeControl => &[
            ELEMENT_ATTRIBUTE,
            BIND_ATTRIBUTE,
            ACTION_ATTRIBUTE,
            TARGET_ATTRIBUTE,
            ENABLED_BIND_ATTRIBUTE,
        ],
        BuiltInElementKind::PeakMonitor => {
            &[ELEMENT_ATTRIBUTE, TARGET_ATTRIBUTE, ENABLED_ATTRIBUTE]
        }
    }
}

fn repeat_ancestor(document: &HtmlDocument, slot: usize) -> Option<usize> {
    let mut parent = document.get_node(slot).and_then(|node| node.parent);
    while let Some(candidate) = parent {
        let node = document.get_node(candidate)?;
        if node
            .element_data()
            .and_then(|element| element.attr(LocalName::from(ELEMENT_ATTRIBUTE)))
            == Some("repeat")
        {
            return Some(candidate);
        }
        parent = node.parent;
    }
    None
}

fn peak_monitor_ancestor(document: &HtmlDocument, slot: usize) -> Option<usize> {
    let mut parent = document.get_node(slot).and_then(|node| node.parent);
    while let Some(candidate) = parent {
        let node = document.get_node(candidate)?;
        if node
            .element_data()
            .and_then(|element| element.attr(LocalName::from(ELEMENT_ATTRIBUTE)))
            == Some("peak-monitor")
        {
            return Some(candidate);
        }
        parent = node.parent;
    }
    None
}

fn parse_peak_default_target(
    value: &str,
    context: &str,
) -> Result<PeakMonitorTarget, RuntimeError> {
    match value {
        "pipewire.default_sink" => Ok(PeakMonitorTarget::DefaultSink),
        "pipewire.default_source" => Ok(PeakMonitorTarget::DefaultSource),
        _ => Err(invalid_declaration(
            context,
            format!(
                "unsupported peak-monitor target `{value}`; expected `pipewire.default_sink` or `pipewire.default_source`"
            ),
        )),
    }
}

#[derive(Debug, Clone)]
struct PeakMonitorAnalysis {
    target: PeakMonitorTarget,
    enabled: bool,
    repeated: bool,
    base_prototype_order: usize,
}

fn analyze_peak_monitor(
    document: &HtmlDocument,
    monitor_slot: usize,
    id: &str,
    analysis: PeakMonitorAnalysis,
    context: &str,
) -> Result<PeakMonitorDeclaration, RuntimeError> {
    let PeakMonitorAnalysis {
        target,
        enabled,
        repeated,
        base_prototype_order,
    } = analysis;
    let mut descendants = Vec::new();
    let mut channel_repeats = Vec::new();
    let mut local_ids = BTreeSet::new();
    let mut stack = vec![(monitor_slot, 0usize)];
    let mut prototype_order = 0usize;
    while let Some((slot, depth)) = stack.pop() {
        if depth > MAX_REPEAT_TEMPLATE_DEPTH {
            return Err(RuntimeError::LimitExceeded(format!(
                "{context}: peak monitor depth exceeds {MAX_REPEAT_TEMPLATE_DEPTH}"
            )));
        }
        let node = document
            .get_node(slot)
            .ok_or_else(|| invalid_declaration(context, "peak monitor subtree disappeared"))?;
        let relative_order = prototype_order;
        prototype_order = prototype_order.saturating_add(1);
        if slot == monitor_slot {
            stack.extend(
                node.children
                    .iter()
                    .rev()
                    .copied()
                    .map(|child| (child, depth.saturating_add(1))),
            );
            continue;
        }
        if let Some(element) = node.element_data() {
            if repeated && element.has_attr(local_name!("id")) {
                return Err(invalid_declaration(
                    context,
                    "normal `id` attributes are forbidden inside repeated peak monitors",
                ));
            }
            if !repeated && element.has_attr(LocalName::from(LOCAL_ID_ATTRIBUTE)) {
                return Err(invalid_declaration(
                    context,
                    "`data-htm-local-id` is only valid in repeated monitor clones",
                ));
            }
            let kind_name = element.attr(LocalName::from(ELEMENT_ATTRIBUTE));
            if kind_name == Some("peak-monitor") {
                return Err(invalid_declaration(
                    context,
                    "nested `peak-monitor` declarations are forbidden",
                ));
            }
            if kind_name == Some("repeat") {
                if element.attr(LocalName::from(SOURCE_ATTRIBUTE)) != Some("peak.channels") {
                    return Err(invalid_declaration(
                        context,
                        "only `peak.channels` may repeat inside `peak-monitor`",
                    ));
                }
                if element.has_attr(LocalName::from(LOCAL_ID_ATTRIBUTE))
                    || element.has_attr(local_name!("id"))
                {
                    return Err(invalid_declaration(
                        context,
                        "`peak.channels` declarations do not use an author ID",
                    ));
                }
                if channel_repeats.len() >= MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR {
                    return Err(RuntimeError::LimitExceeded(format!(
                        "{context}: peak monitor exceeds {MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR} `peak.channels` repeats"
                    )));
                }
                channel_repeats.push(analyze_peak_channel_repeat(
                    document,
                    slot,
                    base_prototype_order.saturating_add(relative_order),
                    channel_repeats.len(),
                    context,
                )?);
                let nested_nodes = subtree_node_count(document, slot)?;
                prototype_order = prototype_order
                    .checked_add(nested_nodes.saturating_sub(1))
                    .ok_or_else(|| {
                        RuntimeError::LimitExceeded("peak channel clone count overflow".into())
                    })?;
                continue;
            }
            if let Some(kind_name) = kind_name {
                let kind = BuiltInElementKind::parse(kind_name).ok_or_else(|| {
                    invalid_declaration(
                        context,
                        format!("unknown peak-monitor built-in `{kind_name}`"),
                    )
                })?;
                if !matches!(
                    kind,
                    BuiltInElementKind::StateText
                        | BuiltInElementKind::StateToken
                        | BuiltInElementKind::StateValue
                        | BuiltInElementKind::ActionButton
                ) {
                    return Err(invalid_declaration(
                        context,
                        format!("`{}` is forbidden inside `peak-monitor`", kind.as_str()),
                    ));
                }
                let definition = definition(kind);
                let tag = element.name.local.as_ref();
                if !definition.allowed_tags.contains(&tag) {
                    return Err(invalid_declaration(
                        context,
                        format!("`{}` is not allowed on <{tag}>", kind.as_str()),
                    ));
                }
                for attribute in element.attrs() {
                    let name = attribute.name.local.as_ref();
                    if name.starts_with("data-htm-")
                        && name != LOCAL_ID_ATTRIBUTE
                        && !allowed_behavior_attributes(kind).contains(&name)
                    {
                        return Err(invalid_declaration(
                            context,
                            format!("unsupported peak-monitor attribute `{name}`"),
                        ));
                    }
                }
                let local_id = if repeated {
                    element
                        .attr(LocalName::from(LOCAL_ID_ATTRIBUTE))
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            invalid_declaration(
                                context,
                                "registered repeated monitor descendant requires `data-htm-local-id`",
                            )
                        })?
                } else {
                    element
                        .attr(local_name!("id"))
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            invalid_declaration(
                                context,
                                "registered monitor descendant requires `id`",
                            )
                        })?
                };
                if !local_ids.insert(local_id.to_owned()) {
                    return Err(invalid_declaration(
                        context,
                        format!("duplicate peak-monitor descendant id `{local_id}`"),
                    ));
                }
                let mut binding = None;
                let mut action = None;
                let mut value_format = None;
                match kind {
                    BuiltInElementKind::StateText
                    | BuiltInElementKind::StateToken
                    | BuiltInElementKind::StateValue => {
                        let binding_value = element
                            .attr(LocalName::from(BIND_ATTRIBUTE))
                            .filter(|value| !value.is_empty())
                            .ok_or_else(|| {
                                invalid_declaration(
                                    context,
                                    format!("`{}` requires `data-htm-bind`", kind.as_str()),
                                )
                            })?;
                        let parsed = binding_value.parse::<PeakBindingKey>().map_err(|()| {
                            invalid_declaration(
                                context,
                                format!("unsupported monitor binding `{binding_value}`"),
                            )
                        })?;
                        match kind {
                            BuiltInElementKind::StateText if parsed.supports_text() => {
                                validate_state_text_children(document, slot, context)?;
                            }
                            BuiltInElementKind::StateToken if parsed.supports_token() => {
                                if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                                    return Err(invalid_declaration(
                                        context,
                                        "`data-htm-state` is runtime-owned",
                                    ));
                                }
                            }
                            BuiltInElementKind::StateValue if parsed.supports_value() => {
                                validate_state_text_children(document, slot, context)?;
                                if element.has_attr(local_name!("value")) {
                                    return Err(invalid_declaration(
                                        context,
                                        "`value` is runtime-owned for `state-value`",
                                    ));
                                }
                                value_format = Some(parse_value_format(
                                    element.attr(LocalName::from(FORMAT_ATTRIBUTE)),
                                    &[StateValueFormat::Raw, StateValueFormat::Percent],
                                    context,
                                )?);
                            }
                            _ => {
                                return Err(invalid_declaration(
                                    context,
                                    format!(
                                        "monitor binding `{binding_value}` does not support `{}`",
                                        kind.as_str()
                                    ),
                                ));
                            }
                        }
                        binding = Some(parsed);
                    }
                    BuiltInElementKind::ActionButton => {
                        if element.has_attr(LocalName::from(TARGET_ATTRIBUTE))
                            || element.has_attr(LocalName::from(ENABLED_BIND_ATTRIBUTE))
                        {
                            return Err(invalid_declaration(
                                context,
                                "peak monitor actions forbid explicit targets and enabled bindings",
                            ));
                        }
                        let parsed = element
                            .attr(LocalName::from(ACTION_ATTRIBUTE))
                            .and_then(|value| value.parse::<ShellAction>().ok())
                            .filter(|action| {
                                matches!(
                                    action,
                                    ShellAction::PipeWirePeaksEnable
                                        | ShellAction::PipeWirePeaksDisable
                                        | ShellAction::PipeWirePeaksToggle
                                )
                            })
                            .ok_or_else(|| {
                                invalid_declaration(
                                    context,
                                    "only PipeWire peak enable, disable, and toggle actions are allowed inside `peak-monitor`",
                                )
                            })?;
                        if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                            return Err(invalid_declaration(
                                context,
                                "`data-htm-state` is runtime-owned for peak controls",
                            ));
                        }
                        action = Some(parsed);
                    }
                    _ => unreachable!("peak monitor descendant kind was filtered"),
                }
                descendants.push(PeakScopedElementDeclaration {
                    local_id: local_id.to_owned(),
                    kind,
                    binding,
                    action,
                    value_format,
                    disabled: element.has_attr(local_name!("disabled")),
                    prototype_order: base_prototype_order.saturating_add(relative_order),
                });
                if descendants.len() > MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR {
                    return Err(RuntimeError::LimitExceeded(format!(
                        "{context}: peak monitor exceeds {MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR} registered descendants"
                    )));
                }
                let actions = descendants
                    .iter()
                    .filter(|descendant| descendant.action.is_some())
                    .count();
                if actions > MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR {
                    return Err(RuntimeError::LimitExceeded(format!(
                        "{context}: peak monitor exceeds {MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR} action buttons"
                    )));
                }
            } else {
                for attribute in element.attrs() {
                    if attribute.name.local.as_ref().starts_with("data-htm-")
                        && attribute.name.local.as_ref() != LOCAL_ID_ATTRIBUTE
                    {
                        return Err(invalid_declaration(
                            context,
                            format!(
                                "unsupported peak-monitor subtree attribute `{}`",
                                attribute.name.local
                            ),
                        ));
                    }
                }
            }
        }
        stack.extend(
            node.children
                .iter()
                .rev()
                .copied()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
    Ok(PeakMonitorDeclaration {
        id: id.to_owned(),
        target,
        enabled,
        root_prototype_order: base_prototype_order,
        descendants,
        channel_repeats,
        prototype_nodes: prototype_order,
    })
}

fn analyze_peak_channel_repeat(
    document: &HtmlDocument,
    template_slot: usize,
    template_prototype_order: usize,
    ordinal: usize,
    context: &str,
) -> Result<PeakChannelRepeatDeclaration, RuntimeError> {
    let template = document
        .get_node(template_slot)
        .ok_or_else(|| invalid_declaration(context, "peak channel template disappeared"))?;
    let element = template
        .element_data()
        .ok_or_else(|| invalid_declaration(context, "peak channel repeat must be a template"))?;
    for attribute in element.attrs() {
        let name = attribute.name.local.as_ref();
        if name.starts_with("data-htm-") && !matches!(name, ELEMENT_ATTRIBUTE | SOURCE_ATTRIBUTE) {
            return Err(invalid_declaration(
                context,
                format!("unsupported peak-channel repeat attribute `{name}`"),
            ));
        }
    }
    let roots = template
        .children
        .iter()
        .filter_map(|slot| {
            document
                .get_node(*slot)
                .and_then(|node| matches!(node.data, NodeData::Element(_)).then_some(*slot))
        })
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(invalid_declaration(
            context,
            "peak channel repeat must contain exactly one root element",
        ));
    }
    let mut descendants = Vec::new();
    let mut local_ids = BTreeSet::new();
    let mut stack = vec![(roots[0], 1usize)];
    let mut prototype_order = 0usize;
    while let Some((slot, depth)) = stack.pop() {
        if depth > MAX_REPEAT_TEMPLATE_DEPTH {
            return Err(RuntimeError::LimitExceeded(format!(
                "{context}: peak channel template depth exceeds {MAX_REPEAT_TEMPLATE_DEPTH}"
            )));
        }
        let node = document
            .get_node(slot)
            .ok_or_else(|| invalid_declaration(context, "peak channel subtree disappeared"))?;
        let order = prototype_order;
        prototype_order = prototype_order.saturating_add(1);
        if let Some(element) = node.element_data() {
            if element.has_attr(local_name!("id")) {
                return Err(invalid_declaration(
                    context,
                    "normal `id` attributes are forbidden inside `peak.channels`",
                ));
            }
            let local_id = element.attr(LocalName::from(LOCAL_ID_ATTRIBUTE));
            if let Some(kind_name) = element.attr(LocalName::from(ELEMENT_ATTRIBUTE)) {
                let kind = BuiltInElementKind::parse(kind_name).ok_or_else(|| {
                    invalid_declaration(
                        context,
                        format!("unknown peak-channel built-in `{kind_name}`"),
                    )
                })?;
                if !matches!(
                    kind,
                    BuiltInElementKind::StateText
                        | BuiltInElementKind::StateToken
                        | BuiltInElementKind::StateValue
                ) {
                    return Err(invalid_declaration(
                        context,
                        format!("`{}` is forbidden inside `peak.channels`", kind.as_str()),
                    ));
                }
                let tag = element.name.local.as_ref();
                if !definition(kind).allowed_tags.contains(&tag) {
                    return Err(invalid_declaration(
                        context,
                        format!("`{}` is not allowed on <{tag}>", kind.as_str()),
                    ));
                }
                let local_id = local_id.filter(|value| !value.is_empty()).ok_or_else(|| {
                    invalid_declaration(
                        context,
                        "registered peak-channel descendant requires `data-htm-local-id`",
                    )
                })?;
                if !local_ids.insert(local_id.to_owned()) {
                    return Err(invalid_declaration(
                        context,
                        format!("duplicate peak-channel local id `{local_id}`"),
                    ));
                }
                let binding_value = element
                    .attr(LocalName::from(BIND_ATTRIBUTE))
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        invalid_declaration(
                            context,
                            format!("`{}` requires `data-htm-bind`", kind.as_str()),
                        )
                    })?;
                let binding = binding_value.parse::<ItemBindingKey>().map_err(|()| {
                    invalid_declaration(
                        context,
                        format!("unsupported peak-channel binding `{binding_value}`"),
                    )
                })?;
                if !binding.supports_peak_channel() {
                    return Err(invalid_declaration(
                        context,
                        format!("`{binding_value}` is not valid inside `peak.channels`"),
                    ));
                }
                let value_format = match kind {
                    BuiltInElementKind::StateText if binding.supports_text() => {
                        validate_state_text_children(document, slot, context)?;
                        None
                    }
                    BuiltInElementKind::StateToken if binding.supports_token() => {
                        if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                            return Err(invalid_declaration(
                                context,
                                "`data-htm-state` is runtime-owned",
                            ));
                        }
                        None
                    }
                    BuiltInElementKind::StateValue if binding.supports_value() => {
                        validate_state_text_children(document, slot, context)?;
                        if element.has_attr(local_name!("value")) {
                            return Err(invalid_declaration(
                                context,
                                "`value` is runtime-owned for `state-value`",
                            ));
                        }
                        Some(parse_value_format(
                            element.attr(LocalName::from(FORMAT_ATTRIBUTE)),
                            &[StateValueFormat::Raw, StateValueFormat::Percent],
                            context,
                        )?)
                    }
                    _ => {
                        return Err(invalid_declaration(
                            context,
                            format!(
                                "peak-channel binding `{binding_value}` does not support `{}`",
                                kind.as_str()
                            ),
                        ));
                    }
                };
                descendants.push(RepeatedElementDeclaration {
                    local_id: local_id.to_owned(),
                    kind,
                    binding: Some(binding),
                    action: None,
                    enabled_binding: None,
                    range: None,
                    disabled: false,
                    property_key: None,
                    value_format,
                    prototype_order: order,
                });
                if descendants.len() > MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM {
                    return Err(RuntimeError::LimitExceeded(format!(
                        "{context}: peak channel template exceeds {MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM} bindings"
                    )));
                }
            } else {
                if let Some(local_id) = local_id
                    && (local_id.is_empty() || !local_ids.insert(local_id.to_owned()))
                {
                    return Err(invalid_declaration(
                        context,
                        "static peak-channel local IDs must be nonempty and unique",
                    ));
                }
                for attribute in element.attrs() {
                    if attribute.name.local.as_ref().starts_with("data-htm-")
                        && attribute.name.local.as_ref() != LOCAL_ID_ATTRIBUTE
                    {
                        return Err(invalid_declaration(
                            context,
                            format!(
                                "unsupported peak-channel subtree attribute `{}`",
                                attribute.name.local
                            ),
                        ));
                    }
                }
            }
        }
        stack.extend(
            node.children
                .iter()
                .rev()
                .copied()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
    Ok(PeakChannelRepeatDeclaration {
        id: format!("peak-channels-{ordinal}"),
        template_prototype_order,
        descendants,
        prototype_nodes: prototype_order,
    })
}

fn analyze_repeat(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    template_slot: usize,
    document_generation: ExperimentalDocumentIdentity,
    html_id: &str,
    source_value: &str,
    context: &str,
) -> Result<RepeatDeclaration, RuntimeError> {
    let source = source_value.parse::<RepeatSource>().map_err(|()| {
        invalid_declaration(
            context,
            format!("unsupported repeat source `{source_value}`"),
        )
    })?;
    let template = document
        .get_node(template_slot)
        .ok_or_else(|| invalid_declaration(context, "template node lookup failed"))?;
    let mut roots = Vec::new();
    for child in &template.children {
        let node = document
            .get_node(*child)
            .ok_or_else(|| invalid_declaration(context, "template child lookup failed"))?;
        match &node.data {
            NodeData::Text(text) if text.content.trim().is_empty() => {}
            NodeData::Text(_) => {
                return Err(invalid_declaration(
                    context,
                    "repeat template top-level text must be whitespace",
                ));
            }
            NodeData::Element(_) => roots.push(*child),
            _ => {}
        }
    }
    if roots.len() != 1 {
        return Err(invalid_declaration(
            context,
            format!(
                "repeat template must contain exactly one root element; found {}",
                roots.len()
            ),
        ));
    }
    let root = roots[0];
    let mut descendants = Vec::new();
    let mut contextual_repeats = Vec::new();
    let mut peak_monitors = Vec::new();
    let mut local_ids = BTreeSet::new();
    let mut stack = vec![(root, 1usize)];
    let mut prototype_order = 0usize;
    while let Some((slot, depth)) = stack.pop() {
        if depth > MAX_REPEAT_TEMPLATE_DEPTH {
            return Err(RuntimeError::LimitExceeded(format!(
                "{context}: repeat template depth exceeds {MAX_REPEAT_TEMPLATE_DEPTH}"
            )));
        }
        let node = document
            .get_node(slot)
            .ok_or_else(|| invalid_declaration(context, "repeat subtree lookup failed"))?;
        let order = prototype_order;
        prototype_order = prototype_order.saturating_add(1);
        if let Some(element) = node.element_data() {
            if element.has_attr(local_name!("id")) {
                return Err(invalid_declaration(
                    context,
                    "normal `id` attributes are forbidden inside repeat templates",
                ));
            }
            let local_id = element.attr(LocalName::from(LOCAL_ID_ATTRIBUTE));
            let kind_name = element.attr(LocalName::from(ELEMENT_ATTRIBUTE));
            match kind_name {
                None => {
                    if let Some(local_id) = local_id
                        && (local_id.is_empty() || !local_ids.insert(local_id.to_owned()))
                    {
                        return Err(invalid_declaration(
                            context,
                            "static repeat local IDs must be nonempty and template-unique",
                        ));
                    }
                    for attribute in element.attrs() {
                        if attribute.name.local.as_ref().starts_with("data-htm-")
                            && attribute.name.local.as_ref() != LOCAL_ID_ATTRIBUTE
                        {
                            return Err(invalid_declaration(
                                context,
                                format!(
                                    "unsupported repeat-template attribute `{}`",
                                    attribute.name.local
                                ),
                            ));
                        }
                    }
                }
                Some(kind_name) => {
                    let kind = BuiltInElementKind::parse(kind_name).ok_or_else(|| {
                        invalid_declaration(
                            context,
                            format!("unknown repeated built-in element `{kind_name}`"),
                        )
                    })?;
                    if kind == BuiltInElementKind::PeakMonitor {
                        let definition = definition(kind);
                        let tag = element.name.local.as_ref();
                        if !definition.allowed_tags.contains(&tag) {
                            return Err(invalid_declaration(
                                context,
                                format!(
                                    "`{}` requires one of [{}], not <{tag}>",
                                    definition.name,
                                    definition.allowed_tags.join(", ")
                                ),
                            ));
                        }
                        for attribute in element.attrs() {
                            let name = attribute.name.local.as_ref();
                            if name.starts_with("data-htm-")
                                && name != LOCAL_ID_ATTRIBUTE
                                && !allowed_behavior_attributes(kind).contains(&name)
                            {
                                return Err(invalid_declaration(
                                    context,
                                    format!("unsupported repeated peak-monitor attribute `{name}`"),
                                ));
                            }
                        }
                        if source != RepeatSource::PipeWireNodes {
                            return Err(invalid_declaration(
                                context,
                                "`peak-monitor` is valid only inside `pipewire.nodes`",
                            ));
                        }
                        if element.has_attr(LocalName::from(TARGET_ATTRIBUTE)) {
                            return Err(invalid_declaration(
                                context,
                                "item-local `peak-monitor` forbids `data-htm-target`",
                            ));
                        }
                        if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                            return Err(invalid_declaration(
                                context,
                                "`data-htm-state` is runtime-owned for `peak-monitor`",
                            ));
                        }
                        let local_id =
                            local_id.filter(|value| !value.is_empty()).ok_or_else(|| {
                                invalid_declaration(
                                    context,
                                    "repeated `peak-monitor` requires `data-htm-local-id`",
                                )
                            })?;
                        if !local_ids.insert(local_id.to_owned()) {
                            return Err(invalid_declaration(
                                context,
                                format!("duplicate repeat local id `{local_id}`"),
                            ));
                        }
                        let enabled = match element.attr(LocalName::from(ENABLED_ATTRIBUTE)) {
                            Some("true") => true,
                            Some("false") => false,
                            Some(value) => {
                                return Err(invalid_declaration(
                                    context,
                                    format!(
                                        "`data-htm-enabled` must be `true` or `false`, not `{value}`"
                                    ),
                                ));
                            }
                            None => {
                                return Err(invalid_declaration(
                                    context,
                                    "`peak-monitor` requires `data-htm-enabled`",
                                ));
                            }
                        };
                        if peak_monitors.len() >= MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM {
                            return Err(RuntimeError::LimitExceeded(format!(
                                "{context}: node template exceeds {MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM} peak monitors"
                            )));
                        }
                        peak_monitors.push(analyze_peak_monitor(
                            document,
                            slot,
                            local_id,
                            PeakMonitorAnalysis {
                                target: PeakMonitorTarget::CurrentItem,
                                enabled,
                                repeated: true,
                                base_prototype_order: order,
                            },
                            context,
                        )?);
                        let nested_nodes = subtree_node_count(document, slot)?;
                        prototype_order = prototype_order
                            .checked_add(nested_nodes.saturating_sub(1))
                            .ok_or_else(|| {
                                RuntimeError::LimitExceeded(
                                    "peak monitor clone count overflow".into(),
                                )
                            })?;
                        continue;
                    }
                    if kind == BuiltInElementKind::Repeat {
                        let contextual_source = element
                            .attr(LocalName::from(SOURCE_ATTRIBUTE))
                            .and_then(|value| value.parse::<ContextualRepeatSource>().ok())
                            .filter(|contextual| contextual.parent() == source)
                            .ok_or_else(|| {
                                invalid_declaration(
                                    context,
                                    format!(
                                        "contextual repeat source is not valid inside `{source_value}`"
                                    ),
                                )
                            })?;
                        if !matches!(
                            source,
                            RepeatSource::PipeWireNodes | RepeatSource::PipeWireLinkGroups
                        ) {
                            return Err(invalid_declaration(
                                context,
                                "this repeat source does not support contextual repetition",
                            ));
                        }
                        if local_id.is_some() {
                            return Err(invalid_declaration(
                                context,
                                "contextual repeat declarations do not use `data-htm-local-id`",
                            ));
                        }
                        let source_count = contextual_repeats
                            .iter()
                            .filter(|repeat: &&ContextualRepeatDeclaration| {
                                repeat.source == contextual_source
                            })
                            .count();
                        let source_limit = match contextual_source {
                            ContextualRepeatSource::Channels => {
                                MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE
                            }
                            ContextualRepeatSource::GroupLinks => {
                                MAX_CONTEXTUAL_LINK_REPEATS_PER_GROUP_TEMPLATE
                            }
                            ContextualRepeatSource::NodeLinkGroups => {
                                MAX_CONTEXTUAL_LINK_GROUP_REPEATS_PER_NODE_TEMPLATE
                            }
                        };
                        if source_count >= source_limit {
                            return Err(RuntimeError::LimitExceeded(format!(
                                "{context}: `{}` exceeds {source_limit} declarations in one template",
                                contextual_source.as_str()
                            )));
                        }
                        let declaration = analyze_contextual_repeat(
                            document,
                            slot,
                            order,
                            contextual_source,
                            contextual_repeats.len(),
                            context,
                        )?;
                        let nested_nodes = subtree_node_count(document, slot)?;
                        prototype_order = prototype_order
                            .checked_add(nested_nodes.saturating_sub(1))
                            .ok_or_else(|| {
                                RuntimeError::LimitExceeded(
                                    "contextual repeat node count overflow".into(),
                                )
                            })?;
                        contextual_repeats.push(declaration);
                        continue;
                    }
                    let read_only = matches!(
                        kind,
                        BuiltInElementKind::StateText
                            | BuiltInElementKind::StateToken
                            | BuiltInElementKind::StateValue
                    );
                    let pipewire_control = source == RepeatSource::PipeWireNodes
                        && matches!(
                            kind,
                            BuiltInElementKind::ActionButton | BuiltInElementKind::RangeControl
                        );
                    if !read_only && !pipewire_control {
                        return Err(invalid_declaration(
                            context,
                            format!("`{}` is not allowed inside a repeat", kind.as_str()),
                        ));
                    }
                    let definition = definition(kind);
                    let tag = element.name.local.as_ref();
                    if !definition.allowed_tags.contains(&tag) {
                        return Err(invalid_declaration(
                            context,
                            format!("`{}` is not allowed on <{tag}>", kind.as_str()),
                        ));
                    }
                    for attribute in element.attrs() {
                        let name = attribute.name.local.as_ref();
                        if name.starts_with("data-htm-")
                            && name != LOCAL_ID_ATTRIBUTE
                            && name != PROPERTY_KEY_ATTRIBUTE
                            && !allowed_behavior_attributes(kind).contains(&name)
                        {
                            return Err(invalid_declaration(
                                context,
                                format!("unsupported HTMShell behavior attribute `{name}`"),
                            ));
                        }
                    }
                    let local_id = local_id.filter(|value| !value.is_empty()).ok_or_else(|| {
                        invalid_declaration(
                            context,
                            "registered repeat descendant requires `data-htm-local-id`",
                        )
                    })?;
                    if !local_ids.insert(local_id.to_owned()) {
                        return Err(invalid_declaration(
                            context,
                            format!("duplicate repeat local id `{local_id}`"),
                        ));
                    }
                    let mut binding = None;
                    let mut action = None;
                    let mut enabled_binding = None;
                    let mut range = None;
                    let mut property_key = None;
                    let mut value_format = None;
                    match kind {
                        BuiltInElementKind::StateText
                        | BuiltInElementKind::StateToken
                        | BuiltInElementKind::StateValue => {
                            let binding_value = element
                                .attr(LocalName::from(BIND_ATTRIBUTE))
                                .filter(|value| !value.is_empty())
                                .ok_or_else(|| {
                                    invalid_declaration(
                                        context,
                                        format!("`{}` requires `data-htm-bind`", kind.as_str()),
                                    )
                                })?;
                            let parsed = binding_value.parse::<ItemBindingKey>().map_err(|()| {
                                invalid_declaration(
                                    context,
                                    format!("unsupported item binding `{binding_value}`"),
                                )
                            })?;
                            if !parsed.supports_source(source) {
                                return Err(invalid_declaration(
                                    context,
                                    format!(
                                        "item binding `{binding_value}` does not belong to `{source_value}`"
                                    ),
                                ));
                            }
                            property_key = match (
                                parsed,
                                element.attr(LocalName::from(PROPERTY_KEY_ATTRIBUTE)),
                            ) {
                                (ItemBindingKey::Property, Some("")) => {
                                    return Err(invalid_declaration(
                                        context,
                                        "`data-htm-property-key` must not be empty",
                                    ));
                                }
                                (ItemBindingKey::Property, Some(key))
                                    if key.len() > MAX_PIPEWIRE_PROPERTY_KEY_BYTES =>
                                {
                                    return Err(invalid_declaration(
                                        context,
                                        format!(
                                            "`data-htm-property-key` exceeds {MAX_PIPEWIRE_PROPERTY_KEY_BYTES} bytes"
                                        ),
                                    ));
                                }
                                (ItemBindingKey::Property, Some(key))
                                    if !valid_pipewire_property_key(key) =>
                                {
                                    return Err(invalid_declaration(
                                        context,
                                        format!("invalid `data-htm-property-key` `{key}`"),
                                    ));
                                }
                                (ItemBindingKey::Property, Some(key)) => Some(key.to_owned()),
                                (ItemBindingKey::Property, None) => {
                                    return Err(invalid_declaration(
                                        context,
                                        "`item.property` requires `data-htm-property-key`",
                                    ));
                                }
                                (_, Some(_)) => {
                                    return Err(invalid_declaration(
                                        context,
                                        "`data-htm-property-key` is only valid with `item.property`",
                                    ));
                                }
                                (_, None) => None,
                            };
                            value_format = match kind {
                                BuiltInElementKind::StateText if parsed.supports_text() => {
                                    validate_state_text_children(document, slot, context)?;
                                    None
                                }
                                BuiltInElementKind::StateToken if parsed.supports_token() => {
                                    if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                                        return Err(invalid_declaration(
                                            context,
                                            "`data-htm-state` is runtime-owned",
                                        ));
                                    }
                                    None
                                }
                                BuiltInElementKind::StateValue if parsed.supports_value() => {
                                    validate_state_text_children(document, slot, context)?;
                                    if element.has_attr(local_name!("value")) {
                                        return Err(invalid_declaration(
                                            context,
                                            "`value` is runtime-owned for `state-value`",
                                        ));
                                    }
                                    Some(parse_value_format(
                                        element.attr(LocalName::from(FORMAT_ATTRIBUTE)),
                                        item_value_formats(parsed),
                                        context,
                                    )?)
                                }
                                _ => {
                                    return Err(invalid_declaration(
                                        context,
                                        format!(
                                            "item binding `{binding_value}` does not support `{}`",
                                            kind.as_str()
                                        ),
                                    ));
                                }
                            };
                            binding = Some(parsed);
                        }
                        BuiltInElementKind::ActionButton => {
                            if element.has_attr(LocalName::from(TARGET_ATTRIBUTE)) {
                                return Err(invalid_declaration(
                                    context,
                                    "`data-htm-target` is forbidden for an item-local action",
                                ));
                            }
                            let parsed = element
                                .attr(LocalName::from(ACTION_ATTRIBUTE))
                                .and_then(|value| value.parse::<ShellAction>().ok())
                                .filter(|action| {
                                    matches!(
                                        action,
                                        ShellAction::PipeWireAudioMute
                                            | ShellAction::PipeWireAudioUnmute
                                            | ShellAction::PipeWireAudioToggleMute
                                            | ShellAction::PipeWireDefaultsSetPreferredSink
                                            | ShellAction::PipeWireDefaultsSetPreferredSource
                                    )
                                })
                                .ok_or_else(|| {
                                    invalid_declaration(
                                        context,
                                        "only PipeWire mute and preferred-default selection actions are allowed inside `pipewire.nodes`",
                                    )
                                })?;
                            let enabled = element
                                .attr(LocalName::from(ENABLED_BIND_ATTRIBUTE))
                                .and_then(|value| value.parse::<ItemBindingKey>().ok());
                            let required_enabled = match parsed {
                                ShellAction::PipeWireAudioMute
                                | ShellAction::PipeWireAudioUnmute
                                | ShellAction::PipeWireAudioToggleMute => {
                                    ItemBindingKey::CanSetMute
                                }
                                ShellAction::PipeWireDefaultsSetPreferredSink => {
                                    ItemBindingKey::CanSetPreferredSink
                                }
                                ShellAction::PipeWireDefaultsSetPreferredSource => {
                                    ItemBindingKey::CanSetPreferredSource
                                }
                                _ => unreachable!("filtered item-local action"),
                            };
                            if enabled != Some(required_enabled) {
                                return Err(invalid_declaration(
                                    context,
                                    format!(
                                        "item-local action `{}` requires `data-htm-enabled-bind=\"{}\"`",
                                        parsed.as_str(),
                                        required_enabled.as_str()
                                    ),
                                ));
                            }
                            if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                                return Err(invalid_declaration(
                                    context,
                                    "`data-htm-state` is runtime-owned for PipeWire controls",
                                ));
                            }
                            action = Some(parsed);
                            enabled_binding = enabled;
                        }
                        BuiltInElementKind::RangeControl => {
                            if element.attr(local_name!("type")) != Some("range") {
                                return Err(invalid_declaration(
                                    context,
                                    "`range-control` requires `<input type=\"range\">`",
                                ));
                            }
                            if !node.children.is_empty() {
                                return Err(invalid_declaration(
                                    context,
                                    "`range-control` cannot contain descendants",
                                ));
                            }
                            if element.has_attr(LocalName::from(TARGET_ATTRIBUTE)) {
                                return Err(invalid_declaration(
                                    context,
                                    "`data-htm-target` is forbidden for an item-local range control",
                                ));
                            }
                            if element.attr(LocalName::from(BIND_ATTRIBUTE)) != Some("item.volume")
                            {
                                return Err(invalid_declaration(
                                    context,
                                    "item-local range control requires `data-htm-bind=\"item.volume\"`",
                                ));
                            }
                            if element.attr(LocalName::from(ACTION_ATTRIBUTE))
                                != Some("pipewire.audio.set_volume")
                            {
                                return Err(invalid_declaration(
                                    context,
                                    "item-local range control requires `pipewire.audio.set_volume`",
                                ));
                            }
                            if element.attr(LocalName::from(ENABLED_BIND_ATTRIBUTE))
                                != Some("item.can_set_volume")
                            {
                                return Err(invalid_declaration(
                                    context,
                                    "item-local range control requires `data-htm-enabled-bind=\"item.can_set_volume\"`",
                                ));
                            }
                            if element.has_attr(local_name!("value"))
                                || element.has_attr(LocalName::from(STATE_ATTRIBUTE))
                            {
                                return Err(invalid_declaration(
                                    context,
                                    "`value` and `data-htm-state` are runtime-owned for `range-control`",
                                ));
                            }
                            binding = Some(ItemBindingKey::Volume);
                            action = Some(ShellAction::PipeWireAudioSetVolume);
                            enabled_binding = Some(ItemBindingKey::CanSetVolume);
                            range = Some(parse_range_control(
                                element,
                                PipeWireControlTarget::CurrentItem,
                                context,
                            )?);
                        }
                        BuiltInElementKind::ClockText
                        | BuiltInElementKind::Repeat
                        | BuiltInElementKind::PeakMonitor => {
                            unreachable!("repeat child kind was rejected above")
                        }
                    }
                    descendants.push(RepeatedElementDeclaration {
                        local_id: local_id.to_owned(),
                        kind,
                        binding,
                        action,
                        enabled_binding,
                        range,
                        disabled: element.has_attr(local_name!("disabled")),
                        property_key,
                        value_format,
                        prototype_order: order,
                    });
                    let source_binding_limit = match source {
                        RepeatSource::PipeWireNodes => MAX_PIPEWIRE_BINDINGS_PER_ITEM,
                        RepeatSource::PipeWireLinks | RepeatSource::PipeWireLinkGroups => {
                            MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM
                        }
                        _ => MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE,
                    };
                    if descendants.len() > source_binding_limit {
                        return Err(RuntimeError::LimitExceeded(format!(
                            "{context}: `{source_value}` template has more than {source_binding_limit} registered bindings"
                        )));
                    }
                    if descendants.len() > MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE {
                        return Err(RuntimeError::LimitExceeded(format!(
                            "{context}: repeat template has more than {MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE} registered descendants"
                        )));
                    }
                }
            }
        }
        stack.extend(
            node.children
                .iter()
                .rev()
                .copied()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
    let property_lookups = descendants
        .iter()
        .filter(|descendant| descendant.property_key.is_some())
        .count();
    if property_lookups > MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM {
        return Err(RuntimeError::LimitExceeded(format!(
            "{context}: repeat template has {property_lookups} PipeWire property lookups; the limit is {MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM}"
        )));
    }
    let relation_bindings = descendants
        .iter()
        .filter(|descendant| descendant.binding.is_some_and(ItemBindingKey::is_relation))
        .count();
    if relation_bindings > MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM {
        return Err(RuntimeError::LimitExceeded(format!(
            "{context}: repeat template has {relation_bindings} PipeWire relation bindings; the limit is {MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM}"
        )));
    }
    let audio_controls = descendants
        .iter()
        .filter(|descendant| {
            descendant
                .action
                .is_some_and(|action| action.as_str().starts_with("pipewire.audio."))
        })
        .count();
    if audio_controls > MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM {
        return Err(RuntimeError::LimitExceeded(format!(
            "{context}: repeat template has {audio_controls} PipeWire audio controls; the limit is {MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM}"
        )));
    }
    let default_controls = descendants
        .iter()
        .filter(|descendant| {
            descendant
                .action
                .is_some_and(|action| action.as_str().starts_with("pipewire.defaults."))
        })
        .count();
    if default_controls > MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM {
        return Err(RuntimeError::LimitExceeded(format!(
            "{context}: repeat template has {default_controls} PipeWire preferred-default controls; the limit is {MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM}"
        )));
    }
    let range_controls = descendants
        .iter()
        .filter(|descendant| descendant.range.is_some())
        .count();
    if range_controls > MAX_RANGE_CONTROLS_PER_ITEM {
        return Err(RuntimeError::LimitExceeded(format!(
            "{context}: repeat template has {range_controls} range controls; the limit is {MAX_RANGE_CONTROLS_PER_ITEM}"
        )));
    }
    Ok(RepeatDeclaration {
        id: ElementInstanceId {
            document_generation,
            html_id: html_id.to_owned(),
        },
        source,
        template_node: identities.identity_for_slot(document, template_slot)?,
        root_node: identities.identity_for_slot(document, root)?,
        descendants,
        contextual_repeats,
        peak_monitors,
        prototype_nodes: prototype_order,
    })
}

fn valid_pipewire_property_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_PIPEWIRE_PROPERTY_KEY_BYTES
        && key.chars().all(|character| {
            !character.is_control()
                && !character.is_whitespace()
                && !matches!(character, '*' | '?' | '[' | ']' | '{' | '}' | '$')
        })
}

fn analyze_contextual_repeat(
    document: &HtmlDocument,
    template_slot: usize,
    template_prototype_order: usize,
    source: ContextualRepeatSource,
    ordinal: usize,
    context: &str,
) -> Result<ContextualRepeatDeclaration, RuntimeError> {
    let template = document
        .get_node(template_slot)
        .ok_or_else(|| invalid_declaration(context, "contextual template lookup failed"))?;
    let element = template
        .element_data()
        .ok_or_else(|| invalid_declaration(context, "contextual repeat must be an element"))?;
    if element.name.local.as_ref() != "template" {
        return Err(invalid_declaration(
            context,
            "contextual repeat requires a `<template>` element",
        ));
    }
    for attribute in element.attrs() {
        let name = attribute.name.local.as_ref();
        if name.starts_with("data-htm-") && !matches!(name, ELEMENT_ATTRIBUTE | SOURCE_ATTRIBUTE) {
            return Err(invalid_declaration(
                context,
                format!("unsupported contextual-repeat attribute `{name}`"),
            ));
        }
    }
    let mut roots = Vec::new();
    for child in &template.children {
        let node = document
            .get_node(*child)
            .ok_or_else(|| invalid_declaration(context, "contextual template child disappeared"))?;
        match &node.data {
            NodeData::Text(text) if text.content.trim().is_empty() => {}
            NodeData::Text(_) => {
                return Err(invalid_declaration(
                    context,
                    "contextual repeat top-level text must be whitespace",
                ));
            }
            NodeData::Element(_) => roots.push(*child),
            _ => {}
        }
    }
    if roots.len() != 1 {
        return Err(invalid_declaration(
            context,
            format!(
                "contextual repeat must contain exactly one root element; found {}",
                roots.len()
            ),
        ));
    }
    let root = roots[0];
    let mut descendants = Vec::new();
    let mut local_ids = BTreeSet::new();
    let mut stack = vec![(root, 1usize)];
    let mut prototype_order = 0usize;
    while let Some((slot, depth)) = stack.pop() {
        if depth > MAX_REPEAT_TEMPLATE_DEPTH {
            return Err(RuntimeError::LimitExceeded(format!(
                "{context}: contextual template depth exceeds {MAX_REPEAT_TEMPLATE_DEPTH}"
            )));
        }
        let node = document
            .get_node(slot)
            .ok_or_else(|| invalid_declaration(context, "contextual subtree disappeared"))?;
        let order = prototype_order;
        prototype_order = prototype_order.saturating_add(1);
        if let Some(element) = node.element_data() {
            if element.has_attr(local_name!("id")) {
                return Err(invalid_declaration(
                    context,
                    "normal `id` attributes are forbidden inside contextual repeats",
                ));
            }
            let local_id = element.attr(LocalName::from(LOCAL_ID_ATTRIBUTE));
            let kind_name = element.attr(LocalName::from(ELEMENT_ATTRIBUTE));
            if let Some(kind_name) = kind_name {
                let kind = BuiltInElementKind::parse(kind_name).ok_or_else(|| {
                    invalid_declaration(
                        context,
                        format!("unknown contextual built-in element `{kind_name}`"),
                    )
                })?;
                let range_allowed = source == ContextualRepeatSource::Channels;
                if !matches!(
                    kind,
                    BuiltInElementKind::StateText
                        | BuiltInElementKind::StateToken
                        | BuiltInElementKind::StateValue
                ) && !(range_allowed && kind == BuiltInElementKind::RangeControl)
                {
                    return Err(invalid_declaration(
                        context,
                        format!(
                            "`{}` is forbidden inside `{}`",
                            kind.as_str(),
                            source.as_str()
                        ),
                    ));
                }
                let definition = definition(kind);
                let tag = element.name.local.as_ref();
                if !definition.allowed_tags.contains(&tag) {
                    return Err(invalid_declaration(
                        context,
                        format!("`{}` is not allowed on <{tag}>", kind.as_str()),
                    ));
                }
                for attribute in element.attrs() {
                    let name = attribute.name.local.as_ref();
                    if name.starts_with("data-htm-")
                        && name != LOCAL_ID_ATTRIBUTE
                        && !allowed_behavior_attributes(kind).contains(&name)
                    {
                        return Err(invalid_declaration(
                            context,
                            format!("unsupported contextual behavior attribute `{name}`"),
                        ));
                    }
                }
                let local_id = local_id.filter(|value| !value.is_empty()).ok_or_else(|| {
                    invalid_declaration(
                        context,
                        "registered contextual descendant requires `data-htm-local-id`",
                    )
                })?;
                if !local_ids.insert(local_id.to_owned()) {
                    return Err(invalid_declaration(
                        context,
                        format!("duplicate contextual local id `{local_id}`"),
                    ));
                }
                let binding;
                let mut action = None;
                let mut enabled_binding = None;
                let mut range = None;
                let mut value_format = None;
                match kind {
                    BuiltInElementKind::StateText
                    | BuiltInElementKind::StateToken
                    | BuiltInElementKind::StateValue => {
                        let binding_value = element
                            .attr(LocalName::from(BIND_ATTRIBUTE))
                            .filter(|value| !value.is_empty())
                            .ok_or_else(|| {
                                invalid_declaration(
                                    context,
                                    format!("`{}` requires `data-htm-bind`", kind.as_str()),
                                )
                            })?;
                        let parsed = binding_value.parse::<ItemBindingKey>().map_err(|()| {
                            invalid_declaration(
                                context,
                                format!("unsupported contextual binding `{binding_value}`"),
                            )
                        })?;
                        if !parsed.supports_contextual(source) {
                            return Err(invalid_declaration(
                                context,
                                format!(
                                    "`{binding_value}` is not valid inside `{}`",
                                    source.as_str()
                                ),
                            ));
                        }
                        value_format = match kind {
                            BuiltInElementKind::StateText if parsed.supports_text() => {
                                validate_state_text_children(document, slot, context)?;
                                None
                            }
                            BuiltInElementKind::StateToken if parsed.supports_token() => {
                                if element.has_attr(LocalName::from(STATE_ATTRIBUTE)) {
                                    return Err(invalid_declaration(
                                        context,
                                        "`data-htm-state` is runtime-owned",
                                    ));
                                }
                                None
                            }
                            BuiltInElementKind::StateValue if parsed.supports_value() => {
                                validate_state_text_children(document, slot, context)?;
                                if element.has_attr(local_name!("value")) {
                                    return Err(invalid_declaration(
                                        context,
                                        "`value` is runtime-owned for `state-value`",
                                    ));
                                }
                                Some(parse_value_format(
                                    element.attr(LocalName::from(FORMAT_ATTRIBUTE)),
                                    item_value_formats(parsed),
                                    context,
                                )?)
                            }
                            _ => {
                                return Err(invalid_declaration(
                                    context,
                                    format!(
                                        "contextual binding `{binding_value}` does not support `{}`",
                                        kind.as_str()
                                    ),
                                ));
                            }
                        };
                        binding = Some(parsed);
                    }
                    BuiltInElementKind::RangeControl => {
                        if source != ContextualRepeatSource::Channels {
                            return Err(invalid_declaration(
                                context,
                                format!(
                                    "`range-control` is forbidden inside `{}`",
                                    source.as_str()
                                ),
                            ));
                        }
                        if element.attr(local_name!("type")) != Some("range")
                            || !node.children.is_empty()
                        {
                            return Err(invalid_declaration(
                                context,
                                "channel range control requires an empty `<input type=\"range\">`",
                            ));
                        }
                        if element.has_attr(LocalName::from(TARGET_ATTRIBUTE)) {
                            return Err(invalid_declaration(
                                context,
                                "channel range controls forbid `data-htm-target`",
                            ));
                        }
                        if element.attr(LocalName::from(BIND_ATTRIBUTE)) != Some("item.volume")
                            || element.attr(LocalName::from(ACTION_ATTRIBUTE))
                                != Some("pipewire.audio.set_channel_volume")
                            || element.attr(LocalName::from(ENABLED_BIND_ATTRIBUTE))
                                != Some("item.can_set_volume")
                        {
                            return Err(invalid_declaration(
                                context,
                                "channel range control requires `item.volume`, `pipewire.audio.set_channel_volume`, and `item.can_set_volume`",
                            ));
                        }
                        if element.has_attr(local_name!("value"))
                            || element.has_attr(LocalName::from(STATE_ATTRIBUTE))
                        {
                            return Err(invalid_declaration(
                                context,
                                "`value` and `data-htm-state` are runtime-owned for channel controls",
                            ));
                        }
                        binding = Some(ItemBindingKey::Volume);
                        action = Some(ShellAction::PipeWireAudioSetChannelVolume);
                        enabled_binding = Some(ItemBindingKey::CanSetVolume);
                        range = Some(parse_range_control(
                            element,
                            PipeWireControlTarget::CurrentItem,
                            context,
                        )?);
                    }
                    _ => unreachable!("contextual kind was checked above"),
                }
                descendants.push(RepeatedElementDeclaration {
                    local_id: local_id.to_owned(),
                    kind,
                    binding,
                    action,
                    enabled_binding,
                    range,
                    disabled: element.has_attr(local_name!("disabled")),
                    property_key: None,
                    value_format,
                    prototype_order: order,
                });
                let binding_limit = if source == ContextualRepeatSource::Channels {
                    MAX_PIPEWIRE_CHANNEL_BINDINGS_PER_ITEM
                } else {
                    MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM
                };
                if descendants.len() > binding_limit {
                    return Err(RuntimeError::LimitExceeded(format!(
                        "{context}: `{}` template exceeds {binding_limit} registered bindings",
                        source.as_str()
                    )));
                }
                let relation_bindings = descendants
                    .iter()
                    .filter(|descendant| {
                        descendant.binding.is_some_and(ItemBindingKey::is_relation)
                    })
                    .count();
                if relation_bindings > MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM {
                    return Err(RuntimeError::LimitExceeded(format!(
                        "{context}: `{}` template exceeds {MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM} relation bindings",
                        source.as_str()
                    )));
                }
            } else {
                if let Some(local_id) = local_id
                    && (local_id.is_empty() || !local_ids.insert(local_id.to_owned()))
                {
                    return Err(invalid_declaration(
                        context,
                        "static contextual local IDs must be nonempty and template-unique",
                    ));
                }
                for attribute in element.attrs() {
                    if attribute.name.local.as_ref().starts_with("data-htm-")
                        && attribute.name.local.as_ref() != LOCAL_ID_ATTRIBUTE
                    {
                        return Err(invalid_declaration(
                            context,
                            format!(
                                "unsupported contextual-template attribute `{}`",
                                attribute.name.local
                            ),
                        ));
                    }
                }
            }
        }
        stack.extend(
            node.children
                .iter()
                .rev()
                .copied()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
    let range_controls = descendants
        .iter()
        .filter(|descendant| descendant.range.is_some())
        .count();
    if range_controls > MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM {
        return Err(RuntimeError::LimitExceeded(format!(
            "{context}: channel template exceeds {MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM} range controls"
        )));
    }
    Ok(ContextualRepeatDeclaration {
        id: format!("{}-{ordinal}", source.as_str().trim_start_matches("item.")),
        source,
        template_prototype_order,
        descendants,
        prototype_nodes: prototype_order,
    })
}

fn subtree_node_count(document: &HtmlDocument, root: usize) -> Result<usize, RuntimeError> {
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(slot) = stack.pop() {
        let node = document.get_node(slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("repeat subtree disappeared".into())
        })?;
        count = count
            .checked_add(1)
            .ok_or_else(|| RuntimeError::LimitExceeded("repeat node count overflow".into()))?;
        stack.extend(node.children.iter().rev().copied());
    }
    Ok(count)
}

fn item_value_formats(binding: ItemBindingKey) -> &'static [StateValueFormat] {
    match binding {
        ItemBindingKey::Energy | ItemBindingKey::EnergyCapacity => {
            &[StateValueFormat::Raw, StateValueFormat::Energy]
        }
        ItemBindingKey::ChangeRate => &[StateValueFormat::Raw, StateValueFormat::Power],
        ItemBindingKey::TimeToEmpty | ItemBindingKey::TimeToFull => {
            &[StateValueFormat::Raw, StateValueFormat::Duration]
        }
        ItemBindingKey::Percentage | ItemBindingKey::HealthPercentage => {
            &[StateValueFormat::Raw, StateValueFormat::Percent]
        }
        ItemBindingKey::RawId
        | ItemBindingKey::ChannelCount
        | ItemBindingKey::Index
        | ItemBindingKey::SourcePortId
        | ItemBindingKey::TargetPortId
        | ItemBindingKey::SourceRawId
        | ItemBindingKey::TargetRawId
        | ItemBindingKey::MemberCount
        | ItemBindingKey::RepresentativeLinkRawId
        | ItemBindingKey::LinkGroupCount
        | ItemBindingKey::PeerRawId => &[StateValueFormat::Raw],
        ItemBindingKey::Volume | ItemBindingKey::Peak => {
            &[StateValueFormat::Raw, StateValueFormat::Percent]
        }
        _ => &[],
    }
}

fn parse_default_pipewire_target(
    value: &str,
    context: &str,
) -> Result<PipeWireControlTarget, RuntimeError> {
    match value {
        "pipewire.default_sink" => Ok(PipeWireControlTarget::DefaultSink),
        "pipewire.default_source" => Ok(PipeWireControlTarget::DefaultSource),
        _ => Err(invalid_declaration(
            context,
            format!("invalid PipeWire audio target `{value}`"),
        )),
    }
}

fn pipewire_volume_key(target: PipeWireControlTarget) -> StateBindingKey {
    match target {
        PipeWireControlTarget::DefaultSink => StateBindingKey::PipeWireDefaultSinkVolume,
        PipeWireControlTarget::DefaultSource => StateBindingKey::PipeWireDefaultSourceVolume,
        PipeWireControlTarget::CurrentItem => {
            unreachable!("current-item target has no process state key")
        }
    }
}

fn pipewire_volume_enabled_key(target: PipeWireControlTarget) -> StateBindingKey {
    match target {
        PipeWireControlTarget::DefaultSink => StateBindingKey::PipeWireDefaultSinkCanSetVolume,
        PipeWireControlTarget::DefaultSource => StateBindingKey::PipeWireDefaultSourceCanSetVolume,
        PipeWireControlTarget::CurrentItem => {
            unreachable!("current-item target has no process state key")
        }
    }
}

fn pipewire_mute_enabled_key(target: PipeWireControlTarget) -> StateBindingKey {
    match target {
        PipeWireControlTarget::DefaultSink => StateBindingKey::PipeWireDefaultSinkCanSetMute,
        PipeWireControlTarget::DefaultSource => StateBindingKey::PipeWireDefaultSourceCanSetMute,
        PipeWireControlTarget::CurrentItem => {
            unreachable!("current-item target has no process state key")
        }
    }
}

fn parse_range_control(
    element: &ElementData,
    target: PipeWireControlTarget,
    context: &str,
) -> Result<RangeControlDeclaration, RuntimeError> {
    let minimum = parse_range_number(element.attr(local_name!("min")), 0.0, "min", context)?;
    let maximum = parse_range_number(element.attr(local_name!("max")), 1.0, "max", context)?;
    let step = parse_range_number(element.attr(local_name!("step")), 0.01, "step", context)?;
    if minimum < 0.0 {
        return Err(invalid_declaration(
            context,
            "range minimum must be nonnegative",
        ));
    }
    if maximum <= minimum {
        return Err(invalid_declaration(
            context,
            "range maximum must be greater than its minimum",
        ));
    }
    if maximum > MAX_PIPEWIRE_PERCEPTUAL_VOLUME {
        return Err(invalid_declaration(
            context,
            format!("range maximum exceeds the runtime limit of {MAX_PIPEWIRE_PERCEPTUAL_VOLUME}"),
        ));
    }
    if step <= 0.0 {
        return Err(invalid_declaration(context, "range step must be positive"));
    }
    Ok(RangeControlDeclaration {
        target,
        minimum: RangeNumber::new(minimum),
        maximum: RangeNumber::new(maximum),
        step: RangeNumber::new(step),
    })
}

fn parse_range_number(
    value: Option<&str>,
    default: f64,
    name: &str,
    context: &str,
) -> Result<f64, RuntimeError> {
    let Some(value) = value else {
        return Ok(default);
    };
    if value.is_empty() || value.len() > MAX_RANGE_NUMBER_BYTES {
        return Err(invalid_declaration(
            context,
            format!("range `{name}` is empty or exceeds {MAX_RANGE_NUMBER_BYTES} bytes"),
        ));
    }
    let parsed = value.parse::<f64>().map_err(|_| {
        invalid_declaration(context, format!("range `{name}` must be a finite number"))
    })?;
    if !parsed.is_finite() {
        return Err(invalid_declaration(
            context,
            format!("range `{name}` must be finite"),
        ));
    }
    Ok(parsed)
}

fn parse_value_format(
    value: Option<&str>,
    allowed: &'static [StateValueFormat],
    context: &str,
) -> Result<StateValueFormat, RuntimeError> {
    let format = match value {
        None => StateValueFormat::Raw,
        Some("") => {
            return Err(invalid_declaration(
                context,
                "`data-htm-format` must not be empty",
            ));
        }
        Some(value) => value.parse::<StateValueFormat>().map_err(|()| {
            invalid_declaration(context, format!("unsupported numeric format `{value}`"))
        })?,
    };
    if !allowed.contains(&format) {
        return Err(invalid_declaration(
            context,
            format!(
                "numeric format `{}` is invalid for this binding",
                format.as_str()
            ),
        ));
    }
    Ok(format)
}

fn declaration_context(source: &str, id: Option<&str>) -> String {
    match id {
        Some(id) if !id.is_empty() => format!("{source} element `#{id}`"),
        _ => format!("{source} registered element"),
    }
}

fn invalid_declaration(context: &str, message: impl fmt::Display) -> RuntimeError {
    RuntimeError::InvalidPackage(format!("{context}: {message}"))
}

fn validate_state_text_children(
    document: &HtmlDocument,
    slot: usize,
    context: &str,
) -> Result<(), RuntimeError> {
    let node = document
        .get_node(slot)
        .ok_or_else(|| invalid_declaration(context, "runtime node lookup failed"))?;
    if node.children.iter().any(|child| {
        document
            .get_node(*child)
            .is_some_and(|node| node.element_data().is_some())
    }) {
        return Err(invalid_declaration(
            context,
            "state-text content must not contain child elements",
        ));
    }
    Ok(())
}

fn node_depth(document: &HtmlDocument, slot: usize) -> usize {
    let mut depth = 0usize;
    let mut current = document.get_node(slot).and_then(|node| node.parent);
    while let Some(parent) = current {
        depth = depth.saturating_add(1);
        current = document.get_node(parent).and_then(|node| node.parent);
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::HtmlProvider;
    use std::sync::Arc;

    fn discover(body: &str, kind: BuiltInSurfaceKind) -> Result<BuiltInElementIndex, RuntimeError> {
        let document = HtmlDocument::from_html(
            &format!("<!doctype html><html><body>{body}</body></html>"),
            DocumentConfig {
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        let identities = IdentityRegistry::from_document(&document);
        BuiltInElementIndex::discover(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 9 },
            kind,
            "fixture.html",
            &BTreeSet::new(),
        )
    }

    #[test]
    fn registry_is_exact_deterministic_and_duplicate_safe() {
        assert_eq!(
            built_in_registry_names(),
            &[
                "state-text",
                "action-button",
                "state-token",
                "clock-text",
                "state-value",
                "repeat",
                "range-control",
                "peak-monitor",
            ]
        );
        assert!(validate_definitions(&DEFINITIONS).is_ok());
        let duplicate = [DEFINITIONS[0], DEFINITIONS[0]];
        assert!(validate_definitions(&duplicate).is_err());
        assert_eq!(StateBindingKey::ALL.len(), 87);
        assert!(StateBindingKey::ALL.contains(&StateBindingKey::UPowerOnBattery));
        assert!(StateBindingKey::ALL.contains(&StateBindingKey::PowerProfileCurrent));
        assert_eq!(ShellAction::ALL.len(), 21);
        assert!(ShellAction::ALL.contains(&ShellAction::PowerProfileSetPerformance));
        for key in StateBindingKey::ALL {
            assert_eq!(key.as_str().parse::<StateBindingKey>(), Ok(key));
        }
        assert_eq!(
            StateBindingKey::ClockTime.scope(),
            StateBindingScope::Process
        );
        for key in [
            StateBindingKey::BatteryPercentage,
            StateBindingKey::BatteryStatus,
            StateBindingKey::BatteryWarning,
        ] {
            assert_eq!(key.scope(), StateBindingScope::Process);
        }
        assert_eq!(
            StateBindingKey::OutputLabel.scope(),
            StateBindingScope::Output
        );
        assert_eq!(
            StateBindingKey::SurfaceTemplateId.scope(),
            StateBindingScope::Surface
        );
        assert_eq!(
            StateBindingKey::SurfaceScaleProfile.scope(),
            StateBindingScope::Surface
        );
        assert!(StateBindingKey::OverlayStatus.supports(StateValueKind::Text));
        assert!(StateBindingKey::OverlayStatus.supports(StateValueKind::Token));
        assert!(StateBindingKey::BatteryPercentage.supports(StateValueKind::Text));
        assert!(!StateBindingKey::BatteryPercentage.supports(StateValueKind::Token));
        assert!(StateBindingKey::BatteryStatus.supports(StateValueKind::Text));
        assert!(StateBindingKey::BatteryStatus.supports(StateValueKind::Token));
        assert!(!StateBindingKey::BatteryWarning.supports(StateValueKind::Text));
        assert!(StateBindingKey::BatteryWarning.supports(StateValueKind::Token));
        assert!(StateBindingKey::SurfaceScaleProfile.supports(StateValueKind::Token));
        assert!(!StateBindingKey::SurfaceScaleProfile.supports(StateValueKind::Text));
        assert!(!StateBindingKey::ClockTime.supports(StateValueKind::Token));
        assert_eq!(StateToken::ALL.len(), 100);
        assert!(StateToken::ALL.contains(&StateToken::BluetoothGeneric));
        assert!(StateToken::ALL.contains(&StateToken::HighTemperature));
        assert!(StateToken::Open.valid_for(StateBindingKey::OverlayStatus));
        assert!(!StateToken::Open.valid_for(StateBindingKey::SurfaceScaleProfile));
        for token in [
            StateToken::Unavailable,
            StateToken::Absent,
            StateToken::Unknown,
            StateToken::Charging,
            StateToken::Discharging,
            StateToken::Empty,
            StateToken::Full,
            StateToken::PendingCharge,
            StateToken::PendingDischarge,
        ] {
            assert!(token.valid_for(StateBindingKey::BatteryStatus));
        }
        for token in [
            StateToken::Unknown,
            StateToken::None,
            StateToken::Discharging,
            StateToken::Low,
            StateToken::Critical,
            StateToken::Action,
        ] {
            assert!(token.valid_for(StateBindingKey::BatteryWarning));
        }
        assert!(!StateToken::Low.valid_for(StateBindingKey::BatteryStatus));
        assert!(!StateToken::Charging.valid_for(StateBindingKey::BatteryWarning));
        assert!("unknown.key".parse::<StateBindingKey>().is_err());
        for action in ShellAction::ALL {
            assert_eq!(action.as_str().parse::<ShellAction>(), Ok(action));
        }
        assert!("unknown.action".parse::<ShellAction>().is_err());
    }

    #[test]
    fn valid_declarations_are_typed_and_indexed_once() {
        let index = discover(
            r#"<span id="status" data-htm-element="state-text" data-htm-bind="overlay.status"></span>
               <button id="toggle" data-htm-element="action-button" data-htm-action="overlay.toggle"><span>Toggle</span></button>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert_eq!(
            index.summary(),
            BuiltInElementSummary {
                registered_elements: 2,
                bindings: 1,
                text_bindings: 1,
                token_bindings: 0,
                actions: 1,
                clock_declarations: 0,
                value_bindings: 0,
                boolean_bindings: 0,
                repeat_declarations: 0,
                discovery_scans: 1,
            }
        );
        assert_eq!(
            index.element("status").unwrap().binding,
            Some(StateBindingKey::OverlayStatus)
        );
        assert_eq!(
            index.element("status").unwrap().binding_kind,
            Some(StateValueKind::Text)
        );
        assert_eq!(
            index.element("toggle").unwrap().action,
            Some(ShellAction::OverlayToggle)
        );
    }

    #[test]
    fn process_clock_bindings_share_the_existing_state_text_kind() {
        let index = discover(
            r#"<span id="clock-a" data-htm-element="state-text" data-htm-bind="clock.time"></span>
               <output id="clock-b" data-htm-element="state-text" data-htm-bind="clock.time"></output>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert_eq!(
            index
                .binding_targets(StateBindingKey::ClockTime, StateValueKind::Text)
                .len(),
            2
        );
        assert_eq!(
            index.element("clock-a").unwrap().kind,
            BuiltInElementKind::StateText
        );
        assert_eq!(built_in_registry_names().len(), 8);
    }

    #[test]
    fn peak_monitors_are_explicit_typed_and_context_bounded() {
        let index = discover(
            r#"
            <section id="sink-peaks" data-htm-element="peak-monitor"
              data-htm-target="pipewire.default_sink" data-htm-enabled="true">
              <span id="status" data-htm-element="state-token"
                data-htm-bind="peak.status"></span>
              <data id="maximum" data-htm-element="state-value"
                data-htm-bind="peak.maximum" data-htm-format="percent"></data>
              <button id="toggle" data-htm-element="action-button"
                data-htm-action="pipewire.peaks.toggle">Toggle</button>
              <template data-htm-element="repeat" data-htm-source="peak.channels">
                <div data-htm-local-id="row">
                  <span data-htm-element="state-text" data-htm-local-id="name"
                    data-htm-bind="item.position_name"></span>
                  <data data-htm-element="state-value" data-htm-local-id="peak"
                    data-htm-bind="item.peak" data-htm-format="percent"></data>
                </div>
              </template>
            </section>
            <template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes">
              <article data-htm-local-id="node">
                <aside data-htm-element="peak-monitor" data-htm-local-id="monitor"
                  data-htm-enabled="false">
                  <span data-htm-element="state-text" data-htm-local-id="active"
                    data-htm-bind="peak.active"></span>
                </aside>
              </article>
            </template>
            "#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();

        let top = index.peak_monitor_declarations();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].id, "sink-peaks");
        assert_eq!(top[0].target, PeakMonitorTarget::DefaultSink);
        assert!(top[0].enabled);
        assert_eq!(top[0].channel_repeats.len(), 1);
        assert_eq!(top[0].channel_repeats[0].descendants.len(), 2);
        assert_eq!(top[0].descendants[0].binding, Some(PeakBindingKey::Status));
        assert_eq!(
            top[0].descendants[2].action,
            Some(ShellAction::PipeWirePeaksToggle)
        );

        let node_repeat = index
            .repeat_declarations()
            .into_iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireNodes)
            .unwrap();
        assert_eq!(node_repeat.peak_monitors.len(), 1);
        assert_eq!(
            node_repeat.peak_monitors[0].target,
            PeakMonitorTarget::CurrentItem
        );
        assert!(!node_repeat.peak_monitors[0].enabled);
    }

    #[test]
    fn invalid_peak_monitor_authoring_is_rejected_atomically() {
        for invalid in [
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="yes"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="{{ enabled }}"></section>"#,
            r#"<input id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true">"#,
            r#"<button id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"></button>"#,
            r#"<br id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true">"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.configured_sink" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.configured_source" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="42" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="alsa_output.card" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="sink-card" data-htm-enabled="true"></section>"#,
            r##"<section id="m" data-htm-element="peak-monitor" data-htm-target="#sink" data-htm-enabled="true"></section>"##,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target=".sink" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="{{ target }}" data-htm-enabled="true"></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><aside data-htm-element="peak-monitor" data-htm-enabled="false"></aside></section>"#,
            r#"<span id="s" data-htm-element="state-token" data-htm-bind="peak.status"></span>"#,
            r#"<template id="p" data-htm-element="repeat" data-htm-source="peak.channels"><div></div></template>"#,
            r#"<button id="a" data-htm-element="action-button" data-htm-action="pipewire.peaks.toggle">Toggle</button>"#,
            r#"<input id="a" type="range" data-htm-element="range-control" data-htm-bind="item.volume" data-htm-action="pipewire.peaks.toggle">"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><button id="a" data-htm-element="action-button" data-htm-action="pipewire.peaks.toggle" data-htm-target="pipewire.default_sink">Toggle</button></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><button id="a" data-htm-element="action-button" data-htm-action="pipewire.peaks.unknown">Toggle</button></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><template data-htm-element="repeat" data-htm-source="peak.channels"><div data-htm-local-id="row"><button data-htm-element="action-button" data-htm-local-id="a" data-htm-action="pipewire.peaks.toggle">Toggle</button></div></template></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><template data-htm-element="repeat" data-htm-source="peak.channels"><div data-htm-local-id="row"><input type="range" data-htm-element="range-control" data-htm-local-id="range" data-htm-bind="item.peak" data-htm-action="pipewire.audio.set_volume"></div></template></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><template data-htm-element="repeat" data-htm-source="peak.channels"><div id="row"></div></template></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><template data-htm-element="repeat" data-htm-source="peak.channels"><div data-htm-local-id="row"><span data-htm-element="state-text" data-htm-local-id="volume" data-htm-bind="item.volume"></span></div></template></section>"#,
            r#"<section id="m" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"><template data-htm-element="repeat" data-htm-source="peak.channels"><div data-htm-local-id="row"><template data-htm-element="repeat" data-htm-source="peak.channels"><div data-htm-local-id="nested"></div></template></div></template></section>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article data-htm-local-id="node"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true" data-htm-target="pipewire.default_sink"></section></article></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article data-htm-local-id="node"><input data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true"></article></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article data-htm-local-id="node"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true" data-htm-action="pipewire.peaks.toggle"></section></article></template>"#,
            r#"<template id="channels" data-htm-element="repeat" data-htm-source="upower.devices"><article data-htm-local-id="device"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true"></section></article></template>"#,
            r#"<template id="holds" data-htm-element="repeat" data-htm-source="power_profile.holds"><article data-htm-local-id="hold"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true"></section></article></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article data-htm-local-id="node"><template data-htm-element="repeat" data-htm-source="item.channels"><div data-htm-local-id="channel"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true"></section></div></template></article></template>"#,
            r#"<template id="links" data-htm-element="repeat" data-htm-source="pipewire.links"><article data-htm-local-id="link"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true"></section></article></template>"#,
            r#"<template id="groups" data-htm-element="repeat" data-htm-source="pipewire.link_groups"><article data-htm-local-id="group"><section data-htm-element="peak-monitor" data-htm-local-id="m" data-htm-enabled="true"></section></article></template>"#,
        ] {
            assert!(
                discover(invalid, BuiltInSurfaceKind::Overlay).is_err(),
                "{invalid}"
            );
        }

        let excessive = (0..=MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT)
            .map(|index| {
                format!(
                    r#"<section id="m-{index}" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="false"></section>"#
                )
            })
            .collect::<String>();
        assert!(discover(&excessive, BuiltInSurfaceKind::Overlay).is_err());

        let excessive_enabled = (0..=MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT)
            .map(|index| {
                format!(
                    r#"<section id="m-{index}" data-htm-element="peak-monitor" data-htm-target="pipewire.default_sink" data-htm-enabled="true"></section>"#
                )
            })
            .collect::<String>();
        assert!(discover(&excessive_enabled, BuiltInSurfaceKind::Overlay).is_err());
    }

    #[test]
    fn standard_template_repeat_is_analyzed_once_with_scoped_descendants() {
        let index = discover(
            r#"<ul>
              <template id="device-row" data-htm-element="repeat" data-htm-source="upower.devices">
                <li class="device">
                  <span data-htm-element="state-text" data-htm-local-id="model" data-htm-bind="item.model"></span>
                  <data data-htm-element="state-value" data-htm-local-id="percentage" data-htm-bind="item.percentage" data-htm-format="percent"></data>
                </li>
              </template>
            </ul>"#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();
        let repeats = index.repeat_declarations();
        assert_eq!(repeats.len(), 1);
        assert_eq!(repeats[0].source, RepeatSource::UPowerDevices);
        assert_eq!(repeats[0].descendants.len(), 2);
        assert!(repeats[0].prototype_nodes >= 3);
        assert_eq!(index.summary().repeat_declarations, 1);

        for invalid in [
            r#"<div id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><span></span></div>"#,
            r#"<template data-htm-element="repeat" data-htm-source="upower.devices"><span></span></template>"#,
            r#"<template id="rows" data-htm-element="repeat"><span></span></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="unknown"><span></span></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices">text<span></span></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li id="duplicate"></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li><span data-htm-element="state-text" data-htm-bind="item.model"></span></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li><span data-htm-element="state-text" data-htm-local-id="same" data-htm-bind="item.model"></span><span data-htm-element="state-token" data-htm-local-id="same" data-htm-bind="item.state"></span></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li><span data-htm-element="state-text" data-htm-local-id="reason" data-htm-bind="item.reason"></span></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li><button data-htm-element="action-button" data-htm-local-id="action" data-htm-action="overlay.close"></button></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li><time data-htm-element="clock-text" data-htm-local-id="clock" data-htm-format="%H"></time></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li></li><li></li></template>"#,
            r#"<template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><li><template data-htm-element="repeat" data-htm-source="upower.devices"><span></span></template></li></template>"#,
            r#"<span data-htm-local-id="outside">plain</span>"#,
        ] {
            assert!(discover(invalid, BuiltInSurfaceKind::Overlay).is_err());
        }
    }

    #[test]
    fn pipewire_repeat_and_exact_property_lookup_are_narrowly_validated() {
        let index = discover(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes">
                 <div>
                   <span data-htm-element="state-text" data-htm-local-id="name" data-htm-bind="item.name"></span>
                   <span data-htm-element="state-token" data-htm-local-id="type" data-htm-bind="item.node_type"></span>
                   <data data-htm-element="state-value" data-htm-local-id="raw" data-htm-bind="item.raw_id"></data>
                   <span data-htm-element="state-text" data-htm-local-id="application"
                         data-htm-bind="item.property"
                         data-htm-property-key="application.name"></span>
                 </div>
               </template>"#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();
        let repeat = &index.repeat_declarations()[0];
        assert_eq!(repeat.source, RepeatSource::PipeWireNodes);
        assert_eq!(
            repeat
                .descendants
                .iter()
                .find(|element| element.binding == Some(ItemBindingKey::Property))
                .and_then(|element| element.property_key.as_deref()),
            Some("application.name")
        );

        for invalid in [
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="property" data-htm-bind="item.property"></span></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="property" data-htm-bind="item.property" data-htm-property-key=""></span></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="property" data-htm-bind="item.property" data-htm-property-key="application.*"></span></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="property" data-htm-bind="item.property" data-htm-property-key="${dynamic}"></span></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="name" data-htm-bind="item.name" data-htm-property-key="node.name"></span></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="model" data-htm-bind="item.model"></span></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><button data-htm-element="action-button" data-htm-local-id="action" data-htm-action="overlay.close"></button></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><time data-htm-element="clock-text" data-htm-local-id="clock" data-htm-format="%H"></time></div></template>"#,
        ] {
            assert!(
                discover(invalid, BuiltInSurfaceKind::Overlay).is_err(),
                "{invalid}"
            );
        }

        let long_key = "k".repeat(MAX_PIPEWIRE_PROPERTY_KEY_BYTES + 1);
        let overlong = format!(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><span data-htm-element="state-text" data-htm-local-id="property" data-htm-bind="item.property" data-htm-property-key="{long_key}"></span></div></template>"#
        );
        assert!(discover(&overlong, BuiltInSurfaceKind::Overlay).is_err());

        let bindings = (0..=MAX_PIPEWIRE_BINDINGS_PER_ITEM)
            .map(|index| {
                format!(
                    r#"<span data-htm-element="state-text" data-htm-local-id="name-{index}" data-htm-bind="item.name"></span>"#
                )
            })
            .collect::<String>();
        let excessive_bindings = format!(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div>{bindings}</div></template>"#
        );
        assert!(discover(&excessive_bindings, BuiltInSurfaceKind::Overlay).is_err());

        let property_lookups = (0..=MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM)
            .map(|index| {
                format!(
                    r#"<span data-htm-element="state-text" data-htm-local-id="property-{index}" data-htm-bind="item.property" data-htm-property-key="property.{index}"></span>"#
                )
            })
            .collect::<String>();
        let excessive_lookups = format!(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div>{property_lookups}</div></template>"#
        );
        assert!(discover(&excessive_lookups, BuiltInSurfaceKind::Overlay).is_err());

        let unique_keys = (0..3)
            .map(|repeat| {
                let descendants = (0..22)
                    .map(|index| {
                        format!(
                            r#"<span data-htm-element="state-text" data-htm-local-id="property-{index}" data-htm-bind="item.property" data-htm-property-key="property.{repeat}.{index}"></span>"#
                        )
                    })
                    .collect::<String>();
                format!(
                    r#"<template id="nodes-{repeat}" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div>{descendants}</div></template>"#
                )
            })
            .collect::<String>();
        assert!(discover(&unique_keys, BuiltInSurfaceKind::Overlay).is_err());

        let repeats = (0..=MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT)
            .map(|index| {
                format!(
                    r#"<template id="nodes-{index}" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div></div></template>"#
                )
            })
            .collect::<String>();
        assert!(discover(&repeats, BuiltInSurfaceKind::Overlay).is_err());
    }

    #[test]
    fn pipewire_audio_controls_are_narrowly_typed_and_bounded() {
        let index = discover(
            r#"<input id="sink-volume" type="range"
                    data-htm-element="range-control"
                    data-htm-bind="pipewire.default_sink.volume"
                    data-htm-action="pipewire.audio.set_volume"
                    data-htm-target="pipewire.default_sink"
                    data-htm-enabled-bind="pipewire.default_sink.can_set_volume">
               <button id="source-mute"
                    data-htm-element="action-button"
                    data-htm-action="pipewire.audio.toggle_mute"
                    data-htm-target="pipewire.default_source"
                    data-htm-enabled-bind="pipewire.default_source.can_set_mute">Mute</button>
               <template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes">
                 <div>
                   <input type="range"
                          data-htm-element="range-control"
                          data-htm-local-id="volume"
                          data-htm-bind="item.volume"
                          data-htm-action="pipewire.audio.set_volume"
                          data-htm-enabled-bind="item.can_set_volume"
                          min="0" max="2" step="0.05">
                   <button data-htm-element="action-button"
                           data-htm-local-id="mute"
                           data-htm-action="pipewire.audio.mute"
                           data-htm-enabled-bind="item.can_set_mute">Mute</button>
                 </div>
               </template>"#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();
        let sink = index.element("sink-volume").unwrap();
        assert_eq!(sink.kind, BuiltInElementKind::RangeControl);
        assert_eq!(
            sink.pipewire_target,
            Some(PipeWireControlTarget::DefaultSink)
        );
        let range = sink.range.unwrap();
        assert_eq!(range.minimum.get(), 0.0);
        assert_eq!(range.maximum.get(), 1.0);
        assert_eq!(range.step.get(), 0.01);
        let repeated = &index.repeat_declarations()[0].descendants;
        let repeated_range = repeated
            .iter()
            .find(|element| element.local_id == "volume")
            .and_then(|element| element.range)
            .unwrap();
        assert_eq!(repeated_range.maximum.get(), 2.0);
        assert_eq!(
            repeated
                .iter()
                .find(|element| element.local_id == "mute")
                .and_then(|element| element.action),
            Some(ShellAction::PipeWireAudioMute)
        );

        for invalid in [
            r#"<div id="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume"></div>"#,
            r#"<input id="range" type="text" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.toggle_mute" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_source" data-htm-enabled-bind="pipewire.default_source.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.configured_sink" data-htm-enabled-bind="pipewire.configured_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="42" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target=".sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume" min="-1">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume" min="NaN">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume" max="0">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume" max="2.01">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume" step="0">"#,
            r#"<input id="range" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume" step="${step}">"#,
            r#"<button id="set" data-htm-element="action-button" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink"></button>"#,
            r#"<button id="mute" data-htm-element="action-button" data-htm-action="pipewire.audio.toggle_mute" data-htm-enabled-bind="pipewire.default_sink.can_set_mute"></button>"#,
            r#"<button id="mute" data-htm-element="action-button" data-htm-action="pipewire.audio.toggle_mute" data-htm-target="pipewire.configured_sink" data-htm-enabled-bind="pipewire.configured_sink.can_set_mute"></button>"#,
            r#"<template id="devices" data-htm-element="repeat" data-htm-source="upower.devices"><div><input type="range" data-htm-element="range-control" data-htm-local-id="volume" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_volume" data-htm-enabled-bind="item.can_set_volume"></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><input type="range" data-htm-element="range-control" data-htm-local-id="volume" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_volume" data-htm-target="pipewire.default_sink" data-htm-enabled-bind="item.can_set_volume"></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><button data-htm-element="action-button" data-htm-local-id="mute" data-htm-action="overlay.close"></button></div></template>"#,
        ] {
            assert!(
                discover(invalid, BuiltInSurfaceKind::Overlay).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn pipewire_preferred_default_actions_are_item_local_typed_and_bounded() {
        let valid = discover(
            r#"
              <span id="clearable-sink" data-htm-element="state-token" data-htm-bind="pipewire.configured_sink.can_clear"></span>
              <button id="clear-sink" data-htm-element="action-button" data-htm-action="pipewire.defaults.clear_preferred_sink" data-htm-enabled-bind="pipewire.configured_sink.can_clear"></button>
              <button id="clear-source" data-htm-element="action-button" data-htm-action="pipewire.defaults.clear_preferred_source" data-htm-enabled-bind="pipewire.configured_source.can_clear"></button>
              <template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes">
                <article>
                  <span data-htm-local-id="sink-capability" data-htm-element="state-token" data-htm-bind="item.can_set_preferred_sink"></span>
                  <span data-htm-local-id="source-capability" data-htm-element="state-token" data-htm-bind="item.can_set_preferred_source"></span>
                  <button data-htm-local-id="set-sink" data-htm-element="action-button" data-htm-action="pipewire.defaults.set_preferred_sink" data-htm-enabled-bind="item.can_set_preferred_sink"></button>
                  <button data-htm-local-id="set-source" data-htm-element="action-button" data-htm-action="pipewire.defaults.set_preferred_source" data-htm-enabled-bind="item.can_set_preferred_source"></button>
                </article>
              </template>
            "#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();
        assert_eq!(
            valid.element("clear-sink").unwrap().action,
            Some(ShellAction::PipeWireDefaultsClearPreferredSink)
        );
        assert_eq!(
            valid
                .element("nodes")
                .unwrap()
                .repeat
                .as_ref()
                .unwrap()
                .descendants
                .iter()
                .filter(|descendant| {
                    descendant
                        .action
                        .is_some_and(|action| action.as_str().starts_with("pipewire.defaults."))
                })
                .count(),
            2
        );

        for invalid in [
            r#"<button id="set" data-htm-element="action-button" data-htm-action="pipewire.defaults.set_preferred_sink" data-htm-enabled-bind="pipewire.configured_sink.can_clear"></button>"#,
            r#"<button id="clear" data-htm-element="action-button" data-htm-action="pipewire.defaults.clear_preferred_sink"></button>"#,
            r#"<button id="clear" data-htm-element="action-button" data-htm-action="pipewire.defaults.clear_preferred_sink" data-htm-enabled-bind="pipewire.configured_sink.can_clear" data-htm-target="42"></button>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article><button data-htm-local-id="set" data-htm-element="action-button" data-htm-action="pipewire.defaults.set_preferred_sink" data-htm-enabled-bind="item.can_set_preferred_source"></button></article></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article><button data-htm-local-id="clear" data-htm-element="action-button" data-htm-action="pipewire.defaults.clear_preferred_sink" data-htm-enabled-bind="item.can_set_preferred_sink"></button></article></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article><button data-htm-local-id="set" data-htm-element="action-button" data-htm-action="pipewire.defaults.set_preferred_sink" data-htm-enabled-bind="item.can_set_preferred_sink" data-htm-target="pipewire.default_sink"></button></article></template>"#,
            r#"<input id="set" type="range" data-htm-element="range-control" data-htm-bind="pipewire.default_sink.volume" data-htm-action="pipewire.defaults.set_preferred_sink" data-htm-enabled-bind="pipewire.default_sink.can_set_volume">"#,
        ] {
            assert!(discover(invalid, BuiltInSurfaceKind::Overlay).is_err());
        }

        let excessive = (0..=MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM)
            .map(|index| {
                format!(
                    r#"<button data-htm-local-id="set-{index}" data-htm-element="action-button" data-htm-action="pipewire.defaults.set_preferred_sink" data-htm-enabled-bind="item.can_set_preferred_sink"></button>"#
                )
            })
            .collect::<String>();
        let excessive = format!(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><article>{excessive}</article></template>"#
        );
        assert!(discover(&excessive, BuiltInSurfaceKind::Overlay).is_err());
    }

    #[test]
    fn pipewire_channel_context_is_one_level_and_narrowly_typed() {
        let index = discover(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes">
                 <article>
                   <data data-htm-element="state-value"
                         data-htm-local-id="channel-count"
                         data-htm-bind="item.channel_count"></data>
                   <template data-htm-element="repeat" data-htm-source="item.channels">
                     <div data-htm-local-id="channel">
                       <span data-htm-element="state-text"
                             data-htm-local-id="position-name"
                             data-htm-bind="item.position_name"></span>
                       <span data-htm-element="state-token"
                             data-htm-local-id="position"
                             data-htm-bind="item.position"></span>
                       <data data-htm-element="state-value"
                             data-htm-local-id="index"
                             data-htm-bind="item.index"></data>
                       <input type="range"
                              data-htm-element="range-control"
                              data-htm-local-id="volume"
                              data-htm-bind="item.volume"
                              data-htm-action="pipewire.audio.set_channel_volume"
                              data-htm-enabled-bind="item.can_set_volume">
                     </div>
                   </template>
                 </article>
               </template>"#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();
        let declaration = &index.repeat_declarations()[0];
        assert_eq!(declaration.contextual_repeats.len(), 1);
        assert_eq!(
            declaration.contextual_repeats[0]
                .descendants
                .iter()
                .find(|element| element.local_id == "volume")
                .and_then(|element| element.action),
            Some(ShellAction::PipeWireAudioSetChannelVolume)
        );

        for invalid in [
            r#"<template id="channels" data-htm-element="repeat" data-htm-source="item.channels"><div></div></template>"#,
            r#"<template id="devices" data-htm-element="repeat" data-htm-source="upower.devices"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div></div></template></div></template>"#,
            r#"<template id="holds" data-htm-element="repeat" data-htm-source="power_profile.holds"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.other"><div></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div></div></template></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div id="channel"></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><button data-htm-element="action-button" data-htm-local-id="action" data-htm-action="pipewire.audio.toggle_mute"></button></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><time data-htm-element="clock-text" data-htm-local-id="clock" data-htm-format="%H"></time></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><span data-htm-element="state-text" data-htm-local-id="parent" data-htm-bind="parent.name"></span></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><input type="range" data-htm-element="range-control" data-htm-local-id="volume" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_channel_volume" data-htm-enabled-bind="item.can_set_volume" data-htm-target="0"></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><input type="range" data-htm-element="range-control" data-htm-local-id="volume" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_channel_volume" data-htm-enabled-bind="item.can_set_volume"></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="{{item.channels}}"><div></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><input type="range" data-htm-element="range-control" data-htm-local-id="volume" data-htm-bind="item.volume" data-htm-enabled-bind="item.can_set_volume"></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><input type="range" data-htm-element="range-control" data-htm-local-id="volume" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_volume" data-htm-enabled-bind="item.can_set_volume"></div></template></div></template>"#,
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div><button data-htm-element="action-button" data-htm-local-id="volume" data-htm-action="pipewire.audio.set_channel_volume"></button></div></template></div></template>"#,
        ] {
            assert!(
                discover(invalid, BuiltInSurfaceKind::Overlay).is_err(),
                "{invalid}"
            );
        }

        let repeats = (0..=MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE)
            .map(|_| {
                r#"<template data-htm-element="repeat" data-htm-source="item.channels"><div></div></template>"#
            })
            .collect::<String>();
        let excessive = format!(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div>{repeats}</div></template>"#
        );
        assert!(discover(&excessive, BuiltInSurfaceKind::Overlay).is_err());

        let ranges = (0..=MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM)
            .map(|index| {
                format!(
                    r#"<input type="range" data-htm-element="range-control" data-htm-local-id="volume-{index}" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_channel_volume" data-htm-enabled-bind="item.can_set_volume">"#
                )
            })
            .collect::<String>();
        let excessive = format!(
            r#"<template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.channels"><div>{ranges}</div></template></div></template>"#
        );
        assert!(discover(&excessive, BuiltInSurfaceKind::Overlay).is_err());

        let valid_ranges = (0..MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM)
            .map(|index| {
                format!(
                    r#"<input type="range" data-htm-element="range-control" data-htm-local-id="volume-{index}" data-htm-bind="item.volume" data-htm-action="pipewire.audio.set_channel_volume" data-htm-enabled-bind="item.can_set_volume">"#
                )
            })
            .collect::<String>();
        let valid_contextuals = (0..MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE)
            .map(|_| {
                format!(
                    r#"<template data-htm-element="repeat" data-htm-source="item.channels"><div>{valid_ranges}</div></template>"#
                )
            })
            .collect::<String>();
        let excessive = (0..5)
            .map(|index| {
                format!(
                    r#"<template id="nodes-{index}" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div>{valid_contextuals}</div></template>"#
                )
            })
            .collect::<String>();
        assert!(discover(&excessive, BuiltInSurfaceKind::Overlay).is_err());
    }

    #[test]
    fn pipewire_graph_repeats_relations_and_contexts_are_narrowly_typed() {
        let index = discover(
            r#"
            <data id="links-count" data-htm-element="state-value" data-htm-bind="pipewire.link_count"></data>
            <data id="groups-count" data-htm-element="state-value" data-htm-bind="pipewire.link_group_count"></data>
            <template id="links" data-htm-element="repeat" data-htm-source="pipewire.links">
              <article>
                <span data-htm-element="state-token" data-htm-local-id="state" data-htm-bind="item.state"></span>
                <span data-htm-element="state-text" data-htm-local-id="source" data-htm-bind="item.source.description"></span>
                <data data-htm-element="state-value" data-htm-local-id="port" data-htm-bind="item.source_port_id"></data>
              </article>
            </template>
            <template id="groups" data-htm-element="repeat" data-htm-source="pipewire.link_groups">
              <article>
                <data data-htm-element="state-value" data-htm-local-id="members" data-htm-bind="item.member_count"></data>
                <template data-htm-element="repeat" data-htm-source="item.links">
                  <div>
                    <span data-htm-element="state-text" data-htm-local-id="target" data-htm-bind="item.target.name"></span>
                  </div>
                </template>
              </article>
            </template>
            <template id="nodes" data-htm-element="repeat" data-htm-source="pipewire.nodes">
              <article>
                <data data-htm-element="state-value" data-htm-local-id="tracked" data-htm-bind="item.link_group_count"></data>
                <template data-htm-element="repeat" data-htm-source="item.link_groups">
                  <div>
                    <span data-htm-element="state-token" data-htm-local-id="direction" data-htm-bind="item.connection_direction"></span>
                    <span data-htm-element="state-text" data-htm-local-id="peer" data-htm-bind="item.peer.description"></span>
                  </div>
                </template>
              </article>
            </template>
            "#,
            BuiltInSurfaceKind::Overlay,
        )
        .unwrap();
        let declarations = index.repeat_declarations();
        assert_eq!(
            declarations
                .iter()
                .map(|repeat| repeat.source)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                RepeatSource::PipeWireNodes,
                RepeatSource::PipeWireLinks,
                RepeatSource::PipeWireLinkGroups,
            ])
        );
        let groups = declarations
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireLinkGroups)
            .unwrap();
        assert_eq!(
            groups.contextual_repeats[0].source,
            ContextualRepeatSource::GroupLinks
        );
        let nodes = declarations
            .iter()
            .find(|repeat| repeat.source == RepeatSource::PipeWireNodes)
            .unwrap();
        assert_eq!(
            nodes.contextual_repeats[0].source,
            ContextualRepeatSource::NodeLinkGroups
        );

        for invalid in [
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="item.links"><div></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="item.link_groups"><div></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.nodes"><div><template data-htm-element="repeat" data-htm-source="item.links"><div></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.link_groups"><div><template data-htm-element="repeat" data-htm-source="item.link_groups"><div></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><template data-htm-element="repeat" data-htm-source="item.links"><div></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="upower.devices"><div><template data-htm-element="repeat" data-htm-source="item.link_groups"><div></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="power_profile.holds"><div><template data-htm-element="repeat" data-htm-source="item.link_groups"><div></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><button data-htm-element="action-button" data-htm-local-id="action" data-htm-action="overlay.close"></button></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.link_groups"><div><template data-htm-element="repeat" data-htm-source="item.links"><div><template data-htm-element="repeat" data-htm-source="item.links"><div></div></template></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><span data-htm-element="state-text" data-htm-local-id="bad" data-htm-bind="item.source.parent.name"></span></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><span data-htm-element="state-text" data-htm-local-id="bad" data-htm-bind="../source.name"></span></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><span data-htm-element="state-text" data-htm-local-id="bad" data-htm-bind="item.source[0].name"></span></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><span data-htm-element="state-text" data-htm-local-id="bad" data-htm-bind="item.source.unknown"></span></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.link_groups"><div><span data-htm-element="state-text" data-htm-local-id="bad" data-htm-bind="item.peer.name"></span></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.link_groups"><div><template data-htm-element="repeat" data-htm-source="item.links"><div id="normal"></div></template></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div id="normal"></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><input type="range" data-htm-element="range-control" data-htm-local-id="range" data-htm-bind="item.state"></div></template>"#,
            r#"<template id="bad" data-htm-element="repeat" data-htm-source="pipewire.links"><div><time data-htm-element="clock-text" data-htm-local-id="clock" data-htm-format="%H"></time></div></template>"#,
        ] {
            assert!(
                discover(invalid, BuiltInSurfaceKind::Overlay).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn clock_text_declarations_are_typed_once_and_targets_are_generation_safe() {
        let index = discover(
            r#"<time id="local" class="clock" aria-label="Local time"
                    data-htm-element="clock-text" data-htm-format="%H:%M"></time>
               <time id="tokyo" data-htm-element="clock-text"
                    data-htm-format="%I:%M:%S %p" data-htm-time-zone="Asia/Tokyo"
                    data-htm-enabled="false"></time>
               <button id="enable" data-htm-element="action-button"
                    data-htm-action="clock.enable" data-htm-target="tokyo">Enable</button>
               <button id="disable" data-htm-element="action-button"
                    data-htm-action="clock.disable" data-htm-target="tokyo">Disable</button>
               <button id="toggle" data-htm-element="action-button"
                    data-htm-action="clock.toggle" data-htm-target="tokyo">Toggle</button>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        let declarations = index.clock_declarations();
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].format.source(), "%H:%M");
        assert_eq!(declarations[0].time_zone, ClockTimeZone::Local);
        assert!(declarations[0].enabled);
        assert_eq!(declarations[1].time_zone.declaration_value(), "Asia/Tokyo");
        assert!(!declarations[1].enabled);
        assert_eq!(index.summary().clock_declarations, 2);
        assert_eq!(index.summary().discovery_scans, 1);
        for id in ["enable", "disable", "toggle"] {
            let action = index.element(id).unwrap();
            assert_eq!(
                action.action_target.as_ref().unwrap().document_generation,
                ExperimentalDocumentIdentity { serial: 9 }
            );
            assert_eq!(action.action_target.as_ref().unwrap().html_id, "tokyo");
        }
    }

    #[test]
    fn invalid_clock_declarations_and_targets_are_rejected_atomically() {
        for body in [
            r#"<span id="clock" data-htm-element="clock-text" data-htm-format="%H"></span>"#,
            r#"<time data-htm-element="clock-text" data-htm-format="%H"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format=""></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%s"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-enabled="TRUE"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-enabled=""></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-time-zone="../UTC"></time>"#,
            r#"<time id="clock" datetime="2026-01-01T00:00:00Z" data-htm-element="clock-text" data-htm-format="%H"></time>"#,
            r#"<time id="clock" data-htm-state="enabled" data-htm-element="clock-text" data-htm-format="%H"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-bind="clock.time"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-action="clock.toggle"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-target="other"></time>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle"></button>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle" data-htm-target=""></button>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle" data-htm-target="missing"></button>"#,
            r#"<span id="target" data-htm-element="state-text" data-htm-bind="clock.time"></span><button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle" data-htm-target="target"></button>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="overlay.toggle" data-htm-target="clock"></button><time id="clock" data-htm-element="clock-text" data-htm-format="%H"></time>"#,
        ] {
            assert!(discover(body, BuiltInSurfaceKind::Panel).is_err(), "{body}");
        }
        let excessive: String = (0..=MAX_CLOCK_DECLARATIONS_PER_DOCUMENT)
            .map(|index| {
                format!(
                    r#"<time id="clock-{index}" data-htm-element="clock-text" data-htm-format="%H"></time>"#
                )
            })
            .collect();
        assert!(discover(&excessive, BuiltInSurfaceKind::Panel).is_err());
    }

    #[test]
    fn state_tokens_are_typed_indexed_and_limited_to_visual_wrappers() {
        let index = discover(
            r#"<span id="status" class="indicator" data-extra="kept"
                    data-htm-element="state-token" data-htm-bind="overlay.status"></span>
               <section id="scale" data-htm-element="state-token"
                    data-htm-bind="surface.scale_profile"></section>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert_eq!(
            index.binding_targets(StateBindingKey::OverlayStatus, StateValueKind::Token),
            &["status"]
        );
        assert_eq!(
            index.element("status").unwrap().binding_kind,
            Some(StateValueKind::Token)
        );
        assert_eq!(index.summary().text_bindings, 0);
        assert_eq!(index.summary().token_bindings, 2);
        for tag in ["div", "span", "section"] {
            assert!(
                discover(
                    &format!(
                        r#"<{tag} id="token" data-htm-element="state-token" data-htm-bind="overlay.status"></{tag}>"#
                    ),
                    BuiltInSurfaceKind::Panel,
                )
                .is_ok()
            );
        }
        for tag in ["button", "img", "svg", "input"] {
            assert!(
                discover(
                    &format!(
                        r#"<{tag} id="token" data-htm-element="state-token" data-htm-bind="overlay.status"></{tag}>"#
                    ),
                    BuiltInSurfaceKind::Panel,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn invalid_declarations_are_rejected_without_affecting_plain_html() {
        for body in [
            r#"<span data-htm-element="state-text" data-htm-bind="overlay.status"></span>"#,
            r#"<span id="same" data-htm-element="state-text" data-htm-bind="overlay.status"></span><span id="same" data-htm-element="state-text" data-htm-bind="output.label"></span>"#,
            r#"<div id="same"></div><span id="same" data-htm-element="state-text" data-htm-bind="overlay.status"></span>"#,
            r#"<span id="x" data-htm-element="unknown" data-htm-bind="overlay.status"></span>"#,
            r#"<span id="x" data-htm-element="action-button" data-htm-action="overlay.toggle"></span>"#,
            r#"<span id="x" data-htm-element="state-text"></span>"#,
            r#"<button id="x" data-htm-element="action-button"></button>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="unknown.key"></span>"#,
            r#"<button id="x" data-htm-element="action-button" data-htm-action="unknown.action"></button>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="overlay.status" data-htm-action="overlay.close"></span>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="overlay.status"><b>nested</b></span>"#,
            r#"<span id="x" data-htm-element="state-token"></span>"#,
            r#"<span id="x" data-htm-element="state-token" data-htm-bind="clock.time"></span>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="surface.scale_profile"></span>"#,
            r#"<span id="x" data-htm-element="state-token" data-htm-bind="overlay.status" data-htm-state="open"></span>"#,
        ] {
            assert!(discover(body, BuiltInSurfaceKind::Panel).is_err(), "{body}");
        }
        let plain = discover(
            r#"<div data-example="allowed">ordinary</div>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert!(plain.is_empty());
    }

    #[test]
    fn action_sources_are_validated() {
        assert!(discover(
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="overlay.toggle"></button>"#,
            BuiltInSurfaceKind::Panel,
        ).is_ok());
        assert!(discover(
            r#"<button id="close" data-htm-element="action-button" data-htm-action="overlay.close"></button>"#,
            BuiltInSurfaceKind::Overlay,
        ).is_ok());
        assert!(discover(
            r#"<button id="activate" data-htm-element="action-button" data-htm-action="overlay.activate"></button>"#,
            BuiltInSurfaceKind::Overlay,
        ).is_ok());
        for action in [
            "power_profile.set_power_saver",
            "power_profile.set_balanced",
            "power_profile.set_performance",
        ] {
            assert!(discover(
                &format!(
                    r#"<button id="profile" data-htm-element="action-button" data-htm-action="{action}" data-htm-enabled-bind="power_profile.availability"></button>"#
                ),
                BuiltInSurfaceKind::Panel,
            ).is_ok());
        }
        assert!(discover(
            r#"<button id="profile" data-htm-element="action-button" data-htm-action="power_profile.set_balanced" data-htm-target="clock"></button>"#,
            BuiltInSurfaceKind::Panel,
        ).is_err());
        assert!(discover(
            r#"<button id="profile" data-htm-element="action-button" data-htm-action="power_profile.set_balanced" data-htm-enabled-bind="battery.status"></button>"#,
            BuiltInSurfaceKind::Panel,
        ).is_err());
        assert!(discover(
            r#"<button id="close" data-htm-element="action-button" data-htm-action="overlay.close"></button>"#,
            BuiltInSurfaceKind::Panel,
        ).is_err());
        let disabled = discover(
            r#"<button id="toggle" disabled="false" data-htm-element="action-button" data-htm-action="overlay.toggle"></button>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert!(disabled.element("toggle").unwrap().disabled);
    }
}
