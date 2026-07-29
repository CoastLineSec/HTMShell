use htm_runtime::{
    ComponentInputConsumerKind, ComponentSlotName, ComponentSlotProjectionOutcome,
    ExperimentOptions, LiveDocument, LiveDocumentKind, MAX_COMPONENT_SLOT_NAME_BYTES,
    MAX_COMPONENT_SLOTS, PackageErrorKind, PackageSnapshotLoader, ValidatedManifest, ViewportSpec,
    run_package_with_options,
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
            "htmshell-component-named-slot-test-{}-{serial}",
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
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":80,"reserveSpace":true}},
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

fn export(name: &str, inputs: &str, slots: &str) -> String {
    format!(
        r#"{{"name":"{name}","source":"components/slots.html","inputs":{inputs},"slots":{slots}}}"#
    )
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

fn live_document(fixture: &Fixture) -> LiveDocument {
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
    LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        80,
    )
    .unwrap()
}

#[test]
fn slot_names_are_bounded_lowercase_identifiers() {
    for valid in [
        "default",
        "a",
        "icon",
        "header-actions",
        "item-2",
        &"a".repeat(MAX_COMPONENT_SLOT_NAME_BYTES),
    ] {
        let parsed = ComponentSlotName::parse(valid).unwrap();
        assert_eq!(parsed.as_str(), valid);
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            format!("\"{valid}\"")
        );
    }
    for invalid in [
        "",
        "Icon",
        "2icon",
        "-icon",
        "icon-",
        "icon--badge",
        "icon.badge",
        "icon badge",
        &"a".repeat(MAX_COMPONENT_SLOT_NAME_BYTES + 1),
    ] {
        assert_eq!(
            ComponentSlotName::parse(invalid).unwrap_err().kind(),
            PackageErrorKind::InvalidComponentSlotName
        );
    }
}

