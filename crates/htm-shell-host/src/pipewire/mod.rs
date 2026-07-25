mod model;
mod public;
mod reconcile;
mod transport;

use htm_runtime::{
    PipeWireAudioOperation, PipeWireAudioTarget, PipeWireControlIdentity, PipeWireControlRequest,
    PipeWireControlState,
};
pub use model::PipeWireAudioChannelPosition;
#[cfg(test)]
use model::PipeWireAvailability;
use model::PipeWireDelta;
pub(crate) use model::PipeWireSnapshot;
pub(crate) use public::PipeWireDemand;
pub use public::{PipeWireNodeDirection, PipeWireNodeType};
use reconcile::PipeWireReconciler;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fd::BorrowedFd;
use std::collections::BTreeMap;
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
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const VOLUME_WRITE_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PipeWireControlOutcome {
    pub control: PipeWireControlIdentity,
    pub state: PipeWireControlState,
}

#[derive(Debug, Clone)]
struct PendingWrite<T> {
    sent: T,
    queued: Option<T>,
    started: Instant,
    layout_generation: Option<u64>,
    controls: BTreeMap<PipeWireControlIdentity, PipeWireAudioTarget>,
}

#[derive(Debug, Default)]
struct NodeWriteCoordinator {
    mute: Option<PendingWrite<bool>>,
    volume: Option<PendingWrite<Vec<model::FiniteVolume>>>,
}

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
    demand: PipeWireDemand,
    writes: BTreeMap<model::PipeWireNodeId, NodeWriteCoordinator>,
    control_outcomes: Vec<PipeWireControlOutcome>,
    control_counters: model::PipeWireControlCounters,
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
            demand: PipeWireDemand::default(),
            writes: BTreeMap::new(),
            control_outcomes: Vec::new(),
            control_counters: model::PipeWireControlCounters::default(),
        }
    }
}

impl PipeWireSource {
    pub(crate) fn start(&mut self) {
        if self.lifecycle != PipeWireLifecycle::Dormant || self.demand.is_empty() {
            return;
        }
        self.attempt_connect(Instant::now(), false);
    }

    pub(crate) fn set_demand(&mut self, demand: PipeWireDemand) -> bool {
        if self.demand == demand {
            return false;
        }
        let was_empty = self.demand.is_empty();
        let had_audio = self.demand.audio_state || self.demand.audio_writes;
        let had_writes = self.demand.audio_writes;
        self.demand = demand.clone();
        let has_audio = demand.audio_state || demand.audio_writes;
        if !had_audio && has_audio {
            self.control_counters.audio_state_activations = self
                .control_counters
                .audio_state_activations
                .saturating_add(1);
        } else if had_audio && !has_audio {
            self.control_counters.audio_state_releases =
                self.control_counters.audio_state_releases.saturating_add(1);
        }
        self.sync_control_counters();
        if had_writes && !demand.audio_writes {
            self.finish_all_controls(PipeWireControlState::Unavailable);
        }
        if demand.is_empty() {
            self.transport.take();
            self.retry_deadline = None;
            self.expected_sync = None;
            self.synchronization = SynchronizationStage::First;
            self.lifecycle = PipeWireLifecycle::Dormant;
            let _ = self.reconciler.mark_unavailable();
        } else if was_empty {
            self.start();
        } else if let Some(transport) = self.transport.as_mut() {
            transport.set_demand(demand);
            self.reconcile_callbacks(Instant::now());
        }
        true
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
        let retry = self
            .retry_deadline
            .map(|deadline| deadline.saturating_duration_since(now));
        let control = self
            .writes
            .values()
            .flat_map(|coordinator| {
                [
                    coordinator.mute.as_ref().map(|write| write.started),
                    coordinator.volume.as_ref().map(|write| write.started),
                ]
            })
            .flatten()
            .min()
            .map(|started| (started + CONTROL_TIMEOUT).saturating_duration_since(now));
        let cadence = self
            .writes
            .values()
            .filter_map(|coordinator| coordinator.volume.as_ref())
            .filter(|write| write.queued.is_some())
            .map(|write| (write.started + VOLUME_WRITE_INTERVAL).saturating_duration_since(now))
            .min();
        [retry, control, cadence].into_iter().flatten().min()
    }

    pub(crate) fn reconnect_if_due(&mut self, now: Instant) -> bool {
        let before = self.snapshot().sequence;
        self.reconcile_pending_writes(now);
        if self.retry_deadline.is_some_and(|deadline| deadline <= now) {
            self.retry_deadline = None;
            self.attempt_connect(now, true);
        }
        self.snapshot().sequence != before
    }

    pub(crate) fn handle_poll_error(&mut self, message: impl Into<String>) -> bool {
        let before = self.snapshot().sequence;
        self.disconnect(Instant::now(), message.into());
        self.snapshot().sequence != before
    }

