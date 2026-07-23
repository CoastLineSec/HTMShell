use jiff::{Timestamp, tz::TimeZone};
use rustix::fd::{BorrowedFd, OwnedFd};
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
    timerfd_settime,
};
use std::fmt;
use std::time::{Instant, SystemTime};

const CLOCK_FORMAT: &str = "HH:mm";
const MAX_VISIBLE_CHANGE_SEARCH_SECONDS: i64 = 7_200;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClockSnapshot {
    pub(crate) display_text: String,
    pub(crate) sampled_instant: Timestamp,
    pub(crate) effective_zone: String,
    pub(crate) sequence: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ClockSample {
    timestamp: Timestamp,
    time_zone: TimeZone,
    effective_zone: String,
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
        let (time_zone, effective_zone, used_utc_fallback) = match TimeZone::try_system() {
            Ok(zone) => {
                let name = zone
                    .iana_name()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "system-local".into());
                (zone, name, false)
            }
            Err(error) => {
                eprintln!(
                    "htmshell-live: system timezone unavailable ({error}); clock.time uses UTC"
                );
                (TimeZone::UTC, "UTC".into(), true)
            }
        };
        Ok(ClockSample {
            timestamp,
            time_zone,
            effective_zone,
            used_utc_fallback,
            sample_us,
            timezone_us: elapsed_us(timezone_started),
        })
    }
}

#[derive(Debug)]
pub(crate) enum ClockError {
    TimeSource(String),
    Deadline(String),
    Timer(String),
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimeSource(message) => write!(f, "clock source error: {message}"),
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

#[derive(Debug)]
pub(crate) struct ClockService<S = SystemClockSource> {
    source: S,
    timer: Option<ClockTimer>,
    snapshot: Option<ClockSnapshot>,
    next_deadline: Option<Timestamp>,
    subscribers: usize,
    generation: u64,
    sequence: u64,
    summary: ClockServiceSummary,
}

impl Default for ClockService<SystemClockSource> {
    fn default() -> Self {
        Self {
            source: SystemClockSource,
            timer: None,
            snapshot: None,
            next_deadline: None,
            subscribers: 0,
            generation: 1,
            sequence: 0,
            summary: ClockServiceSummary {
                format: CLOCK_FORMAT.into(),
                generation: 1,
                ..ClockServiceSummary::default()
            },
        }
    }
}

impl<S: ClockSource> ClockService<S> {
    #[cfg(test)]
    fn with_source(source: S) -> Self {
        Self {
            source,
            timer: None,
            snapshot: None,
            next_deadline: None,
            subscribers: 0,
            generation: 1,
            sequence: 0,
            summary: ClockServiceSummary {
                format: CLOCK_FORMAT.into(),
                generation: 1,
                ..ClockServiceSummary::default()
            },
        }
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        self.subscribers
    }

    pub(crate) fn current_snapshot(&self) -> Option<&ClockSnapshot> {
        self.snapshot.as_ref()
    }

    pub(crate) fn set_subscriber_count(
        &mut self,
        subscribers: usize,
    ) -> Result<Option<ClockSnapshot>, ClockError> {
        let started = Instant::now();
        let previous = self.subscribers;
        self.subscribers = subscribers;
        self.summary.subscribers = subscribers;
        self.summary.maximum_subscribers = self.summary.maximum_subscribers.max(subscribers);
        if subscribers == 0 {
            if let Some(timer) = self.timer.as_mut()
                && timer.armed
            {
                timer.disarm()?;
            }
            self.next_deadline = None;
            return Ok(None);
        }
        if previous == 0 {
            if self.timer.is_none() {
                self.timer = Some(ClockTimer::new()?);
                self.summary.timer_descriptors = 1;
            }
            self.summary.initialization_us = elapsed_us(started);
            return self.refresh(false);
        }
        Ok(None)
    }

    pub(crate) fn poll_fd(&self) -> Option<BorrowedFd<'_>> {
        self.timer.as_ref().and_then(ClockTimer::poll_fd)
    }