#[test]
fn manifest_preserves_order_and_enforces_unique_bounded_declarations() {
    let slots = r#"[{"name":"footer","required":false},{"name":"default","required":false},{"name":"icon","required":true}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<slot name="icon"></slot><slot></slot><slot name="footer"></slot>"#,
        ),
        r#"<htm-use component="content-frame"><b slot="icon">i</b></htm-use>"#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();
    assert_eq!(
        snapshot.packages()[0].components()[0]
            .slots()
            .iter()
            .map(|slot| slot.name().as_str())
            .collect::<Vec<_>>(),
        ["footer", "default", "icon"]
    );
    assert_eq!(snapshot.components().totals().source_parse_count, 1);

    let duplicate = r#"[{"name":"icon","required":false},{"name":"icon","required":true}]"#;
    assert_eq!(
        load_error(
            &format!("[{}]", export("content-frame", "[]", duplicate)),
            &definition("content-frame", r#"<slot name="icon"></slot>"#),
            "",
        ),
        PackageErrorKind::DuplicateComponentSlotDeclaration
    );

    let maximum_slots = (0..MAX_COMPONENT_SLOTS)
        .map(|index| format!(r#"{{"name":"slot-{index:02}","required":false}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let maximum_definitions = (0..MAX_COMPONENT_SLOTS)
        .map(|index| format!(r#"<slot name="slot-{index:02}"></slot>"#))
        .collect::<String>();
    let maximum = Fixture::new();
    maximum.write_package(
        &format!(
            "[{}]",
            export("content-frame", "[]", &format!("[{maximum_slots}]"))
        ),
        &definition("content-frame", &maximum_definitions),
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    PackageSnapshotLoader::new()
        .load_headless(&maximum.root)
        .unwrap();

    let overflow_slots =
        format!("{maximum_slots},{{\"name\":\"overflow-slot\",\"required\":false}}");
    assert_eq!(
        load_error(
            &format!(
                "[{}]",
                export("content-frame", "[]", &format!("[{overflow_slots}]"))
            ),
            &definition("content-frame", &maximum_definitions),
            "",
        ),
        PackageErrorKind::ComponentSlotDeclarationLimit
    );
}

#[test]
fn named_insertion_points_match_manifest_declarations_exactly() {
    let slots = r#"[{"name":"default","required":false},{"name":"icon","required":false}]"#;
    let exports = format!("[{}]", export("content-frame", "[]", slots));
    for (source, expected) in [
        (
            r#"<slot></slot>"#,
            PackageErrorKind::ComponentSlotDefinitionMissing,
        ),
        (
            r#"<slot></slot><slot name="icon"></slot><slot name="icon"></slot>"#,
            PackageErrorKind::ComponentSlotDefinitionDuplicate,
        ),
        (
            r#"<slot></slot><slot name="unknown"></slot>"#,
            PackageErrorKind::ComponentSlotDefinitionUndeclared,
        ),
        (
            r#"<slot name="default"></slot><slot name="icon"></slot>"#,
            PackageErrorKind::ComponentSlotAttributesUnsupported,
        ),
        (
            r#"<slot></slot><slot name="icon" class="bad"></slot>"#,
            PackageErrorKind::ComponentSlotAttributesUnsupported,
        ),
        (
            r#"<slot></slot><slot name="icon" name="other"></slot>"#,
            PackageErrorKind::ComponentSlotAttributesUnsupported,
        ),
        (
            r#"<slot></slot><slot name="icon"><slot name="nested"></slot></slot>"#,
            PackageErrorKind::ComponentSlotNestedFallback,
        ),
    ] {
        assert_eq!(
            load_error(&exports, &definition("content-frame", source), ""),
            expected
        );
    }
}

#[test]
fn direct_children_route_to_named_slots_and_unqualified_children_route_to_default() {
    let slots = r#"[{"name":"footer","required":false},{"name":"default","required":false},{"name":"icon","required":false}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<article><header><slot name="icon"><i>icon fallback</i></slot></header><main><slot><p>body fallback</p></slot></main><footer><slot name="footer"></slot></footer></article>"#,
        ),
        r#"<htm-use component="content-frame"><small slot="footer">footer</small><p>body</p><b slot="icon">icon</b></htm-use>"#,
    );
    let live = live_document(&fixture);
    assert_eq!(live.component_slot_projections().len(), 3);
    let records = live
        .component_slot_projections()
        .iter()
        .map(|projection| {
            (
                projection.id().slot_definition().name().as_str().to_owned(),
                projection.outcome(),
                projection.assigned_node_count(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records,
        [
            (
                "footer".to_owned(),
                ComponentSlotProjectionOutcome::Assigned,
                1
            ),
            (
                "default".to_owned(),
                ComponentSlotProjectionOutcome::Assigned,
                1
            ),
            (
                "icon".to_owned(),
                ComponentSlotProjectionOutcome::Assigned,
                1
            ),
        ]
    );
    assert!(live.component_fallback_nodes().is_empty());

    let template_order = live
        .projected_component_nodes()
        .iter()
        .map(|node| {
            node.projection_id()
                .slot_definition()
                .name()
                .as_str()
                .to_owned()
        })
        .fold(Vec::<String>::new(), |mut names, name| {
            if names.last() != Some(&name) {
                names.push(name);
            }
            names
        });
    assert_eq!(template_order, ["icon", "default", "footer"]);
}

#[test]
fn each_slot_selects_assigned_fallback_or_empty_independently() {
    let slots = r#"[{"name":"default","required":false},{"name":"icon","required":false},{"name":"footer","required":false}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<slot><p>body fallback</p></slot><slot name="icon"><i>icon fallback</i></slot><slot name="footer"></slot>"#,
        ),
        r#"<htm-use component="content-frame"><b slot="icon">assigned icon</b></htm-use>"#,
    );
    let live = live_document(&fixture);
    let outcomes = live
        .component_slot_projections()
        .iter()
        .map(|projection| {
            (
                projection.id().slot_definition().name().as_str().to_owned(),
                projection.outcome(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        [
            (
                "default".to_owned(),
                ComponentSlotProjectionOutcome::Fallback
            ),
            ("icon".to_owned(), ComponentSlotProjectionOutcome::Assigned),
            (
                "footer".to_owned(),
                ComponentSlotProjectionOutcome::EmptyOptional
            ),
        ]
    );
    assert!(!live.component_fallback_nodes().is_empty());
}

#[test]
fn required_named_slots_reject_missing_content_and_fallback() {
    let slots = r#"[{"name":"icon","required":true}]"#;
    let exports = format!("[{}]", export("content-frame", "[]", slots));
    assert_eq!(
        load_error(
            &exports,
            &definition("content-frame", r#"<slot name="icon"></slot>"#),
            r#"<htm-use component="content-frame"> <!-- empty --> </htm-use>"#,
        ),
        PackageErrorKind::ComponentRequiredSlotContentMissing
    );
    assert_eq!(
        load_error(
            &exports,
            &definition(
                "content-frame",
                r#"<slot name="icon"><i>unreachable</i></slot>"#
            ),
            r#"<htm-use component="content-frame"><b slot="icon">assigned</b></htm-use>"#,
        ),
        PackageErrorKind::ComponentRequiredSlotFallback
    );
}

#[test]
fn routing_rejects_unknown_nested_and_unqualified_ambiguous_content() {
    let named_only = r#"[{"name":"icon","required":false}]"#;
    let exports = format!("[{}]", export("content-frame", "[]", named_only));
    let definitions = definition("content-frame", r#"<slot name="icon"></slot>"#);
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><b slot="unknown">bad</b></htm-use>"#,
        ),
        PackageErrorKind::ComponentSlotAssignmentUnknown
    );
    assert_eq!(
        load_error(
            &format!(
                "[{}]",
                export(
                    "content-frame",
                    "[]",
                    r#"[{"name":"default","required":false}]"#
                )
            ),
            &definition("content-frame", "<slot></slot>"),
            r#"<htm-use component="content-frame"><b slot="default">qualified default</b></htm-use>"#,
        ),
        PackageErrorKind::ComponentSlotAttributePlacement
    );
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><div><b slot="icon">nested</b></div></htm-use>"#,
        ),
        PackageErrorKind::ComponentSlotAttributePlacement
    );
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame">unqualified</htm-use>"#,
        ),
        PackageErrorKind::ComponentInvocationContentWithoutSlot
    );
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><b slot="icon" slot="icon">duplicate</b></htm-use>"#,
        ),
        PackageErrorKind::ComponentSlotAssignmentDuplicate
    );
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<p slot="icon">outside invocation</p>"#,
        ),
        PackageErrorKind::ComponentSlotAttributePlacement
    );
}

