use crate::RuntimeError;
use crate::adapter::{
    count_damaged_nodes, count_dirty_descendants, elapsed_ms, encode_png, render_rgba,
    resolve_resources, validate_document_limits,
};
use crate::identity::IdentityRegistry;
use crate::incremental::{
    ExperimentalDocumentIdentity, ExperimentalSceneSnapshot, IncrementalExperimentRun,
    InvalidationEvidence, MutationArtifact, MutationPhase, MutationPhaseMeasurement, ScaleBaseline,
    SlotReuseEvidence, StylesheetReloadAttempt,
};
use crate::model::{DiagnosticMessage, ViewportSpec};
use crate::resource::{LocalOnlyResourceProvider, ResourceAudit};
use crate::scene::{build_scene_snapshot, diff_scenes};
use crate::stylesheet::{load_candidate_css, prepare_author_stylesheet, replace_author_stylesheet};
use blitz_dom::{
    Attribute, DocumentConfig, LocalName, Namespace, NodeData, QualName, StyleThreading, ns,
};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
const DOCUMENT_IDENTITY: ExperimentalDocumentIdentity = ExperimentalDocumentIdentity { serial: 1 };
const BASIC_SIZE_LABEL: usize = 120;
const GENERATED_SIZE_LABELS: [usize; 2] = [1_000, 5_000];
const SLOT_REUSE_CYCLES: usize = 8;

pub fn run_incremental_experiment(
    package: impl AsRef<Path>,
) -> Result<IncrementalExperimentRun, RuntimeError> {
    let package = package.as_ref().to_path_buf();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_inner(&package)))
        .map_err(|payload| RuntimeError::EnginePanic(panic_message(payload)))?
}