    pub(crate) fn dispatch_ready(&mut self) -> bool {
        let before = self.snapshot().sequence;
        let Some(transport) = self.transport.as_mut() else {
            return false;
        };
        if let Err(error) = transport.dispatch_nonblocking() {
            self.disconnect(Instant::now(), error);
            return self.snapshot().sequence != before;
        }
        self.reconcile_callbacks(Instant::now());
        self.snapshot().sequence != before
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
        let audio_updates = deltas
            .iter()
            .filter(|delta| matches!(delta, PipeWireDelta::NodeAudioInfo(_)))
            .count();
        self.control_counters.audio_parameter_updates = self
            .control_counters
            .audio_parameter_updates
            .saturating_add(audio_updates as u64);
        self.sync_control_counters();
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
        self.reconcile_pending_writes(now);
    }

    pub(crate) fn request_control(
        &mut self,
        request: PipeWireControlRequest,
    ) -> Result<(), String> {
        if !self.demand.audio_writes {
            return Err("PipeWire audio writes have no active document demand".into());
        }
        let node_id = match self.resolve_control_target(&request.target) {
            Ok(node_id) => node_id,
            Err(error) => {
                self.control_counters.stale_writes_rejected = self
                    .control_counters
                    .stale_writes_rejected
                    .saturating_add(1);
                if matches!(
                    request.operation,
                    PipeWireAudioOperation::SetVolume | PipeWireAudioOperation::SetChannelVolume
                ) {
                    self.control_counters.stale_vectors_rejected = self
                        .control_counters
                        .stale_vectors_rejected
                        .saturating_add(1);
                    if matches!(request.operation, PipeWireAudioOperation::SetChannelVolume) {
                        self.control_counters.layout_invalidated_intents = self
                            .control_counters
                            .layout_invalidated_intents
                            .saturating_add(1);
                    }
                }
                self.sync_control_counters();
                return Err(error);
            }
        };
        let node = self
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .cloned()
            .ok_or_else(|| "PipeWire audio target is unavailable".to_owned())?;
        match request.operation {
            PipeWireAudioOperation::Mute
            | PipeWireAudioOperation::Unmute
            | PipeWireAudioOperation::ToggleMute => {
                if matches!(request.target, PipeWireAudioTarget::ChannelItem { .. }) {
                    return Err("PipeWire mute operations cannot target a channel".into());
                }
                if !node.audio.can_set_mute {
                    return Err("PipeWire audio target does not permit mute control".into());
                }
                let current = node
                    .audio
                    .muted
                    .ok_or_else(|| "PipeWire mute state is unavailable".to_owned())?;
                let desired = match request.operation {
                    PipeWireAudioOperation::Mute => true,
                    PipeWireAudioOperation::Unmute => false,
                    PipeWireAudioOperation::ToggleMute => !current,
                    PipeWireAudioOperation::SetVolume
                    | PipeWireAudioOperation::SetChannelVolume => unreachable!(),
                };
                if self
                    .writes
                    .get(&node_id)
                    .and_then(|coordinator| coordinator.mute.as_ref())
                    .is_none()
                    && current == desired
                {
                    self.control_counters.duplicate_writes_suppressed = self
                        .control_counters
                        .duplicate_writes_suppressed
                        .saturating_add(1);
                    self.sync_control_counters();
                    self.control_outcomes.push(PipeWireControlOutcome {
                        control: request.control,
                        state: PipeWireControlState::Idle,
                    });
                    return Ok(());
                }
                let pending = self.writes.entry(node_id).or_default().mute.as_mut();
                if let Some(pending) = pending {
                    self.control_counters.writes_coalesced =
                        self.control_counters.writes_coalesced.saturating_add(1);
                    pending.queued = (pending.sent != desired).then_some(desired);
                    pending
                        .controls
                        .insert(request.control.clone(), request.target.clone());
                } else {
                    if let Err(error) = self.send_mute(node_id, desired) {
                        self.control_counters.writes_failed =
                            self.control_counters.writes_failed.saturating_add(1);
                        self.sync_control_counters();
                        return Err(error);
                    }
                    self.writes.entry(node_id).or_default().mute = Some(PendingWrite {
                        sent: desired,
                        queued: None,
                        started: Instant::now(),
                        layout_generation: None,
                        controls: BTreeMap::from([(
                            request.control.clone(),
                            request.target.clone(),
                        )]),
                    });
                }
            }
            PipeWireAudioOperation::SetVolume | PipeWireAudioOperation::SetChannelVolume => {
                if matches!(request.operation, PipeWireAudioOperation::SetVolume)
                    == matches!(request.target, PipeWireAudioTarget::ChannelItem { .. })
                {
                    return Err("PipeWire volume operation and target do not match".into());
                }
                if !node.audio.can_set_volume {
                    return Err("PipeWire audio target does not permit volume control".into());
                }
                let desired_value = request
                    .volume
                    .and_then(|volume| model::FiniteVolume::new(volume.get() as f32))
                    .filter(|volume| volume.get() <= model::MAX_PERCEPTUAL_VOLUME)
                    .ok_or_else(|| "PipeWire volume request is outside its bound".to_owned())?;
                if node.audio.channels.is_empty() {
                    return Err("PipeWire channel volume state is unavailable".into());
                }
                match request.operation {
                    PipeWireAudioOperation::SetVolume => {
                        self.control_counters.average_intents =
                            self.control_counters.average_intents.saturating_add(1);
                    }
                    PipeWireAudioOperation::SetChannelVolume => {
                        self.control_counters.channel_intents =
                            self.control_counters.channel_intents.saturating_add(1);
                    }
                    _ => unreachable!(),
                }
                let base = self
                    .writes
                    .get(&node_id)
                    .and_then(|coordinator| coordinator.volume.as_ref())
                    .map(|pending| pending.queued.as_ref().unwrap_or(&pending.sent).clone())
                    .unwrap_or_else(|| node.audio.channels.clone());
                let desired = desired_volume_vector(
                    request.operation,
                    &request.target,
                    &node,
                    base,
                    desired_value,
                )
                .inspect_err(|_| {
                    self.control_counters.stale_vectors_rejected = self
                        .control_counters
                        .stale_vectors_rejected
                        .saturating_add(1);
                    if matches!(request.operation, PipeWireAudioOperation::SetChannelVolume) {
                        self.control_counters.layout_invalidated_intents = self
                            .control_counters
                            .layout_invalidated_intents
                            .saturating_add(1);
                    }
                })?;
                if self
                    .writes
                    .get(&node_id)
                    .and_then(|coordinator| coordinator.volume.as_ref())
                    .is_none()
                    && volume_vectors_match(&node.audio.channels, &desired)
                {
                    self.control_counters.duplicate_writes_suppressed = self
                        .control_counters
                        .duplicate_writes_suppressed
                        .saturating_add(1);
                    self.control_counters.duplicate_vectors_suppressed = self
                        .control_counters
                        .duplicate_vectors_suppressed
                        .saturating_add(1);
                    self.sync_control_counters();
                    self.control_outcomes.push(PipeWireControlOutcome {
                        control: request.control,
                        state: PipeWireControlState::Idle,
                    });
                    return Ok(());
                }
                let pending = self.writes.entry(node_id).or_default().volume.as_mut();
                if let Some(pending) = pending {
                    self.control_counters.writes_coalesced =
                        self.control_counters.writes_coalesced.saturating_add(1);
                    self.control_counters.vectors_coalesced =
                        self.control_counters.vectors_coalesced.saturating_add(1);
                    pending.queued =
                        (!volume_vectors_match(&pending.sent, &desired)).then_some(desired);
                    pending
                        .controls
                        .insert(request.control.clone(), request.target.clone());
                } else {
                    if let Err(error) = self.send_volume(&node, &desired) {
                        self.control_counters.writes_failed =
                            self.control_counters.writes_failed.saturating_add(1);
                        self.control_counters.vectors_failed =
                            self.control_counters.vectors_failed.saturating_add(1);
                        self.sync_control_counters();
                        return Err(error);
                    }
                    self.writes.entry(node_id).or_default().volume = Some(PendingWrite {
                        sent: desired,
                        queued: None,
                        started: Instant::now(),
                        layout_generation: Some(node.audio.channel_layout_generation),
                        controls: BTreeMap::from([(
                            request.control.clone(),
                            request.target.clone(),
                        )]),
                    });
                }
            }
        }
        self.control_outcomes.push(PipeWireControlOutcome {
            control: request.control,
            state: PipeWireControlState::Pending,
        });
        self.sync_control_counters();
        Ok(())
    }

