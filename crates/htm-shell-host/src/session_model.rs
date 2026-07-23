use crate::{FrameScheduler, OutputKey, ScheduleDecision, SurfaceScaleState};
use std::collections::BTreeMap;

const TEST_BUDGET: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelId {
    template: &'static str,
    output: OutputKey,
    generation: u64,
    owner: u64,
}

#[derive(Debug, Default)]
struct ModelSurface {
    id: Option<ModelId>,
    parse_count: u32,
    document_identity: u64,
    mapped: bool,
    hovered: bool,
    active: bool,
    scheduler: FrameScheduler,
    busy_buffers: usize,
    mapped_bytes: usize,
    scale: Option<SurfaceScaleState>,
}

#[derive(Debug, Default)]
struct ModelSharedState {
    overlay_open: bool,
    activation_count: u64,
    last_action: String,
}

#[derive(Debug, Default)]
struct ModelOutput {
    panel: ModelSurface,
    overlay: ModelSurface,
    shared: ModelSharedState,
}

#[derive(Debug, Default)]
struct SessionModel {
    outputs: BTreeMap<OutputKey, ModelOutput>,
    next_owner: u64,
    next_generation: u64,
    next_document: u64,
    mapped_bytes: usize,
    stale_callbacks: u64,
    stale_releases: u64,
}

impl SessionModel {
    fn add_output(&mut self, key: OutputKey) {
        assert!(!self.outputs.contains_key(&key));
        let panel = self.new_surface("panel", key, true);
        let overlay = self.new_surface("overlay", key, false);
        self.outputs.insert(
            key,
            ModelOutput {
                panel,
                overlay,
                shared: ModelSharedState {
                    last_action: "Ready".into(),
                    ..ModelSharedState::default()
                },
            },
        );
    }

    fn new_surface(
        &mut self,
        template: &'static str,
        output: OutputKey,
        mapped: bool,
    ) -> ModelSurface {
        self.next_owner += 1;
        self.next_generation += 1;
        self.next_document += 1;
        let mut scale = SurfaceScaleState::new(self.next_generation, true);
        scale.set_logical_size(101, 51);
        ModelSurface {
            id: Some(ModelId {
                template,
                output,
                generation: self.next_generation,
                owner: self.next_owner,
            }),
            parse_count: 1,
            document_identity: self.next_document,
            mapped,
            scale: Some(scale),
            ..ModelSurface::default()
        }
    }

    fn remove_output(&mut self, key: OutputKey) -> Option<ModelOutput> {
        let output = self.outputs.remove(&key)?;
        self.mapped_bytes = self
            .mapped_bytes
            .saturating_sub(output.panel.mapped_bytes)
            .saturating_sub(output.overlay.mapped_bytes);
        Some(output)
    }

    fn toggle_from_panel(&mut self, key: OutputKey) {
        let output = self.outputs.get_mut(&key).unwrap();
        output.shared.overlay_open = !output.shared.overlay_open;
        output.shared.last_action = if output.shared.overlay_open {
            "Opened from panel"
        } else {
            "Closed from panel"
        }
        .into();
        output.panel.scheduler.mark_dirty();
        output.overlay.mapped = output.shared.overlay_open;
        if output.shared.overlay_open {
            output.overlay.scheduler.mark_dirty();
        } else {
            output.overlay.scheduler.stop_scheduling();
            output.overlay.hovered = false;
            output.overlay.active = false;
        }
    }

    fn activate_overlay(&mut self, key: OutputKey) {
        let output = self.outputs.get_mut(&key).unwrap();
        output.shared.activation_count += 1;
        output.shared.last_action = "Overlay state updated".into();
        output.overlay.scheduler.mark_dirty();
    }

    fn pointer(&mut self, owner: u64, hovered: bool, active: bool) {
        if let Some(surface) = self.surface_mut(owner)
            && (surface.hovered != hovered || surface.active != active)
        {
            surface.hovered = hovered;
            surface.active = active;
            surface.scheduler.mark_dirty();
        }
    }

