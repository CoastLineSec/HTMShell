use dbus::{
    Message, MessageType,
    arg::{self, PropMap},
    channel::{BusType, Channel, Watch},
};
use htm_runtime::{StateBindingKey, StateToken};
use rustix::fd::{BorrowedFd, OwnedFd};
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
    timerfd_settime,
};
use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MESSAGES_PER_DISPATCH: usize = 64;
const RECONNECT_DELAYS: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];
const RELEVANT_PROPERTIES: [&str; 4] = ["IsPresent", "Percentage", "State", "WarningLevel"];

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
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Empty => "empty",
            Self::Full => "full",
            Self::PendingCharge => "pending-charge",
            Self::PendingDischarge => "pending-discharge",
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
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::None => "none",
            Self::Discharging => "discharging",
            Self::Low => "low",
            Self::Critical => "critical",
            Self::Action => "action",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatterySnapshot {
    pub availability: BatteryAvailability,
    pub percentage: Option<u8>,
    pub charge_state: BatteryChargeState,
    pub warning: BatteryWarning,
    pub sequence: u64,
}

impl BatterySnapshot {
    pub(crate) fn unavailable() -> Self {
        Self {
            availability: BatteryAvailability::Unavailable,
            percentage: None,
            charge_state: BatteryChargeState::Unknown,
            warning: BatteryWarning::Unknown,
            sequence: 0,
        }
    }

    fn semantically_eq(&self, other: &Self) -> bool {
        self.availability == other.availability
            && self.percentage == other.percentage
            && self.charge_state == other.charge_state
            && self.warning == other.warning
    }

    pub fn text_projections(&self) -> [(StateBindingKey, String); 2] {
        [
            (
                StateBindingKey::BatteryPercentage,
                self.percentage
                    .map(|percentage| format!("{percentage}%"))
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                StateBindingKey::BatteryStatus,
                self.status_text().to_owned(),
            ),
        ]
    }

    pub fn token_projections(&self) -> [(StateBindingKey, StateToken); 2] {
        [
            (StateBindingKey::BatteryStatus, self.status_token()),
            (StateBindingKey::BatteryWarning, self.warning_token()),
        ]
    }