#[test]
fn named_projection_preserves_caller_input_scope_and_fallback_uses_callee_scope() {
    let outer_inputs = r#"[{"name":"label","type":"string","required":true}]"#;
    let inner_inputs = r#"[{"name":"label","type":"string","default":"inner"}]"#;
    let inner_slots = r#"[{"name":"body","required":false}]"#;
    let exports = format!(
        "[{},{}]",
        export("outer-frame", outer_inputs, "[]"),
        export("inner-frame", inner_inputs, inner_slots)
    );
    let assigned = Fixture::new();
    assigned.write_package(
        &exports,
        &format!(
            "{}{}",
            definition(
                "outer-frame",
                r#"<htm-use component="inner-frame" input-label="inner"><span slot="body" data-htm-element="state-text" data-htm-bind="input.label"></span></htm-use>"#
            ),
            definition(
                "inner-frame",
                r#"<section><slot name="body"><span data-htm-element="state-text" data-htm-bind="input.label"></span></slot></section>"#
            )
        ),
        r#"<htm-use component="outer-frame" input-label="outer"></htm-use>"#,
    );
    let live = live_document(&assigned);
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
        &format!("[{}]", export("inner-frame", inner_inputs, inner_slots)),
        &definition(
            "inner-frame",
            r#"<slot name="body"><span data-htm-element="state-text" data-htm-bind="input.label"></span></slot>"#,
        ),
        r#"<htm-use component="inner-frame"></htm-use>"#,
    );
    let live = live_document(&fallback);
    assert_eq!(
        live.component_input_consumers()[0].instance_id(),
        live.component_instances()[0].id()
    );
    assert_eq!(
        live.component_slot_projections()[0].outcome(),
        ComponentSlotProjectionOutcome::Fallback
    );
}

#[test]
fn named_projection_preserves_root_state_action_id_and_resource_ownership() {
    let slots = r#"[{"name":"content","required":true}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<article><slot name="content"></slot></article>"#,
        ),
        r##"<htm-use component="content-frame"><section slot="content"><span id="surface-name" data-htm-element="state-text" data-htm-bind="surface.template_id"></span><a href="#surface-name">Status</a><img src="assets/mark.svg"><button id="projected-open" data-htm-element="action-button" data-htm-action="overlay.toggle">Open</button></section></htm-use>"##,
    );
    fixture.write(
        "assets/mark.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#,
    );
    let live = live_document(&fixture);
    assert_eq!(live.built_in_summary().text_bindings, 1);
    assert_eq!(live.built_in_summary().actions, 1);
    assert!(live.resource_request_count() > 0);
    assert!(live.component_input_consumers().is_empty());
    assert!(
        live.component_slot_projections()[0]
            .source()
            .deterministic_string()
            .starts_with("root-document#")
    );
}

#[test]
fn named_projected_repeat_and_raw_slot_content_are_rejected() {
    let slots = r#"[{"name":"content","required":false}]"#;
    let exports = format!("[{}]", export("content-frame", "[]", slots));
    let definitions = definition("content-frame", r#"<slot name="content"></slot>"#);
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><template slot="content" id="rows" data-htm-element="repeat" data-htm-source="upower.devices"><p></p></template></htm-use>"#
        ),
        PackageErrorKind::ComponentProjectedRepeatNotSupported
    );
    assert_eq!(
        load_error(
            &exports,
            &definitions,
            r#"<htm-use component="content-frame"><slot name="content"></slot></htm-use>"#
        ),
        PackageErrorKind::ComponentSlotOutsideDefinition
    );
}