fn run_inner(package: &Path) -> Result<IncrementalExperimentRun, RuntimeError> {
    let total_started = Instant::now();
    let root = package
        .canonicalize()
        .map_err(|error| RuntimeError::io("resolve package directory", package, error))?;
    if !root.is_dir() {
        return Err(RuntimeError::InvalidPackage(format!(
            "{} is not a directory",
            root.display()
        )));
    }
    let source = root.join("index.html");
    let metadata = source
        .metadata()
        .map_err(|error| RuntimeError::io("inspect index.html", &source, error))?;
    if metadata.len() > MAX_HTML_BYTES {
        return Err(RuntimeError::LimitExceeded(format!(
            "index.html is {} bytes; limit is {MAX_HTML_BYTES}",
            metadata.len()
        )));
    }
    let html = std::fs::read_to_string(&source)
        .map_err(|error| RuntimeError::io("read index.html as UTF-8", &source, error))?;
    let viewport = ViewportSpec::default();
    let audit = Arc::new(ResourceAudit::default());

    let parse_started = Instant::now();
    let mut document_parse_count = 0u32;
    let mut document = make_document(
        &html,
        viewport,
        &root,
        Arc::clone(&audit),
        &mut document_parse_count,
    );
    validate_document_limits(&document)?;
    let parse_ms = elapsed_ms(parse_started);
    let engine_document_id = document.id();

    let initial_resolve_started = Instant::now();
    let mut resource_messages: Vec<DiagnosticMessage> = Vec::new();
    resolve_resources(&mut document, &audit, 0.0, &mut resource_messages);
    let initial_resolve_ms = elapsed_ms(initial_resolve_started);
    let mut identities = IdentityRegistry::from_document(&document);
    let initial_node_count = identities.live_identities(&document)?.len();

    let mut artifacts = Vec::new();
    let mut measurements = Vec::new();
    let initial_evidence = InvalidationEvidence::after_resolve(&document, true);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::Initial,
        parse_ms,
        Some(initial_resolve_ms),
        initial_evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;

    let text_element = required_selector(&document, "#mutation-copy")?;
    let text_slot = document
        .get_node(text_element)
        .and_then(|node| {
            node.children.iter().copied().find(|slot| {
                document
                    .get_node(*slot)
                    .is_some_and(|child| matches!(child.data, NodeData::Text(_)))
            })
        })
        .ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("#mutation-copy has no text node".into())
        })?;
    let text_identity = identities.identity_for_slot(&document, text_slot)?;
    let operation_started = Instant::now();
    {
        let target = identities.resolve(&document, text_identity)?;
        document.mutate().set_node_text(
            target,
            "A native desktop scene authored with local HTML and CSS.",
        );
    }
    let operation_ms = elapsed_ms(operation_started);
    let evidence = InvalidationEvidence::before_resolve(&document);
    let resolve_started = Instant::now();
    document.resolve(0.0);
    let resolve_ms = elapsed_ms(resolve_started);
    let evidence = evidence.with_animation(&document);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::TextMutation,
        operation_ms,
        Some(resolve_ms),
        evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;

    let class_target = required_selector(&document, "#launcher-card")?;
    let class_identity = identities.identity_for_slot(&document, class_target)?;
    let operation_started = Instant::now();
    {
        let target = identities.resolve(&document, class_identity)?;
        document.mutate().set_attribute(
            target,
            attribute_name("class"),
            "card launcher-card mutation-highlight",
        );
    }
    let operation_ms = elapsed_ms(operation_started);
    let evidence = InvalidationEvidence::before_resolve(&document);
    let resolve_started = Instant::now();
    document.resolve(0.0);
    let resolve_ms = elapsed_ms(resolve_started);
    let evidence = evidence.with_animation(&document);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::ClassMutation,
        operation_ms,
        Some(resolve_ms),
        evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;

    let list_parent = required_selector(&document, "#quick-grid")?;
    let operation_started = Instant::now();
    let appended = append_items(&mut document, &mut identities, list_parent, 3, "dynamic")?;
    let operation_ms = elapsed_ms(operation_started);
    let evidence = InvalidationEvidence::before_resolve(&document);
    let resolve_started = Instant::now();
    document.resolve(0.0);
    let resolve_ms = elapsed_ms(resolve_started);
    let evidence = evidence.with_animation(&document);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::ListAppend,
        operation_ms,
        Some(resolve_ms),
        evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;

    let removed_root = appended[0].0;
    let removed_slots = identities.subtree_slots(&document, removed_root)?;
    let retained_siblings_before = appended[1..]
        .iter()
        .map(|(identity, _)| *identity)
        .collect::<Vec<_>>();
    let operation_started = Instant::now();
    let removed_slot = identities.resolve(&document, removed_root)?;
    let removed_node = document.mutate().remove_and_drop_node(removed_slot);
    if removed_node.is_none() {
        return Err(RuntimeError::InvalidMutationTarget(format!(
            "dynamic list node at Blitz slot {removed_slot} could not be removed"
        )));
    }
    let initially_removed = identities.retire_removed(&document, &removed_slots)?;
    let operation_ms = elapsed_ms(operation_started);
    let evidence = InvalidationEvidence::before_resolve(&document);
    let resolve_started = Instant::now();
    document.resolve(0.0);
    let resolve_ms = elapsed_ms(resolve_started);
    let evidence = evidence.with_animation(&document);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::ListRemoval,
        operation_ms,
        Some(resolve_ms),
        evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;

    let operation_started = Instant::now();
    let mut all_created = Vec::new();
    let mut stale_lookups_rejected = 0usize;
    let mut previous_removed = initially_removed.clone();
    for cycle in 0..SLOT_REUSE_CYCLES {
        let created = append_items(
            &mut document,
            &mut identities,
            list_parent,
            1,
            &format!("reuse-{cycle}"),
        )?;
        all_created.extend(created.iter().flat_map(|(element, text)| [*element, *text]));
        for stale in &previous_removed {
            if matches!(
                identities.resolve(&document, *stale),
                Err(RuntimeError::StaleIdentity { .. })
            ) {
                stale_lookups_rejected += 1;
            } else {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "stale identity {stale:?} unexpectedly resolved"
                )));
            }
        }
        if cycle + 1 < SLOT_REUSE_CYCLES {
            let created_root = created[0].0;
            let subtree = identities.subtree_slots(&document, created_root)?;
            let slot = identities.resolve(&document, created_root)?;
            if document.mutate().remove_and_drop_node(slot).is_none() {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "slot reuse node at Blitz slot {slot} could not be removed"
                )));
            }
            previous_removed = identities.retire_removed(&document, &subtree)?;
        }
    }
    let operation_ms = elapsed_ms(operation_started);
    let evidence = InvalidationEvidence::before_resolve(&document);
    let resolve_started = Instant::now();
    document.resolve(0.0);
    let resolve_ms = elapsed_ms(resolve_started);
    let retained_sibling_identities_preserved = retained_siblings_before
        .iter()
        .all(|identity| identities.resolve(&document, *identity).is_ok());
    let reused_slots = reused_slots(&initially_removed, &all_created);
    let maximum_generation = all_created
        .iter()
        .map(|identity| identity.generation)
        .max()
        .unwrap_or(0);
    let slot_reuse = SlotReuseEvidence {
        cycles: SLOT_REUSE_CYCLES,
        initially_removed,
        final_created: all_created,
        reused_slots,
        maximum_generation,
        stale_lookups_rejected,
        retained_sibling_identities_preserved,
    };
    let evidence = evidence.with_animation(&document);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::SlotReuse,
        operation_ms,
        Some(resolve_ms),
        evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;

    let stylesheet_owner = required_selector(&document, "link[rel='stylesheet']")?;
    let mut stylesheet_attempts = Vec::new();
    let candidate_load_started = Instant::now();
    let candidate = load_candidate_css(&root, Path::new("style-reload.css"))?;
    let candidate_load_ms = elapsed_ms(candidate_load_started);
    let prepared = prepare_author_stylesheet(&document, &candidate, "style-reload.css")?;
    let operation_started = Instant::now();
    replace_author_stylesheet(&mut document, stylesheet_owner, prepared)?;
    let replacement_ms = elapsed_ms(operation_started) + candidate_load_ms;
    let evidence = InvalidationEvidence::before_resolve(&document);
    let resolve_started = Instant::now();
    document.resolve(0.0);
    let resolve_ms = elapsed_ms(resolve_started);
    let evidence = evidence.with_animation(&document);
    capture_accepted_phase(
        &mut document,
        &identities,
        MutationPhase::StylesheetReplacement,
        replacement_ms,
        Some(resolve_ms),
        evidence,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;
    stylesheet_attempts.push(StylesheetReloadAttempt {
        phase: MutationPhase::StylesheetReplacement,
        candidate: "style-reload.css".into(),
        accepted: true,
        diagnostic: None,
        accepted_snapshot_preserved: true,
        document_identity_preserved: document.id() == engine_document_id,
    });

    let repeated = prepare_author_stylesheet(&document, &candidate, "style-reload.css")?;
    replace_author_stylesheet(&mut document, stylesheet_owner, repeated)?;
    document.resolve(0.0);
    let repeated_snapshot = build_scene_snapshot(
        &document,
        &identities,
        MutationPhase::StylesheetReplacement,
        DOCUMENT_IDENTITY,
        document_parse_count,
        document.id() == engine_document_id,
        viewport,
    )?;
    let repeated_preserved = same_accepted_scene(
        &artifacts
            .last()
            .expect("accepted stylesheet artifact exists")
            .snapshot,
        &repeated_snapshot,
    );
    if !repeated_preserved {
        return Err(RuntimeError::StylesheetRejected(
            "reapplying the accepted stylesheet changed the deterministic scene".into(),
        ));
    }
    stylesheet_attempts.push(StylesheetReloadAttempt {
        phase: MutationPhase::StylesheetReplacement,
        candidate: "style-reload.css (same-document repeat)".into(),
        accepted: true,
        diagnostic: None,
        accepted_snapshot_preserved: repeated_preserved,
        document_identity_preserved: document.id() == engine_document_id,
    });

    let missing_started = Instant::now();
    let missing_error = load_candidate_css(&root, Path::new("style-missing.css"))
        .expect_err("the intentionally missing stylesheet must not exist");
    let missing_ms = elapsed_ms(missing_started);
    let accepted_before_failure = artifacts
        .last()
        .expect("accepted stylesheet phase exists")
        .snapshot
        .clone();
    capture_rejected_phase(
        &document,
        &identities,
        MutationPhase::MissingStylesheetRejected,
        missing_ms,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;
    let missing_preserved = same_accepted_scene(
        &accepted_before_failure,
        &artifacts.last().expect("missing phase exists").snapshot,
    );
    stylesheet_attempts.push(StylesheetReloadAttempt {
        phase: MutationPhase::MissingStylesheetRejected,
        candidate: "style-missing.css".into(),
        accepted: false,
        diagnostic: Some(missing_error.to_string()),
        accepted_snapshot_preserved: missing_preserved,
        document_identity_preserved: document.id() == engine_document_id,
    });

    let malformed_started = Instant::now();
    let malformed = load_candidate_css(&root, Path::new("style-malformed.css"))?;
    let malformed_error = prepare_author_stylesheet(&document, &malformed, "style-malformed.css")
        .expect_err("the intentionally malformed stylesheet must be rejected");
    let malformed_ms = elapsed_ms(malformed_started);
    capture_rejected_phase(
        &document,
        &identities,
        MutationPhase::MalformedStylesheetRejected,
        malformed_ms,
        viewport,
        engine_document_id,
        document_parse_count,
        &mut artifacts,
        &mut measurements,
    )?;
    let malformed_preserved = same_accepted_scene(
        &accepted_before_failure,
        &artifacts.last().expect("malformed phase exists").snapshot,
    );
    stylesheet_attempts.push(StylesheetReloadAttempt {
        phase: MutationPhase::MalformedStylesheetRejected,
        candidate: "style-malformed.css".into(),
        accepted: false,
        diagnostic: Some(malformed_error.to_string()),
        accepted_snapshot_preserved: malformed_preserved,
        document_identity_preserved: document.id() == engine_document_id,
    });

    let mut scale_baselines = Vec::new();
    let final_accepted = artifacts
        .iter()
        .find(|artifact| artifact.phase == MutationPhase::StylesheetReplacement)
        .expect("accepted stylesheet artifact exists");
    let basic_diff_started = Instant::now();
    let basic_diff = diff_scenes(&artifacts[0].snapshot, &final_accepted.snapshot);
    let basic_diff_ms = elapsed_ms(basic_diff_started);
    let basic_paint_ms = measurements
        .iter()
        .find(|measurement| measurement.phase == MutationPhase::StylesheetReplacement)
        .and_then(|measurement| measurement.paint_ms)
        .unwrap_or(0.0);
    scale_baselines.push(ScaleBaseline {
        requested_nodes: BASIC_SIZE_LABEL,
        exact_initial_nodes: initial_node_count,
        exact_final_nodes: final_accepted.snapshot.node_count,
        document_parse_count,
        retained_nodes: basic_diff.summary.retained_unchanged,
        created_nodes: basic_diff.summary.created,
        removed_nodes: basic_diff.summary.removed,
        changed_nodes: basic_diff.summary.changed,
        style_or_paint_changes: basic_diff.summary.style_or_paint_changes,
        geometry_changes: basic_diff.summary.geometry_changes,
        parse_ms,
        initial_resolve_ms,
        mutation_ms: measurements
            .iter()
            .skip(1)
            .map(|value| value.operation_ms)
            .sum(),
        mutation_resolve_ms: measurements
            .iter()
            .skip(1)
            .filter_map(|value| value.resolve_ms)
            .sum(),
        initial_snapshot_ms: measurements[0].snapshot_ms,
        final_snapshot_ms: measurements
            .iter()
            .find(|value| value.phase == MutationPhase::StylesheetReplacement)
            .map(|value| value.snapshot_ms)
            .unwrap_or(0.0),
        diff_ms: basic_diff_ms,
        full_paint_ms: basic_paint_ms,
        initial_snapshot_json_bytes: artifacts[0].snapshot_json.len(),
        final_snapshot_json_bytes: final_accepted.snapshot_json.len(),
        diff_json_bytes: serde_json::to_vec_pretty(&basic_diff)?.len() + 1,
        process_rss_kib: process_rss_kib(),
    });
    for requested in GENERATED_SIZE_LABELS {
        scale_baselines.push(run_scale_fixture(requested, viewport, &root)?);
    }
    if document_parse_count != 1 {
        return Err(RuntimeError::InvalidPackage(format!(
            "the representative document was parsed {document_parse_count} times"
        )));
    }

    let output_directory = root.join("output/mutation");
    write_artifacts(&mut artifacts, &output_directory)?;
    let total_ms = elapsed_ms(total_started);
    write_measurement_summary(
        &output_directory,
        document_parse_count,
        document.id() == engine_document_id,
        &measurements,
        &stylesheet_attempts,
        &slot_reuse,
        &scale_baselines,
        total_ms,
    )?;

    Ok(IncrementalExperimentRun {
        document_parse_count,
        document_identity_preserved: document.id() == engine_document_id,
        artifacts,
        phase_measurements: measurements,
        stylesheet_attempts,
        slot_reuse,
        scale_baselines,
        total_ms,
        package_root: root,
    })
}

#[allow(clippy::too_many_arguments)]
fn capture_accepted_phase(
    document: &mut HtmlDocument,
    identities: &IdentityRegistry,
    phase: MutationPhase,
    operation_ms: f64,
    resolve_ms: Option<f64>,
    evidence: InvalidationEvidence,
    viewport: ViewportSpec,
    engine_document_id: usize,
    document_parse_count: u32,
    artifacts: &mut Vec<MutationArtifact>,
    measurements: &mut Vec<MutationPhaseMeasurement>,
) -> Result<(), RuntimeError> {
    let snapshot_started = Instant::now();
    let snapshot = build_scene_snapshot(
        document,
        identities,
        phase,
        DOCUMENT_IDENTITY,
        document_parse_count,
        document.id() == engine_document_id,
        viewport,
    )?;
    let snapshot_ms = elapsed_ms(snapshot_started);
    let diff_started = Instant::now();
    let diff = artifacts
        .last()
        .map(|previous| diff_scenes(&previous.snapshot, &snapshot));
    let diff_ms = diff.as_ref().map(|_| elapsed_ms(diff_started));
    let snapshot_json = pretty_json(&snapshot)?;
    let diff_json = diff.as_ref().map(pretty_json).transpose()?;

    let paint_started = Instant::now();
    let rgba = render_rgba(document, viewport.logical_width, viewport.logical_height);
    let paint_ms = elapsed_ms(paint_started);
    let png_started = Instant::now();
    let png = encode_png(&rgba, viewport.logical_width, viewport.logical_height)?;
    let png_encode_ms = elapsed_ms(png_started);

    measurements.push(MutationPhaseMeasurement {
        phase,
        operation_ms,
        resolve_ms,
        snapshot_ms,
        diff_ms,
        paint_ms: Some(paint_ms),
        png_encode_ms: Some(png_encode_ms),
        snapshot_json_bytes: snapshot_json.len(),
        diff_json_bytes: diff_json.as_ref().map(Vec::len),
        invalidation: evidence,
    });
    artifacts.push(MutationArtifact {
        phase,
        snapshot,
        diff_from_previous: diff,
        snapshot_json,
        diff_json,
        png: Some(png),
        snapshot_path: None,
        diff_path: None,
        png_path: None,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn capture_rejected_phase(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    phase: MutationPhase,
    operation_ms: f64,
    viewport: ViewportSpec,
    engine_document_id: usize,
    document_parse_count: u32,
    artifacts: &mut Vec<MutationArtifact>,
    measurements: &mut Vec<MutationPhaseMeasurement>,
) -> Result<(), RuntimeError> {
    let snapshot_started = Instant::now();
    let snapshot = build_scene_snapshot(
        document,
        identities,
        phase,
        DOCUMENT_IDENTITY,
        document_parse_count,
        document.id() == engine_document_id,
        viewport,
    )?;
    let snapshot_ms = elapsed_ms(snapshot_started);
    let diff_started = Instant::now();
    let diff = artifacts
        .last()
        .map(|previous| diff_scenes(&previous.snapshot, &snapshot));
    let diff_ms = diff.as_ref().map(|_| elapsed_ms(diff_started));
    let snapshot_json = pretty_json(&snapshot)?;
    let diff_json = diff.as_ref().map(pretty_json).transpose()?;
    measurements.push(MutationPhaseMeasurement {
        phase,
        operation_ms,
        resolve_ms: None,
        snapshot_ms,
        diff_ms,
        paint_ms: None,
        png_encode_ms: None,
        snapshot_json_bytes: snapshot_json.len(),
        diff_json_bytes: diff_json.as_ref().map(Vec::len),
        invalidation: InvalidationEvidence::after_resolve(document, false),
    });
    artifacts.push(MutationArtifact {
        phase,
        snapshot,
        diff_from_previous: diff,
        snapshot_json,
        diff_json,
        png: None,
        snapshot_path: None,
        diff_path: None,
        png_path: None,
    });
    Ok(())
}

fn append_items(
    document: &mut HtmlDocument,
    identities: &mut IdentityRegistry,
    parent: usize,
    count: usize,
    prefix: &str,
) -> Result<
    Vec<(
        crate::ExperimentalNodeIdentity,
        crate::ExperimentalNodeIdentity,
    )>,
    RuntimeError,
> {
    if document.get_node(parent).is_none() {
        return Err(RuntimeError::InvalidMutationTarget(format!(
            "append parent Blitz slot {parent} is not live"
        )));
    }
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let (element, text) = {
            let mut mutator = document.mutate();
            let element = mutator.create_element(
                element_name("div"),
                vec![Attribute {
                    name: attribute_name("class"),
                    value: "quick-tile dynamic-item".into(),
                }],
            );
            let text = mutator.create_text_node(&format!("{prefix} item {index:04}"));
            mutator.append_children(element, &[text]);
            mutator.append_children(parent, &[element]);
            (element, text)
        };
        let element_identity = identities.activate_created(document, element)?;
        let text_identity = identities.activate_created(document, text)?;
        result.push((element_identity, text_identity));
    }
    Ok(result)
}

fn run_scale_fixture(
    requested_nodes: usize,
    viewport: ViewportSpec,
    package_root: &Path,
) -> Result<ScaleBaseline, RuntimeError> {
    let html = generated_fixture(requested_nodes);
    let audit = Arc::new(ResourceAudit::default());
    let parse_started = Instant::now();
    let mut document_parse_count = 0u32;
    let mut document = make_document(
        &html,
        viewport,
        package_root,
        Arc::clone(&audit),
        &mut document_parse_count,
    );
    validate_document_limits(&document)?;
    let parse_ms = elapsed_ms(parse_started);
    let resolve_started = Instant::now();
    let mut messages = Vec::new();
    resolve_resources(&mut document, &audit, 0.0, &mut messages);
    let initial_resolve_ms = elapsed_ms(resolve_started);
    let mut identities = IdentityRegistry::from_document(&document);
    let initial_snapshot_started = Instant::now();
    let initial = build_scene_snapshot(
        &document,
        &identities,
        MutationPhase::Initial,
        DOCUMENT_IDENTITY,
        document_parse_count,
        true,
        viewport,
    )?;
    let initial_snapshot_ms = elapsed_ms(initial_snapshot_started);
    let initial_json = pretty_json(&initial)?;
    let parent = required_selector(&document, "#scale-list")?;
    let mutation_started = Instant::now();
    let appended = append_items(&mut document, &mut identities, parent, 3, "scale")?;
    let remove_root = appended[0].0;
    let remove_slots = identities.subtree_slots(&document, remove_root)?;
    let remove_slot = identities.resolve(&document, remove_root)?;
    if document
        .mutate()
        .remove_and_drop_node(remove_slot)
        .is_none()
    {
        return Err(RuntimeError::InvalidMutationTarget(
            "generated fixture node could not be removed".into(),
        ));
    }
    identities.retire_removed(&document, &remove_slots)?;
    let mutation_ms = elapsed_ms(mutation_started);
    let mutation_resolve_started = Instant::now();
    document.resolve(0.0);
    let mutation_resolve_ms = elapsed_ms(mutation_resolve_started);
    let final_snapshot_started = Instant::now();
    let final_snapshot = build_scene_snapshot(
        &document,
        &identities,
        MutationPhase::ListRemoval,
        DOCUMENT_IDENTITY,
        document_parse_count,
        true,
        viewport,
    )?;
    let final_snapshot_ms = elapsed_ms(final_snapshot_started);
    let final_json = pretty_json(&final_snapshot)?;
    let diff_started = Instant::now();
    let diff = diff_scenes(&initial, &final_snapshot);
    let diff_ms = elapsed_ms(diff_started);
    let diff_json = pretty_json(&diff)?;
    let paint_started = Instant::now();
    let _rgba = render_rgba(
        &mut document,
        viewport.logical_width,
        viewport.logical_height,
    );
    let full_paint_ms = elapsed_ms(paint_started);

    Ok(ScaleBaseline {
        requested_nodes,
        exact_initial_nodes: initial.node_count,
        exact_final_nodes: final_snapshot.node_count,
        document_parse_count,
        retained_nodes: diff.summary.retained_unchanged,
        created_nodes: diff.summary.created,
        removed_nodes: diff.summary.removed,
        changed_nodes: diff.summary.changed,
        style_or_paint_changes: diff.summary.style_or_paint_changes,
        geometry_changes: diff.summary.geometry_changes,
        parse_ms,
        initial_resolve_ms,
        mutation_ms,
        mutation_resolve_ms,
        initial_snapshot_ms,
        final_snapshot_ms,
        diff_ms,
        full_paint_ms,
        initial_snapshot_json_bytes: initial_json.len(),
        final_snapshot_json_bytes: final_json.len(),
        diff_json_bytes: diff_json.len(),
        process_rss_kib: process_rss_kib(),
    })
}

fn make_document(
    html: &str,
    viewport: ViewportSpec,
    package_root: &Path,
    audit: Arc<ResourceAudit>,
    parse_count: &mut u32,
) -> HtmlDocument {
    let provider = Arc::new(LocalOnlyResourceProvider::new(
        package_root.to_path_buf(),
        audit,
    ));
    let width = ((viewport.logical_width as f32) * viewport.scale_factor).round() as u32;
    let height = ((viewport.logical_height as f32) * viewport.scale_factor).round() as u32;
    *parse_count = parse_count.saturating_add(1);
    let mut document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                width,
                height,
                viewport.scale_factor,
                ColorScheme::Dark,
            )),
            base_url: Some(LocalOnlyResourceProvider::virtual_document_url().to_owned()),
            net_provider: Some(provider),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            style_threading: StyleThreading::Sequential,
            ..Default::default()
        },
    );
    document.set_incremental_layout(true);
    document
}