    pub(crate) fn take_control_outcomes(&mut self) -> Vec<PipeWireControlOutcome> {
        std::mem::take(&mut self.control_outcomes)
    }

    pub(crate) fn shutdown(&mut self) {
        self.lifecycle = PipeWireLifecycle::Stopping;
        self.retry_deadline = None;
        self.expected_sync = None;
        self.transport.take();
        self.finish_all_controls(PipeWireControlState::Unavailable);
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
        let transport = match PipeWireTransport::connect(self.next_generation, self.demand.clone())
        {
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
        self.finish_all_controls(PipeWireControlState::Unavailable);
        self.transport.take();
        self.expected_sync = None;
        self.synchronization = SynchronizationStage::First;
        self.lifecycle = PipeWireLifecycle::Disconnected;
        let _ = self.reconciler.mark_unavailable();
        if self.demand.is_empty() {
            self.retry_deadline = None;
            self.lifecycle = PipeWireLifecycle::Dormant;
            self.last_publication = now;
            return;
        }
        let delay = RECONNECT_DELAYS[self.backoff_index.min(RECONNECT_DELAYS.len() - 1)];
        self.backoff_index = (self.backoff_index + 1).min(RECONNECT_DELAYS.len() - 1);
        self.retry_deadline = Some(now + delay);
        self.lifecycle = PipeWireLifecycle::Reconnecting;
        self.last_publication = now;
    }

    fn resolve_control_target(
        &self,
        target: &PipeWireAudioTarget,
    ) -> Result<model::PipeWireNodeId, String> {
        let snapshot = self.snapshot();
        match target {
            PipeWireAudioTarget::DefaultSink => snapshot
                .defaults
                .actual_sink
                .node
                .ok_or_else(|| "default PipeWire sink is unresolved".into()),
            PipeWireAudioTarget::DefaultSource => snapshot
                .defaults
                .actual_source
                .node
                .ok_or_else(|| "default PipeWire source is unresolved".into()),
            PipeWireAudioTarget::NodeItem {
                source_generation,
                item_key,
            } => {
                if *source_generation != snapshot.connection_generation {
                    return Err("PipeWire node target belongs to a stale generation".into());
                }
                let id = parse_node_item_key(item_key)?;
                snapshot
                    .nodes
                    .iter()
                    .any(|node| node.id == id)
                    .then_some(id)
                    .ok_or_else(|| "PipeWire node target is unavailable".into())
            }
            PipeWireAudioTarget::ChannelItem {
                source_generation,
                node_item_key,
                ..
            } => {
                if *source_generation != snapshot.connection_generation {
                    return Err("PipeWire node target belongs to a stale generation".into());
                }
                let id = parse_node_item_key(node_item_key)?;
                let node = snapshot
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| "PipeWire node target is unavailable".to_owned())?;
                channel_target_index(target, node)?;
                Ok(id)
            }
        }
    }