    fn surface_mut(&mut self, owner: u64) -> Option<&mut ModelSurface> {
        self.outputs.values_mut().find_map(|output| {
            if output.panel.id.as_ref().is_some_and(|id| id.owner == owner) {
                Some(&mut output.panel)
            } else if output
                .overlay
                .id
                .as_ref()
                .is_some_and(|id| id.owner == owner)
            {
                Some(&mut output.overlay)
            } else {
                None
            }
        })
    }

    fn reserve(&mut self, owner: u64, bytes: usize) -> bool {
        let existing = self
            .outputs
            .values()
            .find_map(|output| {
                for surface in [&output.panel, &output.overlay] {
                    if surface.id.as_ref().is_some_and(|id| id.owner == owner) {
                        return Some(surface.mapped_bytes);
                    }
                }
                None
            })
            .unwrap_or_default();
        let Some(proposed) = self
            .mapped_bytes
            .checked_sub(existing)
            .and_then(|total| total.checked_add(bytes))
        else {
            return false;
        };
        if proposed > TEST_BUDGET {
            return false;
        }
        let Some(surface) = self.surface_mut(owner) else {
            return false;
        };
        surface.mapped_bytes = bytes;
        self.mapped_bytes = proposed;
        true
    }

    fn callback(&mut self, owner: u64) {
        if let Some(surface) = self.surface_mut(owner) {
            surface.scheduler.frame_callback_done();
        } else {
            self.stale_callbacks += 1;
        }
    }

    fn release(&mut self, owner: u64) {
        if let Some(surface) = self.surface_mut(owner) {
            surface.busy_buffers = surface.busy_buffers.saturating_sub(1);
        } else {
            self.stale_releases += 1;
        }
    }

    fn preferred_scale(&mut self, owner: u64, numerator: u32) -> bool {
        let Some(surface) = self.surface_mut(owner) else {
            return false;
        };
        let generation = surface.id.as_ref().unwrap().generation;
        let changed = surface
            .scale
            .as_mut()
            .unwrap()
            .receive_preferred(generation, numerator)
            .unwrap();
        if changed && surface.mapped {
            surface.scheduler.mark_dirty();
        }
        changed
    }
}

fn key(global_name: u32, generation: u64) -> OutputKey {
    OutputKey {
        global_name,
        generation,
    }
}

#[test]
fn two_outputs_expand_to_four_independent_surface_generations() {
    let mut session = SessionModel::default();
    session.add_output(key(1, 1));
    session.add_output(key(2, 2));
    assert_eq!(session.outputs.len(), 2);
    let documents: Vec<_> = session
        .outputs
        .values()
        .flat_map(|output| [&output.panel, &output.overlay])
        .map(|surface| (surface.parse_count, surface.document_identity))
        .collect();
    assert!(documents.iter().all(|(parse_count, _)| *parse_count == 1));
    let mut identities: Vec<_> = documents.iter().map(|(_, identity)| *identity).collect();
    identities.sort_unstable();
    identities.dedup();
    assert_eq!(identities.len(), 4);
}

#[test]
fn panel_action_and_overlay_mutation_are_output_scoped() {
    let mut session = SessionModel::default();
    let a = key(1, 1);
    let b = key(2, 2);
    session.add_output(a);
    session.add_output(b);
    session.toggle_from_panel(a);
    session.activate_overlay(a);
    assert!(session.outputs[&a].shared.overlay_open);
    assert_eq!(session.outputs[&a].shared.activation_count, 1);
    assert!(!session.outputs[&b].shared.overlay_open);
    assert_eq!(session.outputs[&b].shared.activation_count, 0);
    assert!(!session.outputs[&b].panel.scheduler.dirty());
    assert!(!session.outputs[&b].overlay.scheduler.dirty());
}

