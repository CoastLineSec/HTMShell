use dbus::{
    Message, MessageType,
    arg::{self, PropMap, Variant},
    channel::{BusType, Channel, Watch},
};
use htm_runtime::{
    ItemBindingKey, MAX_POWER_PROFILE_HOLDS_PER_PROCESS, MAX_UPOWER_DEVICES_PER_PROCESS,
    NumericValue, RepeatItemSnapshot, RepeatSource, RepeatSourceSnapshot, StateBindingKey,
    StateToken,
};
use rustix::fd::{BorrowedFd, OwnedFd};
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
    timerfd_settime,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const UPOWER_INTERFACE: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
const PROFILE_SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILE_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const PROFILE_INTERFACE: &str = "org.freedesktop.UPower.PowerProfiles";
const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MESSAGES_PER_DISPATCH: usize = 64;
const MAX_SERVICE_STRING_BYTES: usize = 1024;
const RECONNECT_DELAYS: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryAvailability {
    Unavailable,
    Absent,
    Present,
}

impl BatteryAvailability {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Absent => "absent",
            Self::Present => "present",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryWarning {
    Unknown,
    None,
    Discharging,
    Low,
    Critical,
    Action,
}

impl BatteryWarning {
    const fn from_wire(value: Option<u32>) -> Self {
        match value {
            Some(1) => Self::None,
            Some(2) => Self::Discharging,
            Some(3) => Self::Low,
            Some(4) => Self::Critical,
            Some(5) => Self::Action,
            _ => Self::Unknown,
        }
    }