    fn send_mute(&mut self, node: model::PipeWireNodeId, desired: bool) -> Result<(), String> {
        if node.connection_generation != self.snapshot().connection_generation {
            return Err("PipeWire mute target is stale".into());
        }
        self.transport
            .as_ref()
            .ok_or_else(|| "PipeWire transport is unavailable".to_owned())?
            .set_node_mute(node.global_id, desired)?;
        self.control_counters.mute_writes_sent =
            self.control_counters.mute_writes_sent.saturating_add(1);
        self.sync_control_counters();
        Ok(())
    }

    fn send_volume(
        &mut self,
        node: &model::PipeWireNodeSnapshot,
        desired: &[model::FiniteVolume],
    ) -> Result<(), String> {
        if desired.len() != node.audio.channels.len() {
            self.control_counters.stale_vectors_rejected = self
                .control_counters
                .stale_vectors_rejected
                .saturating_add(1);
            self.control_counters.layout_invalidated_intents = self
                .control_counters
                .layout_invalidated_intents
                .saturating_add(1);
            return Err("PipeWire volume vector belongs to a stale channel layout".into());
        }
        let channels = model::perceptual_channels_to_linear(desired)
            .ok_or_else(|| "PipeWire channel volumes cannot be encoded".to_owned())?;
        self.transport
            .as_ref()
            .ok_or_else(|| "PipeWire transport is unavailable".to_owned())?
            .set_node_channel_volumes(node.raw_global_id, channels)?;
        self.control_counters.volume_writes_sent =
            self.control_counters.volume_writes_sent.saturating_add(1);
        self.control_counters.vectors_sent = self.control_counters.vectors_sent.saturating_add(1);
        self.sync_control_counters();
        Ok(())
    }