#[test]
fn pointer_state_and_scheduling_do_not_cross_output_or_surface() {
    let mut session = SessionModel::default();
    let a = key(1, 1);
    let b = key(2, 2);
    session.add_output(a);
    session.add_output(b);
    let panel_a = session.outputs[&a].panel.id.as_ref().unwrap().owner;
    session.pointer(panel_a, true, false);
    assert!(session.outputs[&a].panel.hovered);
    assert!(session.outputs[&a].panel.scheduler.dirty());
    assert!(!session.outputs[&a].overlay.hovered);
    assert!(!session.outputs[&b].panel.hovered);
    assert!(!session.outputs[&b].panel.scheduler.dirty());
    session.pointer(panel_a, true, false);
    assert_eq!(
        session.outputs[&a].panel.scheduler.decision(true, true),
        ScheduleDecision::Render
    );
}

#[test]
fn busy_buffers_and_callbacks_are_independent_across_outputs() {
    let mut session = SessionModel::default();
    let a = key(1, 1);
    let b = key(2, 2);
    session.add_output(a);
    session.add_output(b);
    session.outputs.get_mut(&a).unwrap().panel.busy_buffers = 2;
    session
        .outputs
        .get_mut(&a)
        .unwrap()
        .panel
        .scheduler
        .mark_dirty();
    session
        .outputs
        .get_mut(&b)
        .unwrap()
        .panel
        .scheduler
        .mark_dirty();
    assert_eq!(
        session.outputs[&a].panel.scheduler.decision(true, false),
        ScheduleDecision::WaitForBuffer
    );
    assert_eq!(
        session.outputs[&b].panel.scheduler.decision(true, true),
        ScheduleDecision::Render
    );
}

#[test]
fn output_removal_preserves_other_output_and_readd_is_fresh() {
    let mut session = SessionModel::default();
    let old = key(1, 1);
    let other = key(2, 2);
    session.add_output(old);
    session.add_output(other);
    session.toggle_from_panel(old);
    let old_panel = session.outputs[&old].panel.id.clone().unwrap();
    let other_document = session.outputs[&other].panel.document_identity;
    session.remove_output(old).unwrap();
    assert_eq!(
        session.outputs[&other].panel.document_identity,
        other_document
    );
    let fresh = key(1, 3);
    session.add_output(fresh);
    let fresh_panel = session.outputs[&fresh].panel.id.clone().unwrap();
    assert_ne!(old_panel.generation, fresh_panel.generation);
    assert_ne!(old_panel.owner, fresh_panel.owner);
    assert!(!session.outputs[&fresh].shared.overlay_open);
}

#[test]
fn stale_callbacks_and_releases_cannot_alias_recreated_instances() {
    let mut session = SessionModel::default();
    let old = key(1, 1);
    session.add_output(old);
    let old_owner = session.outputs[&old].overlay.id.as_ref().unwrap().owner;
    session.remove_output(old).unwrap();
    session.add_output(key(1, 2));
    session.callback(old_owner);
    session.release(old_owner);
    assert_eq!(session.stale_callbacks, 1);
    assert_eq!(session.stale_releases, 1);
}

#[test]
fn aggregate_budget_is_enforced_and_reclaimed_after_removal() {
    let mut session = SessionModel::default();
    let a = key(1, 1);
    let b = key(2, 2);
    session.add_output(a);
    session.add_output(b);
    let panel_a = session.outputs[&a].panel.id.as_ref().unwrap().owner;
    let panel_b = session.outputs[&b].panel.id.as_ref().unwrap().owner;
    assert!(session.reserve(panel_a, 40 * 1024 * 1024));
    assert!(!session.reserve(panel_b, 40 * 1024 * 1024));
    session.remove_output(a).unwrap();
    assert!(session.reserve(panel_b, 40 * 1024 * 1024));
    assert_eq!(session.mapped_bytes, 40 * 1024 * 1024);
}