    pub(crate) fn handle_ready(&mut self) -> Result<Option<ClockSnapshot>, ClockError> {
        let read = self
            .timer
            .as_ref()
            .ok_or_else(|| ClockError::Timer("timer readiness without a timerfd".into()))?
            .consume()?;
        self.handle_timer_read(read)
    }

    fn handle_timer_read(&mut self, read: TimerRead) -> Result<Option<ClockSnapshot>, ClockError> {
        match read {
            TimerRead::NotReady => Ok(None),
            TimerRead::Expirations(expirations) => {
                self.summary.wakeups = self.summary.wakeups.saturating_add(1);
                self.summary.expirations = self.summary.expirations.saturating_add(expirations);
                self.refresh(true)
            }
            TimerRead::ClockReset => {
                self.summary.wakeups = self.summary.wakeups.saturating_add(1);
                self.summary.wall_clock_resets = self.summary.wall_clock_resets.saturating_add(1);
                self.refresh(true)
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
        if let Some(snapshot) = &self.snapshot {
            summary.sequence = snapshot.sequence;
            summary.sampled_unix_seconds = snapshot.sampled_instant.as_second();
            summary.effective_zone = snapshot.effective_zone.clone();
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
        self.next_deadline = None;
        self.subscribers = 0;
        self.summary.subscribers = 0;
        self.generation = self.generation.saturating_add(1);
        self.summary.generation = self.generation;
        Ok(())
    }

    fn refresh(&mut self, timer_wakeup: bool) -> Result<Option<ClockSnapshot>, ClockError> {
        if self.subscribers == 0 {
            return Ok(None);
        }
        let sample = self.source.sample()?;
        self.refresh_from_sample(sample, timer_wakeup)
    }

    fn refresh_from_sample(
        &mut self,
        sample: ClockSample,
        timer_wakeup: bool,
    ) -> Result<Option<ClockSnapshot>, ClockError> {
        self.summary.last_sample_us = sample.sample_us;
        self.summary.last_timezone_us = sample.timezone_us;
        if sample.used_utc_fallback {
            self.summary.utc_fallbacks = self.summary.utc_fallbacks.saturating_add(1);
        }

        let format_started = Instant::now();
        let (hour, minute) = visible_minute(sample.timestamp, &sample.time_zone);
        let display_text = format!("{hour:02}:{minute:02}");
        self.summary.last_format_us = elapsed_us(format_started);

        let deadline_started = Instant::now();
        let deadline = next_visible_change(sample.timestamp, &sample.time_zone)?;
        self.summary.last_deadline_us = elapsed_us(deadline_started);

        let changed = self
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.display_text != display_text);
        self.sequence = self.sequence.saturating_add(1);
        let snapshot = ClockSnapshot {
            display_text,
            sampled_instant: sample.timestamp,
            effective_zone: sample.effective_zone,
            sequence: self.sequence,
        };
        self.summary.effective_zone = snapshot.effective_zone.clone();
        if changed {
            self.summary.changed_values = self.summary.changed_values.saturating_add(1);
        } else if timer_wakeup {
            self.summary.unchanged_values_suppressed =
                self.summary.unchanged_values_suppressed.saturating_add(1);
        }
        self.snapshot = Some(snapshot.clone());
        self.next_deadline = Some(deadline);

        let arm_started = Instant::now();
        self.timer
            .as_mut()
            .ok_or_else(|| ClockError::Timer("clock refresh has no timerfd".into()))?
            .arm(deadline)?;
        self.summary.last_timer_arm_us = elapsed_us(arm_started);
        Ok(changed.then_some(snapshot))
    }
}

fn visible_minute(timestamp: Timestamp, time_zone: &TimeZone) -> (i8, i8) {
    let local = timestamp.to_zoned(time_zone.clone());
    (local.hour(), local.minute())
}

fn next_visible_change(
    timestamp: Timestamp,
    time_zone: &TimeZone,
) -> Result<Timestamp, ClockError> {
    let current = visible_minute(timestamp, time_zone);
    let first_second = timestamp
        .as_second()
        .checked_add(1)
        .ok_or_else(|| ClockError::Deadline("timestamp second overflow".into()))?;
    for offset in 0..MAX_VISIBLE_CHANGE_SEARCH_SECONDS {
        let second = first_second
            .checked_add(offset)
            .ok_or_else(|| ClockError::Deadline("deadline second overflow".into()))?;
        let candidate =
            Timestamp::new(second, 0).map_err(|error| ClockError::Deadline(error.to_string()))?;
        if visible_minute(candidate, time_zone) != current {
            return Ok(candidate);
        }
    }
    Err(ClockError::Deadline(
        "visible clock value did not change within two hours".into(),
    ))
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
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
            effective_zone: zone.iana_name().unwrap_or("test-zone").into(),
            time_zone: zone,
            used_utc_fallback: false,
            sample_us: 3,
            timezone_us: 4,
        }
    }