    fn reconcile_pending_writes(&mut self, now: Instant) {
        let ids = self.writes.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let Some(mut coordinator) = self.writes.remove(&id) else {
                continue;
            };
            let node = self
                .snapshot()
                .nodes
                .iter()
                .find(|node| node.id == id)
                .cloned();
            if let Some(mut write) = coordinator.mute.take() {
                self.reject_retargeted_controls(id, &mut write.controls);
                if !write.controls.is_empty() {
                    let state = node.as_ref().and_then(|node| node.audio.muted);
                    if state.is_none() {
                        self.finish_controls(write.controls, PipeWireControlState::Unavailable);
                    } else if state == Some(write.sent) {
                        if let Some(queued) =
                            write.queued.take().filter(|queued| Some(*queued) != state)
                        {
                            if self.send_mute(id, queued).is_ok() {
                                write.sent = queued;
                                write.started = now;
                                coordinator.mute = Some(write);
                            } else {
                                self.control_counters.writes_failed =
                                    self.control_counters.writes_failed.saturating_add(1);
                                self.finish_controls(write.controls, PipeWireControlState::Failed);
                            }
                        } else {
                            self.control_counters.writes_confirmed =
                                self.control_counters.writes_confirmed.saturating_add(1);
                            self.finish_controls(write.controls, PipeWireControlState::Idle);
                        }
                    } else if now.duration_since(write.started) >= CONTROL_TIMEOUT {
                        self.control_counters.writes_timed_out =
                            self.control_counters.writes_timed_out.saturating_add(1);
                        self.finish_controls(write.controls, PipeWireControlState::Failed);
                    } else {
                        coordinator.mute = Some(write);
                    }
                }
            }
            if let Some(mut write) = coordinator.volume.take() {
                self.reject_retargeted_controls(id, &mut write.controls);
                if !write.controls.is_empty() {
                    let layout_matches = node.as_ref().is_some_and(|node| {
                        write.layout_generation == Some(node.audio.channel_layout_generation)
                    });
                    let state = layout_matches
                        .then(|| node.as_ref().map(|node| node.audio.channels.as_slice()))
                        .flatten()
                        .filter(|channels| !channels.is_empty());
                    if !layout_matches {
                        self.control_counters.stale_vectors_rejected = self
                            .control_counters
                            .stale_vectors_rejected
                            .saturating_add(1);
                        self.control_counters.layout_invalidated_intents = self
                            .control_counters
                            .layout_invalidated_intents
                            .saturating_add(1);
                        self.finish_controls(write.controls, PipeWireControlState::Unavailable);
                    } else if state.is_none() {
                        self.finish_controls(write.controls, PipeWireControlState::Unavailable);
                    } else if state.is_some_and(|state| volume_vectors_match(state, &write.sent)) {
                        if let Some(queued) = write.queued.take().filter(|queued| {
                            !state.is_some_and(|state| volume_vectors_match(state, queued))
                        }) {
                            if now.duration_since(write.started) < VOLUME_WRITE_INTERVAL {
                                write.queued = Some(queued);
                                coordinator.volume = Some(write);
                            } else if node
                                .as_ref()
                                .is_some_and(|node| self.send_volume(node, &queued).is_ok())
                            {
                                write.sent = queued;
                                write.started = now;
                                coordinator.volume = Some(write);
                            } else {
                                self.control_counters.writes_failed =
                                    self.control_counters.writes_failed.saturating_add(1);
                                self.control_counters.vectors_failed =
                                    self.control_counters.vectors_failed.saturating_add(1);
                                self.finish_controls(write.controls, PipeWireControlState::Failed);
                            }
                        } else {
                            self.control_counters.writes_confirmed =
                                self.control_counters.writes_confirmed.saturating_add(1);
                            self.control_counters.vectors_confirmed =
                                self.control_counters.vectors_confirmed.saturating_add(1);
                            self.finish_controls(write.controls, PipeWireControlState::Idle);
                        }
                    } else if now.duration_since(write.started) >= CONTROL_TIMEOUT {
                        self.control_counters.writes_timed_out =
                            self.control_counters.writes_timed_out.saturating_add(1);
                        self.control_counters.vectors_timed_out =
                            self.control_counters.vectors_timed_out.saturating_add(1);
                        self.finish_controls(write.controls, PipeWireControlState::Failed);
                    } else {
                        coordinator.volume = Some(write);
                    }
                }
            }
            if coordinator.mute.is_some() || coordinator.volume.is_some() {
                self.writes.insert(id, coordinator);
            }
        }
        self.sync_control_counters();
    }

    fn reject_retargeted_controls(
        &mut self,
        node: model::PipeWireNodeId,
        controls: &mut BTreeMap<PipeWireControlIdentity, PipeWireAudioTarget>,
    ) {
        let stale = controls
            .iter()
            .filter_map(|(control, target)| {
                (self.resolve_control_target(target) != Ok(node)).then_some(control.clone())
            })
            .collect::<Vec<_>>();
        for control in stale {
            let channel_target = matches!(
                controls.get(&control),
                Some(PipeWireAudioTarget::ChannelItem { .. })
            );
            controls.remove(&control);
            self.control_outcomes.push(PipeWireControlOutcome {
                control,
                state: PipeWireControlState::Unavailable,
            });
            self.control_counters.stale_writes_rejected = self
                .control_counters
                .stale_writes_rejected
                .saturating_add(1);
            if channel_target {
                self.control_counters.stale_vectors_rejected = self
                    .control_counters
                    .stale_vectors_rejected
                    .saturating_add(1);
                self.control_counters.layout_invalidated_intents = self
                    .control_counters
                    .layout_invalidated_intents
                    .saturating_add(1);
            }
        }
    }

    fn finish_controls(
        &mut self,
        controls: BTreeMap<PipeWireControlIdentity, PipeWireAudioTarget>,
        state: PipeWireControlState,
    ) {
        self.control_outcomes.extend(
            controls
                .into_keys()
                .map(|control| PipeWireControlOutcome { control, state }),
        );
    }

    fn finish_all_controls(&mut self, state: PipeWireControlState) {
        let writes = std::mem::take(&mut self.writes);
        for coordinator in writes.into_values() {
            if let Some(write) = coordinator.mute {
                self.finish_controls(write.controls, state);
            }
            if let Some(write) = coordinator.volume {
                self.finish_controls(write.controls, state);
            }
        }
    }

    fn sync_control_counters(&mut self) {
        self.reconciler
            .update_control_counters(&self.control_counters);
    }
}

fn volumes_match(left: model::FiniteVolume, right: model::FiniteVolume) -> bool {
    (left.get() - right.get()).abs() <= 0.0005
}

fn volume_vectors_match(left: &[model::FiniteVolume], right: &[model::FiniteVolume]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| volumes_match(*left, *right))
}

fn parse_node_item_key(value: &str) -> Result<model::PipeWireNodeId, String> {
    let (generation, raw_id) = value
        .split_once(':')
        .ok_or_else(|| "PipeWire node item key is malformed".to_owned())?;
    Ok(model::PipeWireNodeId {
        connection_generation: generation
            .parse::<u64>()
            .map_err(|_| "PipeWire node item generation is malformed".to_owned())?,
        global_id: raw_id
            .parse::<u32>()
            .map_err(|_| "PipeWire node item ID is malformed".to_owned())?,
    })
}