    const fn token(self) -> StateToken {
        match self {
            Self::Unknown => StateToken::Unknown,
            Self::None => StateToken::None,
            Self::Discharging => StateToken::Discharging,
            Self::Low => StateToken::Low,
            Self::Critical => StateToken::Critical,
            Self::Action => StateToken::Action,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum UPowerDeviceType {
    Unknown = 0,
    LinePower = 1,
    Battery = 2,
    Ups = 3,
    Monitor = 4,
    Mouse = 5,
    Keyboard = 6,
    Pda = 7,
    Phone = 8,
    MediaPlayer = 9,
    Tablet = 10,
    Computer = 11,
    GamingInput = 12,
    Pen = 13,
    Touchpad = 14,
    Modem = 15,
    Network = 16,
    Headset = 17,
    Speakers = 18,
    Headphones = 19,
    Video = 20,
    OtherAudio = 21,
    RemoteControl = 22,
    Printer = 23,
    Scanner = 24,
    Camera = 25,
    Wearable = 26,
    Toy = 27,
    BluetoothGeneric = 28,
}

impl UPowerDeviceType {
    pub const ALL: [Self; 29] = [
        Self::Unknown,
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
    ];

    const fn from_wire(value: Option<u32>) -> Self {
        match value {
            Some(1) => Self::LinePower,
            Some(2) => Self::Battery,
            Some(3) => Self::Ups,
            Some(4) => Self::Monitor,
            Some(5) => Self::Mouse,
            Some(6) => Self::Keyboard,
            Some(7) => Self::Pda,
            Some(8) => Self::Phone,
            Some(9) => Self::MediaPlayer,
            Some(10) => Self::Tablet,
            Some(11) => Self::Computer,
            Some(12) => Self::GamingInput,
            Some(13) => Self::Pen,
            Some(14) => Self::Touchpad,
            Some(15) => Self::Modem,
            Some(16) => Self::Network,
            Some(17) => Self::Headset,
            Some(18) => Self::Speakers,
            Some(19) => Self::Headphones,
            Some(20) => Self::Video,
            Some(21) => Self::OtherAudio,
            Some(22) => Self::RemoteControl,
            Some(23) => Self::Printer,
            Some(24) => Self::Scanner,
            Some(25) => Self::Camera,
            Some(26) => Self::Wearable,
            Some(27) => Self::Toy,
            Some(28) => Self::BluetoothGeneric,
            _ => Self::Unknown,
        }
    }

    pub const fn token(self) -> StateToken {
        match self {
            Self::Unknown => StateToken::Unknown,
            Self::LinePower => StateToken::LinePower,
            Self::Battery => StateToken::Battery,
            Self::Ups => StateToken::Ups,
            Self::Monitor => StateToken::Monitor,
            Self::Mouse => StateToken::Mouse,
            Self::Keyboard => StateToken::Keyboard,
            Self::Pda => StateToken::Pda,
            Self::Phone => StateToken::Phone,
            Self::MediaPlayer => StateToken::MediaPlayer,
            Self::Tablet => StateToken::Tablet,
            Self::Computer => StateToken::Computer,
            Self::GamingInput => StateToken::GamingInput,
            Self::Pen => StateToken::Pen,
            Self::Touchpad => StateToken::Touchpad,
            Self::Modem => StateToken::Modem,
            Self::Network => StateToken::Network,
            Self::Headset => StateToken::Headset,
            Self::Speakers => StateToken::Speakers,
            Self::Headphones => StateToken::Headphones,
            Self::Video => StateToken::Video,
            Self::OtherAudio => StateToken::OtherAudio,
            Self::RemoteControl => StateToken::RemoteControl,
            Self::Printer => StateToken::Printer,
            Self::Scanner => StateToken::Scanner,
            Self::Camera => StateToken::Camera,
            Self::Wearable => StateToken::Wearable,
            Self::Toy => StateToken::Toy,
            Self::BluetoothGeneric => StateToken::BluetoothGeneric,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::LinePower => "Line power",
            Self::Battery => "Battery",
            Self::Ups => "UPS",
            Self::Monitor => "Monitor",
            Self::Mouse => "Mouse",
            Self::Keyboard => "Keyboard",
            Self::Pda => "PDA",
            Self::Phone => "Phone",
            Self::MediaPlayer => "Media player",
            Self::Tablet => "Tablet",
            Self::Computer => "Computer",
            Self::GamingInput => "Gaming input",
            Self::Pen => "Pen",
            Self::Touchpad => "Touchpad",
            Self::Modem => "Modem",
            Self::Network => "Network",
            Self::Headset => "Headset",
            Self::Speakers => "Speakers",
            Self::Headphones => "Headphones",
            Self::Video => "Video",
            Self::OtherAudio => "Other audio",
            Self::RemoteControl => "Remote control",
            Self::Printer => "Printer",
            Self::Scanner => "Scanner",
            Self::Camera => "Camera",
            Self::Wearable => "Wearable",
            Self::Toy => "Toy",
            Self::BluetoothGeneric => "Bluetooth",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UPowerDeviceState {
    Unknown,
    Charging,
    Discharging,
    Empty,
    FullyCharged,
    PendingCharge,
    PendingDischarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryChargeState {
    Unknown,
    Charging,
    Discharging,
    Empty,
    Full,
    PendingCharge,
    PendingDischarge,
}

impl BatteryChargeState {
    const fn from_device_state(state: UPowerDeviceState) -> Self {
        match state {
            UPowerDeviceState::Unknown => Self::Unknown,
            UPowerDeviceState::Charging => Self::Charging,
            UPowerDeviceState::Discharging => Self::Discharging,
            UPowerDeviceState::Empty => Self::Empty,
            UPowerDeviceState::FullyCharged => Self::Full,
            UPowerDeviceState::PendingCharge => Self::PendingCharge,
            UPowerDeviceState::PendingDischarge => Self::PendingDischarge,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Empty => "Empty",
            Self::Full => "Fully charged",
            Self::PendingCharge => "Pending charge",
            Self::PendingDischarge => "Pending discharge",
        }
    }

    const fn token(self) -> StateToken {
        match self {
            Self::Unknown => StateToken::Unknown,
            Self::Charging => StateToken::Charging,
            Self::Discharging => StateToken::Discharging,
            Self::Empty => StateToken::Empty,
            Self::Full => StateToken::Full,
            Self::PendingCharge => StateToken::PendingCharge,
            Self::PendingDischarge => StateToken::PendingDischarge,
        }
    }
}

impl UPowerDeviceState {
    pub const ALL: [Self; 7] = [
        Self::Unknown,
        Self::Charging,
        Self::Discharging,
        Self::Empty,
        Self::FullyCharged,
        Self::PendingCharge,
        Self::PendingDischarge,
    ];

    const fn from_wire(value: Option<u32>) -> Self {
        match value {
            Some(1) => Self::Charging,
            Some(2) => Self::Discharging,
            Some(3) => Self::Empty,
            Some(4) => Self::FullyCharged,
            Some(5) => Self::PendingCharge,
            Some(6) => Self::PendingDischarge,
            _ => Self::Unknown,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Empty => "Empty",
            Self::FullyCharged => "Fully charged",
            Self::PendingCharge => "Pending charge",
            Self::PendingDischarge => "Pending discharge",
        }
    }

    pub const fn token(self) -> StateToken {
        match self {
            Self::Unknown => StateToken::Unknown,
            Self::Charging => StateToken::Charging,
            Self::Discharging => StateToken::Discharging,
            Self::Empty => StateToken::Empty,
            Self::FullyCharged => StateToken::FullyCharged,
            Self::PendingCharge => StateToken::PendingCharge,
            Self::PendingDischarge => StateToken::PendingDischarge,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PowerProfile {
    PowerSaver,
    Balanced,
    Performance,
    Unknown,
}

impl PowerProfile {
    pub const ALL: [Self; 4] = [
        Self::PowerSaver,
        Self::Balanced,
        Self::Performance,
        Self::Unknown,
    ];

    fn parse(value: &str) -> Self {
        match value {
            "power-saver" => Self::PowerSaver,
            "balanced" => Self::Balanced,
            "performance" => Self::Performance,
            _ => Self::Unknown,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            Self::PowerSaver => "power-saver",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Unknown => "unknown",
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::PowerSaver => "Power saver",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::Unknown => "Unknown profile",
        }
    }

    pub const fn token(self) -> StateToken {
        match self {
            Self::PowerSaver => StateToken::PowerSaver,
            Self::Balanced => StateToken::Balanced,
            Self::Performance => StateToken::Performance,
            Self::Unknown => StateToken::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceDegradationReason {
    None,
    HighTemperature,
    LapDetected,
    Unknown,
}

impl PerformanceDegradationReason {
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::HighTemperature,
        Self::LapDetected,
        Self::Unknown,
    ];

    fn parse(value: &str) -> Self {
        match value {
            "" => Self::None,
            "high-operating-temperature" => Self::HighTemperature,
            "lap-detected" => Self::LapDetected,
            _ => Self::Unknown,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Self::None => "Not degraded",
            Self::HighTemperature => "High operating temperature",
            Self::LapDetected => "Lap detected",
            Self::Unknown => "Unknown degradation",
        }
    }

    pub const fn token(self) -> StateToken {
        match self {
            Self::None => StateToken::None,
            Self::HighTemperature => StateToken::HighTemperature,
            Self::LapDetected => StateToken::LapDetected,
            Self::Unknown => StateToken::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UPowerDeviceSnapshot {
    pub key: String,
    pub ready: bool,
    pub device_type: UPowerDeviceType,
    pub power_supply: Option<bool>,
    pub energy: Option<f64>,
    pub energy_capacity: Option<f64>,
    pub change_rate: Option<f64>,
    pub time_to_empty: Option<u64>,
    pub time_to_full: Option<u64>,
    pub percentage: Option<f64>,
    pub is_present: Option<bool>,
    pub state: UPowerDeviceState,
    pub health_percentage: Option<f64>,
    pub health_supported: Option<bool>,
    pub icon_name: Option<String>,
    pub is_laptop_battery: Option<bool>,
    pub native_path: Option<String>,
    pub model: Option<String>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatterySnapshot {
    pub availability: BatteryAvailability,
    pub percentage: Option<u8>,
    pub charge_state: BatteryChargeState,
    pub warning: BatteryWarning,
    pub display_device: Option<UPowerDeviceSnapshot>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerProfileHold {
    pub key: String,
    pub profile: PowerProfile,
    pub application_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerProfilesSnapshot {
    pub available: bool,
    pub current: PowerProfile,
    pub performance_available: bool,
    pub holds: Vec<PowerProfileHold>,
    pub degradation: PerformanceDegradationReason,
    pub source_generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PowerSnapshot {
    pub upower_available: bool,
    pub on_battery: Option<bool>,
    pub battery: BatterySnapshot,
    pub devices: Vec<UPowerDeviceSnapshot>,
    pub profiles: PowerProfilesSnapshot,
    pub upower_source_generation: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PowerProjections {
    pub text: Vec<(StateBindingKey, String)>,
    pub tokens: Vec<(StateBindingKey, StateToken)>,
    pub values: Vec<(StateBindingKey, NumericValue)>,
    pub booleans: Vec<(StateBindingKey, Option<bool>)>,
    pub repeats: Vec<RepeatSourceSnapshot>,
}

impl PowerSnapshot {
    fn unavailable() -> Self {
        Self {
            upower_available: false,
            on_battery: None,
            battery: BatterySnapshot {
                availability: BatteryAvailability::Unavailable,
                percentage: None,
                charge_state: BatteryChargeState::Unknown,
                warning: BatteryWarning::Unknown,
                display_device: None,
                sequence: 0,
            },
            devices: Vec::new(),
            profiles: PowerProfilesSnapshot {
                available: false,
                current: PowerProfile::Unknown,
                performance_available: false,
                holds: Vec::new(),
                degradation: PerformanceDegradationReason::Unknown,
                source_generation: 0,
            },
            upower_source_generation: 0,
            sequence: 0,
        }
    }

    fn semantically_eq(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.sequence = 0;
        right.sequence = 0;
        left.battery.sequence = 0;
        right.battery.sequence = 0;
        if let Some(device) = &mut left.battery.display_device {
            device.sequence = 0;
        }
        if let Some(device) = &mut right.battery.display_device {
            device.sequence = 0;
        }
        for device in &mut left.devices {
            device.sequence = 0;
        }
        for device in &mut right.devices {
            device.sequence = 0;
        }
        left == right
    }

    pub fn projections(&self) -> PowerProjections {
        let mut projections = PowerProjections::default();
        let availability_text = if self.upower_available {
            "available"
        } else {
            "unavailable"
        };
        projections.text.push((
            StateBindingKey::UPowerAvailability,
            availability_text.into(),
        ));
        projections.tokens.push((
            StateBindingKey::UPowerAvailability,
            if self.upower_available {
                StateToken::Available
            } else {
                StateToken::Unavailable
            },
        ));
        let (on_battery_text, on_battery_token) = match self.on_battery {
            Some(true) => ("On battery", StateToken::Battery),
            Some(false) => ("On external power", StateToken::External),
            None => ("Power state unavailable", StateToken::Unavailable),
        };
        projections
            .text
            .push((StateBindingKey::UPowerOnBattery, on_battery_text.into()));
        projections
            .tokens
            .push((StateBindingKey::UPowerOnBattery, on_battery_token));
        projections.values.push((
            StateBindingKey::UPowerDeviceCount,
            NumericValue::Integer(self.devices.len() as i64),
        ));
        project_battery(&self.battery, &mut projections);
        project_profiles(&self.profiles, &mut projections);
        projections.repeats.push(RepeatSourceSnapshot {
            source: RepeatSource::UPowerDevices,
            source_generation: self.upower_source_generation,
            items: self.devices.iter().map(device_repeat_item).collect(),
        });
        projections.repeats.push(RepeatSourceSnapshot {
            source: RepeatSource::PowerProfileHolds,
            source_generation: self.profiles.source_generation,
            items: self.profiles.holds.iter().map(hold_repeat_item).collect(),
        });
        projections
    }
}

fn project_battery(snapshot: &BatterySnapshot, projections: &mut PowerProjections) {
    let status_text = match snapshot.availability {
        BatteryAvailability::Unavailable => "Battery unavailable",
        BatteryAvailability::Absent => "No battery",
        BatteryAvailability::Present => snapshot.charge_state.text(),
    };
    let status_token = match snapshot.availability {
        BatteryAvailability::Unavailable => StateToken::Unavailable,
        BatteryAvailability::Absent => StateToken::Absent,
        BatteryAvailability::Present => snapshot.charge_state.token(),
    };
    projections.text.extend([
        (
            StateBindingKey::BatteryPercentage,
            snapshot
                .percentage
                .map(|percentage| format!("{percentage}%"))
                .unwrap_or_else(|| "—".into()),
        ),
        (StateBindingKey::BatteryStatus, status_text.into()),
    ]);
    projections.tokens.extend([
        (StateBindingKey::BatteryStatus, status_token),
        (StateBindingKey::BatteryWarning, snapshot.warning.token()),
    ]);
    let device = snapshot.display_device.as_ref();
    projections.text.extend([
        (
            StateBindingKey::BatteryReady,
            bool_text(device.map(|device| device.ready)),
        ),
        (
            StateBindingKey::BatteryType,
            device
                .map(|device| device.device_type.text().to_owned())
                .unwrap_or_else(|| "—".into()),
        ),
        (
            StateBindingKey::BatteryIsPresent,
            bool_text(device.and_then(|device| device.is_present)),
        ),
        (
            StateBindingKey::BatteryHealthSupported,
            bool_text(device.and_then(|device| device.health_supported)),
        ),
        (
            StateBindingKey::BatteryIconName,
            option_text(device.and_then(|device| device.icon_name.as_deref())),
        ),
        (
            StateBindingKey::BatteryIsLaptopBattery,
            bool_text(device.and_then(|device| device.is_laptop_battery)),
        ),
        (
            StateBindingKey::BatteryPowerSupply,
            bool_text(device.and_then(|device| device.power_supply)),
        ),
        (
            StateBindingKey::BatteryNativePath,
            option_text(device.and_then(|device| device.native_path.as_deref())),
        ),
        (
            StateBindingKey::BatteryModel,
            option_text(device.and_then(|device| device.model.as_deref())),
        ),
    ]);
    projections.tokens.extend([
        (
            StateBindingKey::BatteryReady,
            bool_token(device.map(|device| device.ready)),
        ),
        (
            StateBindingKey::BatteryType,
            device
                .map(|device| device.device_type.token())
                .unwrap_or(StateToken::Unknown),
        ),
        (
            StateBindingKey::BatteryIsPresent,
            bool_token(device.and_then(|device| device.is_present)),
        ),
        (
            StateBindingKey::BatteryHealthSupported,
            bool_token(device.and_then(|device| device.health_supported)),
        ),
        (
            StateBindingKey::BatteryIsLaptopBattery,
            bool_token(device.and_then(|device| device.is_laptop_battery)),
        ),
        (
            StateBindingKey::BatteryPowerSupply,
            bool_token(device.and_then(|device| device.power_supply)),
        ),
    ]);
    projections.values.extend([
        (
            StateBindingKey::BatteryPercentage,
            option_decimal(device.and_then(|device| device.percentage)),
        ),
        (
            StateBindingKey::BatteryEnergy,
            option_decimal(device.and_then(|device| device.energy)),
        ),
        (
            StateBindingKey::BatteryEnergyCapacity,
            option_decimal(device.and_then(|device| device.energy_capacity)),
        ),
        (
            StateBindingKey::BatteryChangeRate,
            option_decimal(device.and_then(|device| device.change_rate)),
        ),
        (
            StateBindingKey::BatteryTimeToEmpty,
            option_integer(device.and_then(|device| device.time_to_empty)),
        ),
        (
            StateBindingKey::BatteryTimeToFull,
            option_integer(device.and_then(|device| device.time_to_full)),
        ),
        (
            StateBindingKey::BatteryHealthPercentage,
            option_decimal(device.and_then(|device| device.health_percentage)),
        ),
    ]);
}

fn project_profiles(snapshot: &PowerProfilesSnapshot, projections: &mut PowerProjections) {
    projections.text.push((
        StateBindingKey::PowerProfileAvailability,
        if snapshot.available {
            "available".into()
        } else {
            "unavailable".into()
        },
    ));
    projections.tokens.push((
        StateBindingKey::PowerProfileAvailability,
        if snapshot.available {
            StateToken::Available
        } else {
            StateToken::Unavailable
        },
    ));
    projections.booleans.push((
        StateBindingKey::PowerProfileAvailability,
        snapshot.available.then_some(true),
    ));
    let (current_text, current_token) = if snapshot.available {
        (snapshot.current.text(), snapshot.current.token())
    } else {
        ("Power profiles unavailable", StateToken::Unavailable)
    };
    projections
        .text
        .push((StateBindingKey::PowerProfileCurrent, current_text.into()));
    projections
        .tokens
        .push((StateBindingKey::PowerProfileCurrent, current_token));
    let performance_text = if !snapshot.available {
        "unavailable"
    } else if snapshot.performance_available {
        "true"
    } else {
        "false"
    };
    let performance_token = if !snapshot.available {
        StateToken::Unavailable
    } else {
        bool_token(Some(snapshot.performance_available))
    };
    projections.text.push((
        StateBindingKey::PowerProfilePerformanceAvailable,
        performance_text.into(),
    ));
    projections.tokens.push((
        StateBindingKey::PowerProfilePerformanceAvailable,
        performance_token,
    ));
    projections.booleans.push((
        StateBindingKey::PowerProfilePerformanceAvailable,
        snapshot.available.then_some(snapshot.performance_available),
    ));
    let (degradation_text, degradation_token) = if snapshot.available {
        (snapshot.degradation.text(), snapshot.degradation.token())
    } else {
        ("Power profiles unavailable", StateToken::Unavailable)
    };
    projections.text.push((
        StateBindingKey::PowerProfileDegradation,
        degradation_text.into(),
    ));
    projections
        .tokens
        .push((StateBindingKey::PowerProfileDegradation, degradation_token));
    projections.values.push((
        StateBindingKey::PowerProfileHoldCount,
        NumericValue::Integer(snapshot.holds.len() as i64),
    ));
}

fn device_repeat_item(device: &UPowerDeviceSnapshot) -> RepeatItemSnapshot {
    let mut text = BTreeMap::new();
    let mut tokens = BTreeMap::new();
    let mut values = BTreeMap::new();
    text.insert(ItemBindingKey::Ready, "true".into());
    tokens.insert(ItemBindingKey::Ready, StateToken::True);
    text.insert(ItemBindingKey::Type, device.device_type.text().into());
    tokens.insert(ItemBindingKey::Type, device.device_type.token());
    insert_bool(
        &mut text,
        &mut tokens,
        ItemBindingKey::PowerSupply,
        device.power_supply,
    );
    insert_value(&mut values, ItemBindingKey::Energy, device.energy);
    insert_value(
        &mut values,
        ItemBindingKey::EnergyCapacity,
        device.energy_capacity,
    );
    insert_value(&mut values, ItemBindingKey::ChangeRate, device.change_rate);
    values.insert(
        ItemBindingKey::TimeToEmpty,
        option_integer(device.time_to_empty),
    );
    values.insert(
        ItemBindingKey::TimeToFull,
        option_integer(device.time_to_full),
    );
    insert_value(&mut values, ItemBindingKey::Percentage, device.percentage);
    insert_bool(
        &mut text,
        &mut tokens,
        ItemBindingKey::IsPresent,
        device.is_present,
    );
    text.insert(ItemBindingKey::State, device.state.text().into());
    tokens.insert(ItemBindingKey::State, device.state.token());
    insert_value(
        &mut values,
        ItemBindingKey::HealthPercentage,
        device.health_percentage,
    );
    insert_bool(
        &mut text,
        &mut tokens,
        ItemBindingKey::HealthSupported,
        device.health_supported,
    );
    text.insert(
        ItemBindingKey::IconName,
        option_text(device.icon_name.as_deref()),
    );
    insert_bool(
        &mut text,
        &mut tokens,
        ItemBindingKey::IsLaptopBattery,
        device.is_laptop_battery,
    );
    text.insert(
        ItemBindingKey::NativePath,
        option_text(device.native_path.as_deref()),
    );
    text.insert(ItemBindingKey::Model, option_text(device.model.as_deref()));
    RepeatItemSnapshot {
        key: device.key.clone(),
        text,
        tokens,
        values,
    }
}

fn hold_repeat_item(hold: &PowerProfileHold) -> RepeatItemSnapshot {
    RepeatItemSnapshot {
        key: hold.key.clone(),
        text: BTreeMap::from([
            (ItemBindingKey::Profile, hold.profile.text().into()),
            (ItemBindingKey::ApplicationId, hold.application_id.clone()),
            (ItemBindingKey::Reason, hold.reason.clone()),
        ]),
        tokens: BTreeMap::from([(ItemBindingKey::Profile, hold.profile.token())]),
        values: BTreeMap::new(),
    }
}

fn insert_bool(
    text: &mut BTreeMap<ItemBindingKey, String>,
    tokens: &mut BTreeMap<ItemBindingKey, StateToken>,
    key: ItemBindingKey,
    value: Option<bool>,
) {
    text.insert(key, bool_text(value));
    tokens.insert(key, bool_token(value));
}

fn insert_value(
    values: &mut BTreeMap<ItemBindingKey, NumericValue>,
    key: ItemBindingKey,
    value: Option<f64>,
) {
    values.insert(key, option_decimal(value));
}

fn bool_text(value: Option<bool>) -> String {
    match value {
        Some(true) => "true".into(),
        Some(false) => "false".into(),
        None => "—".into(),
    }
}

fn bool_token(value: Option<bool>) -> StateToken {
    match value {
        Some(true) => StateToken::True,
        Some(false) => StateToken::False,
        None => StateToken::Unknown,
    }
}

fn option_text(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("—")
        .into()
}

fn option_decimal(value: Option<f64>) -> NumericValue {
    value
        .map(NumericValue::finite_decimal)
        .unwrap_or(NumericValue::Unknown)
}

fn option_integer(value: Option<u64>) -> NumericValue {
    value
        .and_then(|value| i64::try_from(value).ok())
        .map(NumericValue::Integer)
        .unwrap_or(NumericValue::Unknown)
}

#[derive(Debug, Clone, Default)]
struct RawDeviceProperties {
    device_type: Option<u32>,
    power_supply: Option<bool>,
    energy: Option<f64>,
    energy_full: Option<f64>,
    energy_rate: Option<f64>,
    time_to_empty: Option<i64>,
    time_to_full: Option<i64>,
    percentage: Option<f64>,
    is_present: Option<bool>,
    state: Option<u32>,
    capacity: Option<f64>,
    icon_name: Option<String>,
    native_path: Option<String>,
    model: Option<String>,
    warning: Option<u32>,
    malformed_fields: u64,
}

#[derive(Debug, Clone, Default)]
struct RawUPowerSnapshot {
    available: bool,
    source_generation: u64,
    on_battery: Option<bool>,
    display: Option<RawDeviceProperties>,
    devices: BTreeMap<String, RawDeviceProperties>,
}

#[derive(Debug, Clone, Default)]
struct RawProfileHold {
    profile: String,
    application_id: String,
    reason: String,
}

#[derive(Debug, Clone, Default)]
struct RawProfilesSnapshot {
    available: bool,
    source_generation: u64,
    active_profile: Option<String>,
    profiles: Vec<String>,
    holds: Vec<RawProfileHold>,
    degradation: Option<String>,
    malformed_fields: u64,
}

fn normalize_power(
    upower: &RawUPowerSnapshot,
    profiles: &RawProfilesSnapshot,
    sequence: u64,
) -> (PowerSnapshot, u64) {
    let mut malformed = profiles.malformed_fields;
    let mut devices = Vec::new();
    for (path, raw) in &upower.devices {
        let (device, count) = normalize_device(
            raw,
            format!("{}:{path}", upower.source_generation),
            sequence,
        );
        malformed = malformed.saturating_add(count);
        devices.push((path, device));
    }
    devices.sort_by(|(left_path, left), (right_path, right)| {
        (
            left.device_type as u32,
            left.model.as_deref().unwrap_or(""),
            left_path,
        )
            .cmp(&(
                right.device_type as u32,
                right.model.as_deref().unwrap_or(""),
                right_path,
            ))
    });
    let devices = devices.into_iter().map(|(_, device)| device).collect();
    let (display_device, display_malformed) = upower
        .display
        .as_ref()
        .map(|raw| {
            normalize_device(
                raw,
                format!("{}:{DISPLAY_DEVICE_PATH}", upower.source_generation),
                sequence,
            )
        })
        .map_or((None, 0), |(device, malformed)| (Some(device), malformed));
    malformed = malformed.saturating_add(display_malformed);
    let availability = if !upower.available {
        BatteryAvailability::Unavailable
    } else if display_device.as_ref().and_then(|device| device.is_present) == Some(true) {
        BatteryAvailability::Present
    } else {
        BatteryAvailability::Absent
    };
    let percentage = if availability == BatteryAvailability::Present {
        display_device
            .as_ref()
            .and_then(|device| device.percentage)
            .and_then(|value| normalize_percentage(value).ok())
    } else {
        None
    };
    let charge_state = if availability == BatteryAvailability::Present {
        display_device
            .as_ref()
            .map(|device| BatteryChargeState::from_device_state(device.state))
            .unwrap_or(BatteryChargeState::Unknown)
    } else {
        BatteryChargeState::Unknown
    };
    let warning = if availability == BatteryAvailability::Present {
        upower
            .display
            .as_ref()
            .map(|display| BatteryWarning::from_wire(display.warning))
            .unwrap_or(BatteryWarning::Unknown)
    } else {
        BatteryWarning::Unknown
    };
    let profiles = normalize_profiles(profiles);
    (
        PowerSnapshot {
            upower_available: upower.available,
            on_battery: upower.available.then_some(upower.on_battery).flatten(),
            battery: BatterySnapshot {
                availability,
                percentage,
                charge_state,
                warning,
                display_device,
                sequence,
            },
            devices,
            profiles,
            upower_source_generation: upower.source_generation,
            sequence,
        },
        malformed,
    )
}

fn normalize_device(
    raw: &RawDeviceProperties,
    key: String,
    sequence: u64,
) -> (UPowerDeviceSnapshot, u64) {
    let mut malformed = raw.malformed_fields;
    let device_type = UPowerDeviceType::from_wire(raw.device_type);
    let energy = checked_nonnegative(raw.energy, &mut malformed);
    let energy_capacity = checked_nonnegative(raw.energy_full, &mut malformed);
    let change_rate = raw.energy_rate.and_then(|value| {
        if value.is_finite() {
            Some(-value)
        } else {
            malformed = malformed.saturating_add(1);
            None
        }
    });
    let percentage = raw.percentage.and_then(|value| {
        if normalize_percentage(value).is_ok() {
            Some(value)
        } else {
            malformed = malformed.saturating_add(1);
            None
        }
    });
    let health_percentage = raw.capacity.and_then(|value| {
        if value.is_finite() && (0.0..=100.0).contains(&value) {
            Some(value)
        } else {
            malformed = malformed.saturating_add(1);
            None
        }
    });
    let power_supply = raw.power_supply;
    let is_laptop_battery =
        power_supply.map(|power_supply| device_type == UPowerDeviceType::Battery && power_supply);
    let health_supported = health_percentage.map(|capacity| capacity != 0.0);
    (
        UPowerDeviceSnapshot {
            key,
            ready: true,
            device_type,
            power_supply,
            energy,
            energy_capacity,
            change_rate,
            time_to_empty: checked_duration(raw.time_to_empty, &mut malformed),
            time_to_full: checked_duration(raw.time_to_full, &mut malformed),
            percentage,
            is_present: raw.is_present,
            state: UPowerDeviceState::from_wire(raw.state),
            health_percentage,
            health_supported,
            icon_name: bounded_string(raw.icon_name.as_deref(), &mut malformed),
            is_laptop_battery,
            native_path: bounded_string(raw.native_path.as_deref(), &mut malformed),
            model: bounded_string(raw.model.as_deref(), &mut malformed),
            sequence,
        },
        malformed,
    )
}

fn normalize_profiles(raw: &RawProfilesSnapshot) -> PowerProfilesSnapshot {
    if !raw.available {
        return PowerProfilesSnapshot {
            available: false,
            current: PowerProfile::Unknown,
            performance_available: false,
            holds: Vec::new(),
            degradation: PerformanceDegradationReason::Unknown,
            source_generation: raw.source_generation,
        };
    }
    let mut holds = raw.holds.clone();
    holds.sort_by(|left, right| {
        (
            profile_order(PowerProfile::parse(&left.profile)),
            &left.application_id,
            &left.reason,
        )
            .cmp(&(
                profile_order(PowerProfile::parse(&right.profile)),
                &right.application_id,
                &right.reason,
            ))
    });
    let mut occurrences: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    let holds = holds
        .into_iter()
        .map(|hold| {
            let tuple = (
                hold.profile.clone(),
                hold.application_id.clone(),
                hold.reason.clone(),
            );
            let ordinal = occurrences.entry(tuple.clone()).or_default();
            let key = format!(
                "{}:{}\u{1f}{}\u{1f}{}\u{1f}{}",
                raw.source_generation, tuple.0, tuple.1, tuple.2, *ordinal
            );
            *ordinal = ordinal.saturating_add(1);
            PowerProfileHold {
                key,
                profile: PowerProfile::parse(&hold.profile),
                application_id: hold.application_id,
                reason: hold.reason,
            }
        })
        .collect();
    PowerProfilesSnapshot {
        available: true,
        current: raw
            .active_profile
            .as_deref()
            .map(PowerProfile::parse)
            .unwrap_or(PowerProfile::Unknown),
        performance_available: raw.profiles.iter().any(|profile| profile == "performance"),
        holds,
        degradation: raw
            .degradation
            .as_deref()
            .map(PerformanceDegradationReason::parse)
            .unwrap_or(PerformanceDegradationReason::Unknown),
        source_generation: raw.source_generation,
    }
}

fn profile_order(profile: PowerProfile) -> u8 {
    match profile {
        PowerProfile::PowerSaver => 0,
        PowerProfile::Balanced => 1,
        PowerProfile::Performance => 2,
        PowerProfile::Unknown => 3,
    }
}

fn normalize_percentage(value: f64) -> Result<u8, PercentageError> {
    if !value.is_finite() {
        return Err(PercentageError::NonFinite);
    }
    if !(0.0..=100.0).contains(&value) {
        return Err(PercentageError::OutOfRange);
    }
    Ok(value.round() as u8)
}

fn checked_nonnegative(value: Option<f64>, malformed: &mut u64) -> Option<f64> {
    value.and_then(|value| {
        if value.is_finite() && value >= 0.0 {
            Some(value)
        } else {
            *malformed = malformed.saturating_add(1);
            None
        }
    })
}

fn checked_duration(value: Option<i64>, malformed: &mut u64) -> Option<u64> {
    value.and_then(|value| match u64::try_from(value) {
        Ok(value) => Some(value),
        Err(_) => {
            *malformed = malformed.saturating_add(1);
            None
        }
    })
}

fn bounded_string(value: Option<&str>, malformed: &mut u64) -> Option<String> {
    value.and_then(|value| {
        if value.len() <= MAX_SERVICE_STRING_BYTES {
            Some(value.to_owned())
        } else {
            *malformed = malformed.saturating_add(1);
            None
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PercentageError {
    NonFinite,
    OutOfRange,
}

impl fmt::Display for PercentageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => formatter.write_str("percentage is not finite"),
            Self::OutOfRange => formatter.write_str("percentage is outside 0 through 100"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceKind {
    UPower,
    Profiles,
}

impl ServiceKind {
    const fn name(self) -> &'static str {
        match self {
            Self::UPower => UPOWER_SERVICE,
            Self::Profiles => PROFILE_SERVICE,
        }
    }
}

#[derive(Debug, Clone)]
enum PowerTransportEvent {
    UPowerUnavailable {
        connection_generation: u64,
        source_generation: u64,
    },
    UPowerSnapshot {
        connection_generation: u64,
        snapshot: RawUPowerSnapshot,
    },
    ProfilesUnavailable {
        connection_generation: u64,
        source_generation: u64,
    },
    ProfilesSnapshot {
        connection_generation: u64,
        snapshot: RawProfilesSnapshot,
    },
    ProfileRequestResult {
        connection_generation: u64,
        source_generation: u64,
        profile: PowerProfile,
        succeeded: bool,
    },
    BusDisconnected {
        connection_generation: u64,
        reason: String,
    },
}

#[derive(Debug, Clone)]
enum PendingRequest {
    AddMatch,
    OwnerLookup {
        service: ServiceKind,
        epoch: u64,
        started: Instant,
    },
    UPowerRoot {
        epoch: u64,
        owner: String,
        started: Instant,
    },
    DisplayDevice {
        epoch: u64,
        owner: String,
        started: Instant,
    },
    EnumerateDevices {
        epoch: u64,
        owner: String,
        started: Instant,
    },
    Device {
        epoch: u64,
        owner: String,
        path: String,
        started: Instant,
    },
    Profiles {
        epoch: u64,
        owner: String,
        started: Instant,
    },
    SetProfile {
        epoch: u64,
        owner: String,
        source_generation: u64,
        profile: PowerProfile,
        started: Instant,
    },
}

struct PowerTransport {
    channel: Channel,
    connection_generation: u64,
    pending: BTreeMap<u32, PendingRequest>,
    matches_pending: usize,
    owner_lookups_sent: bool,
    upower_owner: Option<String>,
    profile_owner: Option<String>,
    upower_epoch: u64,
    profile_epoch: u64,
    upower_source_generation: u64,
    profile_source_generation: u64,
    raw_upower: RawUPowerSnapshot,
    raw_profiles: RawProfilesSnapshot,
    root_ready: bool,
    display_ready: bool,
    enumeration_ready: bool,
    known_device_paths: BTreeSet<String>,
    needs_immediate_drain: bool,
    last_owner_lookup_us: u64,
    last_property_read_us: u64,
    last_enumeration_us: u64,
    last_device_read_us: u64,
    last_profiles_read_us: u64,
}

#[derive(Debug, Default)]
struct TransportDispatch {
    events: Vec<PowerTransportEvent>,
    messages_drained: usize,
    relevant_property_signals: usize,
    irrelevant_property_signals: usize,
    owner_lookup_us: u64,
    property_read_us: u64,
    enumeration_us: u64,
    device_read_us: u64,
    profiles_read_us: u64,
}

impl PowerTransport {
    fn connect(connection_generation: u64) -> Result<Self, String> {
        let mut channel = Channel::get_private(BusType::System)
            .map_err(|error| format!("connect to system bus: {error}"))?;
        channel.set_watch_enabled(true);
        let mut transport = Self {
            channel,
            connection_generation,
            pending: BTreeMap::new(),
            matches_pending: 0,
            owner_lookups_sent: false,
            upower_owner: None,
            profile_owner: None,
            upower_epoch: 0,
            profile_epoch: 0,
            upower_source_generation: 0,
            profile_source_generation: 0,
            raw_upower: RawUPowerSnapshot::default(),
            raw_profiles: RawProfilesSnapshot::default(),
            root_ready: false,
            display_ready: false,
            enumeration_ready: false,
            known_device_paths: BTreeSet::new(),
            needs_immediate_drain: false,
            last_owner_lookup_us: 0,
            last_property_read_us: 0,
            last_enumeration_us: 0,
            last_device_read_us: 0,
            last_profiles_read_us: 0,
        };
        for rule in [
            "type='signal',sender='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged'",
            "type='signal',path='/org/freedesktop/UPower',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
            "type='signal',path='/org/freedesktop/UPower',interface='org.freedesktop.UPower',member='DeviceAdded'",
            "type='signal',path='/org/freedesktop/UPower',interface='org.freedesktop.UPower',member='DeviceRemoved'",
            "type='signal',path_namespace='/org/freedesktop/UPower/devices',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
            "type='signal',path='/org/freedesktop/UPower/PowerProfiles',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
        ] {
            let serial = transport.send_add_match(rule)?;
            transport.pending.insert(serial, PendingRequest::AddMatch);
            transport.matches_pending = transport.matches_pending.saturating_add(1);
        }
        Ok(transport)
    }

    fn watch(&self) -> Watch {
        self.channel.watch()
    }

    fn is_connected(&self) -> bool {
        self.channel.is_connected()
    }

    fn has_pending_requests(&self) -> bool {
        !self.pending.is_empty()
    }

    fn needs_immediate_drain(&self) -> bool {
        self.needs_immediate_drain
    }

    fn send_add_match(&self, rule: &str) -> Result<u32, String> {
        let message = Message::new_method_call(DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE, "AddMatch")
            .map_err(|error| format!("construct AddMatch: {error}"))?
            .append1(rule);
        self.channel
            .send(message)
            .map_err(|()| "send AddMatch".into())
    }

    fn send_owner_lookup(&mut self, service: ServiceKind) -> Result<(), String> {
        let epoch = match service {
            ServiceKind::UPower => self.upower_epoch,
            ServiceKind::Profiles => self.profile_epoch,
        };
        let mut message =
            Message::new_method_call(DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE, "GetNameOwner")
                .map_err(|error| format!("construct GetNameOwner: {error}"))?
                .append1(service.name());
        message.set_auto_start(false);
        let serial = self
            .channel
            .send(message)
            .map_err(|()| "send GetNameOwner".to_owned())?;
        self.pending.insert(
            serial,
            PendingRequest::OwnerLookup {
                service,
                epoch,
                started: Instant::now(),
            },
        );
        Ok(())
    }

    fn send_get_all(
        &mut self,
        service: &str,
        path: &str,
        interface: &str,
        request: PendingRequest,
    ) -> Result<(), String> {
        let mut message = Message::new_method_call(service, path, PROPERTIES_INTERFACE, "GetAll")
            .map_err(|error| format!("construct {path} GetAll: {error}"))?
            .append1(interface);
        message.set_auto_start(false);
        let serial = self
            .channel
            .send(message)
            .map_err(|()| format!("send {path} GetAll"))?;
        self.pending.insert(serial, request);
        Ok(())
    }

    fn request_upower_root(&mut self) -> Result<(), String> {
        if self
            .pending
            .values()
            .any(|request| matches!(request, PendingRequest::UPowerRoot { .. }))
        {
            return Ok(());
        }
        let Some(owner) = self.upower_owner.clone() else {
            return Ok(());
        };
        self.send_get_all(
            UPOWER_SERVICE,
            UPOWER_PATH,
            UPOWER_INTERFACE,
            PendingRequest::UPowerRoot {
                epoch: self.upower_epoch,
                owner,
                started: Instant::now(),
            },
        )
    }

    fn request_display(&mut self) -> Result<(), String> {
        if self
            .pending
            .values()
            .any(|request| matches!(request, PendingRequest::DisplayDevice { .. }))
        {
            return Ok(());
        }
        let Some(owner) = self.upower_owner.clone() else {
            return Ok(());
        };
        self.send_get_all(
            UPOWER_SERVICE,
            DISPLAY_DEVICE_PATH,
            DEVICE_INTERFACE,
            PendingRequest::DisplayDevice {
                epoch: self.upower_epoch,
                owner,
                started: Instant::now(),
            },
        )
    }

    fn request_enumeration(&mut self) -> Result<(), String> {
        if self
            .pending
            .values()
            .any(|request| matches!(request, PendingRequest::EnumerateDevices { .. }))
        {
            return Ok(());
        }
        let Some(owner) = self.upower_owner.clone() else {
            return Ok(());
        };
        let mut message = Message::new_method_call(
            UPOWER_SERVICE,
            UPOWER_PATH,
            UPOWER_INTERFACE,
            "EnumerateDevices",
        )
        .map_err(|error| format!("construct EnumerateDevices: {error}"))?;
        message.set_auto_start(false);
        let serial = self
            .channel
            .send(message)
            .map_err(|()| "send EnumerateDevices".to_owned())?;
        self.pending.insert(
            serial,
            PendingRequest::EnumerateDevices {
                epoch: self.upower_epoch,
                owner,
                started: Instant::now(),
            },
        );
        Ok(())
    }

    fn request_device(&mut self, path: &str) -> Result<(), String> {
        if path == DISPLAY_DEVICE_PATH
            || self.pending.values().any(|request| {
                matches!(request, PendingRequest::Device { path: pending, .. } if pending == path)
            })
        {
            return Ok(());
        }
        let Some(owner) = self.upower_owner.clone() else {
            return Ok(());
        };
        self.send_get_all(
            UPOWER_SERVICE,
            path,
            DEVICE_INTERFACE,
            PendingRequest::Device {
                epoch: self.upower_epoch,
                owner,
                path: path.to_owned(),
                started: Instant::now(),
            },
        )
    }

    fn request_profiles(&mut self) -> Result<(), String> {
        if self
            .pending
            .values()
            .any(|request| matches!(request, PendingRequest::Profiles { .. }))
        {
            return Ok(());
        }
        let Some(owner) = self.profile_owner.clone() else {
            return Ok(());
        };
        self.send_get_all(
            PROFILE_SERVICE,
            PROFILE_PATH,
            PROFILE_INTERFACE,
            PendingRequest::Profiles {
                epoch: self.profile_epoch,
                owner,
                started: Instant::now(),
            },
        )
    }

    fn set_profile(&mut self, profile: PowerProfile) -> Result<(), String> {
        if profile == PowerProfile::Unknown {
            return Err("cannot request an unknown power profile".into());
        }
        if self.pending.values().any(|request| {
            matches!(request, PendingRequest::SetProfile { profile: pending, .. } if *pending == profile)
        }) {
            return Ok(());
        }
        let Some(owner) = self.profile_owner.clone() else {
            return Err("power-profiles-daemon is unavailable".into());
        };
        let mut message =
            Message::new_method_call(PROFILE_SERVICE, PROFILE_PATH, PROPERTIES_INTERFACE, "Set")
                .map_err(|error| format!("construct profile Set: {error}"))?
                .append3(
                    PROFILE_INTERFACE,
                    "ActiveProfile",
                    Variant(profile.wire().to_owned()),
                );
        message.set_auto_start(false);
        let serial = self
            .channel
            .send(message)
            .map_err(|()| "send profile Set".to_owned())?;
        self.pending.insert(
            serial,
            PendingRequest::SetProfile {
                epoch: self.profile_epoch,
                owner,
                source_generation: self.profile_source_generation,
                profile,
                started: Instant::now(),
            },
        );
        Ok(())
    }

    fn begin_upower(&mut self) -> Result<(), String> {
        self.root_ready = false;
        self.display_ready = false;
        self.enumeration_ready = false;
        self.known_device_paths.clear();
        self.raw_upower = RawUPowerSnapshot {
            available: true,
            source_generation: self.upower_source_generation,
            ..RawUPowerSnapshot::default()
        };
        self.request_upower_root()?;
        self.request_display()?;
        self.request_enumeration()
    }

    fn begin_profiles(&mut self) -> Result<(), String> {
        self.raw_profiles = RawProfilesSnapshot {
            available: true,
            source_generation: self.profile_source_generation,
            ..RawProfilesSnapshot::default()
        };
        self.request_profiles()
    }

    fn process_ready(&mut self) -> TransportDispatch {
        if self.channel.read_write(Some(Duration::ZERO)).is_err() || !self.is_connected() {
            return TransportDispatch {
                events: vec![PowerTransportEvent::BusDisconnected {
                    connection_generation: self.connection_generation,
                    reason: "system-bus connection closed".into(),
                }],
                ..TransportDispatch::default()
            };
        }
        let mut events = Vec::new();
        let mut drained = 0usize;
        let mut relevant_property_signals = 0usize;
        let mut irrelevant_property_signals = 0usize;
        while drained < MAX_MESSAGES_PER_DISPATCH {
            let Some(message) = self.channel.pop_message() else {
                break;
            };
            drained = drained.saturating_add(1);
            if message.msg_type() == MessageType::Signal
                && message
                    .interface()
                    .map(|interface| interface.to_string())
                    .as_deref()
                    == Some(PROPERTIES_INTERFACE)
                && message.member().map(|member| member.to_string()).as_deref()
                    == Some("PropertiesChanged")
            {
                let path = message
                    .path()
                    .map(|path| path.to_string())
                    .unwrap_or_default();
                if properties_signal_is_relevant(&message, &path) {
                    relevant_property_signals = relevant_property_signals.saturating_add(1);
                } else {
                    irrelevant_property_signals = irrelevant_property_signals.saturating_add(1);
                }
            }
            self.process_message(message, &mut events);
        }
        self.needs_immediate_drain = drained == MAX_MESSAGES_PER_DISPATCH;
        if self.matches_pending == 0 && !self.owner_lookups_sent {
            self.owner_lookups_sent = true;
            for service in [ServiceKind::UPower, ServiceKind::Profiles] {
                if let Err(error) = self.send_owner_lookup(service) {
                    events.push(PowerTransportEvent::BusDisconnected {
                        connection_generation: self.connection_generation,
                        reason: error,
                    });
                }
            }
        }
        TransportDispatch {
            events,
            messages_drained: drained,
            relevant_property_signals,
            irrelevant_property_signals,
            owner_lookup_us: std::mem::take(&mut self.last_owner_lookup_us),
            property_read_us: std::mem::take(&mut self.last_property_read_us),
            enumeration_us: std::mem::take(&mut self.last_enumeration_us),
            device_read_us: std::mem::take(&mut self.last_device_read_us),
            profiles_read_us: std::mem::take(&mut self.last_profiles_read_us),
        }
    }

    fn process_message(&mut self, mut message: Message, events: &mut Vec<PowerTransportEvent>) {
        if let Some(reply_serial) = message.get_reply_serial()
            && let Some(request) = self.pending.remove(&reply_serial)
        {
            self.process_reply(request, &mut message, events);
            return;
        }
        if message.msg_type() != MessageType::Signal {
            return;
        }
        if message_matches(&message, DBUS_PATH, DBUS_INTERFACE, "NameOwnerChanged") {
            self.process_owner_changed(&message, events);
            return;
        }
        let path = message
            .path()
            .map(|path| path.to_string())
            .unwrap_or_default();
        let interface = message
            .interface()
            .map(|interface| interface.to_string())
            .unwrap_or_default();
        let member = message
            .member()
            .map(|member| member.to_string())
            .unwrap_or_default();
        let sender = message
            .sender()
            .map(|sender| sender.to_string())
            .unwrap_or_default();
        if (path == UPOWER_PATH || path.starts_with("/org/freedesktop/UPower/devices/"))
            && self.upower_owner.as_deref() != Some(sender.as_str())
        {
            return;
        }
        if path == PROFILE_PATH && self.profile_owner.as_deref() != Some(sender.as_str()) {
            return;
        }
        if path == UPOWER_PATH && interface == UPOWER_INTERFACE {
            match member.as_str() {
                "DeviceAdded" => {
                    if let Ok(path) = message.read1::<dbus::Path<'static>>() {
                        let path = path.to_string();
                        if path == DISPLAY_DEVICE_PATH {
                            return;
                        }
                        if is_enumerated_device_path(&path)
                            && (self.known_device_paths.contains(&path)
                                || self.known_device_paths.len() < MAX_UPOWER_DEVICES_PER_PROCESS)
                        {
                            self.known_device_paths.insert(path.clone());
                            let _ = self.request_device(&path);
                        } else if !self.known_device_paths.contains(&path) {
                            events.push(PowerTransportEvent::UPowerUnavailable {
                                connection_generation: self.connection_generation,
                                source_generation: self.upower_source_generation,
                            });
                        }
                    }
                }
                "DeviceRemoved" => {
                    if let Ok(path) = message.read1::<dbus::Path<'static>>() {
                        let path = path.to_string();
                        self.known_device_paths.remove(&path);
                        self.raw_upower.devices.remove(&path);
                        self.emit_upower_if_ready(events);
                    }
                }
                _ => {}
            }
            return;
        }
        if interface != PROPERTIES_INTERFACE || member != "PropertiesChanged" {
            return;
        }
        if !properties_signal_is_relevant(&message, &path) {
            return;
        }
        if path == UPOWER_PATH {
            let _ = self.request_upower_root();
        } else if path == DISPLAY_DEVICE_PATH {
            let _ = self.request_display();
        } else if path.starts_with("/org/freedesktop/UPower/devices/")
            && self.known_device_paths.contains(&path)
        {
            let _ = self.request_device(&path);
        } else if path == PROFILE_PATH {
            let _ = self.request_profiles();
        }
    }

    fn process_reply(
        &mut self,
        request: PendingRequest,
        message: &mut Message,
        events: &mut Vec<PowerTransportEvent>,
    ) {
        match request {
            PendingRequest::AddMatch => {
                if message.as_result().is_ok() {
                    self.matches_pending = self.matches_pending.saturating_sub(1);
                } else {
                    events.push(self.reply_failure("install D-Bus match", message));
                }
            }
            PendingRequest::OwnerLookup {
                service,
                epoch,
                started,
            } => {
                let current_epoch = match service {
                    ServiceKind::UPower => self.upower_epoch,
                    ServiceKind::Profiles => self.profile_epoch,
                };
                if epoch != current_epoch {
                    return;
                }
                self.last_owner_lookup_us = elapsed_us(started);
                let owner = if message.as_result().is_ok() {
                    message
                        .read1::<String>()
                        .ok()
                        .filter(|owner| !owner.is_empty())
                } else {
                    None
                };
                self.set_owner(service, owner, events);
            }
            PendingRequest::UPowerRoot {
                epoch,
                owner,
                started,
            } => {
                self.last_property_read_us = elapsed_us(started);
                if !self.upower_reply_is_current(epoch, &owner, message) {
                    return;
                }
                if message.as_result().is_err() {
                    return;
                }
                if let Ok(map) = message.read1::<PropMap>() {
                    let mut malformed = 0;
                    self.raw_upower.on_battery =
                        typed_property::<bool>(&map, "OnBattery", &mut malformed);
                    self.root_ready = true;
                    self.emit_upower_if_ready(events);
                }
            }
            PendingRequest::DisplayDevice {
                epoch,
                owner,
                started,
            } => {
                self.last_property_read_us = elapsed_us(started);
                if !self.upower_reply_is_current(epoch, &owner, message) {
                    return;
                }
                if let Ok(map) = message.read1::<PropMap>() {
                    self.raw_upower.display = Some(decode_device_map(&map));
                    self.display_ready = true;
                    self.emit_upower_if_ready(events);
                }
            }
            PendingRequest::EnumerateDevices {
                epoch,
                owner,
                started,
            } => {
                self.last_enumeration_us = elapsed_us(started);
                if !self.upower_reply_is_current(epoch, &owner, message) {
                    return;
                }
                let Ok(paths) = message.read1::<Vec<dbus::Path<'static>>>() else {
                    return;
                };
                if paths.len() > MAX_UPOWER_DEVICES_PER_PROCESS {
                    events.push(PowerTransportEvent::UPowerUnavailable {
                        connection_generation: self.connection_generation,
                        source_generation: self.upower_source_generation,
                    });
                    return;
                }
                let mut known = BTreeSet::new();
                for path in paths {
                    let path = path.to_string();
                    if path == DISPLAY_DEVICE_PATH {
                        continue;
                    }
                    if !is_enumerated_device_path(&path) || !known.insert(path) {
                        events.push(PowerTransportEvent::UPowerUnavailable {
                            connection_generation: self.connection_generation,
                            source_generation: self.upower_source_generation,
                        });
                        return;
                    }
                }
                self.known_device_paths = known;
                self.raw_upower
                    .devices
                    .retain(|path, _| self.known_device_paths.contains(path));
                self.enumeration_ready = true;
                let paths: Vec<_> = self.known_device_paths.iter().cloned().collect();
                for path in paths {
                    let _ = self.request_device(&path);
                }
                self.emit_upower_if_ready(events);
            }
            PendingRequest::Device {
                epoch,
                owner,
                path,
                started,
            } => {
                self.last_device_read_us = elapsed_us(started);
                self.last_property_read_us = self.last_device_read_us;
                if !self.upower_reply_is_current(epoch, &owner, message)
                    || !self.known_device_paths.contains(&path)
                {
                    return;
                }
                if let Ok(map) = message.read1::<PropMap>() {
                    self.raw_upower
                        .devices
                        .insert(path, decode_device_map(&map));
                    self.emit_upower_if_ready(events);
                }
            }
            PendingRequest::Profiles {
                epoch,
                owner,
                started,
            } => {
                self.last_profiles_read_us = elapsed_us(started);
                self.last_property_read_us = self.last_profiles_read_us;
                if !self.profile_reply_is_current(epoch, &owner, message) {
                    return;
                }
                if let Ok(map) = message.read1::<PropMap>() {
                    self.raw_profiles = decode_profiles_map(&map, self.profile_source_generation);
                    events.push(PowerTransportEvent::ProfilesSnapshot {
                        connection_generation: self.connection_generation,
                        snapshot: self.raw_profiles.clone(),
                    });
                }
            }
            PendingRequest::SetProfile {
                epoch,
                owner,
                source_generation,
                profile,
                started,
            } => {
                let _elapsed = elapsed_us(started);
                if epoch != self.profile_epoch
                    || source_generation != self.profile_source_generation
                    || self.profile_owner.as_deref() != Some(owner.as_str())
                {
                    return;
                }
                let succeeded = message.as_result().is_ok();
                events.push(PowerTransportEvent::ProfileRequestResult {
                    connection_generation: self.connection_generation,
                    source_generation,
                    profile,
                    succeeded,
                });
                if succeeded {
                    let _ = self.request_profiles();
                }
            }
        }
    }

    fn process_owner_changed(&mut self, message: &Message, events: &mut Vec<PowerTransportEvent>) {
        let Ok((name, _old_owner, new_owner)) = message.read3::<String, String, String>() else {
            return;
        };
        let service = match name.as_str() {
            UPOWER_SERVICE => ServiceKind::UPower,
            PROFILE_SERVICE => ServiceKind::Profiles,
            _ => return,
        };
        match service {
            ServiceKind::UPower => self.upower_epoch = self.upower_epoch.saturating_add(1),
            ServiceKind::Profiles => self.profile_epoch = self.profile_epoch.saturating_add(1),
        }
        self.set_owner(
            service,
            (!new_owner.is_empty()).then_some(new_owner),
            events,
        );
    }

    fn set_owner(
        &mut self,
        service: ServiceKind,
        owner: Option<String>,
        events: &mut Vec<PowerTransportEvent>,
    ) {
        match service {
            ServiceKind::UPower => {
                if self.upower_owner == owner {
                    return;
                }
                let replacing_owner = self.upower_owner.is_some() && owner.is_some();
                self.pending.retain(|_, request| {
                    !matches!(
                        request,
                        PendingRequest::UPowerRoot { .. }
                            | PendingRequest::DisplayDevice { .. }
                            | PendingRequest::EnumerateDevices { .. }
                            | PendingRequest::Device { .. }
                    )
                });
                self.upower_owner = owner;
                if self.upower_owner.is_some() {
                    self.upower_source_generation = self.upower_source_generation.saturating_add(1);
                    if replacing_owner {
                        events.push(PowerTransportEvent::UPowerUnavailable {
                            connection_generation: self.connection_generation,
                            source_generation: self.upower_source_generation,
                        });
                    }
                    let _ = self.begin_upower();
                } else {
                    self.raw_upower = RawUPowerSnapshot {
                        source_generation: self.upower_source_generation,
                        ..RawUPowerSnapshot::default()
                    };
                    self.known_device_paths.clear();
                    events.push(PowerTransportEvent::UPowerUnavailable {
                        connection_generation: self.connection_generation,
                        source_generation: self.upower_source_generation,
                    });
                }
            }
            ServiceKind::Profiles => {
                if self.profile_owner == owner {
                    return;
                }
                let replacing_owner = self.profile_owner.is_some() && owner.is_some();
                self.pending.retain(|_, request| {
                    !matches!(
                        request,
                        PendingRequest::Profiles { .. } | PendingRequest::SetProfile { .. }
                    )
                });
                self.profile_owner = owner;
                if self.profile_owner.is_some() {
                    self.profile_source_generation =
                        self.profile_source_generation.saturating_add(1);
                    if replacing_owner {
                        events.push(PowerTransportEvent::ProfilesUnavailable {
                            connection_generation: self.connection_generation,
                            source_generation: self.profile_source_generation,
                        });
                    }
                    let _ = self.begin_profiles();
                } else {
                    self.raw_profiles = RawProfilesSnapshot {
                        source_generation: self.profile_source_generation,
                        ..RawProfilesSnapshot::default()
                    };
                    events.push(PowerTransportEvent::ProfilesUnavailable {
                        connection_generation: self.connection_generation,
                        source_generation: self.profile_source_generation,
                    });
                }
            }
        }
    }

    fn emit_upower_if_ready(&self, events: &mut Vec<PowerTransportEvent>) {
        if !self.root_ready || !self.display_ready || !self.enumeration_ready {
            return;
        }
        if self.known_device_paths.len() != self.raw_upower.devices.len()
            || self.pending.values().any(|request| {
                matches!(
                    request,
                    PendingRequest::Device { epoch, .. } if *epoch == self.upower_epoch
                )
            })
        {
            return;
        }
        events.push(PowerTransportEvent::UPowerSnapshot {
            connection_generation: self.connection_generation,
            snapshot: self.raw_upower.clone(),
        });
    }

    fn upower_reply_is_current(&self, epoch: u64, owner: &str, message: &Message) -> bool {
        epoch == self.upower_epoch
            && self.upower_owner.as_deref() == Some(owner)
            && message.sender().map(|sender| sender.to_string()).as_deref() == Some(owner)
    }

    fn profile_reply_is_current(&self, epoch: u64, owner: &str, message: &Message) -> bool {
        epoch == self.profile_epoch
            && self.profile_owner.as_deref() == Some(owner)
            && message.sender().map(|sender| sender.to_string()).as_deref() == Some(owner)
    }

    fn reply_failure(&self, operation: &str, message: &mut Message) -> PowerTransportEvent {
        PowerTransportEvent::BusDisconnected {
            connection_generation: self.connection_generation,
            reason: message_error(message, operation),
        }
    }
}

fn message_matches(message: &Message, path: &str, interface: &str, member: &str) -> bool {
    message.path().map(|value| value.to_string()).as_deref() == Some(path)
        && message
            .interface()
            .map(|value| value.to_string())
            .as_deref()
            == Some(interface)
        && message.member().map(|value| value.to_string()).as_deref() == Some(member)
}

fn message_error(message: &mut Message, fallback: &str) -> String {
    message
        .as_result()
        .err()
        .map(|error| {
            let name = error.name().unwrap_or("org.freedesktop.DBus.Error.Failed");
            format!("{name}: {error}")
        })
        .unwrap_or_else(|| fallback.into())
}

fn properties_signal_is_relevant(message: &Message, path: &str) -> bool {
    let (expected_interface, properties): (&str, &[&str]) = if path == UPOWER_PATH {
        (UPOWER_INTERFACE, &["OnBattery"])
    } else if path == PROFILE_PATH {
        (
            PROFILE_INTERFACE,
            &[
                "ActiveProfile",
                "Profiles",
                "ActiveProfileHolds",
                "PerformanceDegraded",
            ],
        )
    } else if path == DISPLAY_DEVICE_PATH || path.starts_with("/org/freedesktop/UPower/devices/") {
        (
            DEVICE_INTERFACE,
            &[
                "Type",
                "PowerSupply",
                "Energy",
                "EnergyFull",
                "EnergyRate",
                "TimeToEmpty",
                "TimeToFull",
                "Percentage",
                "IsPresent",
                "State",
                "Capacity",
                "IconName",
                "NativePath",
                "Model",
                "WarningLevel",
            ],
        )
    } else {
        return false;
    };
    message
        .read3::<String, PropMap, Vec<String>>()
        .map(|(interface, changed, invalidated)| {
            interface == expected_interface
                && changed
                    .keys()
                    .chain(invalidated.iter())
                    .any(|name| properties.contains(&name.as_str()))
        })
        .unwrap_or(true)
}

fn is_enumerated_device_path(path: &str) -> bool {
    path.starts_with("/org/freedesktop/UPower/devices/") && path != DISPLAY_DEVICE_PATH
}

fn decode_device_map(map: &PropMap) -> RawDeviceProperties {
    let mut malformed_fields = 0u64;
    RawDeviceProperties {
        device_type: typed_property(map, "Type", &mut malformed_fields),
        power_supply: typed_property(map, "PowerSupply", &mut malformed_fields),
        energy: typed_property(map, "Energy", &mut malformed_fields),
        energy_full: typed_property(map, "EnergyFull", &mut malformed_fields),
        energy_rate: typed_property(map, "EnergyRate", &mut malformed_fields),
        time_to_empty: typed_property(map, "TimeToEmpty", &mut malformed_fields),
        time_to_full: typed_property(map, "TimeToFull", &mut malformed_fields),
        percentage: typed_property(map, "Percentage", &mut malformed_fields),
        is_present: typed_property(map, "IsPresent", &mut malformed_fields),
        state: typed_property(map, "State", &mut malformed_fields),
        capacity: typed_property(map, "Capacity", &mut malformed_fields),
        icon_name: typed_property_string(map, "IconName", &mut malformed_fields),
        native_path: typed_property_string(map, "NativePath", &mut malformed_fields),
        model: typed_property_string(map, "Model", &mut malformed_fields),
        warning: typed_property(map, "WarningLevel", &mut malformed_fields),
        malformed_fields,
    }
}

fn decode_profiles_map(map: &PropMap, source_generation: u64) -> RawProfilesSnapshot {
    let mut malformed_fields = 0u64;
    let active_profile = typed_property_string(map, "ActiveProfile", &mut malformed_fields);
    let degradation = typed_property_string(map, "PerformanceDegraded", &mut malformed_fields);
    let profiles = arg::prop_cast::<Vec<PropMap>>(map, "Profiles")
        .map(|profiles| {
            profiles
                .iter()
                .filter_map(|profile| arg::prop_cast::<String>(profile, "Profile").cloned())
                .collect()
        })
        .unwrap_or_else(|| {
            malformed_fields = malformed_fields.saturating_add(1);
            Vec::new()
        });
    let holds = arg::prop_cast::<Vec<PropMap>>(map, "ActiveProfileHolds")
        .map(|holds| {
            if holds.len() > MAX_POWER_PROFILE_HOLDS_PER_PROCESS {
                malformed_fields = malformed_fields.saturating_add(1);
                return Vec::new();
            }
            holds
                .iter()
                .filter_map(|hold| {
                    let profile = arg::prop_cast::<String>(hold, "Profile")?.clone();
                    let application_id = arg::prop_cast::<String>(hold, "ApplicationId")?.clone();
                    let reason = arg::prop_cast::<String>(hold, "Reason")?.clone();
                    if profile.len() > MAX_SERVICE_STRING_BYTES
                        || application_id.len() > MAX_SERVICE_STRING_BYTES
                        || reason.len() > MAX_SERVICE_STRING_BYTES
                    {
                        return None;
                    }
                    Some(RawProfileHold {
                        profile,
                        application_id,
                        reason,
                    })
                })
                .collect()
        })
        .unwrap_or_else(|| {
            malformed_fields = malformed_fields.saturating_add(1);
            Vec::new()
        });
    RawProfilesSnapshot {
        available: true,
        source_generation,
        active_profile,
        profiles,
        holds,
        degradation,
        malformed_fields,
    }
}

fn typed_property<T: Copy + 'static>(
    map: &PropMap,
    name: &str,
    malformed_fields: &mut u64,
) -> Option<T> {
    let value = arg::prop_cast::<T>(map, name).copied();
    if value.is_none() {
        *malformed_fields = malformed_fields.saturating_add(1);
    }
    value
}

fn typed_property_string(map: &PropMap, name: &str, malformed_fields: &mut u64) -> Option<String> {
    let value = arg::prop_cast::<String>(map, name).cloned();
    if value.is_none() {
        *malformed_fields = malformed_fields.saturating_add(1);
    }
    value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeadlineMode {
    RequestTimeout,
    Retry,
}

#[derive(Debug)]
struct PowerDeadline {
    fd: OwnedFd,
    mode: Option<DeadlineMode>,
}

impl PowerDeadline {
    fn new() -> Result<Self, String> {
        let fd = timerfd_create(
            TimerfdClockId::Monotonic,
            TimerfdFlags::CLOEXEC | TimerfdFlags::NONBLOCK,
        )
        .map_err(|error| format!("create power deadline timerfd: {error}"))?;
        Ok(Self { fd, mode: None })
    }

    fn arm(&mut self, duration: Duration, mode: DeadlineMode) -> Result<(), String> {
        let seconds = i64::try_from(duration.as_secs())
            .map_err(|_| "power deadline seconds exceed timerfd range".to_owned())?;
        timerfd_settime(
            &self.fd,
            TimerfdTimerFlags::empty(),
            &Itimerspec {
                it_interval: Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
                it_value: Timespec {
                    tv_sec: seconds,
                    tv_nsec: i64::from(duration.subsec_nanos()),
                },
            },
        )
        .map_err(|error| format!("arm power deadline timerfd: {error}"))?;
        self.mode = Some(mode);
        Ok(())
    }

    fn disarm(&mut self) -> Result<(), String> {
        timerfd_settime(
            &self.fd,
            TimerfdTimerFlags::empty(),
            &Itimerspec {
                it_interval: Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
                it_value: Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
            },
        )
        .map_err(|error| format!("disarm power deadline timerfd: {error}"))?;
        self.mode = None;
        Ok(())
    }

    fn poll_fd(&self) -> Option<BorrowedFd<'_>> {
        self.mode.map(|_| std::os::fd::AsFd::as_fd(&self.fd))
    }

    fn consume(&mut self) -> Result<Option<DeadlineMode>, String> {
        let mut bytes = [0_u8; std::mem::size_of::<u64>()];
        match rustix::io::read(&self.fd, &mut bytes) {
            Ok(length) if length == bytes.len() => Ok(self.mode.take()),
            Ok(length) => Err(format!(
                "power deadline timerfd returned {length} bytes instead of {}",
                bytes.len()
            )),
            Err(error) if error == rustix::io::Errno::AGAIN => Ok(None),
            Err(error) => Err(format!("read power deadline timerfd: {error}")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PowerServiceSummary {
    pub transport: String,
    pub lifecycle_state: String,
    pub subscribers: usize,
    pub upower_subscribers: usize,
    pub profile_subscribers: usize,
    pub maximum_subscribers: usize,
    pub source_generation: u64,
    pub profile_source_generation: u64,
    pub sequence: u64,
    pub availability: String,
    pub on_battery: String,
    pub percentage: Option<u8>,
    pub charge_state: String,
    pub warning: String,
    pub profile: String,
    pub profile_available: bool,
    pub performance_available: bool,
    pub degradation: String,
    pub device_count: usize,
    pub hold_count: usize,
    pub system_bus_connections: u64,
    pub connection_failures: u64,
    pub service_appearances: u64,
    pub service_disappearances: u64,
    pub owner_replacements: u64,
    pub property_signals: u64,
    pub irrelevant_signals: u64,
    pub property_bursts: u64,
    pub refreshes: u64,
    pub refresh_failures: u64,
    pub bus_disconnects: u64,
    pub reconnect_attempts: u64,
    pub retry_wakeups: u64,
    pub request_timeouts: u64,
    pub stale_events_contained: u64,
    pub messages_drained: u64,
    pub maximum_messages_per_dispatch: usize,
    pub malformed_values: u64,
    pub changed_snapshots: u64,
    pub duplicate_snapshots_suppressed: u64,
    pub initial_connection_us: u64,
    pub last_owner_lookup_us: u64,
    pub last_property_read_us: u64,
    pub last_enumeration_us: u64,
    pub last_device_read_us: u64,
    pub last_profiles_read_us: u64,
    pub last_signal_to_refresh_us: u64,
    pub last_refresh_us: u64,
    pub last_owner_loss_us: u64,
    pub last_reconnect_us: u64,
    pub last_normalization_us: u64,
    pub last_projection_us: u64,
    pub transport_descriptors: usize,
    pub maximum_transport_descriptors: usize,
    pub dbus_watch_count_peak: usize,
    pub match_rules_installed: usize,
    pub deadline_descriptors: usize,
    pub explicit_worker_threads: usize,
    pub internal_threads: usize,
    pub documents_visited: u64,
    pub elements_mutated: u64,
    pub fanout_us: u64,
    pub frames_scheduled: u64,
    pub unrelated_frames_scheduled: u64,
    pub closed_surface_frames_suppressed: u64,
    pub mutation_failures_contained: u64,
    pub profile_requests: u64,
    pub profile_request_failures: u64,
}

pub type BatteryServiceSummary = PowerServiceSummary;

struct PowerCore {
    upower_subscribers: usize,
    profile_subscribers: usize,
    connection_generation: u64,
    sequence: u64,
    raw_upower: RawUPowerSnapshot,
    raw_profiles: RawProfilesSnapshot,
    snapshot: PowerSnapshot,
    summary: PowerServiceSummary,
}

impl Default for PowerCore {
    fn default() -> Self {
        Self {
            upower_subscribers: 0,
            profile_subscribers: 0,
            connection_generation: 0,
            sequence: 0,
            raw_upower: RawUPowerSnapshot::default(),
            raw_profiles: RawProfilesSnapshot::default(),
            snapshot: PowerSnapshot::unavailable(),
            summary: PowerServiceSummary {
                transport: "shared-libdbus-watch".into(),
                lifecycle_state: "dormant".into(),
                maximum_messages_per_dispatch: MAX_MESSAGES_PER_DISPATCH,
                ..PowerServiceSummary::default()
            },
        }
    }
}

impl PowerCore {
    fn subscriber_count(&self) -> usize {
        self.upower_subscribers
            .saturating_add(self.profile_subscribers)
    }

    fn next_connection_generation(&mut self) -> u64 {
        self.connection_generation = self.connection_generation.saturating_add(1);
        self.connection_generation
    }

    fn apply_event(&mut self, event: PowerTransportEvent) -> Option<PowerSnapshot> {
        let connection_generation = match &event {
            PowerTransportEvent::UPowerUnavailable {
                connection_generation,
                ..
            }
            | PowerTransportEvent::UPowerSnapshot {
                connection_generation,
                ..
            }
            | PowerTransportEvent::ProfilesUnavailable {
                connection_generation,
                ..
            }
            | PowerTransportEvent::ProfilesSnapshot {
                connection_generation,
                ..
            }
            | PowerTransportEvent::ProfileRequestResult {
                connection_generation,
                ..
            }
            | PowerTransportEvent::BusDisconnected {
                connection_generation,
                ..
            } => *connection_generation,
        };
        if connection_generation != self.connection_generation {
            self.summary.stale_events_contained =
                self.summary.stale_events_contained.saturating_add(1);
            return None;
        }
        match event {
            PowerTransportEvent::UPowerUnavailable {
                source_generation, ..
            } => {
                if source_generation < self.raw_upower.source_generation {
                    self.summary.stale_events_contained =
                        self.summary.stale_events_contained.saturating_add(1);
                    return None;
                }
                if self.raw_upower.available
                    && source_generation > self.raw_upower.source_generation
                {
                    self.summary.owner_replacements =
                        self.summary.owner_replacements.saturating_add(1);
                }
                if self.raw_upower.available {
                    self.summary.service_disappearances =
                        self.summary.service_disappearances.saturating_add(1);
                }
                self.raw_upower = RawUPowerSnapshot {
                    source_generation,
                    ..RawUPowerSnapshot::default()
                };
            }
            PowerTransportEvent::UPowerSnapshot { snapshot, .. } => {
                if snapshot.source_generation < self.raw_upower.source_generation {
                    self.summary.stale_events_contained =
                        self.summary.stale_events_contained.saturating_add(1);
                    return None;
                }
                if !self.raw_upower.available && snapshot.available {
                    self.summary.service_appearances =
                        self.summary.service_appearances.saturating_add(1);
                } else if self.raw_upower.available
                    && snapshot.source_generation > self.raw_upower.source_generation
                {
                    self.summary.owner_replacements =
                        self.summary.owner_replacements.saturating_add(1);
                }
                self.raw_upower = snapshot;
                self.summary.refreshes = self.summary.refreshes.saturating_add(1);
                self.summary.source_generation = self.raw_upower.source_generation;
            }
            PowerTransportEvent::ProfilesUnavailable {
                source_generation, ..
            } => {
                if source_generation < self.raw_profiles.source_generation {
                    self.summary.stale_events_contained =
                        self.summary.stale_events_contained.saturating_add(1);
                    return None;
                }
                if self.raw_profiles.available
                    && source_generation > self.raw_profiles.source_generation
                {
                    self.summary.owner_replacements =
                        self.summary.owner_replacements.saturating_add(1);
                }
                if self.raw_profiles.available {
                    self.summary.service_disappearances =
                        self.summary.service_disappearances.saturating_add(1);
                }
                self.raw_profiles = RawProfilesSnapshot {
                    source_generation,
                    ..RawProfilesSnapshot::default()
                };
            }
            PowerTransportEvent::ProfilesSnapshot { snapshot, .. } => {
                if snapshot.source_generation < self.raw_profiles.source_generation {
                    self.summary.stale_events_contained =
                        self.summary.stale_events_contained.saturating_add(1);
                    return None;
                }
                if !self.raw_profiles.available && snapshot.available {
                    self.summary.service_appearances =
                        self.summary.service_appearances.saturating_add(1);
                } else if self.raw_profiles.available
                    && snapshot.source_generation > self.raw_profiles.source_generation
                {
                    self.summary.owner_replacements =
                        self.summary.owner_replacements.saturating_add(1);
                }
                self.raw_profiles = snapshot;
                self.summary.refreshes = self.summary.refreshes.saturating_add(1);
                self.summary.profile_source_generation = self.raw_profiles.source_generation;
            }
            PowerTransportEvent::ProfileRequestResult {
                source_generation,
                succeeded,
                ..
            } => {
                if source_generation != self.raw_profiles.source_generation {
                    self.summary.stale_events_contained =
                        self.summary.stale_events_contained.saturating_add(1);
                    return None;
                }
                if !succeeded {
                    self.summary.profile_request_failures =
                        self.summary.profile_request_failures.saturating_add(1);
                }
                return None;
            }
            PowerTransportEvent::BusDisconnected { reason, .. } => {
                let _ = reason;
                self.summary.bus_disconnects = self.summary.bus_disconnects.saturating_add(1);
                self.raw_upower = RawUPowerSnapshot {
                    source_generation: self.raw_upower.source_generation,
                    ..RawUPowerSnapshot::default()
                };
                self.raw_profiles = RawProfilesSnapshot {
                    source_generation: self.raw_profiles.source_generation,
                    ..RawProfilesSnapshot::default()
                };
            }
        }
        let started = Instant::now();
        let next_sequence = self.sequence.saturating_add(1);
        let (mut snapshot, malformed) =
            normalize_power(&self.raw_upower, &self.raw_profiles, next_sequence);
        self.summary.last_normalization_us = elapsed_us(started);
        self.summary.last_refresh_us = self.summary.last_normalization_us;
        self.summary.malformed_values = self.summary.malformed_values.saturating_add(malformed);
        if self.snapshot.semantically_eq(&snapshot) {
            self.summary.duplicate_snapshots_suppressed = self
                .summary
                .duplicate_snapshots_suppressed
                .saturating_add(1);
            return None;
        }
        self.sequence = next_sequence;
        snapshot.sequence = self.sequence;
        snapshot.battery.sequence = self.sequence;
        for device in &mut snapshot.devices {
            device.sequence = self.sequence;
        }
        self.snapshot = snapshot.clone();
        self.summary.sequence = self.sequence;
        self.summary.changed_snapshots = self.summary.changed_snapshots.saturating_add(1);
        self.summary.availability = snapshot.battery.availability.as_str().into();
        self.summary.on_battery = match snapshot.on_battery {
            Some(true) => "battery",
            Some(false) => "external",
            None => "unavailable",
        }
        .into();
        self.summary.percentage = snapshot.battery.percentage;
        self.summary.charge_state = snapshot.battery.charge_state.text().into();
        self.summary.warning = format!("{:?}", snapshot.battery.warning).to_lowercase();
        self.summary.profile = snapshot.profiles.current.wire().into();
        self.summary.profile_available = snapshot.profiles.available;
        self.summary.performance_available = snapshot.profiles.performance_available;
        self.summary.degradation = snapshot.profiles.degradation.token().as_str().into();
        self.summary.device_count = snapshot.devices.len();
        self.summary.hold_count = snapshot.profiles.holds.len();
        Some(snapshot)
    }
}

#[derive(Debug, Default)]
pub(crate) struct PowerFanoutMetrics {
    pub documents: usize,
    pub elements: usize,
    pub frames: usize,
    pub closed_frames_suppressed: usize,
    pub failures: usize,
    pub fanout_us: u64,
    pub projection_us: u64,
}

#[derive(Default)]
pub(crate) struct PowerService {
    core: PowerCore,
    transport: Option<PowerTransport>,
    deadline: Option<PowerDeadline>,
    reconnect_index: usize,
    pending_profile: Option<PowerProfile>,
    queued_profile: Option<PowerProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileRequestDecision {
    Ignore,
    Queue,
    Send,
}

fn profile_request_decision(
    snapshot: &PowerProfilesSnapshot,
    pending: Option<PowerProfile>,
    queued: Option<PowerProfile>,
    profile: PowerProfile,
) -> Result<ProfileRequestDecision, String> {
    if !snapshot.available {
        return Err("power-profiles-daemon is unavailable".into());
    }
    if profile == PowerProfile::Unknown {
        return Err("cannot request an unknown power profile".into());
    }
    if profile == PowerProfile::Performance && !snapshot.performance_available {
        return Err("performance profile is unavailable".into());
    }
    if snapshot.current == profile && pending.is_none() {
        return Ok(ProfileRequestDecision::Ignore);
    }
    if pending == Some(profile) || queued == Some(profile) {
        return Ok(ProfileRequestDecision::Ignore);
    }
    if pending.is_some() {
        return Ok(ProfileRequestDecision::Queue);
    }
    Ok(ProfileRequestDecision::Send)
}

impl PowerService {
    pub(crate) fn upower_subscriber_count(&self) -> usize {
        self.core.upower_subscribers
    }

    pub(crate) fn profile_subscriber_count(&self) -> usize {
        self.core.profile_subscribers
    }

    pub(crate) fn current_snapshot(&self) -> &PowerSnapshot {
        &self.core.snapshot
    }

    pub(crate) fn summary(&self) -> PowerServiceSummary {
        let mut summary = self.core.summary.clone();
        summary.subscribers = self.core.subscriber_count();
        summary.upower_subscribers = self.core.upower_subscribers;
        summary.profile_subscribers = self.core.profile_subscribers;
        summary.transport_descriptors = usize::from(self.transport.is_some());
        summary.deadline_descriptors = usize::from(self.deadline.is_some());
        summary
    }

    pub(crate) fn bus_watch(&self) -> Option<Watch> {
        self.transport.as_ref().map(PowerTransport::watch)
    }

    pub(crate) fn deadline_fd(&self) -> Option<BorrowedFd<'_>> {
        self.deadline.as_ref().and_then(PowerDeadline::poll_fd)
    }

    pub(crate) fn needs_immediate_dispatch(&self) -> bool {
        self.transport
            .as_ref()
            .is_some_and(PowerTransport::needs_immediate_drain)
    }

    pub(crate) fn set_subscriber_counts(
        &mut self,
        upower: usize,
        profiles: usize,
    ) -> Option<PowerSnapshot> {
        let previous = self.core.subscriber_count();
        self.core.upower_subscribers = upower;
        self.core.profile_subscribers = profiles;
        let current = self.core.subscriber_count();
        self.core.summary.maximum_subscribers = self.core.summary.maximum_subscribers.max(current);
        match (previous, current) {
            (0, count) if count > 0 => {
                self.core.summary.lifecycle_state = "connecting".into();
                self.start_transport(false);
                Some(self.core.snapshot.clone())
            }
            (count, 0) if count > 0 => {
                self.stop_source();
                None
            }
            (old, new) if new > old => Some(self.core.snapshot.clone()),
            _ => None,
        }
    }

    pub(crate) fn request_profile(&mut self, profile: PowerProfile) -> Result<bool, String> {
        match profile_request_decision(
            &self.core.snapshot.profiles,
            self.pending_profile,
            self.queued_profile,
            profile,
        )? {
            ProfileRequestDecision::Ignore => return Ok(false),
            ProfileRequestDecision::Queue => {
                self.queued_profile = Some(profile);
                return Ok(true);
            }
            ProfileRequestDecision::Send => {}
        }
        self.transport
            .as_mut()
            .ok_or_else(|| "system-bus transport is unavailable".to_owned())?
            .set_profile(profile)?;
        self.pending_profile = Some(profile);
        self.core.summary.profile_requests = self.core.summary.profile_requests.saturating_add(1);
        self.sync_request_deadline();
        Ok(true)
    }

    pub(crate) fn handle_bus_ready(&mut self) -> Option<PowerSnapshot> {
        let started = Instant::now();
        let dispatch = self.transport.as_mut()?.process_ready();
        self.core.summary.messages_drained = self
            .core
            .summary
            .messages_drained
            .saturating_add(dispatch.messages_drained as u64);
        self.core.summary.property_signals = self
            .core
            .summary
            .property_signals
            .saturating_add(dispatch.relevant_property_signals as u64);
        self.core.summary.irrelevant_signals = self
            .core
            .summary
            .irrelevant_signals
            .saturating_add(dispatch.irrelevant_property_signals as u64);
        if dispatch.relevant_property_signals > 1 {
            self.core.summary.property_bursts = self.core.summary.property_bursts.saturating_add(1);
        }
        if dispatch.relevant_property_signals > 0 {
            self.core.summary.last_signal_to_refresh_us = elapsed_us(started);
        }
        if dispatch.owner_lookup_us > 0 {
            self.core.summary.last_owner_lookup_us = dispatch.owner_lookup_us;
        }
        if dispatch.property_read_us > 0 {
            self.core.summary.last_property_read_us = dispatch.property_read_us;
        }
        if dispatch.enumeration_us > 0 {
            self.core.summary.last_enumeration_us = dispatch.enumeration_us;
        }
        if dispatch.device_read_us > 0 {
            self.core.summary.last_device_read_us = dispatch.device_read_us;
        }
        if dispatch.profiles_read_us > 0 {
            self.core.summary.last_profiles_read_us = dispatch.profiles_read_us;
        }
        let mut latest = None;
        let mut disconnected = false;
        for event in dispatch.events {
            if matches!(
                &event,
                PowerTransportEvent::UPowerSnapshot { .. }
                    | PowerTransportEvent::ProfilesSnapshot { .. }
            ) {
                self.reconnect_index = 0;
            }
            if matches!(&event, PowerTransportEvent::BusDisconnected { .. }) {
                disconnected = true;
            }
            if matches!(&event, PowerTransportEvent::ProfilesUnavailable { .. }) {
                self.pending_profile = None;
                self.queued_profile = None;
            }
            if let PowerTransportEvent::ProfilesSnapshot { snapshot, .. } = &event
                && self.core.raw_profiles.source_generation != 0
                && snapshot.source_generation != self.core.raw_profiles.source_generation
            {
                self.pending_profile = None;
                self.queued_profile = None;
            }
            if let PowerTransportEvent::ProfileRequestResult {
                connection_generation,
                source_generation,
                profile,
                succeeded: _,
            } = &event
                && *connection_generation == self.core.connection_generation
                && *source_generation == self.core.raw_profiles.source_generation
                && self.pending_profile == Some(*profile)
            {
                self.pending_profile = None;
            }
            if let Some(snapshot) = self.core.apply_event(event) {
                latest = Some(snapshot);
            }
        }
        if disconnected {
            self.transport = None;
            self.pending_profile = None;
            self.queued_profile = None;
            self.schedule_retry();
        } else {
            if self.pending_profile.is_none()
                && let Some(profile) = self.queued_profile.take()
            {
                let _ = self.request_profile(profile);
            }
            self.sync_request_deadline();
        }
        latest
    }

    pub(crate) fn handle_immediate_dispatch(&mut self) -> Option<PowerSnapshot> {
        self.handle_bus_ready()
    }

    pub(crate) fn handle_bus_failure(
        &mut self,
        reason: impl Into<String>,
    ) -> Option<PowerSnapshot> {
        let generation = self.core.connection_generation;
        self.transport = None;
        self.pending_profile = None;
        self.queued_profile = None;
        let snapshot = self.core.apply_event(PowerTransportEvent::BusDisconnected {
            connection_generation: generation,
            reason: reason.into(),
        });
        self.schedule_retry();
        snapshot
    }

    pub(crate) fn handle_deadline_failure(
        &mut self,
        reason: impl Into<String>,
    ) -> Option<PowerSnapshot> {
        self.deadline = None;
        self.handle_bus_failure(reason)
    }

    pub(crate) fn handle_deadline_ready(&mut self) -> Option<PowerSnapshot> {
        let mode = self
            .deadline
            .as_mut()
            .and_then(|deadline| deadline.consume().ok().flatten())?;
        match mode {
            DeadlineMode::RequestTimeout => {
                self.core.summary.request_timeouts =
                    self.core.summary.request_timeouts.saturating_add(1);
                self.handle_bus_failure("power D-Bus request timed out")
            }
            DeadlineMode::Retry => {
                self.core.summary.retry_wakeups = self.core.summary.retry_wakeups.saturating_add(1);
                self.start_transport(true);
                None
            }
        }
    }

    pub(crate) fn record_fanout(&mut self, metrics: PowerFanoutMetrics) {
        self.core.summary.documents_visited = self
            .core
            .summary
            .documents_visited
            .saturating_add(metrics.documents as u64);
        self.core.summary.elements_mutated = self
            .core
            .summary
            .elements_mutated
            .saturating_add(metrics.elements as u64);
        self.core.summary.frames_scheduled = self
            .core
            .summary
            .frames_scheduled
            .saturating_add(metrics.frames as u64);
        self.core.summary.closed_surface_frames_suppressed = self
            .core
            .summary
            .closed_surface_frames_suppressed
            .saturating_add(metrics.closed_frames_suppressed as u64);
        self.core.summary.mutation_failures_contained = self
            .core
            .summary
            .mutation_failures_contained
            .saturating_add(metrics.failures as u64);
        self.core.summary.fanout_us = metrics.fanout_us;
        self.core.summary.last_projection_us = metrics.projection_us;
    }

    pub(crate) fn shutdown(&mut self) {
        self.transport = None;
        self.deadline = None;
        self.pending_profile = None;
        self.queued_profile = None;
        self.core.upower_subscribers = 0;
        self.core.profile_subscribers = 0;
        self.core.summary.lifecycle_state = "dormant".into();
    }

    fn start_transport(&mut self, reconnect: bool) {
        if self.core.subscriber_count() == 0 {
            return;
        }
        if reconnect {
            self.core.summary.reconnect_attempts =
                self.core.summary.reconnect_attempts.saturating_add(1);
        }
        let started = Instant::now();
        let generation = self.core.next_connection_generation();
        self.core.summary.lifecycle_state = "connecting".into();
        match PowerTransport::connect(generation) {
            Ok(transport) => {
                self.transport = Some(transport);
                self.core.summary.system_bus_connections =
                    self.core.summary.system_bus_connections.saturating_add(1);
                self.core.summary.maximum_transport_descriptors = 1;
                self.core.summary.dbus_watch_count_peak = 1;
                self.core.summary.match_rules_installed = 6;
                if self.core.summary.initial_connection_us == 0 {
                    self.core.summary.initial_connection_us = elapsed_us(started);
                }
                if reconnect {
                    self.core.summary.last_reconnect_us = elapsed_us(started);
                }
                self.core.summary.lifecycle_state = "watching".into();
                self.sync_request_deadline();
            }
            Err(error) => {
                eprintln!("htmshell-live: power source unavailable: {error}");
                self.core.summary.connection_failures =
                    self.core.summary.connection_failures.saturating_add(1);
                self.transport = None;
                self.core.summary.lifecycle_state = "service-unavailable".into();
                self.schedule_retry();
            }
        }
    }

    fn sync_request_deadline(&mut self) {
        let pending = self
            .transport
            .as_ref()
            .is_some_and(PowerTransport::has_pending_requests);
        if pending {
            if self.deadline.as_ref().and_then(|deadline| deadline.mode)
                != Some(DeadlineMode::RequestTimeout)
                && let Err(error) = self.arm_deadline(REQUEST_TIMEOUT, DeadlineMode::RequestTimeout)
            {
                eprintln!("htmshell-live: power request timeout unavailable: {error}");
            }
        } else if self.deadline.as_ref().and_then(|deadline| deadline.mode)
            == Some(DeadlineMode::RequestTimeout)
            && let Some(deadline) = self.deadline.as_mut()
            && let Err(error) = deadline.disarm()
        {
            eprintln!("htmshell-live: power deadline disarm failed: {error}");
        }
    }

    fn schedule_retry(&mut self) {
        if self.core.subscriber_count() == 0 {
            return;
        }
        let delay = RECONNECT_DELAYS[self.reconnect_index.min(RECONNECT_DELAYS.len() - 1)];
        self.reconnect_index = self
            .reconnect_index
            .saturating_add(1)
            .min(RECONNECT_DELAYS.len() - 1);
        if let Err(error) = self.arm_deadline(delay, DeadlineMode::Retry) {
            eprintln!("htmshell-live: power reconnect scheduling failed: {error}");
        }
    }

    fn arm_deadline(&mut self, duration: Duration, mode: DeadlineMode) -> Result<(), String> {
        if self.deadline.is_none() {
            self.deadline = Some(PowerDeadline::new()?);
        }
        self.deadline
            .as_mut()
            .expect("created above")
            .arm(duration, mode)
    }

    fn stop_source(&mut self) {
        self.transport = None;
        self.deadline = None;
        self.reconnect_index = 0;
        self.pending_profile = None;
        self.queued_profile = None;
        self.core.raw_upower = RawUPowerSnapshot::default();
        self.core.raw_profiles = RawProfilesSnapshot::default();
        self.core.snapshot = PowerSnapshot::unavailable();
        self.core.summary.lifecycle_state = "dormant".into();
    }
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_device(device_type: u32, model: &str) -> RawDeviceProperties {
        RawDeviceProperties {
            device_type: Some(device_type),
            power_supply: Some(device_type == 2),
            energy: Some(44.2),
            energy_full: Some(51.4),
            energy_rate: Some(8.5),
            time_to_empty: Some(3_900),
            time_to_full: Some(0),
            percentage: Some(42.5),
            is_present: Some(true),
            state: Some(2),
            capacity: Some(88.0),
            icon_name: Some("battery-good-symbolic".into()),
            native_path: Some("BAT0".into()),
            model: Some(model.into()),
            warning: Some(1),
            malformed_fields: 0,
        }
    }

    #[test]
    fn every_device_type_and_state_is_typed() {
        for (wire, expected) in UPowerDeviceType::ALL.into_iter().enumerate() {
            assert_eq!(UPowerDeviceType::from_wire(Some(wire as u32)), expected);
            assert!(!expected.text().is_empty());
            assert!(!expected.token().as_str().is_empty());
        }
        assert_eq!(
            UPowerDeviceType::from_wire(Some(999)),
            UPowerDeviceType::Unknown
        );
        for (wire, expected) in UPowerDeviceState::ALL.into_iter().enumerate() {
            assert_eq!(UPowerDeviceState::from_wire(Some(wire as u32)), expected);
            assert!(!expected.text().is_empty());
            assert!(!expected.token().as_str().is_empty());
        }
        assert_eq!(
            UPowerDeviceState::from_wire(Some(999)),
            UPowerDeviceState::Unknown
        );
    }

    #[test]
    fn percentage_and_device_normalization_match_the_audited_semantics() {
        for (value, expected) in [(0.0, 0), (42.4, 42), (42.5, 43), (99.6, 100)] {
            assert_eq!(normalize_percentage(value), Ok(expected));
        }
        for invalid in [f64::NAN, f64::INFINITY, -0.1, 100.1] {
            assert!(normalize_percentage(invalid).is_err());
        }
        let (device, malformed) = normalize_device(&raw_device(2, "Primary"), "1:path".into(), 1);
        assert_eq!(malformed, 0);
        assert_eq!(device.change_rate, Some(-8.5));
        assert_eq!(device.percentage, Some(42.5));
        assert_eq!(device.health_supported, Some(true));
        assert_eq!(device.is_laptop_battery, Some(true));
        let mut invalid_duration = raw_device(2, "Invalid duration");
        invalid_duration.time_to_empty = Some(-1);
        let (invalid_duration, malformed) =
            normalize_device(&invalid_duration, "1:invalid".into(), 1);
        assert_eq!(invalid_duration.time_to_empty, None);
        assert_eq!(malformed, 1);

        let mut malformed_device = raw_device(999, "Malformed");
        malformed_device.energy = Some(-1.0);
        malformed_device.energy_full = Some(f64::INFINITY);
        malformed_device.energy_rate = Some(f64::NAN);
        malformed_device.percentage = Some(101.0);
        malformed_device.capacity = Some(f64::NAN);
        malformed_device.model = Some("x".repeat(MAX_SERVICE_STRING_BYTES + 1));
        malformed_device.state = Some(999);
        let (malformed_device, malformed) =
            normalize_device(&malformed_device, "1:malformed".into(), 1);
        assert_eq!(malformed_device.device_type, UPowerDeviceType::Unknown);
        assert_eq!(malformed_device.state, UPowerDeviceState::Unknown);
        assert_eq!(malformed_device.energy, None);
        assert_eq!(malformed_device.energy_capacity, None);
        assert_eq!(malformed_device.change_rate, None);
        assert_eq!(malformed_device.percentage, None);
        assert_eq!(malformed_device.health_percentage, None);
        assert_eq!(malformed_device.health_supported, None);
        assert_eq!(malformed_device.model, None);
        assert_eq!(malformed, 6);
    }

    #[test]
    fn absent_and_unavailable_are_distinct_from_external_power() {
        let profiles = RawProfilesSnapshot::default();
        let unavailable = normalize_power(&RawUPowerSnapshot::default(), &profiles, 1).0;
        assert_eq!(
            unavailable.battery.availability,
            BatteryAvailability::Unavailable
        );
        assert_eq!(unavailable.on_battery, None);

        let absent = normalize_power(
            &RawUPowerSnapshot {
                available: true,
                source_generation: 2,
                on_battery: Some(false),
                display: Some(RawDeviceProperties {
                    is_present: Some(false),
                    ..raw_device(2, "")
                }),
                devices: BTreeMap::new(),
            },
            &profiles,
            2,
        )
        .0;
        assert_eq!(absent.battery.availability, BatteryAvailability::Absent);
        assert_eq!(absent.on_battery, Some(false));
        let projections = absent.projections();
        assert!(
            projections
                .text
                .contains(&(StateBindingKey::UPowerOnBattery, "On external power".into()))
        );
    }

    #[test]
    fn devices_are_ordered_and_project_every_parity_field() {
        let upower = RawUPowerSnapshot {
            available: true,
            source_generation: 9,
            on_battery: Some(true),
            display: Some(raw_device(2, "Display")),
            devices: BTreeMap::from([
                ("/z".into(), raw_device(2, "Zulu")),
                ("/a".into(), raw_device(1, "Line")),
                ("/b".into(), raw_device(2, "Alpha")),
            ]),
        };
        let snapshot = normalize_power(&upower, &RawProfilesSnapshot::default(), 4).0;
        assert_eq!(snapshot.devices[0].device_type, UPowerDeviceType::LinePower);
        assert_eq!(snapshot.devices[1].model.as_deref(), Some("Alpha"));
        assert_eq!(snapshot.devices[2].model.as_deref(), Some("Zulu"));
        let repeat = snapshot
            .projections()
            .repeats
            .into_iter()
            .find(|repeat| repeat.source == RepeatSource::UPowerDevices)
            .unwrap();
        assert_eq!(repeat.items.len(), 3);
        let item = &repeat.items[1];
        for key in ItemBindingKey::ALL
            .into_iter()
            .filter(|key| key.source() == RepeatSource::UPowerDevices)
        {
            assert!(
                item.text.contains_key(&key)
                    || item.tokens.contains_key(&key)
                    || item.values.contains_key(&key),
                "missing item projection for {}",
                key.as_str()
            );
        }
    }

    #[test]
    fn profile_holds_have_deterministic_duplicate_identity() {
        let raw = RawProfilesSnapshot {
            available: true,
            source_generation: 3,
            active_profile: Some("balanced".into()),
            profiles: vec![
                "power-saver".into(),
                "balanced".into(),
                "performance".into(),
            ],
            holds: vec![
                RawProfileHold {
                    profile: "performance".into(),
                    application_id: "app".into(),
                    reason: "render".into(),
                },
                RawProfileHold {
                    profile: "performance".into(),
                    application_id: "app".into(),
                    reason: "render".into(),
                },
            ],
            degradation: Some("high-operating-temperature".into()),
            malformed_fields: 0,
        };
        let profiles = normalize_profiles(&raw);
        assert_eq!(profiles.current, PowerProfile::Balanced);
        assert!(profiles.performance_available);
        assert_eq!(
            profiles.degradation,
            PerformanceDegradationReason::HighTemperature
        );
        assert_eq!(profiles.holds.len(), 2);
        assert_ne!(profiles.holds[0].key, profiles.holds[1].key);
    }

    #[test]
    fn service_generations_reject_stale_events_and_duplicate_snapshots() {
        let mut core = PowerCore::default();
        core.next_connection_generation();
        let mut raw = RawUPowerSnapshot {
            available: true,
            source_generation: 1,
            on_battery: Some(false),
            display: Some(RawDeviceProperties {
                is_present: Some(false),
                ..raw_device(2, "")
            }),
            devices: BTreeMap::new(),
        };
        assert!(
            core.apply_event(PowerTransportEvent::UPowerSnapshot {
                connection_generation: 1,
                snapshot: raw.clone(),
            })
            .is_some()
        );
        assert!(
            core.apply_event(PowerTransportEvent::UPowerSnapshot {
                connection_generation: 1,
                snapshot: raw.clone(),
            })
            .is_none()
        );
        raw.on_battery = Some(true);
        assert!(
            core.apply_event(PowerTransportEvent::UPowerSnapshot {
                connection_generation: 0,
                snapshot: raw,
            })
            .is_none()
        );
        assert_eq!(core.summary.stale_events_contained, 1);
        assert_eq!(core.summary.duplicate_snapshots_suppressed, 1);
    }

    #[test]
    fn one_connection_lifecycle_is_shared_by_both_subscriber_classes() {
        let mut service = PowerService::default();
        service.core.upower_subscribers = 1;
        service.core.profile_subscribers = 1;
        assert_eq!(service.core.subscriber_count(), 2);
        service.core.summary.system_bus_connections = 1;
        assert_eq!(service.summary().system_bus_connections, 1);
        service.core.upower_subscribers = 0;
        assert_eq!(service.core.subscriber_count(), 1);
        service.core.profile_subscribers = 0;
        assert_eq!(service.core.subscriber_count(), 0);
    }

    #[test]
    fn profile_request_policy_is_bounded_and_availability_safe() {
        let unavailable = PowerProfilesSnapshot {
            available: false,
            current: PowerProfile::Unknown,
            performance_available: false,
            holds: Vec::new(),
            degradation: PerformanceDegradationReason::Unknown,
            source_generation: 0,
        };
        assert!(
            profile_request_decision(&unavailable, None, None, PowerProfile::Balanced).is_err()
        );
        let mut available = unavailable;
        available.available = true;
        available.current = PowerProfile::Balanced;
        assert_eq!(
            profile_request_decision(&available, None, None, PowerProfile::Balanced),
            Ok(ProfileRequestDecision::Ignore)
        );
        assert!(
            profile_request_decision(&available, None, None, PowerProfile::Performance).is_err()
        );
        available.performance_available = true;
        assert_eq!(
            profile_request_decision(&available, None, None, PowerProfile::Performance),
            Ok(ProfileRequestDecision::Send)
        );
        assert_eq!(
            profile_request_decision(
                &available,
                Some(PowerProfile::Performance),
                None,
                PowerProfile::PowerSaver
            ),
            Ok(ProfileRequestDecision::Queue)
        );
        assert_eq!(
            profile_request_decision(
                &available,
                Some(PowerProfile::Performance),
                Some(PowerProfile::PowerSaver),
                PowerProfile::PowerSaver
            ),
            Ok(ProfileRequestDecision::Ignore)
        );
    }

    #[test]
    fn profile_state_changes_only_after_confirmed_snapshot_and_loss_clears_it() {
        let mut core = PowerCore::default();
        core.next_connection_generation();
        let balanced = RawProfilesSnapshot {
            available: true,
            source_generation: 4,
            active_profile: Some("balanced".into()),
            profiles: vec!["balanced".into(), "performance".into()],
            holds: Vec::new(),
            degradation: Some(String::new()),
            malformed_fields: 0,
        };
        assert!(
            core.apply_event(PowerTransportEvent::ProfilesSnapshot {
                connection_generation: 1,
                snapshot: balanced.clone(),
            })
            .is_some()
        );
        assert_eq!(
            core.snapshot.profiles.current,
            PowerProfile::Balanced,
            "initial confirmed state"
        );
        assert!(
            core.apply_event(PowerTransportEvent::ProfileRequestResult {
                connection_generation: 1,
                source_generation: 4,
                profile: PowerProfile::Performance,
                succeeded: true,
            })
            .is_none()
        );
        assert_eq!(
            core.snapshot.profiles.current,
            PowerProfile::Balanced,
            "request reply is not an optimistic state update"
        );
        let mut performance = balanced;
        performance.active_profile = Some("performance".into());
        assert!(
            core.apply_event(PowerTransportEvent::ProfilesSnapshot {
                connection_generation: 1,
                snapshot: performance,
            })
            .is_some()
        );
        assert_eq!(core.snapshot.profiles.current, PowerProfile::Performance);
        assert!(
            core.apply_event(PowerTransportEvent::ProfilesUnavailable {
                connection_generation: 1,
                source_generation: 4,
            })
            .is_some()
        );
        assert!(!core.snapshot.profiles.available);
        assert_eq!(core.snapshot.profiles.current, PowerProfile::Unknown);
    }

    #[test]
    fn profile_and_degradation_domains_contain_future_values() {
        for profile in PowerProfile::ALL {
            assert!(!profile.text().is_empty());
            assert!(!profile.token().as_str().is_empty());
        }
        assert_eq!(PowerProfile::parse("future"), PowerProfile::Unknown);
        for reason in PerformanceDegradationReason::ALL {
            assert!(!reason.text().is_empty());
            assert!(!reason.token().as_str().is_empty());
        }
        assert_eq!(
            PerformanceDegradationReason::parse("future"),
            PerformanceDegradationReason::Unknown
        );
    }

    #[test]
    fn repeated_source_replacements_remain_generation_safe() {
        let mut core = PowerCore::default();
        core.next_connection_generation();
        for generation in 1..=100 {
            let raw = RawUPowerSnapshot {
                available: true,
                source_generation: generation,
                on_battery: Some(generation % 2 == 0),
                display: Some(RawDeviceProperties {
                    is_present: Some(false),
                    ..raw_device(2, "")
                }),
                devices: BTreeMap::new(),
            };
            assert!(
                core.apply_event(PowerTransportEvent::UPowerSnapshot {
                    connection_generation: 1,
                    snapshot: raw,
                })
                .is_some()
            );
        }
        assert_eq!(core.snapshot.upower_source_generation, 100);
        let stale = RawUPowerSnapshot {
            available: true,
            source_generation: 99,
            on_battery: Some(true),
            display: None,
            devices: BTreeMap::new(),
        };
        assert!(
            core.apply_event(PowerTransportEvent::UPowerSnapshot {
                connection_generation: 1,
                snapshot: stale,
            })
            .is_none()
        );
        assert_eq!(core.snapshot.upower_source_generation, 100);
        assert_eq!(core.summary.stale_events_contained, 1);
    }

    #[test]
    fn bus_disconnect_clears_every_stale_power_projection() {
        let mut core = PowerCore::default();
        core.next_connection_generation();
        core.raw_upower = RawUPowerSnapshot {
            available: true,
            source_generation: 2,
            on_battery: Some(true),
            display: Some(raw_device(2, "Display")),
            devices: BTreeMap::from([("/device".into(), raw_device(2, "Device"))]),
        };
        core.raw_profiles = RawProfilesSnapshot {
            available: true,
            source_generation: 3,
            active_profile: Some("performance".into()),
            profiles: vec!["performance".into()],
            holds: vec![RawProfileHold {
                profile: "performance".into(),
                application_id: "app".into(),
                reason: "work".into(),
            }],
            degradation: Some("lap-detected".into()),
            malformed_fields: 0,
        };
        core.snapshot = normalize_power(&core.raw_upower, &core.raw_profiles, 1).0;
        let cleared = core
            .apply_event(PowerTransportEvent::BusDisconnected {
                connection_generation: 1,
                reason: "test".into(),
            })
            .unwrap();
        assert_eq!(
            cleared.battery.availability,
            BatteryAvailability::Unavailable
        );
        assert!(cleared.battery.display_device.is_none());
        assert!(cleared.devices.is_empty());
        assert!(!cleared.profiles.available);
        assert!(cleared.profiles.holds.is_empty());
    }
}