fn generated_fixture(requested_nodes: usize) -> String {
    let item_count = requested_nodes.saturating_sub(8) / 2;
    let mut html = String::with_capacity(item_count * 64);
    html.push_str(
        "<!doctype html><html><head><style>html,body{margin:0}body{font-family:sans-serif;background:#101522;color:white}#scale-list{display:grid;grid-template-columns:repeat(20,1fr);gap:2px}.scale-item{min-height:8px;background:#26324a}</style></head><body><main id=\"scale-list\">",
    );
    for index in 0..item_count {
        html.push_str(&format!("<div class=\"scale-item\">item {index:05}</div>"));
    }
    html.push_str("</main></body></html>");
    html
}

fn required_selector(document: &HtmlDocument, selector: &str) -> Result<usize, RuntimeError> {
    document
        .query_selector(selector)
        .map_err(|error| RuntimeError::InvalidMutationTarget(format!("{error:?}")))?
        .ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("selector `{selector}` did not match"))
        })
}

fn element_name(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(html),
        local: LocalName::from(local),
    }
}

fn attribute_name(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: Namespace::from(""),
        local: LocalName::from(local),
    }
}

fn reused_slots(
    removed: &[crate::ExperimentalNodeIdentity],
    created: &[crate::ExperimentalNodeIdentity],
) -> Vec<usize> {
    let removed_slots: BTreeSet<_> = removed.iter().map(|identity| identity.slot).collect();
    created
        .iter()
        .filter(|identity| removed_slots.contains(&identity.slot) && identity.generation > 0)
        .map(|identity| identity.slot)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn same_accepted_scene(
    accepted: &ExperimentalSceneSnapshot,
    candidate: &ExperimentalSceneSnapshot,
) -> bool {
    accepted.document_identity == candidate.document_identity
        && accepted.document_parse_count == candidate.document_parse_count
        && accepted.nodes == candidate.nodes
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, RuntimeError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_artifacts(
    artifacts: &mut [MutationArtifact],
    output_directory: &Path,
) -> Result<(), RuntimeError> {
    std::fs::create_dir_all(output_directory).map_err(|error| {
        RuntimeError::io("create mutation output directory", output_directory, error)
    })?;
    for artifact in artifacts {
        let snapshot_path =
            output_directory.join(format!("{}.snapshot.json", artifact.phase.filename()));
        std::fs::write(&snapshot_path, &artifact.snapshot_json)
            .map_err(|error| RuntimeError::io("write mutation snapshot", &snapshot_path, error))?;
        artifact.snapshot_path = Some(snapshot_path);
        if let Some(diff_json) = &artifact.diff_json {
            let diff_path =
                output_directory.join(format!("{}.diff.json", artifact.phase.filename()));
            std::fs::write(&diff_path, diff_json)
                .map_err(|error| RuntimeError::io("write mutation diff", &diff_path, error))?;
            artifact.diff_path = Some(diff_path);
        }
        if let Some(png) = &artifact.png {
            let png_path = output_directory.join(format!("{}.png", artifact.phase.filename()));
            let mut file = std::fs::File::create(&png_path)
                .map_err(|error| RuntimeError::io("create mutation PNG", &png_path, error))?;
            file.write_all(png)
                .map_err(|error| RuntimeError::io("write mutation PNG", &png_path, error))?;
            artifact.png_path = Some(png_path);
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct MeasurementSummary<'a> {
    schema_version: &'static str,
    note: &'static str,
    document_parse_count: u32,
    document_identity_preserved: bool,
    full_anyrender_reconstruction: &'static str,
    unavailable_exact_counters: [&'static str; 4],
    phase_measurements: &'a [MutationPhaseMeasurement],
    stylesheet_attempts: &'a [StylesheetReloadAttempt],
    slot_reuse: &'a SlotReuseEvidence,
    scale_baselines: &'a [ScaleBaseline],
    total_ms: f64,
}

#[allow(clippy::too_many_arguments)]
fn write_measurement_summary(
    output_directory: &Path,
    document_parse_count: u32,
    document_identity_preserved: bool,
    measurements: &[MutationPhaseMeasurement],
    stylesheet_attempts: &[StylesheetReloadAttempt],
    slot_reuse: &SlotReuseEvidence,
    scale_baselines: &[ScaleBaseline],
    total_ms: f64,
) -> Result<(), RuntimeError> {
    let summary = MeasurementSummary {
        schema_version: "htmshell.incremental-measurements.v1",
        note: "Feasibility timings are volatile and are not a benchmark or deterministic artifact.",
        document_parse_count,
        document_identity_preserved,
        full_anyrender_reconstruction: "Every accepted painted phase invokes blitz_paint::paint_scene and rebuilds the AnyRender scene.",
        unavailable_exact_counters: [
            "nodes_restyled",
            "taffy_nodes_recomputed",
            "paint_commands_regenerated",
            "paint_nodes_retained",
        ],
        phase_measurements: measurements,
        stylesheet_attempts,
        slot_reuse,
        scale_baselines,
        total_ms,
    };
    let path = output_directory.join("measurements.json");
    std::fs::write(&path, pretty_json(&summary)?)
        .map_err(|error| RuntimeError::io("write mutation measurements", path, error))
}

fn process_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_ascii_whitespace().nth(1)?.parse().ok()
}

fn style_snapshot_count(document: &HtmlDocument) -> usize {
    document
        .tree()
        .iter()
        .filter(|(_, node)| node.has_snapshot)
        .count()
}

impl InvalidationEvidence {
    fn before_resolve(document: &HtmlDocument) -> Self {
        Self {
            dirty_descendant_flags_before_resolve: count_dirty_descendants(document),
            damaged_nodes_before_resolve: count_damaged_nodes(document),
            style_snapshots_before_resolve: style_snapshot_count(document),
            animation_running_after_resolve: false,
            exact_nodes_restyled: None,
            exact_layout_nodes_recomputed: None,
            exact_paint_commands_regenerated: None,
            exact_paint_nodes_retained: None,
            full_anyrender_scene_rebuilt: true,
        }
    }

    fn after_resolve(document: &HtmlDocument, painted: bool) -> Self {
        Self {
            dirty_descendant_flags_before_resolve: count_dirty_descendants(document),
            damaged_nodes_before_resolve: count_damaged_nodes(document),
            style_snapshots_before_resolve: style_snapshot_count(document),
            animation_running_after_resolve: document.is_animating(),
            exact_nodes_restyled: None,
            exact_layout_nodes_recomputed: None,
            exact_paint_commands_regenerated: None,
            exact_paint_nodes_retained: None,
            full_anyrender_scene_rebuilt: painted,
        }
    }

    fn with_animation(mut self, document: &HtmlDocument) -> Self {
        self.animation_running_after_resolve = document.is_animating();
        self
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
