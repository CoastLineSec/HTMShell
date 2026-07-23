use crate::StateToken;
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_REPEAT_DECLARATIONS_PER_DOCUMENT: usize = 32;
pub const MAX_UPOWER_DEVICES_PER_PROCESS: usize = 128;
pub const MAX_POWER_PROFILE_HOLDS_PER_PROCESS: usize = 128;
pub const MAX_ITEMS_PER_REPEAT: usize = 256;
pub const MAX_CLONED_NODES_PER_REPEAT: usize = 4_096;
pub const MAX_CLONED_NODES_PER_DOCUMENT: usize = 16_384;
pub const MAX_REGISTERED_DESCENDANTS_PER_TEMPLATE: usize = 64;
pub const MAX_REPEAT_TEMPLATE_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepeatSource {
    UPowerDevices,
    PowerProfileHolds,
}

impl RepeatSource {
    pub const ALL: [Self; 2] = [Self::UPowerDevices, Self::PowerProfileHolds];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UPowerDevices => "upower.devices",
            Self::PowerProfileHolds => "power_profile.holds",
        }
    }
}

impl std::str::FromStr for RepeatSource {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "upower.devices" => Ok(Self::UPowerDevices),
            "power_profile.holds" => Ok(Self::PowerProfileHolds),
            _ => Err(()),
        }
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
}

impl ItemBindingKey {
    pub const ALL: [Self; 20] = [
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
        }
    }

    pub const fn source(self) -> RepeatSource {
        match self {
            Self::Profile | Self::ApplicationId | Self::Reason => RepeatSource::PowerProfileHolds,
            _ => RepeatSource::UPowerDevices,
        }
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
    pub tokens: BTreeMap<ItemBindingKey, StateToken>,
    pub values: BTreeMap<ItemBindingKey, NumericValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepeatSourceSnapshot {
    pub source: RepeatSource,
    pub source_generation: u64,
    pub items: Vec<RepeatItemSnapshot>,
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
