use htm_runtime::{
    ClockCadence, ClockDeclaration, ClockFormat, ClockTimeZone, ElementInstanceId,
    MAX_CLOCK_DECLARATIONS_PER_PROCESS, MAX_CLOCK_FORMATS_PER_PROCESS, MAX_CLOCK_OUTPUT_BYTES,
    MAX_CLOCK_ZONES_PER_PROCESS,
};
use jiff::{Timestamp, Zoned, fmt::strtime, tz::TimeZone};
use rustix::fd::{BorrowedFd, OwnedFd};
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
    timerfd_settime,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Instant, SystemTime};

const LEGACY_CLOCK_FORMAT: &str = "%H:%M";
const LEGACY_FORMAT_NAME: &str = "HH:mm";
const DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%:z";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClockServiceSummary {
    pub format: String,
    pub effective_zone: String,
    pub utc_fallbacks: u64,
    pub initialization_us: u64,
    pub last_sample_us: u64,
    pub last_timezone_us: u64,
    pub last_format_us: u64,
    pub last_deadline_us: u64,
    pub last_timer_arm_us: u64,
    pub wakeups: u64,
    pub expirations: u64,
    pub changed_values: u64,
    pub unchanged_values_suppressed: u64,
    pub wall_clock_resets: u64,
    pub subscribers: usize,
    pub maximum_subscribers: usize,
    pub timer_descriptors: usize,
    pub generation: u64,
    pub sequence: u64,
    pub sampled_unix_seconds: i64,
    pub documents_visited: u64,
    pub elements_mutated: u64,
    pub fanout_us: u64,
    pub panel_frames_scheduled: u64,
    pub unrelated_frames_scheduled: u64,
    pub closed_surface_frames_suppressed: u64,
    pub mutation_failures_contained: u64,
    pub declarations: usize,
    pub enabled_declarations: usize,
    pub maximum_declarations: usize,
    pub unique_formats: usize,
    pub unique_zones: usize,
    pub unique_zone_conversions: u64,
    pub unique_format_operations: u64,
    pub cached_render_key_reuse: u64,
    pub format_compilation_us: u64,
    pub timezone_lookup_us: u64,
    pub deadline_calculation_us: u64,
    pub changed_declarations: u64,
    pub suppressed_declarations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClockSnapshot {
    pub(crate) display_text: String,
    pub(crate) sampled_instant: Timestamp,
    pub(crate) effective_zone: String,
    pub(crate) sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClockDeclarationUpdate {
    pub(crate) id: ElementInstanceId,
    pub(crate) display_text: String,
    pub(crate) datetime: String,
    pub(crate) enabled: bool,
    pub(crate) sequence: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClockUpdate {
    pub(crate) legacy: Option<ClockSnapshot>,
    pub(crate) declarations: Vec<ClockDeclarationUpdate>,
    pub(crate) sequence: u64,
}

impl ClockUpdate {
    pub(crate) fn is_empty(&self) -> bool {
        self.legacy.is_none() && self.declarations.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ClockSample {
    timestamp: Timestamp,
    local_time_zone: TimeZone,
    effective_local_zone: String,
    used_utc_fallback: bool,
    sample_us: u64,
    timezone_us: u64,
}

pub(crate) trait ClockSource {
    fn sample(&mut self) -> Result<ClockSample, ClockError>;
}

#[derive(Debug, Default)]
pub(crate) struct SystemClockSource;

impl ClockSource for SystemClockSource {
    fn sample(&mut self) -> Result<ClockSample, ClockError> {
        let sample_started = Instant::now();
        let timestamp = Timestamp::try_from(SystemTime::now())
            .map_err(|error| ClockError::TimeSource(error.to_string()))?;
        let sample_us = elapsed_us(sample_started);

        let timezone_started = Instant::now();
        let (local_time_zone, effective_local_zone, used_utc_fallback) =
            match TimeZone::try_system() {
                Ok(zone) => {
                    let name = zone
                        .iana_name()
                        .map(str::to_owned)
                        .unwrap_or_else(|| "system-local".into());
                    (zone, name, false)
                }
                Err(error) => {
                    eprintln!(
                        "htmshell-live: system timezone unavailable ({error}); clocks use UTC"
                    );
                    (TimeZone::UTC, "UTC".into(), true)
                }
            };
        Ok(ClockSample {
            timestamp,
            local_time_zone,
            effective_local_zone,
            used_utc_fallback,
            sample_us,
            timezone_us: elapsed_us(timezone_started),
        })
    }
}

#[derive(Debug)]
pub(crate) enum ClockError {
    TimeSource(String),
    Declaration(String),
    Deadline(String),
    Timer(String),
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimeSource(message) => write!(f, "clock source error: {message}"),
            Self::Declaration(message) => write!(f, "clock declaration error: {message}"),
            Self::Deadline(message) => write!(f, "clock deadline error: {message}"),
            Self::Timer(message) => write!(f, "clock timer error: {message}"),
        }
    }
}

impl std::error::Error for ClockError {}

#[derive(Debug)]
struct ClockTimer {
    fd: OwnedFd,
    armed: bool,
}

impl ClockTimer {
    fn new() -> Result<Self, ClockError> {
        let fd = timerfd_create(
            TimerfdClockId::Realtime,
            TimerfdFlags::CLOEXEC | TimerfdFlags::NONBLOCK,
        )
        .map_err(|error| ClockError::Timer(format!("create timerfd: {error}")))?;
        Ok(Self { fd, armed: false })
    }

    fn arm(&mut self, deadline: Timestamp) -> Result<(), ClockError> {
        let value = Timespec {
            tv_sec: deadline.as_second(),
            tv_nsec: i64::from(deadline.subsec_nanosecond()),
        };
        timerfd_settime(
            &self.fd,
            TimerfdTimerFlags::ABSTIME | TimerfdTimerFlags::CANCEL_ON_SET,
            &Itimerspec {
                it_interval: Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
                it_value: value,
            },
        )
        .map_err(|error| ClockError::Timer(format!("arm timerfd: {error}")))?;
        self.armed = true;
        Ok(())
    }

    fn disarm(&mut self) -> Result<(), ClockError> {
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
        .map_err(|error| ClockError::Timer(format!("disarm timerfd: {error}")))?;
        self.armed = false;
        Ok(())
    }

    fn poll_fd(&self) -> Option<BorrowedFd<'_>> {
        self.armed.then(|| std::os::fd::AsFd::as_fd(&self.fd))
    }

    fn consume(&self) -> Result<TimerRead, ClockError> {
        let mut bytes = [0_u8; std::mem::size_of::<u64>()];
        match rustix::io::read(&self.fd, &mut bytes) {
            Ok(length) if length == bytes.len() => {
                Ok(TimerRead::Expirations(u64::from_ne_bytes(bytes)))
            }
            Ok(length) => Err(ClockError::Timer(format!(
                "timerfd returned {length} bytes instead of {}",
                bytes.len()
            ))),
            Err(error) if error == rustix::io::Errno::CANCELED => Ok(TimerRead::ClockReset),
            Err(error) if error == rustix::io::Errno::AGAIN => Ok(TimerRead::NotReady),
            Err(error) => Err(ClockError::Timer(format!("read timerfd: {error}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerRead {
    Expirations(u64),
    ClockReset,
    NotReady,
}

#[derive(Debug, Clone)]
struct ActiveDeclaration {
    declaration: ClockDeclaration,
    resolved_zone: Option<ResolvedZone>,
    last_text: Option<String>,
    last_datetime: Option<String>,
    next_deadline: Option<Timestamp>,
}

#[derive(Debug, Clone)]
struct ResolvedZone {
    canonical: String,
    time_zone: TimeZone,
}

#[derive(Debug)]
pub(crate) struct ClockService<S = SystemClockSource> {
    source: S,
    timer: Option<ClockTimer>,
    snapshot: Option<ClockSnapshot>,
    legacy_next_deadline: Option<Timestamp>,
    next_deadline: Option<Timestamp>,
    subscribers: usize,
    legacy_consumers: usize,
    declarations: BTreeMap<ElementInstanceId, ActiveDeclaration>,
    format_cache: BTreeMap<String, ClockFormat>,
    zone_cache: BTreeMap<String, ResolvedZone>,
    generation: u64,
    sequence: u64,
    last_sample: Option<ClockSample>,
    summary: ClockServiceSummary,
}

impl Default for ClockService<SystemClockSource> {
    fn default() -> Self {
        Self::new(SystemClockSource)
    }
}

impl<S: ClockSource> ClockService<S> {
    fn new(source: S) -> Self {
        Self {
            source,
            timer: None,
            snapshot: None,
            legacy_next_deadline: None,
            next_deadline: None,
            subscribers: 0,
            legacy_consumers: 0,
            declarations: BTreeMap::new(),
            format_cache: BTreeMap::new(),
            zone_cache: BTreeMap::new(),
            generation: 1,
            sequence: 0,
            last_sample: None,
            summary: ClockServiceSummary {
                format: LEGACY_FORMAT_NAME.into(),
                generation: 1,
                ..ClockServiceSummary::default()
            },
        }
    }

    #[cfg(test)]
    fn with_source(source: S) -> Self {
        Self::new(source)
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        self.subscribers
    }

    pub(crate) fn current_snapshot(&self) -> Option<&ClockSnapshot> {
        self.snapshot.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn declaration_count(&self) -> usize {
        self.declarations.len()
    }

    pub(crate) fn reconcile(
        &mut self,
        subscribers: usize,
        legacy_consumers: usize,
        declarations: Vec<ClockDeclaration>,
    ) -> Result<Option<ClockUpdate>, ClockError> {
        let started = Instant::now();
        if declarations.len() > MAX_CLOCK_DECLARATIONS_PER_PROCESS {
            return Err(ClockError::Declaration(format!(
                "{} declarations exceed the process limit of {MAX_CLOCK_DECLARATIONS_PER_PROCESS}",
                declarations.len()
            )));
        }
        let mut ids = BTreeSet::new();
        let mut unique_formats = BTreeMap::new();
        let mut zone_values = BTreeMap::new();
        for declaration in &declarations {
            if !ids.insert(declaration.id.clone()) {
                return Err(ClockError::Declaration(format!(
                    "clock `#{}` appears more than once in one process generation",
                    declaration.id.html_id
                )));
            }
            unique_formats
                .entry(declaration.format.source().to_owned())
                .or_insert_with(|| declaration.format.clone());
            zone_values
                .entry(declaration.time_zone.declaration_value().to_owned())
                .or_insert_with(|| declaration.id.html_id.clone());
        }
        if unique_formats.len() > MAX_CLOCK_FORMATS_PER_PROCESS {
            return Err(ClockError::Declaration(format!(
                "{} unique formats exceed the process limit of {MAX_CLOCK_FORMATS_PER_PROCESS}",
                unique_formats.len()
            )));
        }
        if zone_values.len() > MAX_CLOCK_ZONES_PER_PROCESS {
            return Err(ClockError::Declaration(format!(
                "{} unique time zones exceed the process limit of {MAX_CLOCK_ZONES_PER_PROCESS}",
                zone_values.len()
            )));
        }
        self.summary.format_compilation_us = elapsed_us(started);

        let zone_started = Instant::now();
        let previous_zone_cache = self.zone_cache.clone();
        let mut zone_cache = BTreeMap::new();
        for (zone, element_id) in zone_values {
            if zone == "local" {
                continue;
            }
            if let Some(resolved) = previous_zone_cache.get(&zone) {
                zone_cache.insert(zone, resolved.clone());
                continue;
            }
            let time_zone = match zone.as_str() {
                "UTC" => TimeZone::UTC,
                _ => TimeZone::get(&zone).map_err(|error| {
                    ClockError::Declaration(format!(
                        "clock `#{element_id}` time zone `{zone}` could not be resolved: {error}"
                    ))
                })?,
            };
            let canonical = time_zone
                .iana_name()
                .map(str::to_owned)
                .unwrap_or_else(|| zone.clone());
            zone_cache.insert(
                zone,
                ResolvedZone {
                    canonical,
                    time_zone,
                },
            );
        }
        self.summary.timezone_lookup_us = elapsed_us(zone_started);

        let previous_subscribers = self.subscribers;
        let previous_legacy = self.legacy_consumers;
        let old = self.declarations.clone();
        let mut next = BTreeMap::new();
        let mut needs_sample = previous_legacy == 0 && legacy_consumers > 0;
        let mut state_only_updates = Vec::new();
        for declaration in declarations {
            let id = declaration.id.clone();
            let resolved_zone = match &declaration.time_zone {
                ClockTimeZone::Local => None,
                zone => zone_cache
                    .get(zone.declaration_value())
                    .cloned()
                    .ok_or_else(|| {
                        ClockError::Declaration(format!(
                            "time zone cache lost `{}`",
                            zone.declaration_value()
                        ))
                    })?
                    .into(),
            };
            let entry = match old.get(&id) {
                Some(previous) => {
                    if previous.declaration.format != declaration.format
                        || previous.declaration.time_zone != declaration.time_zone
                    {
                        return Err(ClockError::Declaration(format!(
                            "clock `#{}` changed immutable declaration attributes",
                            id.html_id
                        )));
                    }
                    let enabled_changed = previous.declaration.enabled != declaration.enabled;
                    if !previous.declaration.enabled && declaration.enabled {
                        needs_sample = true;
                    }
                    let mut entry = previous.clone();
                    entry.declaration = declaration;
                    entry.resolved_zone = resolved_zone;
                    if !entry.declaration.enabled {
                        entry.next_deadline = None;
                    }
                    if enabled_changed
                        && !entry.declaration.enabled
                        && let (Some(display_text), Some(datetime)) =
                            (entry.last_text.clone(), entry.last_datetime.clone())
                    {
                        state_only_updates.push(ClockDeclarationUpdate {
                            id: id.clone(),
                            display_text,
                            datetime,
                            enabled: false,
                            sequence: self.sequence,
                        });
                    }
                    entry
                }
                None => {
                    needs_sample = true;
                    ActiveDeclaration {
                        declaration,
                        resolved_zone,
                        last_text: None,
                        last_datetime: None,
                        next_deadline: None,
                    }
                }
            };
            next.insert(id, entry);
        }
        let configuration_sample = needs_sample.then(|| self.source.sample()).transpose()?;
        self.declarations = next;
        self.format_cache = unique_formats;
        self.zone_cache = zone_cache;
        self.subscribers = subscribers;
        self.legacy_consumers = legacy_consumers;
        self.summary.subscribers = subscribers;
        self.summary.maximum_subscribers = self.summary.maximum_subscribers.max(subscribers);
        self.summary.declarations = self.declarations.len();
        self.summary.enabled_declarations = self
            .declarations
            .values()
            .filter(|entry| entry.declaration.enabled)
            .count();
        self.summary.maximum_declarations = self
            .summary
            .maximum_declarations
            .max(self.declarations.len());
        self.summary.unique_formats = self.format_cache.len();
        self.summary.unique_zones = self.zone_cache.len()
            + usize::from(
                self.declarations
                    .values()
                    .any(|entry| entry.declaration.time_zone == ClockTimeZone::Local),
            );

        if needs_sample {
            if previous_subscribers == 0 && subscribers > 0 {
                self.summary.initialization_us = elapsed_us(started);
            }
            let mut update = self.refresh_from_sample(
                configuration_sample.expect("configuration sample is present"),
                RefreshReason::Configuration,
            )?;
            update.declarations.extend(state_only_updates);
            update
                .declarations
                .sort_by(|left, right| left.id.cmp(&right.id));
            return Ok((!update.is_empty()).then_some(update));
        }

        self.recalculate_deadline_from_last_sample()?;
        if state_only_updates.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ClockUpdate {
                legacy: None,
                declarations: state_only_updates,
                sequence: self.sequence,
            }))
        }
    }

    pub(crate) fn poll_fd(&self) -> Option<BorrowedFd<'_>> {
        self.timer.as_ref().and_then(ClockTimer::poll_fd)
    }

    pub(crate) fn handle_ready(&mut self) -> Result<Option<ClockUpdate>, ClockError> {
        let read = self
            .timer
            .as_ref()
            .ok_or_else(|| ClockError::Timer("timer readiness without a timerfd".into()))?
            .consume()?;
        match read {
            TimerRead::NotReady => Ok(None),
            TimerRead::Expirations(expirations) => {
                self.summary.wakeups = self.summary.wakeups.saturating_add(1);
                self.summary.expirations = self.summary.expirations.saturating_add(expirations);
                let update = self.refresh(RefreshReason::Deadline)?;
                Ok((!update.is_empty()).then_some(update))
            }
            TimerRead::ClockReset => {
                self.summary.wakeups = self.summary.wakeups.saturating_add(1);
                self.summary.wall_clock_resets = self.summary.wall_clock_resets.saturating_add(1);
                let update = self.refresh(RefreshReason::ClockReset)?;
                Ok((!update.is_empty()).then_some(update))
            }
        }
    }

    pub(crate) fn record_fanout(
        &mut self,
        documents: usize,
        elements: usize,
        panel_frames: usize,
        closed_frames_suppressed: usize,
        failures: usize,
        duration_us: u64,
    ) {
        self.summary.documents_visited = self
            .summary
            .documents_visited
            .saturating_add(documents as u64);
        self.summary.elements_mutated = self
            .summary
            .elements_mutated
            .saturating_add(elements as u64);
        self.summary.panel_frames_scheduled = self
            .summary
            .panel_frames_scheduled
            .saturating_add(panel_frames as u64);
        self.summary.closed_surface_frames_suppressed = self
            .summary
            .closed_surface_frames_suppressed
            .saturating_add(closed_frames_suppressed as u64);
        self.summary.mutation_failures_contained = self
            .summary
            .mutation_failures_contained
            .saturating_add(failures as u64);
        self.summary.fanout_us = duration_us;
    }

    pub(crate) fn summary(&self) -> ClockServiceSummary {
        let mut summary = self.summary.clone();
        summary.subscribers = self.subscribers;
        summary.generation = self.generation;
        summary.sequence = self.sequence;
        if let Some(sample) = &self.last_sample {
            summary.sampled_unix_seconds = sample.timestamp.as_second();
            summary.effective_zone = sample.effective_local_zone.clone();
        }
        summary
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), ClockError> {
        if let Some(timer) = self.timer.as_mut()
            && timer.armed
        {
            timer.disarm()?;
        }
        self.timer = None;
        self.legacy_next_deadline = None;
        self.next_deadline = None;
        self.subscribers = 0;
        self.legacy_consumers = 0;
        self.declarations.clear();
        self.format_cache.clear();
        self.zone_cache.clear();
        self.summary.subscribers = 0;
        self.generation = self.generation.saturating_add(1);
        self.summary.generation = self.generation;
        Ok(())
    }

    fn active(&self) -> bool {
        self.legacy_consumers > 0
            || self
                .declarations
                .values()
                .any(|entry| entry.declaration.enabled)
    }

    fn ensure_timer_if_active(&mut self) -> Result<(), ClockError> {
        if self.active() && self.timer.is_none() {
            self.timer = Some(ClockTimer::new()?);
            self.summary.timer_descriptors = 1;
        }
        Ok(())
    }

    fn refresh(&mut self, reason: RefreshReason) -> Result<ClockUpdate, ClockError> {
        if self.subscribers == 0 && self.declarations.is_empty() && self.legacy_consumers == 0 {
            return Ok(ClockUpdate::default());
        }
        let sample = self.source.sample()?;
        self.refresh_from_sample(sample, reason)
    }

    fn refresh_from_sample(
        &mut self,
        sample: ClockSample,
        reason: RefreshReason,
    ) -> Result<ClockUpdate, ClockError> {
        self.summary.last_sample_us = sample.sample_us;
        self.summary.last_timezone_us = sample.timezone_us;
        if sample.used_utc_fallback {
            self.summary.utc_fallbacks = self.summary.utc_fallbacks.saturating_add(1);
        }
        self.sequence = self.sequence.saturating_add(1);
        let sequence = self.sequence;
        let force_all = matches!(
            reason,
            RefreshReason::Configuration | RefreshReason::ClockReset
        );
        let mut update = ClockUpdate {
            sequence,
            ..ClockUpdate::default()
        };
        let format_started = Instant::now();

        if self.legacy_consumers > 0 {
            let legacy_due = force_all
                || self.snapshot.as_ref().is_none()
                || self
                    .legacy_next_deadline
                    .is_some_and(|deadline| sample.timestamp >= deadline);
            if legacy_due {
                let zoned = sample.timestamp.to_zoned(sample.local_time_zone.clone());
                let display_text = checked_format(LEGACY_CLOCK_FORMAT, &zoned)?;
                let changed = self
                    .snapshot
                    .as_ref()
                    .is_none_or(|snapshot| snapshot.display_text != display_text);
                let snapshot = ClockSnapshot {
                    display_text,
                    sampled_instant: sample.timestamp,
                    effective_zone: sample.effective_local_zone.clone(),
                    sequence,
                };
                if changed {
                    update.legacy = Some(snapshot.clone());
                    self.summary.changed_values = self.summary.changed_values.saturating_add(1);
                } else {
                    self.summary.unchanged_values_suppressed =
                        self.summary.unchanged_values_suppressed.saturating_add(1);
                }
                self.snapshot = Some(snapshot);
            }
        }

        let due_ids: Vec<_> = self
            .declarations
            .iter()
            .filter(|(_, entry)| {
                entry.last_text.is_none()
                    || (entry.declaration.enabled
                        && (force_all
                            || entry
                                .next_deadline
                                .is_some_and(|deadline| sample.timestamp >= deadline)))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut zoned_cache: BTreeMap<String, Zoned> = BTreeMap::new();
        let mut render_cache: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
        for id in due_ids {
            let entry = self
                .declarations
                .get(&id)
                .expect("due declaration came from this map");
            let (zone_key, time_zone) = match &entry.declaration.time_zone {
                ClockTimeZone::Local => (
                    format!("local:{}", sample.effective_local_zone),
                    sample.local_time_zone.clone(),
                ),
                ClockTimeZone::Utc | ClockTimeZone::Named(_) => {
                    let zone = entry.resolved_zone.as_ref().ok_or_else(|| {
                        ClockError::Declaration(format!(
                            "clock `#{}` lost its resolved time zone",
                            id.html_id
                        ))
                    })?;
                    (zone.canonical.clone(), zone.time_zone.clone())
                }
            };
            let zoned = zoned_cache.entry(zone_key.clone()).or_insert_with(|| {
                self.summary.unique_zone_conversions =
                    self.summary.unique_zone_conversions.saturating_add(1);
                sample.timestamp.to_zoned(time_zone)
            });
            let render_key = (
                entry.declaration.format.source().to_owned(),
                zone_key.clone(),
            );
            let rendered = if let Some(rendered) = render_cache.get(&render_key) {
                self.summary.cached_render_key_reuse =
                    self.summary.cached_render_key_reuse.saturating_add(1);
                rendered.clone()
            } else {
                self.summary.unique_format_operations =
                    self.summary.unique_format_operations.saturating_add(1);
                let rendered = (
                    checked_format(entry.declaration.format.source(), zoned)?,
                    checked_datetime(entry.declaration.format.cadence(), zoned)?,
                );
                render_cache.insert(render_key, rendered.clone());
                rendered
            };
            let entry = self
                .declarations
                .get_mut(&id)
                .expect("declaration remains present");
            let changed = entry.last_text.as_ref() != Some(&rendered.0)
                || entry.last_datetime.as_ref() != Some(&rendered.1);
            entry.last_text = Some(rendered.0.clone());
            entry.last_datetime = Some(rendered.1.clone());
            if changed {
                update.declarations.push(ClockDeclarationUpdate {
                    id: id.clone(),
                    display_text: rendered.0,
                    datetime: rendered.1,
                    enabled: entry.declaration.enabled,
                    sequence,
                });
                self.summary.changed_declarations =
                    self.summary.changed_declarations.saturating_add(1);
                self.summary.changed_values = self.summary.changed_values.saturating_add(1);
            } else {
                self.summary.suppressed_declarations =
                    self.summary.suppressed_declarations.saturating_add(1);
                self.summary.unchanged_values_suppressed =
                    self.summary.unchanged_values_suppressed.saturating_add(1);
            }
        }
        self.summary.last_format_us = elapsed_us(format_started);
        self.last_sample = Some(sample);
        self.recalculate_deadline_from_last_sample()?;
        update
            .declarations
            .sort_by(|left, right| left.id.cmp(&right.id));
        Ok(update)
    }

    fn recalculate_deadline_from_last_sample(&mut self) -> Result<(), ClockError> {
        let deadline_started = Instant::now();
        let Some(sample) = self.last_sample.clone() else {
            self.disarm()?;
            return Ok(());
        };
        let mut earliest: Option<Timestamp> = None;
        if self.legacy_consumers > 0 {
            self.legacy_next_deadline = next_deadline(
                sample.timestamp,
                &sample.local_time_zone,
                ClockCadence::Minute,
                true,
            )?;
            earliest = self.legacy_next_deadline;
        } else {
            self.legacy_next_deadline = None;
        }
        for entry in self.declarations.values_mut() {
            if !entry.declaration.enabled {
                entry.next_deadline = None;
                continue;
            }
            let time_zone = match &entry.declaration.time_zone {
                ClockTimeZone::Local => sample.local_time_zone.clone(),
                ClockTimeZone::Utc | ClockTimeZone::Named(_) => entry
                    .resolved_zone
                    .as_ref()
                    .ok_or_else(|| {
                        ClockError::Declaration(format!(
                            "clock `#{}` lost its resolved time zone",
                            entry.declaration.id.html_id
                        ))
                    })?
                    .time_zone
                    .clone(),
            };
            let deadline = next_deadline(
                sample.timestamp,
                &time_zone,
                entry.declaration.format.cadence(),
                entry.declaration.format.observes_zone_transition(),
            )?;
            entry.next_deadline = deadline;
            if let Some(deadline) = deadline {
                earliest = Some(match earliest {
                    Some(current) => current.min(deadline),
                    None => deadline,
                });
            }
        }
        self.summary.last_deadline_us = elapsed_us(deadline_started);
        self.summary.deadline_calculation_us = self.summary.last_deadline_us;
        self.next_deadline = earliest;
        match earliest {
            Some(deadline) => {
                self.ensure_timer_if_active()?;
                let arm_started = Instant::now();
                self.timer
                    .as_mut()
                    .ok_or_else(|| ClockError::Timer("clock deadline has no timerfd".into()))?
                    .arm(deadline)?;
                self.summary.last_timer_arm_us = elapsed_us(arm_started);
            }
            None => self.disarm()?,
        }
        Ok(())
    }

    fn disarm(&mut self) -> Result<(), ClockError> {
        if let Some(timer) = self.timer.as_mut()
            && timer.armed
        {
            timer.disarm()?;
        }
        self.next_deadline = None;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshReason {
    Configuration,
    Deadline,
    ClockReset,
}

fn checked_format(format: &str, zoned: &Zoned) -> Result<String, ClockError> {
    let output = strtime::format(format, zoned).map_err(|error| {
        ClockError::Declaration(format!("validated clock format failed: {error}"))
    })?;
    if output.len() > MAX_CLOCK_OUTPUT_BYTES {
        return Err(ClockError::Declaration(format!(
            "formatted clock output exceeds {MAX_CLOCK_OUTPUT_BYTES} UTF-8 bytes"
        )));
    }
    Ok(output)
}

fn checked_datetime(cadence: ClockCadence, zoned: &Zoned) -> Result<String, ClockError> {
    let time_zone = zoned.time_zone().clone();
    let precise = match cadence {
        ClockCadence::Day => zoned
            .date()
            .to_zoned(time_zone)
            .map_err(|error| ClockError::Deadline(error.to_string()))?,
        ClockCadence::Hour | ClockCadence::Minute => {
            let elapsed = match cadence {
                ClockCadence::Hour => i64::from(zoned.minute()) * 60 + i64::from(zoned.second()),
                ClockCadence::Minute => i64::from(zoned.second()),
                _ => unreachable!("matched above"),
            };
            let second = zoned
                .timestamp()
                .as_second()
                .checked_sub(elapsed)
                .ok_or_else(|| ClockError::Deadline("datetime precision underflow".into()))?;
            Timestamp::new(second, 0)
                .map_err(|error| ClockError::Deadline(error.to_string()))?
                .to_zoned(time_zone)
        }
        ClockCadence::Second | ClockCadence::ZoneTransitionOnly | ClockCadence::Static => {
            Timestamp::new(zoned.timestamp().as_second(), 0)
                .map_err(|error| ClockError::Deadline(error.to_string()))?
                .to_zoned(time_zone)
        }
    };
    checked_format(DATETIME_FORMAT, &precise)
}

fn next_deadline(
    timestamp: Timestamp,
    time_zone: &TimeZone,
    cadence: ClockCadence,
    observes_zone_transition: bool,
) -> Result<Option<Timestamp>, ClockError> {
    let zoned = timestamp.to_zoned(time_zone.clone());
    let second = i64::from(zoned.second());
    let minute = i64::from(zoned.minute());
    let base = match cadence {
        ClockCadence::Static | ClockCadence::ZoneTransitionOnly => None,
        ClockCadence::Second => Some(timestamp_at_next_whole_second(timestamp)?),
        ClockCadence::Minute => Some(timestamp_after_seconds(timestamp, 60 - second)?),
        ClockCadence::Hour => Some(timestamp_after_seconds(
            timestamp,
            (60 - minute) * 60 - second,
        )?),
        ClockCadence::Day => Some(
            zoned
                .date()
                .tomorrow()
                .and_then(|date| date.to_zoned(time_zone.clone()))
                .map_err(|error| ClockError::Deadline(error.to_string()))?
                .timestamp(),
        ),
    };
    let transition_relevant =
        observes_zone_transition || matches!(cadence, ClockCadence::Hour | ClockCadence::Minute);
    let transition = transition_relevant
        .then(|| {
            time_zone
                .following(timestamp)
                .next()
                .map(|item| item.timestamp())
        })
        .flatten();
    Ok(match (base, transition) {
        (Some(base), Some(transition)) => Some(base.min(transition)),
        (Some(base), None) => Some(base),
        (None, Some(transition)) => Some(transition),
        (None, None) => None,
    })
}

fn timestamp_at_next_whole_second(timestamp: Timestamp) -> Result<Timestamp, ClockError> {
    let second = timestamp
        .as_second()
        .checked_add(1)
        .ok_or_else(|| ClockError::Deadline("timestamp second overflow".into()))?;
    Timestamp::new(second, 0).map_err(|error| ClockError::Deadline(error.to_string()))
}

fn timestamp_after_seconds(timestamp: Timestamp, seconds: i64) -> Result<Timestamp, ClockError> {
    let second = timestamp
        .as_second()
        .checked_add(seconds)
        .ok_or_else(|| ClockError::Deadline("deadline second overflow".into()))?;
    Timestamp::new(second, 0).map_err(|error| ClockError::Deadline(error.to_string()))
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use htm_runtime::{
        CLOCK_FORMAT_CONVERSIONS, ClockFormat, ExperimentalDocumentIdentity,
        MAX_CLOCK_DECLARATIONS_PER_PROCESS,
    };
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct FakeClockSource {
        samples: VecDeque<ClockSample>,
    }

    impl ClockSource for FakeClockSource {
        fn sample(&mut self) -> Result<ClockSample, ClockError> {
            self.samples
                .pop_front()
                .ok_or_else(|| ClockError::TimeSource("fake clock exhausted".into()))
        }
    }

    fn timestamp(value: &str) -> Timestamp {
        value.parse().unwrap()
    }

    fn sample(value: &str, zone: TimeZone) -> ClockSample {
        ClockSample {
            timestamp: timestamp(value),
            effective_local_zone: zone.iana_name().unwrap_or("test-zone").into(),
            local_time_zone: zone,
            used_utc_fallback: false,
            sample_us: 3,
            timezone_us: 4,
        }
    }

    fn sample_at(timestamp: Timestamp, zone: TimeZone) -> ClockSample {
        ClockSample {
            timestamp,
            effective_local_zone: zone.iana_name().unwrap_or("test-zone").into(),
            local_time_zone: zone,
            used_utc_fallback: false,
            sample_us: 3,
            timezone_us: 4,
        }
    }

    fn declaration(
        serial: u64,
        id: &str,
        format: &str,
        zone: ClockTimeZone,
        enabled: bool,
    ) -> ClockDeclaration {
        ClockDeclaration {
            id: ElementInstanceId {
                document_generation: ExperimentalDocumentIdentity { serial },
                html_id: id.into(),
            },
            format: ClockFormat::compile(format).unwrap(),
            time_zone: zone,
            enabled,
        }
    }

    #[test]
    fn fixed_and_custom_formats_cover_midnight_noon_and_dates() {
        let zoned = timestamp("2026-02-28T12:05:09Z").to_zoned(TimeZone::UTC);
        for (format, expected) in [
            ("%H:%M", "12:05"),
            ("%I:%M:%S %p", "12:05:09 PM"),
            ("%-I:%M %P", "12:05 pm"),
            ("%F", "2026-02-28"),
            ("%A, %B %-d, %Y", "Saturday, February 28, 2026"),
            ("%G-W%V-%u", "2026-W09-6"),
            ("%H:%M %:z %Q", "12:05 +00:00 UTC"),
        ] {
            assert_eq!(checked_format(format, &zoned).unwrap(), expected);
        }
        let midnight = timestamp("2026-03-01T00:00:00Z").to_zoned(TimeZone::UTC);
        assert_eq!(checked_format("%I:%M %p", &midnight).unwrap(), "12:00 AM");
        let leap_day = timestamp("2024-02-29T23:59:59Z").to_zoned(TimeZone::UTC);
        assert_eq!(
            checked_format("%F %j %G-W%V-%u", &leap_day).unwrap(),
            "2024-02-29 060 2024-W09-4"
        );
        for conversion in CLOCK_FORMAT_CONVERSIONS {
            checked_format(conversion, &zoned)
                .unwrap_or_else(|error| panic!("{conversion} should format: {error}"));
        }
        for format in ["%-I", "%_4d", "%04Y", "%^A", "%#p", "%20Y"] {
            checked_format(format, &zoned)
                .unwrap_or_else(|error| panic!("{format} should format: {error}"));
        }
    }

    #[test]
    fn deadlines_cover_all_cadences_and_transitions() {
        let now = timestamp("2026-01-01T10:20:30.500Z");
        assert_eq!(
            next_deadline(now, &TimeZone::UTC, ClockCadence::Second, false).unwrap(),
            Some(timestamp("2026-01-01T10:20:31Z"))
        );
        assert_eq!(
            next_deadline(now, &TimeZone::UTC, ClockCadence::Minute, false).unwrap(),
            Some(timestamp("2026-01-01T10:21:00Z"))
        );
        assert_eq!(
            next_deadline(now, &TimeZone::UTC, ClockCadence::Hour, false).unwrap(),
            Some(timestamp("2026-01-01T11:00:00Z"))
        );
        assert_eq!(
            next_deadline(now, &TimeZone::UTC, ClockCadence::Day, false).unwrap(),
            Some(timestamp("2026-01-02T00:00:00Z"))
        );
        assert_eq!(
            next_deadline(now, &TimeZone::UTC, ClockCadence::Static, false).unwrap(),
            None
        );

        let eastern = TimeZone::get("America/New_York").unwrap();
        assert_eq!(
            next_deadline(
                timestamp("2026-03-08T06:59:30Z"),
                &eastern,
                ClockCadence::Hour,
                false
            )
            .unwrap(),
            Some(timestamp("2026-03-08T07:00:00Z"))
        );
        assert_eq!(
            next_deadline(
                timestamp("2026-11-01T05:59:30Z"),
                &eastern,
                ClockCadence::Hour,
                false
            )
            .unwrap(),
            Some(timestamp("2026-11-01T06:00:00Z"))
        );
        assert_eq!(
            next_deadline(
                timestamp("2026-03-08T05:30:00Z"),
                &eastern,
                ClockCadence::Day,
                false
            )
            .unwrap(),
            Some(timestamp("2026-03-09T04:00:00Z"))
        );
        assert_eq!(
            next_deadline(
                timestamp("2026-11-01T04:30:00Z"),
                &eastern,
                ClockCadence::Day,
                false
            )
            .unwrap(),
            Some(timestamp("2026-11-02T05:00:00Z"))
        );
        assert_eq!(
            next_deadline(
                timestamp("2026-03-08T06:00:00Z"),
                &eastern,
                ClockCadence::ZoneTransitionOnly,
                true
            )
            .unwrap(),
            Some(timestamp("2026-03-08T07:00:00Z"))
        );
        assert_eq!(
            checked_datetime(
                ClockCadence::Day,
                &timestamp("2026-03-08T16:00:00Z").to_zoned(eastern.clone())
            )
            .unwrap(),
            "2026-03-08T00:00:00-05:00"
        );
        assert_eq!(
            checked_datetime(
                ClockCadence::Day,
                &timestamp("2026-11-01T17:00:00Z").to_zoned(eastern)
            )
            .unwrap(),
            "2026-11-01T00:00:00-04:00"
        );
    }

    #[test]
    fn one_sample_fans_out_mixed_formats_zones_and_disabled_state() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([sample("2026-01-01T12:34:56Z", TimeZone::UTC)]),
        });
        let update = service
            .reconcile(
                2,
                1,
                vec![
                    declaration(1, "minute", "%H:%M", ClockTimeZone::Utc, true),
                    declaration(
                        2,
                        "tokyo",
                        "%H:%M",
                        ClockTimeZone::Named("Asia/Tokyo".into()),
                        true,
                    ),
                    declaration(3, "paused", "%T", ClockTimeZone::Utc, false),
                ],
            )
            .unwrap()
            .unwrap();
        assert_eq!(update.legacy.unwrap().display_text, "12:34");
        assert_eq!(update.declarations.len(), 3);
        assert_eq!(update.declarations[0].display_text, "12:34");
        assert_eq!(update.declarations[0].datetime, "2026-01-01T12:34:00+00:00");
        assert_eq!(update.declarations[1].display_text, "21:34");
        assert_eq!(update.declarations[2].display_text, "12:34:56");
        assert!(!update.declarations[2].enabled);
        assert_eq!(service.summary().sequence, 1);
        assert_eq!(service.summary().timer_descriptors, 1);
        assert_eq!(service.summary().unique_zone_conversions, 2);
        assert_eq!(service.summary().unique_format_operations, 3);
        service.shutdown().unwrap();
    }

    #[test]
    fn identical_render_keys_share_format_work_in_one_sequence() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([sample("2026-07-23T13:14:15Z", TimeZone::UTC)]),
        });
        let update = service
            .reconcile(
                2,
                0,
                vec![
                    declaration(1, "clock", "%H:%M", ClockTimeZone::Utc, true),
                    declaration(2, "clock", "%H:%M", ClockTimeZone::Utc, true),
                ],
            )
            .unwrap()
            .unwrap();
        assert_eq!(update.declarations.len(), 2);
        assert_eq!(update.declarations[0].display_text, "13:14");
        assert_eq!(update.declarations[1].display_text, "13:14");
        assert_eq!(
            update.declarations[0].sequence,
            update.declarations[1].sequence
        );
        assert_eq!(service.summary().unique_zone_conversions, 1);
        assert_eq!(service.summary().unique_format_operations, 1);
        assert_eq!(service.summary().cached_render_key_reuse, 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn second_clock_does_not_refresh_minute_only_legacy_output() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([
                sample("2026-07-23T13:14:00Z", TimeZone::UTC),
                sample("2026-07-23T13:14:01Z", TimeZone::UTC),
            ]),
        });
        service
            .reconcile(
                2,
                1,
                vec![declaration(1, "seconds", "%T", ClockTimeZone::Utc, true)],
            )
            .unwrap();
        let update = service.refresh(RefreshReason::Deadline).unwrap();
        assert!(update.legacy.is_none());
        assert_eq!(update.declarations.len(), 1);
        assert_eq!(update.declarations[0].display_text, "13:14:01");
        service.shutdown().unwrap();
    }

    #[test]
    fn disabling_freezes_and_rearming_uses_one_timer() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([
                sample("2026-01-01T10:00:00Z", TimeZone::UTC),
                sample("2026-01-01T10:00:09Z", TimeZone::UTC),
            ]),
        });
        let enabled = declaration(1, "clock", "%T", ClockTimeZone::Utc, true);
        service
            .reconcile(1, 0, vec![enabled.clone()])
            .unwrap()
            .unwrap();
        let disabled = declaration(1, "clock", "%T", ClockTimeZone::Utc, false);
        let update = service.reconcile(1, 0, vec![disabled]).unwrap().unwrap();
        assert_eq!(update.declarations[0].display_text, "10:00:00");
        assert!(!update.declarations[0].enabled);
        assert!(service.poll_fd().is_none());
        let update = service.reconcile(1, 0, vec![enabled]).unwrap().unwrap();
        assert_eq!(update.declarations[0].display_text, "10:00:09");
        assert!(service.poll_fd().is_some());
        assert_eq!(service.summary().timer_descriptors, 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn duplicate_due_output_is_suppressed() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([
                sample("2026-01-01T10:00:00Z", TimeZone::UTC),
                sample("2026-01-01T10:00:30Z", TimeZone::UTC),
            ]),
        });
        service
            .reconcile(
                1,
                1,
                vec![declaration(1, "hour", "%H", ClockTimeZone::Utc, true)],
            )
            .unwrap();
        let update = service.refresh(RefreshReason::ClockReset).unwrap();
        assert!(update.is_empty());
        assert!(service.summary().unchanged_values_suppressed >= 2);
        service.shutdown().unwrap();
    }

    #[test]
    fn process_limits_and_invalid_named_zone_are_contained() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::new(),
        });
        assert!(
            service
                .reconcile(
                    1,
                    0,
                    vec![declaration(
                        1,
                        "bad",
                        "%H",
                        ClockTimeZone::Named("Etc/Not-A-Zone".into()),
                        true,
                    )],
                )
                .is_err()
        );
        assert_eq!(service.declaration_count(), 0);

        let declarations = (0..=MAX_CLOCK_DECLARATIONS_PER_PROCESS)
            .map(|index| {
                declaration(
                    index as u64 + 1,
                    &format!("clock-{index}"),
                    "%H",
                    ClockTimeZone::Utc,
                    true,
                )
            })
            .collect();
        assert!(service.reconcile(1, 0, declarations).is_err());
    }

    #[test]
    fn wall_clock_jumps_resample_once_without_replaying_intervals() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([
                sample("2026-01-01T10:00:00Z", TimeZone::UTC),
                sample("2026-03-01T12:34:56Z", TimeZone::UTC),
                sample("2025-12-31T23:59:59Z", TimeZone::UTC),
            ]),
        });
        service
            .reconcile(
                1,
                0,
                vec![declaration(1, "clock", "%F %T", ClockTimeZone::Utc, true)],
            )
            .unwrap();
        let forward = service.refresh(RefreshReason::ClockReset).unwrap();
        assert_eq!(forward.declarations.len(), 1);
        assert_eq!(forward.declarations[0].display_text, "2026-03-01 12:34:56");
        let backward = service.refresh(RefreshReason::ClockReset).unwrap();
        assert_eq!(backward.declarations.len(), 1);
        assert_eq!(backward.declarations[0].display_text, "2025-12-31 23:59:59");
        assert_eq!(service.summary().sequence, 3);
        service.shutdown().unwrap();
    }

    #[test]
    fn named_zones_represent_one_instant_with_independent_civil_values() {
        let instant = timestamp("2026-07-23T12:00:00Z");
        for (zone, expected) in [
            ("America/New_York", "08:00 EDT -04:00"),
            ("Europe/London", "13:00 BST +01:00"),
            ("Asia/Tokyo", "21:00 JST +09:00"),
        ] {
            let zoned = instant.to_zoned(TimeZone::get(zone).unwrap());
            assert_eq!(checked_format("%H:%M %Z %:z", &zoned).unwrap(), expected);
            assert_eq!(checked_format("%Q", &zoned).unwrap(), zone);
        }
    }

    #[test]
    fn dst_transitions_preserve_skipped_and_repeated_civil_times() {
        let eastern = TimeZone::get("America/New_York").unwrap();
        let before_spring = timestamp("2026-03-08T06:59:59Z").to_zoned(eastern.clone());
        let after_spring = timestamp("2026-03-08T07:00:00Z").to_zoned(eastern.clone());
        assert_eq!(
            checked_format("%F %T %Z %:z", &before_spring).unwrap(),
            "2026-03-08 01:59:59 EST -05:00"
        );
        assert_eq!(
            checked_format("%F %T %Z %:z", &after_spring).unwrap(),
            "2026-03-08 03:00:00 EDT -04:00"
        );

        let first_one_thirty = timestamp("2026-11-01T05:30:00Z").to_zoned(eastern.clone());
        let second_one_thirty = timestamp("2026-11-01T06:30:00Z").to_zoned(eastern);
        assert_eq!(
            checked_format("%F %T %Z %:z", &first_one_thirty).unwrap(),
            "2026-11-01 01:30:00 EDT -04:00"
        );
        assert_eq!(
            checked_format("%F %T %Z %:z", &second_one_thirty).unwrap(),
            "2026-11-01 01:30:00 EST -05:00"
        );
        assert_eq!(
            checked_datetime(ClockCadence::Minute, &first_one_thirty).unwrap(),
            "2026-11-01T01:30:00-04:00"
        );
        assert_eq!(
            checked_datetime(ClockCadence::Minute, &second_one_thirty).unwrap(),
            "2026-11-01T01:30:00-05:00"
        );
        assert_eq!(
            checked_format(
                "%F %T",
                &timestamp("2026-11-01T06:30:00Z").to_zoned(TimeZone::UTC)
            )
            .unwrap(),
            "2026-11-01 06:30:00"
        );
    }

    #[test]
    fn utc_fallback_is_explicit_and_shared_with_local_declarations() {
        let mut fallback = sample("2026-01-01T12:34:56Z", TimeZone::UTC);
        fallback.used_utc_fallback = true;
        fallback.effective_local_zone = "UTC".into();
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([fallback]),
        });
        let update = service
            .reconcile(
                1,
                1,
                vec![declaration(1, "local", "%T", ClockTimeZone::Local, true)],
            )
            .unwrap()
            .unwrap();
        assert_eq!(update.legacy.unwrap().display_text, "12:34");
        assert_eq!(update.declarations[0].display_text, "12:34:56");
        assert_eq!(service.summary().effective_zone, "UTC");
        assert_eq!(service.summary().utc_fallbacks, 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn disabled_and_static_only_consumers_create_no_timer_descriptor() {
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([sample("2026-01-01T12:34:56Z", TimeZone::UTC)]),
        });
        service
            .reconcile(
                1,
                0,
                vec![
                    declaration(1, "disabled", "%T", ClockTimeZone::Utc, false),
                    declaration(1, "static", "HTMShell %%", ClockTimeZone::Utc, true),
                ],
            )
            .unwrap();
        assert!(service.poll_fd().is_none());
        assert_eq!(service.summary().timer_descriptors, 0);
        service.shutdown().unwrap();
    }

    #[test]
    fn one_thousand_modeled_seconds_and_minutes_use_one_sequence_each() {
        for (step_seconds, format) in [(1_i64, "%T"), (60_i64, "%H:%M")] {
            let start = timestamp("2026-01-01T00:00:00Z").as_second();
            let samples = (0..1_000_i64)
                .map(|step| {
                    sample_at(
                        Timestamp::new(start + step * step_seconds, 0).unwrap(),
                        TimeZone::UTC,
                    )
                })
                .collect();
            let mut service = ClockService::with_source(FakeClockSource { samples });
            service
                .reconcile(
                    1,
                    0,
                    vec![declaration(1, "clock", format, ClockTimeZone::Utc, true)],
                )
                .unwrap();
            for _ in 1..1_000 {
                let update = service.refresh(RefreshReason::Deadline).unwrap();
                assert_eq!(update.declarations.len(), 1);
            }
            assert_eq!(service.summary().sequence, 1_000);
            assert_eq!(service.summary().changed_declarations, 1_000);
            assert_eq!(service.summary().timer_descriptors, 1);
            service.shutdown().unwrap();
        }
    }

    #[test]
    fn five_hundred_duplicate_outputs_are_suppressed_without_new_timers() {
        let samples = (0..=500)
            .map(|_| sample("2026-01-01T10:00:30Z", TimeZone::UTC))
            .collect();
        let mut service = ClockService::with_source(FakeClockSource { samples });
        service
            .reconcile(
                1,
                0,
                vec![declaration(1, "hour", "%H", ClockTimeZone::Utc, true)],
            )
            .unwrap();
        for _ in 0..500 {
            assert!(
                service
                    .refresh(RefreshReason::ClockReset)
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(service.summary().changed_declarations, 1);
        assert_eq!(service.summary().suppressed_declarations, 500);
        assert_eq!(service.summary().timer_descriptors, 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn repeated_enabled_transitions_and_generation_replacements_remain_bounded() {
        let start = timestamp("2026-01-01T00:00:00Z").as_second();
        let samples = (0..=300_i64)
            .map(|step| sample_at(Timestamp::new(start + step, 0).unwrap(), TimeZone::UTC))
            .collect();
        let mut service = ClockService::with_source(FakeClockSource { samples });
        let mut enabled = true;
        service
            .reconcile(
                1,
                0,
                vec![declaration(1, "clock", "%T", ClockTimeZone::Utc, enabled)],
            )
            .unwrap();
        for _ in 0..250 {
            enabled = !enabled;
            let update = service
                .reconcile(
                    1,
                    0,
                    vec![declaration(1, "clock", "%T", ClockTimeZone::Utc, enabled)],
                )
                .unwrap()
                .expect("enabled transition updates the target");
            assert_eq!(update.declarations.len(), 1);
            assert_eq!(update.declarations[0].enabled, enabled);
        }
        for generation in 2..=101 {
            let update = service
                .reconcile(
                    1,
                    0,
                    vec![declaration(
                        generation,
                        "clock",
                        "%T",
                        ClockTimeZone::Utc,
                        true,
                    )],
                )
                .unwrap()
                .expect("fresh generation receives an initial value");
            assert_eq!(
                update.declarations[0].id.document_generation.serial,
                generation
            );
            assert_eq!(service.declaration_count(), 1);
        }
        assert_eq!(service.summary().timer_descriptors, 1);
        assert_eq!(service.summary().maximum_declarations, 1);
        service.shutdown().unwrap();
    }
}
