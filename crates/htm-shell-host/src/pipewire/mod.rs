mod model;
mod reconcile;
mod transport;

#[cfg(test)]
use model::PipeWireAvailability;
use model::{PipeWireDelta, PipeWireSnapshot};
use reconcile::PipeWireReconciler;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fd::BorrowedFd;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};
use transport::PipeWireTransport;

const RECONNECT_DELAYS: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];
const DIAGNOSTIC_MAXIMUM_RUNTIME: Duration = Duration::from_secs(3);
const DIAGNOSTIC_SETTLE_TIME: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipeWireLifecycle {
    Dormant,
    Connecting,
    Synchronizing,
    Ready,
    Disconnected,
    Reconnecting,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SynchronizationStage {
    First,
    Second,
    Ready,
}

pub(crate) struct PipeWireSource {
    lifecycle: PipeWireLifecycle,
    transport: Option<PipeWireTransport>,
    reconciler: PipeWireReconciler,
    next_generation: u64,
    expected_sync: Option<i32>,
    synchronization: SynchronizationStage,
    retry_deadline: Option<Instant>,
    backoff_index: usize,
    reconnect_attempts: u64,
    last_publication: Instant,
}

impl Default for PipeWireSource {
    fn default() -> Self {
        Self {
            lifecycle: PipeWireLifecycle::Dormant,
            transport: None,
            reconciler: PipeWireReconciler::default(),
            next_generation: 0,
            expected_sync: None,
            synchronization: SynchronizationStage::First,
            retry_deadline: None,
            backoff_index: 0,
            reconnect_attempts: 0,
            last_publication: Instant::now(),
        }
    }
}

impl PipeWireSource {
    pub(crate) fn start(&mut self) {
        if self.lifecycle != PipeWireLifecycle::Dormant {
            return;
        }
        self.attempt_connect(Instant::now(), false);
    }

    pub(crate) fn lifecycle(&self) -> PipeWireLifecycle {
        self.lifecycle
    }

    pub(crate) fn snapshot(&self) -> &PipeWireSnapshot {
        self.reconciler.current()
    }

    pub(crate) fn raw_poll_fd(&self) -> Option<RawFd> {
        self.transport.as_ref().map(PipeWireTransport::raw_fd)
    }

    pub(crate) fn retry_timeout(&self, now: Instant) -> Option<Duration> {
        self.retry_deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    pub(crate) fn reconnect_if_due(&mut self, now: Instant) {
        if self.retry_deadline.is_some_and(|deadline| deadline <= now) {
            self.retry_deadline = None;
            self.attempt_connect(now, true);
        }
    }

    pub(crate) fn handle_poll_error(&mut self, message: impl Into<String>) {
        self.disconnect(Instant::now(), message.into());
    }

    pub(crate) fn dispatch_ready(&mut self) {
        let Some(transport) = self.transport.as_mut() else {
            return;
        };
        if let Err(error) = transport.dispatch_nonblocking() {
            self.disconnect(Instant::now(), error);
            return;
        }
        self.reconcile_callbacks(Instant::now());
    }

    pub(crate) fn reconcile_callbacks(&mut self, now: Instant) {
        let Some(transport) = self.transport.as_mut() else {
            return;
        };
        let deltas = transport.take_staged();
        let mut resources = transport.resources();
        resources.reconnect_attempts = self.reconnect_attempts;
        self.reconciler.update_transport_counters(&resources);
        if deltas.is_empty() {
            return;
        }
        if let Some(error) = deltas.iter().find_map(|delta| match delta {
            PipeWireDelta::CoreError(message) | PipeWireDelta::SourceError(message) => {
                Some(message.clone())
            }
            _ => None,
        }) {
            self.disconnect(now, error);
            return;
        }
        let diagnostic_count = deltas
            .iter()
            .filter_map(|delta| match delta {
                PipeWireDelta::Diagnostic(message) => Some(message),
                _ => None,
            })
            .count();
        self.reconciler.record_diagnostics(diagnostic_count);
        let completed = deltas
            .iter()
            .filter_map(|delta| match delta {
                PipeWireDelta::CoreDone(sequence) => Some(*sequence),
                _ => None,
            })
            .collect::<Vec<_>>();
        let graph_deltas = deltas.into_iter().filter(|delta| {
            !matches!(
                delta,
                PipeWireDelta::CoreDone(_)
                    | PipeWireDelta::CoreError(_)
                    | PipeWireDelta::SourceError(_)
                    | PipeWireDelta::Diagnostic(_)
            )
        });

        let result = if self.synchronization == SynchronizationStage::Ready {
            self.reconciler.apply(graph_deltas)
        } else {
            self.reconciler
                .apply_unpublished(graph_deltas)
                .map(|()| None)
        };
        let changed = match result {
            Ok(publication) => publication.is_some(),
            Err(error) => {
                self.disconnect(now, error.to_string());
                return;
            }
        };

        if completed
            .iter()
            .any(|sequence| Some(*sequence) == self.expected_sync)
        {
            match self.synchronization {
                SynchronizationStage::First => {
                    let Some(transport) = self.transport.as_ref() else {
                        self.disconnect(
                            now,
                            "PipeWire transport disappeared during synchronization".into(),
                        );
                        return;
                    };
                    let next = transport.request_sync(2);
                    match next {
                        Ok(sequence) => {
                            self.expected_sync = Some(sequence);
                            self.synchronization = SynchronizationStage::Second;
                        }
                        Err(error) => self.disconnect(now, error),
                    }
                }
                SynchronizationStage::Second => {
                    self.synchronization = SynchronizationStage::Ready;
                    self.expected_sync = None;
                    self.lifecycle = PipeWireLifecycle::Ready;
                    self.backoff_index = 0;
                    self.retry_deadline = None;
                    match self.reconciler.mark_ready() {
                        Ok(Some(_)) => self.last_publication = now,
                        Ok(None) => {}
                        Err(error) => self.disconnect(now, error.to_string()),
                    }
                }
                SynchronizationStage::Ready => {}
            }
        } else if self.synchronization == SynchronizationStage::Ready && changed {
            self.last_publication = now;
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.lifecycle = PipeWireLifecycle::Stopping;
        self.retry_deadline = None;
        self.expected_sync = None;
        self.transport.take();
    }

    fn attempt_connect(&mut self, now: Instant, reconnect: bool) {
        self.lifecycle = if reconnect {
            PipeWireLifecycle::Reconnecting
        } else {
            PipeWireLifecycle::Connecting
        };
        if reconnect {
            self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        }
        self.next_generation = self.next_generation.saturating_add(1);
        if self.next_generation == 0 {
            self.next_generation = 1;
        }
        if self
            .reconciler
            .begin_generation(self.next_generation)
            .is_err()
        {
            self.disconnect(now, "failed to begin PipeWire generation".into());
            return;
        }
        let transport = match PipeWireTransport::connect(self.next_generation) {
            Ok(transport) => transport,
            Err(error) => {
                self.disconnect(now, error);
                return;
            }
        };
        let sequence = match transport.request_sync(1) {
            Ok(sequence) => sequence,
            Err(error) => {
                self.transport = Some(transport);
                self.disconnect(now, error);
                return;
            }
        };
        self.transport = Some(transport);
        self.expected_sync = Some(sequence);
        self.synchronization = SynchronizationStage::First;
        self.lifecycle = PipeWireLifecycle::Synchronizing;
    }

    fn disconnect(&mut self, now: Instant, _reason: String) {
        self.transport.take();
        self.expected_sync = None;
        self.synchronization = SynchronizationStage::First;
        self.lifecycle = PipeWireLifecycle::Disconnected;
        let _ = self.reconciler.mark_unavailable();
        let delay = RECONNECT_DELAYS[self.backoff_index.min(RECONNECT_DELAYS.len() - 1)];
        self.backoff_index = (self.backoff_index + 1).min(RECONNECT_DELAYS.len() - 1);
        self.retry_deadline = Some(now + delay);
        self.lifecycle = PipeWireLifecycle::Reconnecting;
        self.last_publication = now;
    }
}

pub(crate) fn run_pipewire_graph_diagnostic() -> Result<PipeWireSnapshot, String> {
    let started = Instant::now();
    let mut source = PipeWireSource::default();
    source.start();
    loop {
        let now = Instant::now();
        source.reconnect_if_due(now);
        if now.duration_since(started) >= DIAGNOSTIC_MAXIMUM_RUNTIME {
            break;
        }
        if source.lifecycle() == PipeWireLifecycle::Ready
            && now.duration_since(source.last_publication) >= DIAGNOSTIC_SETTLE_TIME
        {
            break;
        }
        let timeout = source
            .retry_timeout(now)
            .unwrap_or(Duration::from_millis(100))
            .min(Duration::from_millis(100));
        let timespec = duration_to_timespec(timeout);
        let raw_fd = source.raw_poll_fd();
        let mut descriptors = Vec::with_capacity(1);
        if let Some(raw_fd) = raw_fd {
            // The transport remains alive for the complete poll call.
            let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
            descriptors.push(PollFd::from_borrowed_fd(
                fd,
                PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
            ));
        }
        match poll(&mut descriptors, Some(&timespec)) {
            Ok(_) => {}
            Err(error) if error == rustix::io::Errno::INTR => continue,
            Err(error) => return Err(format!("poll PipeWire descriptor: {error}")),
        }
        let events = descriptors.first().map(PollFd::revents);
        drop(descriptors);
        if let Some(events) = events {
            if events.intersects(PollFlags::NVAL) {
                source.handle_poll_error("PipeWire loop descriptor became invalid");
            } else if events.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP) {
                source.dispatch_ready();
            }
        }
    }
    let snapshot = source.snapshot().clone();
    source.shutdown();
    Ok(snapshot)
}

pub fn run_pipewire_graph_diagnostic_json() -> Result<String, String> {
    run_pipewire_graph_diagnostic().and_then(|snapshot| {
        serde_json::to_string_pretty(&snapshot)
            .map_err(|error| format!("serialize PipeWire graph diagnostic: {error}"))
    })
}

pub(crate) fn duration_to_timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: duration.as_secs().min(i64::MAX as u64) as i64,
        tv_nsec: duration.subsec_nanos() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_schedule_is_bounded() {
        assert_eq!(
            RECONNECT_DELAYS,
            [
                Duration::from_millis(250),
                Duration::from_secs(1),
                Duration::from_secs(5),
                Duration::from_secs(30)
            ]
        );
    }

    #[test]
    fn duration_conversion_is_exact() {
        assert_eq!(
            duration_to_timespec(Duration::new(5, 123)),
            Timespec {
                tv_sec: 5,
                tv_nsec: 123
            }
        );
    }

    #[test]
    fn unavailable_snapshot_is_empty() {
        let source = PipeWireSource::default();
        assert_eq!(
            source.snapshot().availability,
            PipeWireAvailability::Unavailable
        );
        assert!(source.snapshot().nodes.is_empty());
        assert!(source.snapshot().links.is_empty());
    }

    #[test]
    fn shutdown_cancels_retry() {
        let mut source = PipeWireSource {
            lifecycle: PipeWireLifecycle::Reconnecting,
            retry_deadline: Some(Instant::now()),
            ..PipeWireSource::default()
        };
        source.shutdown();
        assert_eq!(source.lifecycle(), PipeWireLifecycle::Stopping);
        assert!(source.retry_deadline.is_none());
    }

    #[test]
    fn repeated_failures_advance_to_a_bounded_backoff() {
        let base = Instant::now();
        let mut source = PipeWireSource::default();
        for (index, expected) in RECONNECT_DELAYS.into_iter().enumerate() {
            let now = base + Duration::from_secs(index as u64 * 60);
            source.disconnect(now, "modeled failure".into());
            assert_eq!(source.retry_deadline, Some(now + expected));
        }
        let now = base + Duration::from_secs(300);
        source.disconnect(now, "modeled failure".into());
        assert_eq!(source.retry_deadline, Some(now + Duration::from_secs(30)));
    }

    #[test]
    fn poll_error_clears_stale_snapshot_before_retry() {
        let mut source = PipeWireSource::default();
        source.reconciler.begin_generation(1).unwrap();
        source
            .reconciler
            .apply([PipeWireDelta::NodeAdded {
                raw_id: 1,
                properties: std::collections::BTreeMap::new(),
            }])
            .unwrap();
        source.handle_poll_error("modeled descriptor failure");
        assert_eq!(
            source.snapshot().availability,
            PipeWireAvailability::Unavailable
        );
        assert!(source.snapshot().nodes.is_empty());
        assert_eq!(source.lifecycle(), PipeWireLifecycle::Reconnecting);
    }

    #[test]
    fn model_errors_are_displayable() {
        let error = super::model::PipeWireModelError::InvalidData("bad value".into());
        assert!(error.to_string().contains("bad value"));
    }
}