    #[test]
    fn fixed_clock_format_is_zero_padded() {
        for (value, expected) in [
            ("2026-01-01T00:05:00Z", "00:05"),
            ("2026-01-01T09:07:00Z", "09:07"),
            ("2026-01-01T17:42:00Z", "17:42"),
            ("2026-01-01T23:59:00Z", "23:59"),
        ] {
            let (hour, minute) = visible_minute(timestamp(value), &TimeZone::UTC);
            assert_eq!(format!("{hour:02}:{minute:02}"), expected);
        }
    }

    #[test]
    fn deadline_finds_midnight_month_and_year_boundaries() {
        for (value, expected) in [
            ("2026-01-01T00:00:14.500Z", "2026-01-01T00:01:00Z"),
            ("2026-01-31T23:59:12Z", "2026-02-01T00:00:00Z"),
            ("2026-12-31T23:59:59Z", "2027-01-01T00:00:00Z"),
        ] {
            assert_eq!(
                next_visible_change(timestamp(value), &TimeZone::UTC).unwrap(),
                timestamp(expected)
            );
        }
    }

    #[test]
    fn deadline_verifies_spring_forward_and_fall_back_changes() {
        let eastern = TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap();
        assert_eq!(
            next_visible_change(timestamp("2026-03-08T06:59:30Z"), &eastern).unwrap(),
            timestamp("2026-03-08T07:00:00Z")
        );
        assert_eq!(
            visible_minute(timestamp("2026-03-08T07:00:00Z"), &eastern),
            (3, 0)
        );
        assert_eq!(
            next_visible_change(timestamp("2026-11-01T05:59:30Z"), &eastern).unwrap(),
            timestamp("2026-11-01T06:00:00Z")
        );
        assert_eq!(
            visible_minute(timestamp("2026-11-01T06:00:00Z"), &eastern),
            (1, 0)
        );
    }

    #[test]
    fn fake_source_models_forward_backward_and_duplicate_samples() {
        let mut source = FakeClockSource {
            samples: VecDeque::from([
                sample("2026-01-01T10:00:00Z", TimeZone::UTC),
                sample("2026-01-01T10:04:00Z", TimeZone::UTC),
                sample("2026-01-01T09:58:00Z", TimeZone::UTC),
                sample("2026-01-01T09:58:30Z", TimeZone::UTC),
            ]),
        };
        let values: Vec<_> = (0..4)
            .map(|_| {
                let sample = source.sample().unwrap();
                let (hour, minute) = visible_minute(sample.timestamp, &sample.time_zone);
                format!("{hour:02}:{minute:02}")
            })
            .collect();
        assert_eq!(values, ["10:00", "10:04", "09:58", "09:58"]);
    }

    #[test]
    fn subscriber_lifecycle_is_single_scheduler_state() {
        let mut service = ClockService::default();
        assert_eq!(service.subscriber_count(), 0);
        assert!(service.poll_fd().is_none());
        service.set_subscriber_count(1).unwrap();
        let first = service.current_snapshot().unwrap().clone();
        assert_eq!(service.summary().timer_descriptors, 1);
        assert_eq!(service.summary().subscribers, 1);
        service.set_subscriber_count(4).unwrap();
        assert_eq!(service.current_snapshot().unwrap(), &first);
        assert_eq!(service.summary().timer_descriptors, 1);
        service.set_subscriber_count(0).unwrap();
        assert!(service.poll_fd().is_none());
        service.set_subscriber_count(2).unwrap();
        assert_eq!(service.summary().timer_descriptors, 1);
        service.shutdown().unwrap();
        assert_eq!(service.summary().timer_descriptors, 1);
        assert_eq!(service.summary().subscribers, 0);
    }