#[test]
fn removing_every_output_leaves_an_idle_recoverable_session() {
    let mut session = SessionModel::default();
    let first = key(1, 1);
    session.add_output(first);
    session.remove_output(first).unwrap();
    assert!(session.outputs.is_empty());
    assert_eq!(session.mapped_bytes, 0);
    session.add_output(key(9, 2));
    assert_eq!(session.outputs.len(), 1);
    assert!(
        session
            .outputs
            .values()
            .all(|output| { output.panel.parse_count == 1 && output.overlay.parse_count == 1 })
    );
}

#[test]
fn mixed_scale_changes_are_surface_and_output_local() {
    let mut session = SessionModel::default();
    let a = key(1, 1);
    let b = key(2, 2);
    session.add_output(a);
    session.add_output(b);
    let panel_a = session.outputs[&a].panel.id.as_ref().unwrap().owner;
    let panel_b = session.outputs[&b].panel.id.as_ref().unwrap().owner;
    let overlay_b = session.outputs[&b].overlay.id.as_ref().unwrap().owner;

    assert!(session.preferred_scale(panel_b, 180));
    assert!(session.outputs[&b].panel.scheduler.dirty());
    assert!(!session.outputs[&a].panel.scheduler.dirty());
    assert_eq!(
        session.outputs[&b]
            .panel
            .scale
            .unwrap()
            .render_request()
            .unwrap()
            .unwrap()
            .buffer_width,
        152
    );
    assert_eq!(
        session.outputs[&a]
            .panel
            .scale
            .unwrap()
            .preferred_numerator(),
        120
    );

    assert!(session.preferred_scale(overlay_b, 210));
    assert!(!session.outputs[&b].overlay.scheduler.dirty());
    assert!(!session.preferred_scale(panel_b, 180));
    assert!(!session.preferred_scale(panel_a + 10_000, 150));
}

#[test]
fn readded_output_does_not_inherit_scale_or_stale_generation() {
    let mut session = SessionModel::default();
    let old = key(5, 1);
    session.add_output(old);
    let old_owner = session.outputs[&old].panel.id.as_ref().unwrap().owner;
    assert!(session.preferred_scale(old_owner, 180));
    session.remove_output(old).unwrap();
    assert!(!session.preferred_scale(old_owner, 210));

    let fresh = key(5, 2);
    session.add_output(fresh);
    let new_scale = session.outputs[&fresh].panel.scale.unwrap();
    assert_eq!(new_scale.preferred_numerator(), 120);
    assert_ne!(new_scale.surface_generation(), 0);
}

#[derive(Debug, Default)]
struct ClockDocumentModel {
    generation: u64,
    bound: bool,
    mapped: bool,
    text: String,
    mutations: u64,
    frames: u64,
}

#[derive(Debug, Default)]
struct ClockOutputModel {
    panel: ClockDocumentModel,
    overlay: ClockDocumentModel,
}

#[derive(Debug, Default)]
struct ClockFanoutModel {
    outputs: BTreeMap<OutputKey, ClockOutputModel>,
    snapshot: Option<String>,
    next_generation: u64,
    samples: u64,
    timer_descriptors: usize,
    timer_armed: bool,
}

impl ClockFanoutModel {
    fn add_output(&mut self, key: OutputKey) {
        self.next_generation = self.next_generation.saturating_add(1);
        let mut panel = ClockDocumentModel {
            generation: self.next_generation,
            bound: true,
            mapped: true,
            ..ClockDocumentModel::default()
        };
        self.next_generation = self.next_generation.saturating_add(1);
        let overlay = ClockDocumentModel {
            generation: self.next_generation,
            ..ClockDocumentModel::default()
        };
        if self.timer_descriptors == 0 {
            self.timer_descriptors = 1;
        }
        self.timer_armed = true;
        if let Some(snapshot) = &self.snapshot {
            panel.text.clone_from(snapshot);
            panel.mutations = 1;
            panel.frames = 1;
        }
        self.outputs
            .insert(key, ClockOutputModel { panel, overlay });
    }

