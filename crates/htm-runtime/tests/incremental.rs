use htm_runtime::{MutationPhase, run_incremental_experiment};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic-shell")
}

#[test]
fn long_running_document_mutations_and_reloads_are_repeatable() {
    let first = run_incremental_experiment(fixture()).expect("first incremental experiment");
    let second = run_incremental_experiment(fixture()).expect("second incremental experiment");
    let third = run_incremental_experiment(fixture()).expect("third incremental experiment");

    assert_eq!(first.document_parse_count, 1);
    assert!(first.document_identity_preserved);
    assert_eq!(first.artifacts.len(), 9);
    assert!(
        first
            .artifacts
            .iter()
            .all(|artifact| artifact.snapshot.document_parse_count == 1
                && artifact.snapshot.blitz_document_instance_retained)
    );

    let transition = |phase| {
        first
            .artifacts
            .iter()
            .find(|artifact| artifact.phase == phase)
            .and_then(|artifact| artifact.diff_from_previous.as_ref())
            .expect("phase diff")
    };
    let text = transition(MutationPhase::TextMutation);
    assert!(text.summary.text_changes > 0);
    assert_eq!(text.summary.geometry_changes, 0);
    assert_eq!(text.summary.created, 0);
    assert_eq!(text.summary.removed, 0);

    let class = transition(MutationPhase::ClassMutation);
    assert!(class.summary.style_or_paint_changes > 0);
    assert!(class.changed_nodes.iter().any(|change| {
        change.metadata.as_ref().is_some_and(|metadata| {
            metadata
                .new
                .classes
                .iter()
                .any(|class| class == "mutation-highlight")
        })
    }));

    let append = transition(MutationPhase::ListAppend);
    assert_eq!(append.summary.created, 6);
    assert_eq!(append.summary.removed, 0);
    let removal = transition(MutationPhase::ListRemoval);
    assert_eq!(removal.summary.removed, 2);
    assert!(removal.summary.retained_unchanged > 0);

    assert_eq!(first.slot_reuse.cycles, 8);
    assert!(!first.slot_reuse.reused_slots.is_empty());
    assert!(first.slot_reuse.maximum_generation >= 8);
    assert!(first.slot_reuse.stale_lookups_rejected > 0);
    assert!(first.slot_reuse.retained_sibling_identities_preserved);

    let reload = transition(MutationPhase::StylesheetReplacement);
    assert!(reload.summary.style_or_paint_changes > 0);
    assert_eq!(reload.summary.created, 0);
    assert_eq!(reload.summary.removed, 0);
    let accepted_attempts: Vec<_> = first
        .stylesheet_attempts
        .iter()
        .filter(|attempt| attempt.accepted)
        .collect();
    assert_eq!(accepted_attempts.len(), 2);
    assert!(
        accepted_attempts
            .iter()
            .all(|attempt| attempt.accepted_snapshot_preserved
                && attempt.document_identity_preserved)
    );
    for rejected in first
        .stylesheet_attempts
        .iter()
        .filter(|attempt| !attempt.accepted)
    {
        assert!(!rejected.accepted);
        assert!(rejected.diagnostic.is_some());
        assert!(rejected.accepted_snapshot_preserved);
        assert!(rejected.document_identity_preserved);
    }
    assert!(transition(MutationPhase::MissingStylesheetRejected).is_empty);
    assert!(transition(MutationPhase::MalformedStylesheetRejected).is_empty);

    assert_eq!(first.scale_baselines.len(), 3);
    for (baseline, requested) in first.scale_baselines.iter().zip([120usize, 1_000, 5_000]) {
        assert_eq!(baseline.requested_nodes, requested);
        assert_eq!(baseline.document_parse_count, 1);
        assert!(baseline.exact_initial_nodes.abs_diff(requested) <= 2);
        assert!(baseline.exact_final_nodes > 0);
        assert!(baseline.full_paint_ms >= 0.0);
    }

    for artifact in &first.artifacts {
        assert!(artifact.snapshot.nodes.iter().all(|node| {
            let bounds = &node.logical_bounds;
            [bounds.x, bounds.y, bounds.width, bounds.height]
                .into_iter()
                .all(f32::is_finite)
                && bounds.width >= 0.0
                && bounds.height >= 0.0
        }));
    }
    for measurement in &first.phase_measurements {
        assert!(measurement.invalidation.exact_nodes_restyled.is_none());
        assert!(
            measurement
                .invalidation
                .exact_layout_nodes_recomputed
                .is_none()
        );
        assert!(
            measurement
                .invalidation
                .exact_paint_commands_regenerated
                .is_none()
        );
        assert!(
            measurement
                .invalidation
                .exact_paint_nodes_retained
                .is_none()
        );
    }
    assert!(first.phase_measurements.iter().any(|measurement| {
        measurement
            .invalidation
            .dirty_descendant_flags_before_resolve
            > 0
            || measurement.invalidation.damaged_nodes_before_resolve > 0
            || measurement.invalidation.style_snapshots_before_resolve > 0
    }));
    assert!(
        first
            .phase_measurements
            .iter()
            .filter(|measurement| measurement.paint_ms.is_some())
            .all(|measurement| measurement.invalidation.full_anyrender_scene_rebuilt)
    );

    for ((first, second), third) in first
        .artifacts
        .iter()
        .zip(&second.artifacts)
        .zip(&third.artifacts)
    {
        assert_eq!(first.snapshot_json, second.snapshot_json);
        assert_eq!(first.snapshot_json, third.snapshot_json);
        assert_eq!(first.diff_json, second.diff_json);
        assert_eq!(first.diff_json, third.diff_json);
        assert_eq!(first.png, second.png);
        assert_eq!(first.png, third.png);
    }
}