#[test]
fn nested_named_slots_preserve_independent_projection_identity() {
    let slots = r#"[{"name":"body","required":false}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!(
            "[{},{}]",
            export("outer-frame", "[]", slots),
            export("inner-frame", "[]", slots)
        ),
        &format!(
            "{}{}",
            definition(
                "outer-frame",
                r#"<section><slot name="body"><htm-use component="inner-frame"><i slot="body">inner fallback assignment</i></htm-use></slot></section>"#
            ),
            definition(
                "inner-frame",
                r#"<article><slot name="body"><b>inner fallback</b></slot></article>"#
            )
        ),
        r#"<htm-use component="outer-frame"><htm-use slot="body" component="inner-frame"><strong slot="body">leaf</strong></htm-use></htm-use>"#,
    );
    let live = live_document(&fixture);
    assert_eq!(live.component_instances().len(), 2);
    assert_eq!(live.component_slot_projections().len(), 2);
    assert!(
        live.component_slot_projections()
            .iter()
            .all(|projection| { projection.outcome() == ComponentSlotProjectionOutcome::Assigned })
    );
    assert_ne!(
        live.component_slot_projections()[0].id(),
        live.component_slot_projections()[1].id()
    );
}

#[test]
fn headless_live_and_multi_output_share_semantics_but_not_live_identity() {
    let slots = r#"[{"name":"default","required":false},{"name":"icon","required":false}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<slot name="icon"><i>fallback</i></slot><slot></slot>"#,
        ),
        r#"<htm-use component="content-frame"><b slot="icon">i</b><p>body</p></htm-use>"#,
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
    let first = live_document(&fixture);
    let second = live_document(&fixture);
    assert_eq!(headless.component_slot_projections.len(), 2);
    for (headless, live) in headless
        .component_slot_projections
        .iter()
        .zip(first.component_slot_projections())
    {
        assert_eq!(
            headless.id().slot_definition().name(),
            live.id().slot_definition().name()
        );
        assert_eq!(headless.outcome(), live.outcome());
        assert_eq!(headless.version(), live.version());
        assert_ne!(headless.id(), live.id());
    }
    for (left, right) in first
        .component_slot_projections()
        .iter()
        .zip(second.component_slot_projections())
    {
        assert_ne!(left.id(), right.id());
    }
}

#[test]
fn named_slot_boundaries_are_pixel_equivalent_to_handwritten_markup() {
    let style = "html,body{margin:0;background:#112;color:white}.frame{display:flex;gap:4px;width:240px;height:64px;padding:8px;background:#357}.lead{filter:brightness(1.05)}";
    let slots = r#"[{"name":"lead","required":true},{"name":"default","required":true}]"#;
    let component = Fixture::new();
    component.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<section class="frame"><span>before</span><slot name="lead"></slot><slot></slot><span>after</span></section>"#,
        ),
        r#"<htm-use component="content-frame"><em>body</em><strong class="lead" slot="lead">lead</strong></htm-use>"#,
    );
    component.write(
        "index.html",
        format!(
            r#"<!doctype html><html><head><style>{style}</style></head><body><htm-use component="content-frame"><em>body</em><strong class="lead" slot="lead">lead</strong></htm-use></body></html>"#
        ),
    );
    let handwritten = Fixture::new();
    handwritten.write_package(
        "[]",
        "",
        r#"<section class="frame"><span>before</span><strong class="lead">lead</strong><em>body</em><span>after</span></section>"#,
    );
    handwritten.write(
        "index.html",
        format!(
            r#"<!doctype html><html><head><style>{style}</style></head><body><section class="frame"><span>before</span><strong class="lead">lead</strong><em>body</em><span>after</span></section></body></html>"#
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
        component_run.artifacts[0].png,
        handwritten_run.artifacts[0].png
    );
}