    fn remove_output(&mut self, key: OutputKey) {
        self.outputs.remove(&key);
        self.timer_armed = self
            .outputs
            .values()
            .any(|output| output.panel.bound || output.overlay.bound);
    }

    fn publish(&mut self, value: &str) {
        self.samples = self.samples.saturating_add(1);
        if self.snapshot.as_deref() == Some(value) {
            return;
        }
        self.snapshot = Some(value.into());
        for output in self.outputs.values_mut() {
            for document in [&mut output.panel, &mut output.overlay] {
                if !document.bound || document.text == value {
                    continue;
                }
                document.text = value.into();
                document.mutations = document.mutations.saturating_add(1);
                if document.mapped {
                    document.frames = document.frames.saturating_add(1);
                }
            }
        }
    }
}

#[test]
fn one_clock_snapshot_fans_out_only_to_bound_documents() {
    let mut clock = ClockFanoutModel::default();
    let a = key(1, 1);
    let b = key(2, 2);
    clock.add_output(a);
    clock.add_output(b);
    clock.publish("09:07");
    assert_eq!(clock.samples, 1);
    assert_eq!(clock.timer_descriptors, 1);
    assert_eq!(clock.outputs[&a].panel.text, "09:07");
    assert_eq!(clock.outputs[&b].panel.text, "09:07");
    assert_eq!(clock.outputs[&a].panel.frames, 1);
    assert_eq!(clock.outputs[&b].panel.frames, 1);
    assert_eq!(clock.outputs[&a].overlay.frames, 0);
    assert_eq!(clock.outputs[&b].overlay.frames, 0);

    clock.publish("09:07");
    assert_eq!(clock.samples, 2);
    assert_eq!(clock.outputs[&a].panel.frames, 1);
    assert_eq!(clock.outputs[&b].panel.frames, 1);
}

#[test]
fn clock_subscriptions_follow_output_generations_without_cross_output_state() {
    let mut clock = ClockFanoutModel::default();
    let old = key(1, 1);
    let other = key(2, 2);
    clock.add_output(old);
    clock.add_output(other);
    clock.publish("09:07");
    let old_generation = clock.outputs[&old].panel.generation;
    clock.remove_output(old);
    clock.publish("09:08");
    assert_eq!(clock.outputs[&other].panel.text, "09:08");
    assert_eq!(clock.outputs[&other].panel.frames, 2);

    let fresh = key(1, 3);
    clock.add_output(fresh);
    assert_eq!(clock.outputs[&fresh].panel.text, "09:08");
    assert_eq!(clock.outputs[&fresh].panel.frames, 1);
    assert_ne!(clock.outputs[&fresh].panel.generation, old_generation);
    assert_eq!(clock.outputs[&other].panel.frames, 2);

    clock.remove_output(other);
    clock.remove_output(fresh);
    assert!(!clock.timer_armed);
}

#[test]
fn closed_clock_bound_document_mutates_without_scheduling_a_frame() {
    let mut clock = ClockFanoutModel::default();
    let output = key(1, 1);
    clock.add_output(output);
    clock.outputs.get_mut(&output).unwrap().overlay.bound = true;
    clock.publish("17:42");
    assert_eq!(clock.outputs[&output].overlay.mutations, 1);
    assert_eq!(clock.outputs[&output].overlay.frames, 0);
}

#[test]
fn twenty_five_clock_output_replacements_never_alias_subscriptions() {
    let mut clock = ClockFanoutModel::default();
    let mut generations = Vec::new();
    for generation in 1..=25 {
        let output = key(7, generation);
        clock.add_output(output);
        clock.publish(&format!("{:02}:{:02}", generation / 60, generation % 60));
        generations.push(clock.outputs[&output].panel.generation);
        clock.remove_output(output);
        assert!(!clock.timer_armed);
    }
    let mut unique = generations.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), generations.len());
    assert_eq!(clock.timer_descriptors, 1);
    assert!(clock.outputs.is_empty());
}