    fn status_text(&self) -> &'static str {
        match self.availability {
            BatteryAvailability::Unavailable => "Battery unavailable",
            BatteryAvailability::Absent => "No battery",
            BatteryAvailability::Present => match self.charge_state {
                BatteryChargeState::Unknown => "Battery",
                BatteryChargeState::Charging => "Charging",
                BatteryChargeState::Discharging => "Discharging",
                BatteryChargeState::Empty => "Empty",
                BatteryChargeState::Full => "Fully charged",
                BatteryChargeState::PendingCharge => "Pending charge",
                BatteryChargeState::PendingDischarge => "Pending discharge",
            },
        }
    }

    fn status_token(&self) -> StateToken {
        match self.availability {
            BatteryAvailability::Unavailable => StateToken::Unavailable,
            BatteryAvailability::Absent => StateToken::Absent,
            BatteryAvailability::Present => match self.charge_state {
                BatteryChargeState::Unknown => StateToken::Unknown,
                BatteryChargeState::Charging => StateToken::Charging,
                BatteryChargeState::Discharging => StateToken::Discharging,
                BatteryChargeState::Empty => StateToken::Empty,
                BatteryChargeState::Full => StateToken::Full,
                BatteryChargeState::PendingCharge => StateToken::PendingCharge,
                BatteryChargeState::PendingDischarge => StateToken::PendingDischarge,
            },
        }
    }

    fn warning_token(&self) -> StateToken {
        match self.warning {
            BatteryWarning::Unknown => StateToken::Unknown,
            BatteryWarning::None => StateToken::None,
            BatteryWarning::Discharging => StateToken::Discharging,
            BatteryWarning::Low => StateToken::Low,
            BatteryWarning::Critical => StateToken::Critical,
            BatteryWarning::Action => StateToken::Action,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatteryServiceSummary {
    pub transport: String,
    pub lifecycle_state: String,
    pub subscribers: usize,
    pub maximum_subscribers: usize,
    pub source_generation: u64,
    pub sequence: u64,
    pub availability: String,
    pub percentage: Option<u8>,
    pub charge_state: String,
    pub warning: String,
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
    pub last_signal_to_refresh_us: u64,
    pub last_refresh_us: u64,
    pub last_owner_loss_us: u64,
    pub last_reconnect_us: u64,
    pub last_normalization_us: u64,
    pub last_projection_us: u64,
    pub transport_descriptors: usize,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatteryLifecycleState {
    Dormant,
    Connecting,
    ServiceUnavailable,
    ReadingInitialSnapshot,
    Ready,
    Degraded,
    Stopping,
}

impl BatteryLifecycleState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Dormant => "dormant",
            Self::Connecting => "connecting",
            Self::ServiceUnavailable => "service-unavailable",
            Self::ReadingInitialSnapshot => "reading-initial-snapshot",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Stopping => "stopping",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct RawBatteryProperties {
    is_present: Option<bool>,
    percentage: Option<f64>,
    state: Option<u32>,
    warning: Option<u32>,
    malformed_fields: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PercentageError {
    NonFinite,
    OutOfRange,
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

fn normalize_charge_state(value: Option<u32>) -> BatteryChargeState {
    match value {
        Some(1) => BatteryChargeState::Charging,
        Some(2) => BatteryChargeState::Discharging,
        Some(3) => BatteryChargeState::Empty,
        Some(4) => BatteryChargeState::Full,
        Some(5) => BatteryChargeState::PendingCharge,
        Some(6) => BatteryChargeState::PendingDischarge,
        Some(0) | Some(_) | None => BatteryChargeState::Unknown,
    }
}

fn normalize_warning(value: Option<u32>) -> BatteryWarning {
    match value {
        Some(1) => BatteryWarning::None,
        Some(2) => BatteryWarning::Discharging,
        Some(3) => BatteryWarning::Low,
        Some(4) => BatteryWarning::Critical,
        Some(5) => BatteryWarning::Action,
        Some(0) | Some(_) | None => BatteryWarning::Unknown,
    }
}

fn normalize_properties(raw: Option<&RawBatteryProperties>) -> (BatterySnapshot, u64) {
    let Some(raw) = raw else {
        return (BatterySnapshot::unavailable(), 0);
    };
    let mut malformed = raw.malformed_fields;
    let Some(is_present) = raw.is_present else {
        return (BatterySnapshot::unavailable(), malformed.saturating_add(1));
    };
    if !is_present {
        return (
            BatterySnapshot {
                availability: BatteryAvailability::Absent,
                percentage: None,
                charge_state: BatteryChargeState::Unknown,
                warning: BatteryWarning::Unknown,
                sequence: 0,
            },
            malformed,
        );
    }
    let percentage = match raw.percentage.map(normalize_percentage) {
        Some(Ok(value)) => Some(value),
        Some(Err(_)) => {
            malformed = malformed.saturating_add(1);
            None
        }
        None => None,
    };
    (
        BatterySnapshot {
            availability: BatteryAvailability::Present,
            percentage,
            charge_state: normalize_charge_state(raw.state),
            warning: normalize_warning(raw.warning),
            sequence: 0,
        },
        malformed,
    )
}

#[derive(Debug, Clone)]
enum BatteryTransportEvent {
    ServiceAppeared {
        source_generation: u64,
        replaced: bool,
    },
    ServiceDisappeared {
        source_generation: u64,
    },
    SnapshotResult {
        source_generation: u64,
        properties: Result<RawBatteryProperties, String>,
        elapsed_us: u64,
        signal_to_refresh_us: u64,
    },
    BusDisconnected {
        source_generation: u64,
        reason: String,
    },
}

#[derive(Debug, Clone)]
enum PendingRequest {
    AddOwnerMatch,
    AddPropertiesMatch,
    OwnerLookup {
        owner_epoch: u64,
        started: Instant,
    },
    Snapshot {
        owner_epoch: u64,
        owner: String,
        started: Instant,
        signal_started: Option<Instant>,
    },
}

#[derive(Debug, Default)]
struct RefreshRequest {
    requested: bool,
    first_signal: Option<Instant>,
}

impl RefreshRequest {
    fn request(&mut self) {
        self.requested = true;
        self.first_signal.get_or_insert_with(Instant::now);
    }

    fn request_initial(&mut self) {
        self.requested = true;
    }

    fn take(&mut self) -> Option<Option<Instant>> {
        if !self.requested {
            return None;
        }
        self.requested = false;
        Some(self.first_signal.take())
    }

    fn clear(&mut self) {
        self.requested = false;
        self.first_signal = None;
    }
}

struct DbusBatteryTransport {
    channel: Channel,
    source_generation: u64,
    pending: BTreeMap<u32, PendingRequest>,
    owner_match_ready: bool,
    properties_match_ready: bool,
    owner_lookup_sent: bool,
    owner_epoch: u64,
    owner: Option<String>,
    refresh: RefreshRequest,
    needs_immediate_drain: bool,
    dispatch_property_signals: u64,
    dispatch_irrelevant_signals: u64,
    dispatch_property_bursts: u64,
    last_owner_lookup_us: u64,
}

#[derive(Debug, Default)]
struct TransportDispatch {
    events: Vec<BatteryTransportEvent>,
    messages_drained: usize,
    property_signals: u64,
    irrelevant_signals: u64,
    property_bursts: u64,
    last_owner_lookup_us: u64,
}

impl DbusBatteryTransport {
    fn connect(source_generation: u64) -> Result<Self, String> {
        let mut channel = Channel::get_private(BusType::System)
            .map_err(|error| format!("connect to system bus: {error}"))?;
        channel.set_watch_enabled(true);
        let mut transport = Self {
            channel,
            source_generation,
            pending: BTreeMap::new(),
            owner_match_ready: false,
            properties_match_ready: false,
            owner_lookup_sent: false,
            owner_epoch: 0,
            owner: None,
            refresh: RefreshRequest::default(),
            needs_immediate_drain: false,
            dispatch_property_signals: 0,
            dispatch_irrelevant_signals: 0,
            dispatch_property_bursts: 0,
            last_owner_lookup_us: 0,
        };
        let owner_rule = format!(
            "type='signal',sender='{DBUS_SERVICE}',path='{DBUS_PATH}',interface='{DBUS_INTERFACE}',member='NameOwnerChanged',arg0='{UPOWER_SERVICE}'"
        );
        let properties_rule = format!(
            "type='signal',path='{DISPLAY_DEVICE_PATH}',interface='{PROPERTIES_INTERFACE}',member='PropertiesChanged'"
        );
        let owner_serial = transport.send_add_match(&owner_rule)?;
        transport
            .pending
            .insert(owner_serial, PendingRequest::AddOwnerMatch);
        let properties_serial = transport.send_add_match(&properties_rule)?;
        transport
            .pending
            .insert(properties_serial, PendingRequest::AddPropertiesMatch);
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

    fn send_owner_lookup(&mut self) -> Result<(), String> {
        if self.owner_lookup_sent {
            return Ok(());
        }
        let message =
            Message::new_method_call(DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE, "GetNameOwner")
                .map_err(|error| format!("construct GetNameOwner: {error}"))?
                .append1(UPOWER_SERVICE);
        let serial = self
            .channel
            .send(message)
            .map_err(|()| "send GetNameOwner".to_owned())?;
        self.pending.insert(
            serial,
            PendingRequest::OwnerLookup {
                owner_epoch: self.owner_epoch,
                started: Instant::now(),
            },
        );
        self.owner_lookup_sent = true;
        Ok(())
    }

    fn send_snapshot_request(&mut self) -> Result<(), String> {
        if self
            .pending
            .values()
            .any(|request| matches!(request, PendingRequest::Snapshot { .. }))
        {
            return Ok(());
        }
        let Some(owner) = self.owner.clone() else {
            self.refresh.clear();
            return Ok(());
        };
        let Some(signal_started) = self.refresh.take() else {
            return Ok(());
        };
        let mut message = Message::new_method_call(
            UPOWER_SERVICE,
            DISPLAY_DEVICE_PATH,
            PROPERTIES_INTERFACE,
            "GetAll",
        )
        .map_err(|error| format!("construct display-device GetAll: {error}"))?
        .append1(DEVICE_INTERFACE);
        message.set_auto_start(false);
        let serial = self
            .channel
            .send(message)
            .map_err(|()| "send display-device GetAll".to_owned())?;
        self.pending.insert(
            serial,
            PendingRequest::Snapshot {
                owner_epoch: self.owner_epoch,
                owner,
                started: Instant::now(),
                signal_started,
            },
        );
        Ok(())
    }

    fn process_ready(&mut self) -> TransportDispatch {
        self.dispatch_property_signals = 0;
        self.dispatch_irrelevant_signals = 0;
        self.dispatch_property_bursts = 0;
        self.last_owner_lookup_us = 0;
        if self.channel.read_write(Some(Duration::ZERO)).is_err() || !self.is_connected() {
            return TransportDispatch {
                events: vec![BatteryTransportEvent::BusDisconnected {
                    source_generation: self.source_generation,
                    reason: "system-bus connection closed".into(),
                }],
                ..TransportDispatch::default()
            };
        }
        let mut events = Vec::new();
        let mut drained = 0usize;
        while drained < MAX_MESSAGES_PER_DISPATCH {
            let Some(message) = self.channel.pop_message() else {
                break;
            };
            drained += 1;
            self.process_message(message, &mut events);
        }
        self.needs_immediate_drain = drained == MAX_MESSAGES_PER_DISPATCH;
        if self.owner_match_ready
            && self.properties_match_ready
            && !self.owner_lookup_sent
            && let Err(error) = self.send_owner_lookup()
        {
            events.push(BatteryTransportEvent::BusDisconnected {
                source_generation: self.source_generation,
                reason: error,
            });
        }
        if self.refresh.requested
            && let Err(error) = self.send_snapshot_request()
        {
            events.push(BatteryTransportEvent::BusDisconnected {
                source_generation: self.source_generation,
                reason: error,
            });
        }
        TransportDispatch {
            events,
            messages_drained: drained,
            property_signals: self.dispatch_property_signals,
            irrelevant_signals: self.dispatch_irrelevant_signals,
            property_bursts: self.dispatch_property_bursts,
            last_owner_lookup_us: self.last_owner_lookup_us,
        }
    }

    fn process_message(&mut self, mut message: Message, events: &mut Vec<BatteryTransportEvent>) {
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
        } else if message_matches(
            &message,
            DISPLAY_DEVICE_PATH,
            PROPERTIES_INTERFACE,
            "PropertiesChanged",
        ) {
            self.process_properties_changed(&message);
        }
    }

    fn process_reply(
        &mut self,
        request: PendingRequest,
        message: &mut Message,
        events: &mut Vec<BatteryTransportEvent>,
    ) {
        let result = message.as_result().map(|_| ());
        match request {
            PendingRequest::AddOwnerMatch => {
                if result.is_ok() {
                    self.owner_match_ready = true;
                } else {
                    events.push(self.reply_failure("install owner match", message));
                }
            }
            PendingRequest::AddPropertiesMatch => {
                if result.is_ok() {
                    self.properties_match_ready = true;
                } else {
                    events.push(self.reply_failure("install property match", message));
                }
            }
            PendingRequest::OwnerLookup {
                owner_epoch,
                started,
            } => {
                self.last_owner_lookup_us = elapsed_us(started);
                if owner_epoch != self.owner_epoch {
                    return;
                }
                if result.is_err() {
                    self.set_owner_from_lookup(None, events);
                    return;
                }
                match message.read1::<String>() {
                    Ok(owner) if !owner.is_empty() => {
                        self.set_owner_from_lookup(Some(owner), events);
                        self.refresh.request_initial();
                    }
                    Ok(_) => self.set_owner_from_lookup(None, events),
                    Err(error) => events.push(BatteryTransportEvent::SnapshotResult {
                        source_generation: self.source_generation,
                        properties: Err(format!("decode UPower owner: {error}")),
                        elapsed_us: elapsed_us(started),
                        signal_to_refresh_us: 0,
                    }),
                }
            }
            PendingRequest::Snapshot {
                owner_epoch,
                owner,
                started,
                signal_started,
            } => {
                if owner_epoch != self.owner_epoch
                    || self.owner.as_deref() != Some(owner.as_str())
                    || message.sender().map(|sender| sender.to_string()).as_deref()
                        != Some(owner.as_str())
                {
                    return;
                }
                let properties = if result.is_err() {
                    Err(message_error(message, "display-device GetAll failed"))
                } else {
                    message
                        .read1::<PropMap>()
                        .map(|map| decode_property_map(&map))
                        .map_err(|error| format!("decode display-device GetAll: {error}"))
                };
                events.push(BatteryTransportEvent::SnapshotResult {
                    source_generation: self.source_generation,
                    properties,
                    elapsed_us: elapsed_us(started),
                    signal_to_refresh_us: signal_started.map(elapsed_us).unwrap_or(0),
                });
                if self.refresh.requested {
                    let _ = self.send_snapshot_request();
                }
            }
        }
    }

    fn reply_failure(&self, operation: &str, message: &mut Message) -> BatteryTransportEvent {
        BatteryTransportEvent::BusDisconnected {
            source_generation: self.source_generation,
            reason: message_error(message, operation),
        }
    }

    fn process_owner_changed(
        &mut self,
        message: &Message,
        events: &mut Vec<BatteryTransportEvent>,
    ) {
        let Ok((name, _old_owner, new_owner)) = message.read3::<String, String, String>() else {
            return;
        };
        if name != UPOWER_SERVICE {
            return;
        }
        self.owner_epoch = self.owner_epoch.saturating_add(1);
        self.pending
            .retain(|_, request| !matches!(request, PendingRequest::Snapshot { .. }));
        self.set_owner((!new_owner.is_empty()).then_some(new_owner), events);
        if self.owner.is_some() {
            self.refresh.request_initial();
        }
    }

    fn set_owner(&mut self, owner: Option<String>, events: &mut Vec<BatteryTransportEvent>) {
        if self.owner == owner {
            return;
        }
        let replaced = self.owner.is_some() && owner.is_some();
        let disappeared = self.owner.is_some() && owner.is_none();
        self.owner = owner;
        self.refresh.clear();
        if self.owner.is_some() {
            events.push(BatteryTransportEvent::ServiceAppeared {
                source_generation: self.source_generation,
                replaced,
            });
        } else if disappeared || self.owner_lookup_sent {
            events.push(BatteryTransportEvent::ServiceDisappeared {
                source_generation: self.source_generation,
            });
        }
    }

    fn set_owner_from_lookup(
        &mut self,
        owner: Option<String>,
        events: &mut Vec<BatteryTransportEvent>,
    ) {
        if owner.is_none() && self.owner.is_none() {
            events.push(BatteryTransportEvent::ServiceDisappeared {
                source_generation: self.source_generation,
            });
            return;
        }
        self.set_owner(owner, events);
    }

    fn process_properties_changed(&mut self, message: &Message) {
        let Some(owner) = self.owner.as_deref() else {
            return;
        };
        if message.sender().map(|sender| sender.to_string()).as_deref() != Some(owner) {
            return;
        }
        let relevant = match message.read3::<String, PropMap, Vec<String>>() {
            Ok((interface, changed, invalidated)) if interface == DEVICE_INTERFACE => {
                RELEVANT_PROPERTIES.iter().any(|name| {
                    changed.contains_key(*name)
                        || invalidated.iter().any(|invalidated| invalidated == name)
                })
            }
            Ok(_) => false,
            Err(_) => true,
        };
        if relevant {
            self.dispatch_property_signals = self.dispatch_property_signals.saturating_add(1);
            self.dispatch_property_bursts = 1;
            self.refresh.request();
        } else {
            self.dispatch_irrelevant_signals = self.dispatch_irrelevant_signals.saturating_add(1);
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

fn decode_property_map(map: &PropMap) -> RawBatteryProperties {
    let mut malformed_fields = 0u64;
    let is_present = typed_property::<bool>(map, "IsPresent", &mut malformed_fields);
    let percentage = typed_property::<f64>(map, "Percentage", &mut malformed_fields);
    let state = typed_property::<u32>(map, "State", &mut malformed_fields);
    let warning = typed_property::<u32>(map, "WarningLevel", &mut malformed_fields);
    RawBatteryProperties {
        is_present,
        percentage,
        state,
        warning,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatteryDeadlineMode {
    RequestTimeout,
    Retry,
}

#[derive(Debug)]
struct BatteryDeadline {
    fd: OwnedFd,
    mode: Option<BatteryDeadlineMode>,
}

impl BatteryDeadline {
    fn new() -> Result<Self, String> {
        let fd = timerfd_create(
            TimerfdClockId::Monotonic,
            TimerfdFlags::CLOEXEC | TimerfdFlags::NONBLOCK,
        )
        .map_err(|error| format!("create battery deadline timerfd: {error}"))?;
        Ok(Self { fd, mode: None })
    }

    fn arm(&mut self, duration: Duration, mode: BatteryDeadlineMode) -> Result<(), String> {
        let seconds = i64::try_from(duration.as_secs())
            .map_err(|_| "battery deadline seconds exceed timerfd range".to_owned())?;
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
        .map_err(|error| format!("arm battery deadline timerfd: {error}"))?;
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
        .map_err(|error| format!("disarm battery deadline timerfd: {error}"))?;
        self.mode = None;
        Ok(())
    }

    fn poll_fd(&self) -> Option<BorrowedFd<'_>> {
        self.mode.map(|_| std::os::fd::AsFd::as_fd(&self.fd))
    }

    fn consume(&mut self) -> Result<Option<BatteryDeadlineMode>, String> {
        let mut bytes = [0_u8; std::mem::size_of::<u64>()];
        match rustix::io::read(&self.fd, &mut bytes) {
            Ok(length) if length == bytes.len() => {
                let mode = self.mode.take();
                Ok(mode)
            }
            Ok(length) => Err(format!(
                "battery deadline timerfd returned {length} bytes instead of {}",
                bytes.len()
            )),
            Err(error) if error == rustix::io::Errno::AGAIN => Ok(None),
            Err(error) => Err(format!("read battery deadline timerfd: {error}")),
        }
    }
}

struct BatteryCore {
    lifecycle: BatteryLifecycleState,
    snapshot: Option<BatterySnapshot>,
    subscribers: usize,
    transport_generation: u64,
    sequence: u64,
    summary: BatteryServiceSummary,
}

impl Default for BatteryCore {
    fn default() -> Self {
        Self {
            lifecycle: BatteryLifecycleState::Dormant,
            snapshot: None,
            subscribers: 0,
            transport_generation: 0,
            sequence: 0,
            summary: BatteryServiceSummary {
                transport: "libdbus-watch".into(),
                lifecycle_state: BatteryLifecycleState::Dormant.as_str().into(),
                maximum_messages_per_dispatch: MAX_MESSAGES_PER_DISPATCH,
                explicit_worker_threads: 0,
                internal_threads: 0,
                ..BatteryServiceSummary::default()
            },
        }
    }
}

impl BatteryCore {
    fn set_lifecycle(&mut self, lifecycle: BatteryLifecycleState) {
        self.lifecycle = lifecycle;
        self.summary.lifecycle_state = lifecycle.as_str().into();
    }

    fn set_subscribers(&mut self, subscribers: usize) -> SubscriptionChange {
        let previous = self.subscribers;
        self.subscribers = subscribers;
        self.summary.subscribers = subscribers;
        self.summary.maximum_subscribers = self.summary.maximum_subscribers.max(subscribers);
        match (previous, subscribers) {
            (0, count) if count > 0 => SubscriptionChange::Start,
            (count, 0) if count > 0 => SubscriptionChange::Stop,
            (old, new) if new > old => SubscriptionChange::Added,
            _ => SubscriptionChange::None,
        }
    }

    fn next_transport_generation(&mut self) -> u64 {
        self.transport_generation = self.transport_generation.saturating_add(1);
        self.transport_generation
    }

    fn accept_snapshot(&mut self, mut snapshot: BatterySnapshot) -> Option<BatterySnapshot> {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|current| current.semantically_eq(&snapshot))
        {
            self.summary.duplicate_snapshots_suppressed = self
                .summary
                .duplicate_snapshots_suppressed
                .saturating_add(1);
            return None;
        }
        self.sequence = self.sequence.saturating_add(1);
        snapshot.sequence = self.sequence;
        self.summary.sequence = self.sequence;
        self.summary.changed_snapshots = self.summary.changed_snapshots.saturating_add(1);
        self.snapshot = Some(snapshot.clone());
        Some(snapshot)
    }

    fn apply_event(&mut self, event: BatteryTransportEvent) -> Option<BatterySnapshot> {
        let generation = match &event {
            BatteryTransportEvent::ServiceAppeared {
                source_generation, ..
            }
            | BatteryTransportEvent::ServiceDisappeared { source_generation }
            | BatteryTransportEvent::SnapshotResult {
                source_generation, ..
            }
            | BatteryTransportEvent::BusDisconnected {
                source_generation, ..
            } => *source_generation,
        };
        if generation != self.transport_generation {
            self.summary.stale_events_contained =
                self.summary.stale_events_contained.saturating_add(1);
            return None;
        }
        match event {
            BatteryTransportEvent::ServiceAppeared { replaced, .. } => {
                self.summary.source_generation = self.summary.source_generation.saturating_add(1);
                self.summary.service_appearances =
                    self.summary.service_appearances.saturating_add(1);
                if replaced {
                    self.summary.owner_replacements =
                        self.summary.owner_replacements.saturating_add(1);
                }
                self.set_lifecycle(BatteryLifecycleState::ReadingInitialSnapshot);
                None
            }
            BatteryTransportEvent::ServiceDisappeared { .. } => {
                self.summary.service_disappearances =
                    self.summary.service_disappearances.saturating_add(1);
                self.set_lifecycle(BatteryLifecycleState::ServiceUnavailable);
                self.accept_snapshot(BatterySnapshot::unavailable())
            }
            BatteryTransportEvent::SnapshotResult {
                properties,
                elapsed_us: property_read_us,
                signal_to_refresh_us,
                ..
            } => {
                self.summary.refreshes = self.summary.refreshes.saturating_add(1);
                self.summary.last_property_read_us = property_read_us;
                self.summary.last_refresh_us = property_read_us;
                self.summary.last_signal_to_refresh_us = signal_to_refresh_us;
                let Ok(properties) = properties else {
                    self.summary.refresh_failures = self.summary.refresh_failures.saturating_add(1);
                    self.set_lifecycle(BatteryLifecycleState::Degraded);
                    return None;
                };
                let started = Instant::now();
                let (snapshot, malformed) = normalize_properties(Some(&properties));
                self.summary.last_normalization_us = elapsed_us(started);
                self.summary.malformed_values =
                    self.summary.malformed_values.saturating_add(malformed);
                self.set_lifecycle(BatteryLifecycleState::Ready);
                self.accept_snapshot(snapshot)
            }
            BatteryTransportEvent::BusDisconnected { reason, .. } => {
                let _ = reason;
                self.summary.bus_disconnects = self.summary.bus_disconnects.saturating_add(1);
                self.set_lifecycle(BatteryLifecycleState::ServiceUnavailable);
                self.accept_snapshot(BatterySnapshot::unavailable())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptionChange {
    Start,
    Stop,
    Added,
    None,
}

#[derive(Default)]
pub(crate) struct BatteryService {
    core: BatteryCore,
    transport: Option<DbusBatteryTransport>,
    deadline: Option<BatteryDeadline>,
    reconnect_index: usize,
}

#[derive(Debug, Default)]
pub(crate) struct BatteryFanoutMetrics {
    pub documents: usize,
    pub elements: usize,
    pub frames: usize,
    pub closed_frames_suppressed: usize,
    pub failures: usize,
    pub fanout_us: u64,
    pub projection_us: u64,
}

impl BatteryService {
    pub(crate) fn subscriber_count(&self) -> usize {
        self.core.subscribers
    }

    pub(crate) fn current_snapshot(&self) -> Option<&BatterySnapshot> {
        self.core.snapshot.as_ref()
    }

    pub(crate) fn summary(&self) -> BatteryServiceSummary {
        let mut summary = self.core.summary.clone();
        if let Some(snapshot) = self.core.snapshot.as_ref() {
            summary.availability = snapshot.availability.as_str().into();
            summary.percentage = snapshot.percentage;
            summary.charge_state = snapshot.charge_state.as_str().into();
            summary.warning = snapshot.warning.as_str().into();
        }
        summary.transport_descriptors = usize::from(self.transport.is_some());
        summary.deadline_descriptors = usize::from(self.deadline.is_some());
        summary
    }

    pub(crate) fn bus_watch(&self) -> Option<Watch> {
        self.transport.as_ref().map(DbusBatteryTransport::watch)
    }

    pub(crate) fn deadline_fd(&self) -> Option<BorrowedFd<'_>> {
        self.deadline.as_ref().and_then(BatteryDeadline::poll_fd)
    }

    pub(crate) fn needs_immediate_dispatch(&self) -> bool {
        self.transport
            .as_ref()
            .is_some_and(DbusBatteryTransport::needs_immediate_drain)
    }

    pub(crate) fn set_subscriber_count(&mut self, subscribers: usize) -> Option<BatterySnapshot> {
        match self.core.set_subscribers(subscribers) {
            SubscriptionChange::Start => {
                self.core.set_lifecycle(BatteryLifecycleState::Connecting);
                let unavailable = self.core.accept_snapshot(BatterySnapshot::unavailable());
                self.start_transport(false);
                unavailable
            }
            SubscriptionChange::Stop => {
                self.stop_source();
                None
            }
            SubscriptionChange::Added => self.core.snapshot.clone(),
            SubscriptionChange::None => None,
        }
    }

    pub(crate) fn handle_bus_ready(&mut self) -> Option<BatterySnapshot> {
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
            .saturating_add(dispatch.property_signals);
        self.core.summary.irrelevant_signals = self
            .core
            .summary
            .irrelevant_signals
            .saturating_add(dispatch.irrelevant_signals);
        self.core.summary.property_bursts = self
            .core
            .summary
            .property_bursts
            .saturating_add(dispatch.property_bursts);
        if dispatch.last_owner_lookup_us > 0 {
            self.core.summary.last_owner_lookup_us = dispatch.last_owner_lookup_us;
        }
        let mut latest = None;
        let mut disconnected = false;
        for event in dispatch.events {
            if matches!(event, BatteryTransportEvent::BusDisconnected { .. }) {
                disconnected = true;
            }
            if let Some(snapshot) = self.core.apply_event(event) {
                latest = Some(snapshot);
            }
        }
        if disconnected {
            self.transport = None;
            self.schedule_retry();
        } else {
            if self.core.lifecycle == BatteryLifecycleState::Ready {
                self.reconnect_index = 0;
            }
            self.sync_request_deadline();
        }
        latest
    }

    pub(crate) fn handle_immediate_dispatch(&mut self) -> Option<BatterySnapshot> {
        self.handle_bus_ready()
    }

    pub(crate) fn handle_bus_failure(
        &mut self,
        reason: impl Into<String>,
    ) -> Option<BatterySnapshot> {
        let generation = self.core.transport_generation;
        self.transport = None;
        let snapshot = self
            .core
            .apply_event(BatteryTransportEvent::BusDisconnected {
                source_generation: generation,
                reason: reason.into(),
            });
        self.schedule_retry();
        snapshot
    }

    pub(crate) fn handle_deadline_failure(
        &mut self,
        reason: impl Into<String>,
    ) -> Option<BatterySnapshot> {
        self.deadline = None;
        self.handle_bus_failure(reason)
    }

    pub(crate) fn handle_deadline_ready(&mut self) -> Option<BatterySnapshot> {
        let mode = self.deadline.as_mut().and_then(|deadline| {
            deadline
                .consume()
                .map_err(|error| {
                    eprintln!("htmshell-live: battery deadline failure: {error}");
                })
                .ok()
                .flatten()
        })?;
        match mode {
            BatteryDeadlineMode::RequestTimeout => {
                self.core.summary.request_timeouts =
                    self.core.summary.request_timeouts.saturating_add(1);
                let generation = self.core.transport_generation;
                self.transport = None;
                let snapshot = self
                    .core
                    .apply_event(BatteryTransportEvent::BusDisconnected {
                        source_generation: generation,
                        reason: "battery D-Bus request timed out".into(),
                    });
                self.schedule_retry();
                snapshot
            }
            BatteryDeadlineMode::Retry => {
                self.core.summary.retry_wakeups = self.core.summary.retry_wakeups.saturating_add(1);
                self.start_transport(true);
                None
            }
        }
    }

    pub(crate) fn record_fanout(&mut self, metrics: BatteryFanoutMetrics) {
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
        self.core.set_lifecycle(BatteryLifecycleState::Stopping);
        self.transport = None;
        self.deadline = None;
        self.core.subscribers = 0;
        self.core.summary.subscribers = 0;
        self.core.set_lifecycle(BatteryLifecycleState::Dormant);
    }

    fn start_transport(&mut self, reconnect: bool) {
        if self.core.subscribers == 0 {
            return;
        }
        if reconnect {
            self.core.summary.reconnect_attempts =
                self.core.summary.reconnect_attempts.saturating_add(1);
        }
        let started = Instant::now();
        let generation = self.core.next_transport_generation();
        self.core.set_lifecycle(BatteryLifecycleState::Connecting);
        match DbusBatteryTransport::connect(generation) {
            Ok(transport) => {
                self.transport = Some(transport);
                self.core.summary.system_bus_connections =
                    self.core.summary.system_bus_connections.saturating_add(1);
                if self.core.summary.initial_connection_us == 0 {
                    self.core.summary.initial_connection_us = elapsed_us(started);
                }
                if reconnect {
                    self.core.summary.last_reconnect_us = elapsed_us(started);
                }
                self.sync_request_deadline();
            }
            Err(error) => {
                eprintln!("htmshell-live: battery source unavailable: {error}");
                self.core.summary.connection_failures =
                    self.core.summary.connection_failures.saturating_add(1);
                self.transport = None;
                self.core
                    .set_lifecycle(BatteryLifecycleState::ServiceUnavailable);
                self.schedule_retry();
            }
        }
    }

    fn sync_request_deadline(&mut self) {
        let pending = self
            .transport
            .as_ref()
            .is_some_and(DbusBatteryTransport::has_pending_requests);
        if pending {
            if self.deadline.as_ref().and_then(|deadline| deadline.mode)
                != Some(BatteryDeadlineMode::RequestTimeout)
                && let Err(error) =
                    self.arm_deadline(REQUEST_TIMEOUT, BatteryDeadlineMode::RequestTimeout)
            {
                eprintln!("htmshell-live: battery request timeout unavailable: {error}");
            }
        } else if self.deadline.as_ref().and_then(|deadline| deadline.mode)
            == Some(BatteryDeadlineMode::RequestTimeout)
            && let Some(deadline) = self.deadline.as_mut()
            && let Err(error) = deadline.disarm()
        {
            eprintln!("htmshell-live: battery deadline disarm failed: {error}");
        }
    }

    fn schedule_retry(&mut self) {
        if self.core.subscribers == 0 {
            return;
        }
        let delay = RECONNECT_DELAYS[self.reconnect_index.min(RECONNECT_DELAYS.len() - 1)];
        self.reconnect_index = self
            .reconnect_index
            .saturating_add(1)
            .min(RECONNECT_DELAYS.len() - 1);
        if let Err(error) = self.arm_deadline(delay, BatteryDeadlineMode::Retry) {
            eprintln!("htmshell-live: battery reconnect scheduling failed: {error}");
        }
    }

    fn arm_deadline(
        &mut self,
        duration: Duration,
        mode: BatteryDeadlineMode,
    ) -> Result<(), String> {
        if self.deadline.is_none() {
            self.deadline = Some(BatteryDeadline::new()?);
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
        self.core.snapshot = None;
        self.core.set_lifecycle(BatteryLifecycleState::Dormant);
    }
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

impl fmt::Display for PercentageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => f.write_str("percentage is not finite"),
            Self::OutOfRange => f.write_str("percentage is outside 0 through 100"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct FakeBatterySource {
        events: VecDeque<BatteryTransportEvent>,
    }

    impl FakeBatterySource {
        fn drain_into(&mut self, core: &mut BatteryCore) -> Vec<BatterySnapshot> {
            let mut snapshots = Vec::new();
            while let Some(event) = self.events.pop_front() {
                if let Some(snapshot) = core.apply_event(event) {
                    snapshots.push(snapshot);
                }
            }
            snapshots
        }
    }

    fn raw(
        present: bool,
        percentage: Option<f64>,
        state: u32,
        warning: u32,
    ) -> RawBatteryProperties {
        RawBatteryProperties {
            is_present: Some(present),
            percentage,
            state: Some(state),
            warning: Some(warning),
            malformed_fields: 0,
        }
    }

    fn snapshot_event(generation: u64, properties: RawBatteryProperties) -> BatteryTransportEvent {
        BatteryTransportEvent::SnapshotResult {
            source_generation: generation,
            properties: Ok(properties),
            elapsed_us: 7,
            signal_to_refresh_us: 3,
        }
    }

    fn active_core() -> BatteryCore {
        let mut core = BatteryCore::default();
        assert_eq!(core.set_subscribers(1), SubscriptionChange::Start);
        core.next_transport_generation();
        core.set_lifecycle(BatteryLifecycleState::Connecting);
        core
    }

    #[test]
    fn percentage_normalization_is_checked_and_deterministic() {
        for (input, expected) in [(0.0, 0), (42.4, 42), (42.5, 43), (99.6, 100), (100.0, 100)] {
            assert_eq!(normalize_percentage(input), Ok(expected));
        }
        assert_eq!(
            normalize_percentage(f64::NAN),
            Err(PercentageError::NonFinite)
        );
        assert_eq!(
            normalize_percentage(f64::INFINITY),
            Err(PercentageError::NonFinite)
        );
        assert_eq!(normalize_percentage(-0.1), Err(PercentageError::OutOfRange));
        assert_eq!(
            normalize_percentage(100.1),
            Err(PercentageError::OutOfRange)
        );
    }

    #[test]
    fn availability_precedence_and_malformed_percentage_are_contained() {
        let (unavailable, _) = normalize_properties(None);
        assert_eq!(unavailable.availability, BatteryAvailability::Unavailable);
        assert_eq!(unavailable.percentage, None);

        let (absent, _) = normalize_properties(Some(&raw(false, Some(77.0), 1, 5)));
        assert_eq!(absent.availability, BatteryAvailability::Absent);
        assert_eq!(absent.percentage, None);
        assert_eq!(absent.charge_state, BatteryChargeState::Unknown);
        assert_eq!(absent.warning, BatteryWarning::Unknown);

        let (present, malformed) = normalize_properties(Some(&raw(true, Some(f64::NAN), 2, 3)));
        assert_eq!(present.availability, BatteryAvailability::Present);
        assert_eq!(present.percentage, None);
        assert_eq!(malformed, 1);
    }

    #[test]
    fn every_charge_and_warning_value_maps_to_a_typed_variant() {
        assert_eq!(normalize_charge_state(Some(0)), BatteryChargeState::Unknown);
        assert_eq!(
            normalize_charge_state(Some(1)),
            BatteryChargeState::Charging
        );
        assert_eq!(
            normalize_charge_state(Some(2)),
            BatteryChargeState::Discharging
        );
        assert_eq!(normalize_charge_state(Some(3)), BatteryChargeState::Empty);
        assert_eq!(normalize_charge_state(Some(4)), BatteryChargeState::Full);
        assert_eq!(
            normalize_charge_state(Some(5)),
            BatteryChargeState::PendingCharge
        );
        assert_eq!(
            normalize_charge_state(Some(6)),
            BatteryChargeState::PendingDischarge
        );
        assert_eq!(
            normalize_charge_state(Some(99)),
            BatteryChargeState::Unknown
        );

        assert_eq!(normalize_warning(Some(0)), BatteryWarning::Unknown);
        assert_eq!(normalize_warning(Some(1)), BatteryWarning::None);
        assert_eq!(normalize_warning(Some(2)), BatteryWarning::Discharging);
        assert_eq!(normalize_warning(Some(3)), BatteryWarning::Low);
        assert_eq!(normalize_warning(Some(4)), BatteryWarning::Critical);
        assert_eq!(normalize_warning(Some(5)), BatteryWarning::Action);
        assert_eq!(normalize_warning(Some(99)), BatteryWarning::Unknown);
    }

    #[test]
    fn every_snapshot_projection_is_typed_and_finite() {
        let cases = [
            (
                BatterySnapshot::unavailable(),
                "—",
                "Battery unavailable",
                StateToken::Unavailable,
                StateToken::Unknown,
            ),
            (
                normalize_properties(Some(&raw(false, None, 0, 0))).0,
                "—",
                "No battery",
                StateToken::Absent,
                StateToken::Unknown,
            ),
            (
                normalize_properties(Some(&raw(true, Some(78.0), 1, 1))).0,
                "78%",
                "Charging",
                StateToken::Charging,
                StateToken::None,
            ),
            (
                normalize_properties(Some(&raw(true, Some(12.0), 2, 4))).0,
                "12%",
                "Discharging",
                StateToken::Discharging,
                StateToken::Critical,
            ),
        ];
        for (snapshot, percentage, status, status_token, warning_token) in cases {
            let text = snapshot.text_projections();
            let tokens = snapshot.token_projections();
            assert_eq!(text[0].1, percentage);
            assert_eq!(text[1].1, status);
            assert_eq!(tokens[0].1, status_token);
            assert_eq!(tokens[1].1, warning_token);
            assert!(status_token.valid_for(StateBindingKey::BatteryStatus));
            assert!(warning_token.valid_for(StateBindingKey::BatteryWarning));
        }
    }

    #[test]
    fn fake_source_models_service_lifecycle_restart_and_stale_generation() {
        let mut core = active_core();
        let generation = core.transport_generation;
        let mut source = FakeBatterySource {
            events: VecDeque::from([
                BatteryTransportEvent::ServiceAppeared {
                    source_generation: generation,
                    replaced: false,
                },
                snapshot_event(generation, raw(false, None, 0, 0)),
                BatteryTransportEvent::ServiceDisappeared {
                    source_generation: generation,
                },
                BatteryTransportEvent::ServiceAppeared {
                    source_generation: generation,
                    replaced: false,
                },
                snapshot_event(generation, raw(true, Some(64.0), 2, 2)),
                snapshot_event(generation.saturating_sub(1), raw(true, Some(1.0), 3, 5)),
            ]),
        };
        let snapshots = source.drain_into(&mut core);
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].availability, BatteryAvailability::Absent);
        assert_eq!(snapshots[1].availability, BatteryAvailability::Unavailable);
        assert_eq!(snapshots[2].percentage, Some(64));
        assert_eq!(core.summary.stale_events_contained, 1);
        assert_eq!(core.summary.source_generation, 2);
        assert_eq!(core.summary.service_disappearances, 1);
        assert_eq!(core.summary.service_appearances, 2);
    }

    #[test]
    fn owner_replacement_and_bus_disconnect_clear_stale_display_state() {
        let mut core = active_core();
        let generation = core.transport_generation;
        core.apply_event(BatteryTransportEvent::ServiceAppeared {
            source_generation: generation,
            replaced: false,
        });
        let present = core
            .apply_event(snapshot_event(generation, raw(true, Some(91.0), 1, 1)))
            .unwrap();
        assert_eq!(present.percentage, Some(91));
        core.apply_event(BatteryTransportEvent::ServiceAppeared {
            source_generation: generation,
            replaced: true,
        });
        assert_eq!(core.summary.source_generation, 2);
        assert_eq!(core.summary.owner_replacements, 1);
        let unavailable = core
            .apply_event(BatteryTransportEvent::BusDisconnected {
                source_generation: generation,
                reason: "modeled disconnect".into(),
            })
            .unwrap();
        assert_eq!(unavailable.availability, BatteryAvailability::Unavailable);
        assert_eq!(unavailable.percentage, None);
        assert_eq!(core.summary.bus_disconnects, 1);
    }

    #[test]
    fn duplicate_snapshots_are_suppressed_across_one_thousand_updates() {
        let mut core = active_core();
        let generation = core.transport_generation;
        assert!(
            core.apply_event(snapshot_event(generation, raw(true, Some(55.0), 2, 2)))
                .is_some()
        );
        for _ in 0..500 {
            assert!(
                core.apply_event(snapshot_event(generation, raw(true, Some(55.0), 2, 2)))
                    .is_none()
            );
        }
        for percentage in 0..=100 {
            core.apply_event(snapshot_event(
                generation,
                raw(true, Some(f64::from(percentage)), 2, 2),
            ));
        }
        for percentage in 0..398 {
            core.apply_event(snapshot_event(
                generation,
                raw(true, Some(f64::from(percentage % 101)), 2, 2),
            ));
        }
        assert_eq!(core.summary.duplicate_snapshots_suppressed, 500);
        assert_eq!(core.summary.changed_snapshots, 500);
    }

    #[test]
    fn one_document_with_three_keys_counts_as_one_subscription() {
        let mut core = BatteryCore::default();
        assert_eq!(core.set_subscribers(0), SubscriptionChange::None);
        assert_eq!(core.set_subscribers(1), SubscriptionChange::Start);
        assert_eq!(core.set_subscribers(2), SubscriptionChange::Added);
        assert_eq!(core.set_subscribers(2), SubscriptionChange::None);
        assert_eq!(core.set_subscribers(1), SubscriptionChange::None);
        assert_eq!(core.set_subscribers(0), SubscriptionChange::Stop);
        assert_eq!(core.summary.maximum_subscribers, 2);
    }

    #[test]
    fn reconnect_schedule_is_bounded_and_resets_after_stop() {
        let mut service = BatteryService::default();
        service.core.set_subscribers(1);
        for expected in RECONNECT_DELAYS {
            service.schedule_retry();
            assert_eq!(
                service.deadline.as_ref().and_then(|deadline| deadline.mode),
                Some(BatteryDeadlineMode::Retry)
            );
            let index = RECONNECT_DELAYS
                .iter()
                .position(|delay| *delay == expected)
                .unwrap();
            assert!(service.reconnect_index >= index.min(RECONNECT_DELAYS.len() - 1));
        }
        service.stop_source();
        assert_eq!(service.reconnect_index, 0);
        assert!(service.deadline.is_none());
    }

    #[test]
    fn one_hundred_property_signal_bursts_coalesce_to_one_refresh_each() {
        let mut refresh = RefreshRequest::default();
        let mut refreshes = 0;
        for _ in 0..100 {
            for _ in 0..8 {
                refresh.request();
            }
            assert!(refresh.take().is_some());
            assert!(refresh.take().is_none());
            refreshes += 1;
        }
        assert_eq!(refreshes, 100);
    }

    #[test]
    fn malformed_partial_properties_do_not_leak_dynamic_values() {
        let raw = RawBatteryProperties {
            is_present: Some(true),
            percentage: Some(101.0),
            state: Some(987),
            warning: None,
            malformed_fields: 1,
        };
        let (snapshot, malformed) = normalize_properties(Some(&raw));
        assert_eq!(snapshot.availability, BatteryAvailability::Present);
        assert_eq!(snapshot.percentage, None);
        assert_eq!(snapshot.charge_state, BatteryChargeState::Unknown);
        assert_eq!(snapshot.warning, BatteryWarning::Unknown);
        assert_eq!(malformed, 2);
        assert_eq!(snapshot.status_token(), StateToken::Unknown);
        assert_eq!(snapshot.warning_token(), StateToken::Unknown);
    }
}