fn channel_target_index(
    target: &PipeWireAudioTarget,
    node: &model::PipeWireNodeSnapshot,
) -> Result<usize, String> {
    let PipeWireAudioTarget::ChannelItem {
        layout_generation,
        channel_item_key,
        ..
    } = target
    else {
        return Err("channel volume operation requires a contextual channel target".into());
    };
    if *layout_generation != node.audio.channel_layout_generation {
        return Err("PipeWire channel target belongs to a stale layout".into());
    }
    let mut parts = channel_item_key.split(':');
    let generation = parts
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "PipeWire channel item generation is malformed".to_owned())?;
    let index = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "PipeWire channel item index is malformed".to_owned())?;
    let raw = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| "PipeWire channel item position is malformed".to_owned())?;
    if parts.next().is_some()
        || generation != *layout_generation
        || node
            .audio
            .channel_positions
            .get(index)
            .is_none_or(|position| position.raw != raw)
    {
        return Err("PipeWire channel target is stale".into());
    }
    Ok(index)
}

fn desired_volume_vector(
    operation: PipeWireAudioOperation,
    target: &PipeWireAudioTarget,
    node: &model::PipeWireNodeSnapshot,
    base: Vec<model::FiniteVolume>,
    desired: model::FiniteVolume,
) -> Result<Vec<model::FiniteVolume>, String> {
    if base.len() != node.audio.channels.len() || base.is_empty() {
        return Err("PipeWire coordinated volume vector is stale".into());
    }
    match operation {
        PipeWireAudioOperation::SetVolume => {
            model::scaled_perceptual_channels(&base, desired.get())
                .ok_or_else(|| "PipeWire channel volumes cannot be scaled".to_owned())
        }
        PipeWireAudioOperation::SetChannelVolume => {
            let index = channel_target_index(target, node)?;
            let mut result = base;
            *result.get_mut(index).ok_or_else(|| {
                "PipeWire channel target is outside the current vector".to_owned()
            })? = desired;
            Ok(result)
        }
        _ => Err("mute operation cannot construct a volume vector".into()),
    }
}