    #[test]
    fn utc_fallback_sample_is_explicit() {
        let fallback = ClockSample {
            timestamp: timestamp("2026-01-01T12:34:00Z"),
            time_zone: TimeZone::UTC,
            effective_zone: "UTC".into(),
            used_utc_fallback: true,
            sample_us: 0,
            timezone_us: 0,
        };
        assert!(fallback.used_utc_fallback);
        assert_eq!(
            visible_minute(fallback.timestamp, &fallback.time_zone),
            (12, 34)
        );
        let mut service = ClockService::with_source(FakeClockSource {
            samples: VecDeque::from([fallback]),
        });
        let snapshot = service
            .set_subscriber_count(1)
            .unwrap()
            .expect("initial fallback snapshot");
        assert_eq!(snapshot.display_text, "12:34");
        assert_eq!(snapshot.effective_zone, "UTC");
        assert_eq!(service.summary().utc_fallbacks, 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn modeled_scheduler_coalesces_missed_expirations_and_clock_reset() {
        let source = FakeClockSource {
            samples: VecDeque::from([
                sample("2026-01-01T10:00:10Z", TimeZone::UTC),
                sample("2026-01-01T10:04:10Z", TimeZone::UTC),
                sample("2026-01-01T09:58:10Z", TimeZone::UTC),
            ]),
        };
        let mut service = ClockService::with_source(source);
        assert!(service.set_subscriber_count(2).unwrap().is_some());
        assert!(
            service
                .handle_timer_read(TimerRead::Expirations(4))
                .unwrap()
                .is_some()
        );
        assert_eq!(service.summary().expirations, 4);
        assert!(
            service
                .handle_timer_read(TimerRead::ClockReset)
                .unwrap()
                .is_some()
        );
        assert_eq!(service.current_snapshot().unwrap().display_text, "09:58");
        assert_eq!(service.summary().wall_clock_resets, 1);
        assert_eq!(service.summary().wakeups, 2);
        service.shutdown().unwrap();
    }

    #[test]
    fn one_thousand_modeled_minutes_use_one_sequence_each() {
        let samples = (0..1_000_i64)
            .map(|minute| {
                let second = timestamp("2026-01-01T00:00:00Z").as_second() + minute * 60;
                sample(
                    &Timestamp::new(second, 0).unwrap().to_string(),
                    TimeZone::UTC,
                )
            })
            .collect();
        let mut service = ClockService::with_source(FakeClockSource { samples });
        service.set_subscriber_count(1).unwrap();
        for _ in 1..1_000 {
            service
                .handle_timer_read(TimerRead::Expirations(1))
                .unwrap();
        }
        let summary = service.summary();
        assert_eq!(summary.sequence, 1_000);
        assert_eq!(summary.changed_values, 1_000);
        assert_eq!(summary.wakeups, 999);
        assert_eq!(summary.expirations, 999);
        service.shutdown().unwrap();
    }

    #[test]
    fn duplicate_minutes_and_subscription_churn_stay_quiet() {
        let samples = (0..101)
            .map(|_| sample("2026-01-01T10:00:30Z", TimeZone::UTC))
            .collect();
        let mut service = ClockService::with_source(FakeClockSource { samples });
        service.set_subscriber_count(1).unwrap();
        for _ in 0..100 {
            assert!(
                service
                    .handle_timer_read(TimerRead::Expirations(1))
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(service.summary().changed_values, 1);
        assert_eq!(service.summary().unchanged_values_suppressed, 100);
        for _ in 0..100 {
            service.set_subscriber_count(2).unwrap();
            service.set_subscriber_count(1).unwrap();
        }
        assert_eq!(service.summary().timer_descriptors, 1);
        service.shutdown().unwrap();
    }
}