#[test]
fn failed_named_slot_candidates_preserve_last_known_good() {
    let slots = r#"[{"name":"icon","required":true}]"#;
    let valid_exports = format!("[{}]", export("content-frame", "[]", slots));
    let valid_definition = definition("content-frame", r#"<slot name="icon"></slot>"#);
    let valid_invocation =
        r#"<htm-use component="content-frame"><b slot="icon">icon</b></htm-use>"#;
    let fixture = Fixture::new();
    fixture.write_package(&valid_exports, &valid_definition, valid_invocation);
    let mut loader = PackageSnapshotLoader::new();
    let first = loader.load_headless(&fixture.root).unwrap();
    for (definitions, invocation, expected) in [
        (
            definition("content-frame", "<p>missing</p>"),
            valid_invocation.to_owned(),
            PackageErrorKind::ComponentSlotDefinitionMissing,
        ),
        (
            valid_definition.clone(),
            r#"<htm-use component="content-frame"></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentRequiredSlotContentMissing,
        ),
        (
            valid_definition.clone(),
            r#"<htm-use component="content-frame"><b slot="unknown">bad</b></htm-use>"#.to_owned(),
            PackageErrorKind::ComponentSlotAssignmentUnknown,
        ),
        (
            valid_definition.clone(),
            r#"<htm-use component="content-frame"><div><b slot="icon">nested</b></div></htm-use>"#
                .to_owned(),
            PackageErrorKind::ComponentSlotAttributePlacement,
        ),
    ] {
        fixture.write_package(&valid_exports, &definitions, &invocation);
        assert_eq!(
            loader.load_headless(&fixture.root).unwrap_err().kind(),
            expected
        );
        assert!(Arc::ptr_eq(loader.current().unwrap(), &first));
        assert_eq!(loader.current().unwrap().generation(), first.generation());
    }
    fixture.write_package(&valid_exports, &valid_definition, valid_invocation);
    let second = loader.load_headless(&fixture.root).unwrap();
    assert_ne!(second.generation(), first.generation());
}

#[test]
fn deterministic_diagnostics_include_ordered_named_slots_without_host_paths() {
    let slots = r#"[{"name":"footer","required":false},{"name":"default","required":false},{"name":"icon","required":false}]"#;
    let fixture = Fixture::new();
    fixture.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<slot name="icon"></slot><slot></slot><slot name="footer"></slot>"#,
        ),
        r#"<htm-use component="content-frame"><b slot="icon">i</b><p>body</p></htm-use>"#,
    );
    let first = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap()
        .deterministic_json()
        .unwrap();
    let second = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap()
        .deterministic_json()
        .unwrap();
    assert_eq!(first, second);
    assert!(first.contains(r#""slots": ["#));
    assert!(first.contains(r#""name": "footer""#));
    assert!(first.contains("slot.icon"));
    assert!(!first.contains(fixture.root.to_string_lossy().as_ref()));
}

#[test]
#[ignore = "release-only named-slot measurements and stress"]
fn named_slot_release_measurement_and_stress_probe() {
    fn micros<T>(operation: impl FnOnce() -> T) -> (u128, T) {
        let started = Instant::now();
        let result = operation();
        (started.elapsed().as_micros(), result)
    }

    let slots = r#"[{"name":"lead","required":false},{"name":"default","required":false},{"name":"footer","required":false}]"#;
    let fallback = Fixture::new();
    fallback.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<slot name="lead"><b>lead</b></slot><slot><p>body</p></slot><slot name="footer"><small>footer</small></slot>"#,
        ),
        r#"<htm-use component="content-frame"></htm-use>"#,
    );
    let assigned = Fixture::new();
    assigned.write_package(
        &format!("[{}]", export("content-frame", "[]", slots)),
        &definition(
            "content-frame",
            r#"<slot name="lead"></slot><slot></slot><slot name="footer"></slot>"#,
        ),
        r#"<htm-use component="content-frame"><small slot="footer">f</small><p>b</p><b slot="lead">l</b></htm-use>"#,
    );
    let (fallback_us, _) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&fallback.root)
            .unwrap()
    });
    let (assigned_us, candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&assigned.root)
            .unwrap()
    });
    let mut loader = PackageSnapshotLoader::new();
    let (publication_us, snapshot) = micros(|| loader.publish(candidate).unwrap());
    let (serialization_us, serialized) = micros(|| snapshot.deterministic_json().unwrap());
    assert!(serialized.contains("slot.lead"));

    for _ in 0..1_000 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&assigned.root)
            .unwrap();
    }
    for _ in 0..500 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&fallback.root)
            .unwrap();
    }
    for _ in 0..500 {
        let candidate = loader.build_headless_candidate(&assigned.root).unwrap();
        loader.publish(candidate).unwrap();
    }
    for _ in 0..500 {
        assert!(!snapshot.deterministic_json().unwrap().is_empty());
    }
    println!(
        "component_named_slot_measurements_us fallback={fallback_us} assigned={assigned_us} publication={publication_us} serialization={serialization_us}"
    );
}