pub(crate) fn run_pipewire_graph_diagnostic() -> Result<PipeWireSnapshot, String> {
    let started = Instant::now();
    let mut source = PipeWireSource::default();
    source.set_demand(PipeWireDemand {
        documents: 1,
        service: true,
        nodes: true,
        node_details: true,
        defaults: true,
        links: true,
        audio_state: false,
        audio_writes: false,
        channel_projection: false,
        channel_writes: false,
        property_keys: Default::default(),
    });
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
    use htm_runtime::{ExperimentalDocumentIdentity, PipeWireControlLocator};
    use model::{PipeWireNodeId, RawNodeAudioInfo};

    fn modeled_demand() -> PipeWireDemand {
        PipeWireDemand {
            documents: 1,
            service: true,
            ..PipeWireDemand::default()
        }
    }

    fn control(name: &str) -> PipeWireControlIdentity {
        PipeWireControlIdentity {
            document_generation: ExperimentalDocumentIdentity { serial: 9 },
            locator: PipeWireControlLocator::Element(name.to_owned()),
        }
    }

    fn item_target() -> PipeWireAudioTarget {
        PipeWireAudioTarget::NodeItem {
            source_generation: 1,
            item_key: "1:42".to_owned(),
        }
    }

    fn pending_control(
        control: PipeWireControlIdentity,
    ) -> BTreeMap<PipeWireControlIdentity, PipeWireAudioTarget> {
        BTreeMap::from([(control, item_target())])
    }

    fn source_with_audio_node() -> PipeWireSource {
        let mut source = PipeWireSource {
            lifecycle: PipeWireLifecycle::Ready,
            demand: PipeWireDemand {
                documents: 1,
                service: true,
                nodes: true,
                audio_state: true,
                audio_writes: true,
                ..PipeWireDemand::default()
            },
            ..PipeWireSource::default()
        };
        source.reconciler.begin_generation(1).unwrap();
        source
            .reconciler
            .apply_unpublished([
                PipeWireDelta::NodeAdded {
                    raw_id: 42,
                    properties: BTreeMap::from([
                        ("node.name".into(), "test-sink".into()),
                        ("media.class".into(), "Audio/Sink".into()),
                    ]),
                },
                PipeWireDelta::NodePermissions {
                    raw_id: 42,
                    writable: true,
                },
                PipeWireDelta::NodeTracking {
                    raw_id: 42,
                    tracked: true,
                },
                PipeWireDelta::NodeAudioTracking {
                    raw_id: 42,
                    tracked: true,
                },
                PipeWireDelta::NodeAudioInfo(RawNodeAudioInfo {
                    raw_id: 42,
                    channel_volumes: Some(vec![1.0, 0.125]),
                    channel_positions: Some(vec![3, 4]),
                    muted: Some(false),
                }),
            ])
            .unwrap();
        source.reconciler.mark_ready().unwrap();
        source
    }

    #[test]
    fn channel_and_average_intents_share_one_complete_vector() {
        let source = source_with_audio_node();
        let node = &source.snapshot().nodes[0];
        let channel = PipeWireAudioTarget::ChannelItem {
            source_generation: 1,
            node_item_key: "1:42".into(),
            layout_generation: 1,
            channel_item_key: "1:1:4".into(),
        };
        let right = desired_volume_vector(
            PipeWireAudioOperation::SetChannelVolume,
            &channel,
            node,
            node.audio.channels.clone(),
            model::FiniteVolume::new(0.8).unwrap(),
        )
        .unwrap();
        assert_eq!(right[0], node.audio.channels[0]);
        assert_eq!(right[1], model::FiniteVolume::new(0.8).unwrap());
        let scaled = desired_volume_vector(
            PipeWireAudioOperation::SetVolume,
            &item_target(),
            node,
            right,
            model::FiniteVolume::new(0.9).unwrap(),
        )
        .unwrap();
        assert_eq!(scaled.len(), 2);
        assert!((model::perceptual_average(&scaled).unwrap().get() - 0.9).abs() < 0.000_01);
        let linear = model::perceptual_channels_to_linear(&scaled).unwrap();
        assert_eq!(linear.len(), 2);
    }

    #[test]
    fn stale_channel_layout_cannot_construct_a_write() {
        let source = source_with_audio_node();
        let node = &source.snapshot().nodes[0];
        let stale = PipeWireAudioTarget::ChannelItem {
            source_generation: 1,
            node_item_key: "1:42".into(),
            layout_generation: 2,
            channel_item_key: "2:0:3".into(),
        };
        assert!(
            desired_volume_vector(
                PipeWireAudioOperation::SetChannelVolume,
                &stale,
                node,
                node.audio.channels.clone(),
                model::FiniteVolume::new(0.5).unwrap(),
            )
            .is_err()
        );
    }

    #[test]
    fn layout_replacement_invalidates_the_complete_coordinated_vector() {
        let mut source = source_with_audio_node();
        let node = PipeWireNodeId {
            connection_generation: 1,
            global_id: 42,
        };
        let average_control = control("average");
        let channel_control = control("channel");
        let channel_target = PipeWireAudioTarget::ChannelItem {
            source_generation: 1,
            node_item_key: "1:42".into(),
            layout_generation: 1,
            channel_item_key: "1:0:3".into(),
        };
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: None,
                volume: Some(PendingWrite {
                    sent: vec![model::FiniteVolume::new(0.5).unwrap(); 2],
                    queued: Some(vec![model::FiniteVolume::new(0.75).unwrap(); 2]),
                    started: Instant::now(),
                    layout_generation: Some(1),
                    controls: BTreeMap::from([
                        (average_control.clone(), item_target()),
                        (channel_control.clone(), channel_target),
                    ]),
                }),
            },
        );
        source
            .reconciler
            .apply([PipeWireDelta::NodeAudioInfo(RawNodeAudioInfo {
                raw_id: 42,
                channel_volumes: None,
                channel_positions: Some(vec![4, 3]),
                muted: None,
            })])
            .unwrap();
        source.reconcile_pending_writes(Instant::now());
        let outcomes = source.take_control_outcomes();
        assert!(outcomes.contains(&PipeWireControlOutcome {
            control: average_control,
            state: PipeWireControlState::Unavailable,
        }));
        assert!(outcomes.contains(&PipeWireControlOutcome {
            control: channel_control,
            state: PipeWireControlState::Unavailable,
        }));
        assert!(source.writes.is_empty());
        assert_eq!(source.control_counters.layout_invalidated_intents, 2);
        assert_eq!(source.control_counters.stale_vectors_rejected, 2);
    }

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
        let mut source = PipeWireSource {
            demand: modeled_demand(),
            ..PipeWireSource::default()
        };
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
        let mut source = PipeWireSource {
            demand: modeled_demand(),
            ..PipeWireSource::default()
        };
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
    fn zero_demand_releases_transport_and_reconnect_deadline() {
        let mut source = PipeWireSource {
            lifecycle: PipeWireLifecycle::Reconnecting,
            retry_deadline: Some(Instant::now()),
            demand: modeled_demand(),
            ..PipeWireSource::default()
        };
        source.set_demand(PipeWireDemand::default());
        assert_eq!(source.lifecycle(), PipeWireLifecycle::Dormant);
        assert!(source.raw_poll_fd().is_none());
        assert!(source.retry_deadline.is_none());
        assert_eq!(
            source.snapshot().availability,
            PipeWireAvailability::Unavailable
        );
    }

    #[test]
    fn model_errors_are_displayable() {
        let error = super::model::PipeWireModelError::InvalidData("bad value".into());
        assert!(error.to_string().contains("bad value"));
    }

    #[test]
    fn authoritative_audio_updates_confirm_exact_controls() {
        let mut source = source_with_audio_node();
        let node = PipeWireNodeId {
            connection_generation: 1,
            global_id: 42,
        };
        let mute = control("mute");
        let volume = control("volume");
        let started = Instant::now();
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: Some(PendingWrite {
                    sent: true,
                    queued: None,
                    started,
                    layout_generation: None,
                    controls: pending_control(mute.clone()),
                }),
                volume: Some(PendingWrite {
                    sent: vec![model::FiniteVolume::new(0.5).unwrap(); 2],
                    queued: None,
                    started,
                    layout_generation: Some(1),
                    controls: pending_control(volume.clone()),
                }),
            },
        );
        source
            .reconciler
            .apply([PipeWireDelta::NodeAudioInfo(RawNodeAudioInfo {
                raw_id: 42,
                channel_volumes: Some(vec![0.125, 0.125]),
                channel_positions: Some(vec![3, 4]),
                muted: Some(true),
            })])
            .unwrap();
        source.reconcile_pending_writes(started + Duration::from_millis(5));
        let outcomes = source.take_control_outcomes();
        assert!(outcomes.contains(&PipeWireControlOutcome {
            control: mute,
            state: PipeWireControlState::Idle,
        }));
        assert!(outcomes.contains(&PipeWireControlOutcome {
            control: volume,
            state: PipeWireControlState::Idle,
        }));
        assert!(source.writes.is_empty());
        assert_eq!(source.control_counters.writes_confirmed, 2);
        assert_eq!(source.control_counters.vectors_confirmed, 1);
    }

    #[test]
    fn control_timeout_removal_and_generation_loss_are_contained() {
        let mut source = source_with_audio_node();
        let node = PipeWireNodeId {
            connection_generation: 1,
            global_id: 42,
        };
        let timed_out = control("timed-out");
        let started = Instant::now();
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: Some(PendingWrite {
                    sent: true,
                    queued: None,
                    started,
                    layout_generation: None,
                    controls: pending_control(timed_out.clone()),
                }),
                volume: None,
            },
        );
        source.reconcile_pending_writes(started + CONTROL_TIMEOUT);
        assert_eq!(
            source.take_control_outcomes(),
            vec![PipeWireControlOutcome {
                control: timed_out,
                state: PipeWireControlState::Failed,
            }]
        );
        assert_eq!(source.control_counters.writes_timed_out, 1);
        assert_eq!(source.control_counters.vectors_timed_out, 0);

        let removed = control("removed");
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: Some(PendingWrite {
                    sent: true,
                    queued: None,
                    started,
                    layout_generation: None,
                    controls: pending_control(removed.clone()),
                }),
                volume: None,
            },
        );
        source
            .reconciler
            .apply([PipeWireDelta::NodeRemoved(42)])
            .unwrap();
        source.reconcile_pending_writes(started);
        assert_eq!(
            source.take_control_outcomes(),
            vec![PipeWireControlOutcome {
                control: removed,
                state: PipeWireControlState::Unavailable,
            }]
        );

        let stale = control("stale-generation");
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: None,
                volume: Some(PendingWrite {
                    sent: vec![model::FiniteVolume::new(0.5).unwrap(); 2],
                    queued: None,
                    started,
                    layout_generation: Some(1),
                    controls: pending_control(stale.clone()),
                }),
            },
        );
        source.disconnect(started, "modeled reconnect".into());
        assert_eq!(
            source.take_control_outcomes(),
            vec![PipeWireControlOutcome {
                control: stale,
                state: PipeWireControlState::Unavailable,
            }]
        );
    }

    #[test]
    fn default_target_replacement_rejects_the_old_control_identity() {
        let mut source = source_with_audio_node();
        let node = PipeWireNodeId {
            connection_generation: 1,
            global_id: 42,
        };
        source
            .reconciler
            .apply([
                PipeWireDelta::MetadataAdded { raw_id: 80 },
                PipeWireDelta::MetadataProperty {
                    raw_id: 80,
                    subject: 0,
                    key: Some("default.audio.sink".into()),
                    type_name: Some("Spa:String:JSON".into()),
                    value: Some(r#"{"name":"test-sink"}"#.into()),
                },
            ])
            .unwrap();
        assert_eq!(source.snapshot().defaults.actual_sink.node, Some(node));

        let pending = control("default-sink");
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: Some(PendingWrite {
                    sent: true,
                    queued: None,
                    started: Instant::now(),
                    layout_generation: None,
                    controls: BTreeMap::from([(pending.clone(), PipeWireAudioTarget::DefaultSink)]),
                }),
                volume: None,
            },
        );
        source
            .reconciler
            .apply([PipeWireDelta::MetadataRemoved(80)])
            .unwrap();
        source.reconcile_pending_writes(Instant::now());
        assert_eq!(
            source.take_control_outcomes(),
            vec![PipeWireControlOutcome {
                control: pending,
                state: PipeWireControlState::Unavailable,
            }]
        );
        assert!(source.writes.is_empty());
    }

    #[test]
    fn queued_volume_intent_observes_the_write_cadence() {
        let mut source = source_with_audio_node();
        let node = PipeWireNodeId {
            connection_generation: 1,
            global_id: 42,
        };
        let started = Instant::now();
        source.writes.insert(
            node,
            NodeWriteCoordinator {
                mute: None,
                volume: Some(PendingWrite {
                    sent: vec![model::FiniteVolume::new(0.75).unwrap(); 2],
                    queued: Some(vec![model::FiniteVolume::new(0.5).unwrap(); 2]),
                    started,
                    layout_generation: Some(1),
                    controls: pending_control(control("volume")),
                }),
            },
        );
        source.reconcile_pending_writes(started + Duration::from_millis(5));
        assert!(source.writes[&node].volume.is_some());
        assert!(source.take_control_outcomes().is_empty());
        assert_eq!(
            source.retry_timeout(started + Duration::from_millis(5)),
            Some(Duration::from_millis(11))
        );
    }
}
