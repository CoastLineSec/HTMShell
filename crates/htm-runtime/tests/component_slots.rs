use htm_runtime::{
    ComponentInputConsumerKind, ComponentSlotName, ComponentSlotProjectionOutcome,
    ExperimentOptions, LiveDocument, LiveDocumentKind, PackageErrorKind, PackageSnapshotLoader,
    ValidatedManifest, ViewportSpec, run_package_with_options,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "htmshell-component-slot-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn write_package(&self, exports: &str, definitions: &str, invocation: &str) {
        self.write(
            "shell.json",
            format!(
                r#"{{
                  "version":2,
                  "package":{{"id":"org.example.shell","kind":"shell","version":"1.0.0"}},
                  "dependencies":[],
                  "components":{exports},
                  "surfaces":[
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true}},
                    {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}}
                  ]
                }}"#
            ),
        );
        self.write(
            "index.html",
            format!("<!doctype html><html><body>{invocation}</body></html>"),
        );
        self.write(
            "panel.html",
            format!(
                r#"<!doctype html><html><body><main id="panel-root">{invocation}<button id="overlay-toggle">Open</button></main></body></html>"#
            ),
        );
        self.write(
            "overlay.html",
            r#"<!doctype html><html><body><main><section id="overlay-card"><p id="overlay-status">Closed</p><button id="overlay-close">Close</button><button id="overlay-action">Act</button></section></main></body></html>"#,
        );
        self.write("components/slots.html", definitions);
    }

    fn manifest(&self) -> PathBuf {
        self.root.join("shell.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn slot_export(name: &str, required: bool, inputs: &str) -> String {
    format!(
        r#"{{"name":"{name}","source":"components/slots.html","inputs":{inputs},"slots":[{{"name":"default","required":{required}}}]}}"#
    )
}

fn plain_export(name: &str) -> String {
    format!(r#"{{"name":"{name}","source":"components/slots.html"}}"#)
}

fn definition(name: &str, content: &str) -> String {
    format!(r#"<template data-htm-component="{name}">{content}</template>"#)
}

fn load_error(exports: &str, definitions: &str, invocation: &str) -> PackageErrorKind {
    let fixture = Fixture::new();
    fixture.write_package(exports, definitions, invocation);
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap_err()
        .kind()
}

#[test]
fn default_slot_declarations_and_template_matching_are_exact() {
    let no_slot = Fixture::new();
    no_slot.write_package(
        &format!("[{}]", plain_export("plain-card")),
        &definition("plain-card", "<p>plain</p>"),
        r#"<htm-use component="plain-card"></htm-use>"#,
    );
    PackageSnapshotLoader::new()
        .load_headless(&no_slot.root)
        .unwrap();

    let valid = format!("[{}]", slot_export("content-frame", false, "[]"));
    let fixture = Fixture::new();
    fixture.write_package(
        &valid,
        &definition("content-frame", "<article><slot></slot></article>"),
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();
    let declaration = snapshot.packages()[0].components()[0]
        .default_slot()
        .unwrap();
    assert_eq!(declaration.name(), &ComponentSlotName::Default);
    assert!(!declaration.required());
    let slot = snapshot.components().definitions()[0]
        .default_slot()
        .unwrap();
    assert_eq!(slot.fallback_node_count(), 0);

    for (exports, definitions, expected) in [
        (
            format!("[{}]", plain_export("content-frame")),
            definition("content-frame", "<slot></slot>"),
            PackageErrorKind::ComponentSlotDefinitionUndeclared,
        ),
        (
            valid.clone(),
            definition("content-frame", "<p>missing</p>"),
            PackageErrorKind::ComponentSlotDefinitionMissing,
        ),
        (
            valid.clone(),
            definition("content-frame", "<slot></slot><slot></slot>"),
            PackageErrorKind::ComponentSlotDefinitionDuplicate,
        ),
        (
            valid.clone(),
            definition("content-frame", r#"<slot name="named"></slot>"#),
            PackageErrorKind::ComponentSlotDefinitionUndeclared,
        ),
        (
            valid.clone(),
            definition("content-frame", r#"<slot data-unsupported="true"></slot>"#),
            PackageErrorKind::ComponentSlotAttributesUnsupported,
        ),
        (
            valid,
            definition("content-frame", "<slot><div><slot></slot></div></slot>"),
            PackageErrorKind::ComponentSlotNestedFallback,
        ),
    ] {
        assert_eq!(load_error(&exports, &definitions, ""), expected);
    }
}

#[test]
fn manifest_rejects_duplicate_and_malformed_slot_declarations() {
    let duplicate = r#"[{"name":"content-frame","source":"components/slots.html","slots":[{"name":"default","required":false},{"name":"default","required":true}]}]"#;
    assert_eq!(
        load_error(duplicate, &definition("content-frame", "<slot></slot>"), ""),
        PackageErrorKind::DuplicateDefaultComponentSlot
    );
    let unknown = r#"[{"name":"content-frame","source":"components/slots.html","slots":[{"name":"default","required":false,"fallback":true}]}]"#;
    assert_eq!(
        load_error(unknown, &definition("content-frame", "<slot></slot>"), ""),
        PackageErrorKind::UnknownField
    );
    for malformed in [
        r#"[{"name":"content-frame","source":"components/slots.html","slots":[{"required":false}]}]"#,
        r#"[{"name":"content-frame","source":"components/slots.html","slots":[{"name":"default"}]}]"#,
        r#"[{"name":"content-frame","source":"components/slots.html","slots":[{"name":"default","required":"false"}]}]"#,
    ] {
        assert_eq!(
            load_error(malformed, &definition("content-frame", "<slot></slot>"), ""),
            PackageErrorKind::MalformedJson
        );
    }
}

#[test]
fn optional_assignment_and_fallback_are_mutually_exclusive() {
    let exports = format!("[{}]", slot_export("content-frame", false, "[]"));
    let definitions = definition(
        "content-frame",
        r#"<article><span>before</span><slot><b>fallback</b></slot><span>after</span></article>"#,
    );
    let assigned = Fixture::new();
    assigned.write_package(
        &exports,
        &definitions,
        r#"<htm-use component="content-frame"><strong>assigned</strong></htm-use>"#,
    );
    let manifest = ValidatedManifest::load(assigned.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(live.component_slot_projections().len(), 1);
    assert_eq!(
        live.component_slot_projections()[0].outcome(),
        ComponentSlotProjectionOutcome::Assigned
    );
    assert!(!live.projected_component_nodes().is_empty());
    assert!(live.component_fallback_nodes().is_empty());

    let fallback = Fixture::new();
    fallback.write_package(
        &exports,
        &definitions,
        r#"<htm-use component="content-frame"> <!-- empty --> </htm-use>"#,
    );
    let manifest = ValidatedManifest::load(fallback.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(
        live.component_slot_projections()[0].outcome(),
        ComponentSlotProjectionOutcome::Fallback
    );
    assert!(live.projected_component_nodes().is_empty());
    assert!(!live.component_fallback_nodes().is_empty());
}

#[test]
fn required_slots_reject_empty_content_and_fallback() {
    let exports = format!("[{}]", slot_export("content-frame", true, "[]"));
    assert_eq!(
        load_error(
            &exports,
            &definition("content-frame", "<slot></slot>"),
            r#"<htm-use component="content-frame"> <!-- empty --> </htm-use>"#
        ),
        PackageErrorKind::ComponentRequiredSlotContentMissing
    );
    assert_eq!(
        load_error(
            &exports,
            &definition("content-frame", "<slot><p>fallback</p></slot>"),
            r#"<htm-use component="content-frame"><p>assigned</p></htm-use>"#
        ),
        PackageErrorKind::ComponentRequiredSlotFallback
    );
}

#[test]
fn invocation_content_requires_a_declared_slot() {
    let exports = format!("[{}]", plain_export("plain-card"));
    let definitions = definition("plain-card", "<p>plain</p>");
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="plain-card"><span>lost</span></htm-use>"#
        ),
        PackageErrorKind::ComponentInvocationContentWithoutSlot
    );
    let fixture = Fixture::new();
    fixture.write_package(
        &exports,
        &definitions,
        r#"<htm-use component="plain-card"> <!-- accepted --> </htm-use>"#,
    );
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();

    let slot_exports = format!("[{}]", slot_export("content-frame", false, "[]"));
    assert_eq!(
        load_error(
            &slot_exports,
            &definition("content-frame", "<slot></slot>"),
            r#"<htm-use component="content-frame"><span slot="icon">bad</span></htm-use>"#
        ),
        PackageErrorKind::ComponentSlotAssignmentUnknown
    );

    for (invocation, assigned_nodes) in [
        (
            r#"<htm-use component="content-frame">assigned text</htm-use>"#,
            1,
        ),
        (
            r#"<htm-use component="content-frame"><span>one</span></htm-use>"#,
            1,
        ),
        (
            r#"<htm-use component="content-frame"><span>one</span><b>two</b></htm-use>"#,
            2,
        ),
        (
            r#"<htm-use component="content-frame">before<span>middle</span>after</htm-use>"#,
            3,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write_package(
            &slot_exports,
            &definition("content-frame", "<slot></slot>"),
            invocation,
        );
        let run = run_package_with_options(
            &fixture.root,
            ExperimentOptions {
                output_directory: None,
                render_png: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            run.component_slot_projections[0].assigned_node_count(),
            assigned_nodes
        );
    }
}

#[test]
fn projected_input_consumers_retain_caller_scope_while_fallback_uses_callee_scope() {
    let outer_inputs = r#"[{"name":"label","type":"string","required":true}]"#;
    let inner_inputs = r#"[{"name":"label","type":"string","default":"inner"}]"#;
    let exports = format!(
        "[{},{}]",
        plain_export("outer-frame").replace(
            "\"source\":\"components/slots.html\"",
            &format!("\"source\":\"components/slots.html\",\"inputs\":{outer_inputs}")
        ),
        slot_export("inner-frame", false, inner_inputs)
    );
    let definitions = format!(
        "{}{}",
        definition(
            "outer-frame",
            r#"<htm-use component="inner-frame" input-label="inner"><span data-htm-element="state-text" data-htm-bind="input.label"></span></htm-use>"#
        ),
        definition(
            "inner-frame",
            r#"<section><slot><span data-htm-element="state-text" data-htm-bind="input.label"></span></slot></section>"#
        )
    );
    let fixture = Fixture::new();
    fixture.write_package(
        &exports,
        &definitions,
        r#"<htm-use component="outer-frame" input-label="outer"></htm-use>"#,
    );
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(live.component_instances().len(), 2);
    assert_eq!(live.component_input_consumers().len(), 1);
    assert_eq!(
        live.component_input_consumers()[0].instance_id(),
        live.component_instances()[0].id()
    );
    assert_eq!(
        live.component_input_consumers()[0].kind(),
        ComponentInputConsumerKind::StateText
    );

    let fallback = Fixture::new();
    fallback.write_package(
        &format!("[{}]", slot_export("inner-frame", false, inner_inputs)),
        &definition(
            "inner-frame",
            r#"<slot><span data-htm-element="state-text" data-htm-bind="input.label"></span></slot>"#
        ),
        r#"<htm-use component="inner-frame"></htm-use>"#,
    );
    let manifest = ValidatedManifest::load(fallback.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(
        live.component_input_consumers()[0].instance_id(),
        live.component_instances()[0].id()
    );
}

#[test]
fn root_state_and_resource_ownership_survive_projection() {
    let exports = format!("[{}]", slot_export("content-frame", false, "[]"));
    let definitions = definition("content-frame", "<article><slot></slot></article>");
    let fixture = Fixture::new();
    fixture.write_package(
        &exports,
        &definitions,
        r##"<htm-use component="content-frame"><span id="surface-name" data-htm-element="state-text" data-htm-bind="surface.template_id"></span><a href="#surface-name">Status</a><img src="assets/mark.svg"><button id="projected-open" data-htm-element="action-button" data-htm-action="overlay.toggle">Open</button></htm-use>"##,
    );
    fixture.write(
        "assets/mark.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#,
    );
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(live.built_in_summary().text_bindings, 1);
    assert_eq!(live.built_in_summary().actions, 1);
    assert_eq!(live.component_input_consumers().len(), 0);
    assert!(live.resource_request_count() > 0);
    assert!(
        live.component_slot_projections()[0]
            .source()
            .deterministic_string()
            .starts_with("root-document#")
    );
}

#[test]
fn nested_default_slots_preserve_projection_ownership_and_scope() {
    let exports = format!(
        "[{},{},{}]",
        slot_export("outer-frame", false, "[]"),
        slot_export("inner-frame", false, "[]"),
        plain_export("leaf-card")
    );
    let definitions = format!(
        "{}{}{}",
        definition(
            "outer-frame",
            r#"<section>outer-before<slot><span>fallback-marker</span><htm-use component="leaf-card"></htm-use></slot>outer-after</section>"#
        ),
        definition(
            "inner-frame",
            r#"<article>inner-before<slot><b>inner-fallback</b></slot>inner-after</article>"#
        ),
        definition("leaf-card", "<em>leaf</em>")
    );
    let fixture = Fixture::new();
    fixture.write_package(
        &exports,
        &definitions,
        r#"<htm-use component="outer-frame"><htm-use component="inner-frame"><htm-use component="leaf-card"></htm-use></htm-use></htm-use>"#,
    );
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();

    assert_eq!(live.component_instances().len(), 3);
    assert_eq!(live.component_slot_projections().len(), 2);
    assert!(
        live.component_slot_projections()
            .iter()
            .all(|projection| projection.outcome() == ComponentSlotProjectionOutcome::Assigned)
    );
    assert!(live.component_slot_projections().iter().all(|projection| {
        projection
            .source()
            .deterministic_string()
            .starts_with("root-document#")
    }));
    assert!(live.component_fallback_nodes().is_empty());

    let fallback = Fixture::new();
    fallback.write_package(
        &exports,
        &definitions,
        r#"<htm-use component="outer-frame"></htm-use>"#,
    );
    let manifest = ValidatedManifest::load(fallback.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(live.component_instances().len(), 2);
    assert_eq!(
        live.component_slot_projections()[0].outcome(),
        ComponentSlotProjectionOutcome::Fallback
    );
    assert!(!live.component_fallback_nodes().is_empty());
}

#[test]
fn projection_identities_are_output_local_while_definitions_are_shared() {
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", slot_export("content-frame", false, "[]")),
        &definition("content-frame", "<article><slot></slot></article>"),
        r#"<htm-use component="content-frame"><strong>assigned</strong></htm-use>"#,
    );
    fixture.write(
        "overlay.html",
        r#"<!doctype html><html><body><main><htm-use component="content-frame"><strong>assigned</strong></htm-use><section id="overlay-card"><p id="overlay-status">Closed</p><button id="overlay-close">Close</button><button id="overlay-action">Act</button></section></main></body></html>"#,
    );
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel_surface = manifest.surface("panel").unwrap();
    let overlay_surface = manifest.surface("overlay").unwrap();
    let panel_a = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel_surface,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    let panel_b = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel_surface,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    let overlay = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        overlay_surface,
        LiveDocumentKind::TransientOverlay,
        800,
        600,
    )
    .unwrap();

    assert_eq!(
        panel_a.component_instances()[0].definition_id(),
        panel_b.component_instances()[0].definition_id()
    );
    assert_eq!(
        panel_a.component_instances()[0].definition_id(),
        overlay.component_instances()[0].definition_id()
    );
    assert_ne!(
        panel_a.component_instances()[0].id(),
        panel_b.component_instances()[0].id()
    );
    assert_ne!(
        panel_a.component_slot_projections()[0].id(),
        panel_b.component_slot_projections()[0].id()
    );
    assert_ne!(
        panel_a.component_slot_projections()[0].id(),
        overlay.component_slot_projections()[0].id()
    );
    assert_eq!(
        panel_a.component_slot_projections()[0].version(),
        panel_b.component_slot_projections()[0].version()
    );
    assert_ne!(
        panel_a.projected_component_nodes()[0].projection_id(),
        panel_b.projected_component_nodes()[0].projection_id()
    );
    for document in [&panel_a, &panel_b, &overlay] {
        assert_eq!(
            document.package_snapshot_generation(),
            Some(manifest.snapshot().generation())
        );
        assert!(!document.component_slot_projections().is_empty());
    }
}

#[test]
fn headless_and_live_projection_semantics_are_equivalent() {
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", slot_export("content-frame", false, "[]")),
        &definition(
            "content-frame",
            "<article><span>before</span><slot><b>fallback</b></slot><span>after</span></article>",
        ),
        r#"<htm-use component="content-frame"><strong>assigned</strong></htm-use>"#,
    );
    let headless = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            output_directory: None,
            render_png: false,
            ..Default::default()
        },
    )
    .unwrap();
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();

    assert_eq!(headless.component_slot_projections.len(), 1);
    assert_eq!(
        headless.component_slot_projections[0].outcome(),
        live.component_slot_projections()[0].outcome()
    );
    assert_eq!(
        headless.component_slot_projections[0].assigned_node_count(),
        live.component_slot_projections()[0].assigned_node_count()
    );
    assert_eq!(
        headless.component_slot_projections[0].version(),
        live.component_slot_projections()[0].version()
    );
    assert_eq!(
        headless.projected_component_nodes.len(),
        live.projected_component_nodes().len()
    );
    assert_eq!(
        headless.component_fallback_nodes.len(),
        live.component_fallback_nodes().len()
    );
    assert_ne!(
        headless.component_slot_projections[0].id(),
        live.component_slot_projections()[0].id()
    );
}

#[test]
fn slot_boundaries_are_pixel_equivalent_to_handwritten_markup() {
    for (layout, assigned_layout) in [
        ("display:flex;gap:4px", ""),
        (
            "display:grid;grid-template-columns:repeat(4,auto);gap:4px",
            "",
        ),
        ("position:relative", "position:absolute;left:48px;top:20px"),
    ] {
        let style = format!(
            "html,body{{margin:0;background:#112;color:white}}.frame{{{layout};width:220px;height:60px;padding:8px;background:#357}}.assigned{{{assigned_layout};filter:brightness(1.05)}}"
        );
        let component = Fixture::new();
        component.write_package(
            &format!("[{}]", slot_export("content-frame", false, "[]")),
            &definition(
                "content-frame",
                r#"<section class="frame"><span>before</span><slot><b>fallback</b></slot><span>after</span></section>"#,
            ),
            r#"<htm-use component="content-frame"><strong class="assigned">assigned</strong><em>content</em></htm-use>"#,
        );
        component.write(
            "index.html",
            format!(
                r#"<!doctype html><html><head><style>{style}</style></head><body><htm-use component="content-frame"><strong class="assigned">assigned</strong><em>content</em></htm-use></body></html>"#
            ),
        );
        let handwritten = Fixture::new();
        handwritten.write_package(
            "[]",
            "",
            r#"<section class="frame"><span>before</span><strong class="assigned">assigned</strong><em>content</em><span>after</span></section>"#,
        );
        handwritten.write(
            "index.html",
            format!(
                r#"<!doctype html><html><head><style>{style}</style></head><body><section class="frame"><span>before</span><strong class="assigned">assigned</strong><em>content</em><span>after</span></section></body></html>"#
            ),
        );
        let options = ExperimentOptions {
            viewport: ViewportSpec {
                logical_width: 320,
                logical_height: 200,
                ..Default::default()
            },
            output_directory: None,
            run_interaction: false,
            render_png: true,
        };
        let component_run = run_package_with_options(&component.root, options.clone()).unwrap();
        let handwritten_run = run_package_with_options(&handwritten.root, options).unwrap();
        assert_eq!(
            component_run.artifacts[0].png, handwritten_run.artifacts[0].png,
            "slot projection changed {layout} output"
        );
        assert_eq!(component_run.component_slot_projections.len(), 1);
        assert!(!component_run.projected_component_nodes.is_empty());
    }
}

#[test]
fn repeat_and_raw_slot_content_are_rejected_before_publication() {
    let exports = format!("[{}]", slot_export("content-frame", false, "[]"));
    let definitions = definition("content-frame", "<slot></slot>");
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><template id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><p></p></template></htm-use>"#
        ),
        PackageErrorKind::ComponentProjectedRepeatNotSupported
    );
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><slot></slot></htm-use>"#
        ),
        PackageErrorKind::ComponentSlotOutsideDefinition
    );
    assert_eq!(
        load_error(&exports, &definitions, r#"<slot></slot>"#),
        PackageErrorKind::ComponentSlotOutsideDefinition
    );
}

#[test]
fn failed_slot_candidate_preserves_last_known_good() {
    let valid_exports = format!("[{}]", slot_export("content-frame", false, "[]"));
    let valid_definition = definition("content-frame", "<slot><p>fallback</p></slot>");
    let fixture = Fixture::new();
    fixture.write_package(
        &valid_exports,
        &valid_definition,
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let first = loader.load_headless(&fixture.root).unwrap();
    for (exports, definitions, invocation, expected) in [
        (
            valid_exports.clone(),
            definition("content-frame", "<p>missing slot</p>"),
            r#"<htm-use component="content-frame"></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentSlotDefinitionMissing,
        ),
        (
            valid_exports.clone(),
            definition("content-frame", "<slot></slot><slot></slot>"),
            r#"<htm-use component="content-frame"></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentSlotDefinitionDuplicate,
        ),
        (
            r#"[{"name":"content-frame","source":"components/slots.html","slots":[{"name":"named","required":false}]}]"#.to_owned(),
            definition("content-frame", "<slot></slot>"),
            r#"<htm-use component="content-frame"></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentSlotDefinitionUndeclared,
        ),
        (
            format!("[{}]", slot_export("content-frame", true, "[]")),
            definition("content-frame", "<slot></slot>"),
            r#"<htm-use component="content-frame"></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentRequiredSlotContentMissing,
        ),
        (
            format!("[{}]", slot_export("content-frame", true, "[]")),
            definition("content-frame", "<slot><p>fallback</p></slot>"),
            r#"<htm-use component="content-frame"><p>assigned</p></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentRequiredSlotFallback,
        ),
        (
            format!("[{}]", plain_export("content-frame")),
            definition("content-frame", "<p>plain</p>"),
            r#"<htm-use component="content-frame"><p>assigned</p></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentInvocationContentWithoutSlot,
        ),
        (
            valid_exports.clone(),
            valid_definition.clone(),
            r#"<htm-use component="content-frame"><p slot="named">assigned</p></htm-use>"#
                .to_owned(),
            PackageErrorKind::ComponentSlotAssignmentUnknown,
        ),
        (
            valid_exports.clone(),
            valid_definition.clone(),
            r#"<htm-use component="content-frame"><template data-htm-element="repeat" data-htm-source="upower.devices"><p></p></template></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentProjectedRepeatNotSupported,
        ),
    ] {
        fixture.write_package(&exports, &definitions, &invocation);
        assert_eq!(
            loader.load_headless(&fixture.root).unwrap_err().kind(),
            expected
        );
        assert!(Arc::ptr_eq(loader.current().unwrap(), &first));
        assert_eq!(loader.current().unwrap().generation(), first.generation());
    }
    fixture.write_package(
        &valid_exports,
        &valid_definition,
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    fixture.write("components/slots.html", valid_definition);
    let second = loader.load_headless(&fixture.root).unwrap();
    assert_ne!(second.generation(), first.generation());
}

#[test]
#[ignore = "release-only component slot measurements and stress"]
fn component_slot_release_measurement_and_stress_probe() {
    fn micros<T>(operation: impl FnOnce() -> T) -> (u128, T) {
        let started = Instant::now();
        let result = operation();
        (started.elapsed().as_micros(), result)
    }

    fn process_observation() -> (u64, usize, usize) {
        let rss_kib = fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("VmRSS:"))
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse().ok())
            })
            .unwrap_or(0);
        let file_descriptors = fs::read_dir("/proc/self/fd")
            .map(|entries| entries.count())
            .unwrap_or(0);
        let threads = fs::read_dir("/proc/self/task")
            .map(|entries| entries.count())
            .unwrap_or(0);
        (rss_kib, file_descriptors, threads)
    }

    let optional = Fixture::new();
    optional.write_package(
        &format!("[{}]", slot_export("content-frame", false, "[]")),
        &definition(
            "content-frame",
            "<article><slot><p>fallback</p></slot></article>",
        ),
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    let assigned = Fixture::new();
    assigned.write_package(
        &format!("[{}]", slot_export("content-frame", false, "[]")),
        &definition(
            "content-frame",
            "<article><slot><p>fallback</p></slot></article>",
        ),
        r#"<htm-use component="content-frame"><p>assigned</p></htm-use>"#,
    );
    let required = Fixture::new();
    required.write_package(
        &format!("[{}]", slot_export("content-frame", true, "[]")),
        &definition("content-frame", "<article><slot></slot></article>"),
        r#"<htm-use component="content-frame"><p>assigned</p></htm-use>"#,
    );
    let thousand_nodes = Fixture::new();
    thousand_nodes.write_package(
        &format!("[{}]", slot_export("content-frame", true, "[]")),
        &definition("content-frame", "<slot></slot>"),
        &format!(
            r#"<htm-use component="content-frame">{}</htm-use>"#,
            "<span>node</span>".repeat(1_000)
        ),
    );
    let thousand_instances = Fixture::new();
    thousand_instances.write_package(
        &format!("[{}]", slot_export("content-frame", true, "[]")),
        &definition("content-frame", "<slot></slot>"),
        &r#"<htm-use component="content-frame"><span>node</span></htm-use>"#.repeat(1_000),
    );
    let depth = Fixture::new();
    let depth_exports = (0..32)
        .map(|index| slot_export(&format!("level-{index:02}"), false, "[]"))
        .collect::<Vec<_>>()
        .join(",");
    let depth_definitions = (0..32)
        .map(|index| {
            let fallback = if index == 31 {
                "<span>leaf</span>".to_owned()
            } else {
                format!(r#"<htm-use component="level-{:02}"></htm-use>"#, index + 1)
            };
            definition(
                &format!("level-{index:02}"),
                &format!("<section><slot>{fallback}</slot></section>"),
            )
        })
        .collect::<String>();
    depth.write_package(
        &format!("[{depth_exports}]"),
        &depth_definitions,
        r#"<htm-use component="level-00"></htm-use>"#,
    );
    let maximum_expansion = Fixture::new();
    maximum_expansion.write_package(
        &format!("[{}]", slot_export("content-frame", true, "[]")),
        &definition("content-frame", "<slot></slot>"),
        &format!(
            r#"<htm-use component="content-frame">{}</htm-use>"#,
            "<i>x</i>".repeat(24_990)
        ),
    );

    let (optional_fallback_us, optional_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&optional.root)
            .unwrap()
    });
    let (assigned_content_us, assigned_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&assigned.root)
            .unwrap()
    });
    let (required_us, _) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&required.root)
            .unwrap()
    });
    let (thousand_projected_nodes_us, thousand_node_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&thousand_nodes.root)
            .unwrap()
    });
    let (thousand_instances_us, thousand_instance_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&thousand_instances.root)
            .unwrap()
    });
    let (maximum_depth_us, depth_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&depth.root)
            .unwrap()
    });
    let (maximum_expansion_us, expansion_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&maximum_expansion.root)
            .unwrap()
    });
    let optional_snapshot = PackageSnapshotLoader::new()
        .publish(optional_candidate)
        .unwrap();
    let assigned_snapshot = PackageSnapshotLoader::new()
        .publish(assigned_candidate)
        .unwrap();
    let thousand_node_snapshot = PackageSnapshotLoader::new()
        .publish(thousand_node_candidate)
        .unwrap();
    let thousand_instance_snapshot = PackageSnapshotLoader::new()
        .publish(thousand_instance_candidate)
        .unwrap();
    let depth_snapshot = PackageSnapshotLoader::new()
        .publish(depth_candidate)
        .unwrap();
    let expansion_snapshot = PackageSnapshotLoader::new()
        .publish(expansion_candidate)
        .unwrap();
    let mut publication_loader = PackageSnapshotLoader::new();
    let publication_candidate = publication_loader
        .build_headless_candidate(&assigned.root)
        .unwrap();
    let (publication_us, published) =
        micros(|| publication_loader.publish(publication_candidate).unwrap());
    let (serialization_us, serialized) = micros(|| assigned_snapshot.deterministic_json().unwrap());
    assert!(serialized.contains("\"projections\""));

    let invalid = Fixture::new();
    invalid.write_package(
        &format!("[{}]", slot_export("content-frame", true, "[]")),
        &definition("content-frame", "<slot></slot>"),
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    for _ in 0..1_000 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&optional.root)
            .unwrap();
    }
    eprintln!("component_slot_stress_stage optional_builds=1000");
    for _ in 0..500 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&required.root)
            .unwrap();
        PackageSnapshotLoader::new()
            .build_headless_candidate(&assigned.root)
            .unwrap();
        PackageSnapshotLoader::new()
            .build_headless_candidate(&optional.root)
            .unwrap();
    }
    eprintln!("component_slot_stress_stage required_assigned_fallback_builds=500");
    for _ in 0..500 {
        assert_eq!(
            publication_loader
                .build_headless_candidate(&invalid.root)
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentRequiredSlotContentMissing
        );
        assert!(Arc::ptr_eq(
            publication_loader.current().unwrap(),
            &published
        ));
    }
    eprintln!("component_slot_stress_stage failed_candidates=500");
    for _ in 0..500 {
        let candidate = publication_loader
            .build_headless_candidate(&assigned.root)
            .unwrap();
        publication_loader.publish(candidate).unwrap();
    }
    eprintln!("component_slot_stress_stage publications=500");
    let manifest = ValidatedManifest::load(assigned.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    for _ in 0..500 {
        let first = LiveDocument::load_surface_snapshot(
            Arc::clone(manifest.snapshot()),
            panel,
            LiveDocumentKind::Panel,
            800,
            52,
        )
        .unwrap();
        let second = LiveDocument::load_surface_snapshot(
            Arc::clone(manifest.snapshot()),
            panel,
            LiveDocumentKind::Panel,
            800,
            52,
        )
        .unwrap();
        assert_ne!(
            first.component_slot_projections()[0].id(),
            second.component_slot_projections()[0].id()
        );
    }
    eprintln!("component_slot_stress_stage paired_live_documents=500");
    for _ in 0..16 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&depth.root)
            .unwrap();
    }
    for _ in 0..500 {
        assert!(!assigned_snapshot.deterministic_json().unwrap().is_empty());
    }
    eprintln!("component_slot_stress_stage nested_projections=512 serializations=500");
    for _ in 0..10 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&maximum_expansion.root)
            .unwrap();
    }
    eprintln!("component_slot_stress_stage maximum_expansion=10");

    let optional_stats = optional_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    let assigned_stats = assigned_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    let thousand_node_stats = thousand_node_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    let thousand_instance_stats = thousand_instance_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    let depth_stats = depth_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    let expansion_stats = expansion_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    let (rss_kib, file_descriptors, threads) = process_observation();
    println!(
        "component_slot_measurements_us optional_fallback={optional_fallback_us} assigned_content={assigned_content_us} required={required_us} thousand_projected_nodes={thousand_projected_nodes_us} thousand_instances={thousand_instances_us} maximum_depth={maximum_depth_us} maximum_expansion={maximum_expansion_us} publication={publication_us} serialization={serialization_us} optional_expanded={} assigned_expanded={} thousand_node_expanded={} thousand_instance_count={} maximum_depth_count={} maximum_expansion_count={} rss_kib={rss_kib} file_descriptors={file_descriptors} threads={threads}",
        optional_stats.expanded_nodes,
        assigned_stats.expanded_nodes,
        thousand_node_stats.expanded_nodes,
        thousand_instance_stats.component_instances,
        depth_stats.maximum_nesting_depth,
        expansion_stats.expanded_nodes,
    );
}
