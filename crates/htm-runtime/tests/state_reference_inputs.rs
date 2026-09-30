use htm_runtime::{
    ComponentInputType, ComponentStateValueType, LiveDocument, LiveDocumentKind,
    MAX_STATE_REFERENCE_VALUES_PER_PREPARED_ROOT, MAX_SURFACE_STATE_REFERENCES, NumericValue,
    PackageErrorKind, PackageSnapshotLoader, StateBindingKey, StateToken,
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
            "htmshell-state-reference-input-test-{}-{serial}",
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

    fn package(
        &self,
        components: &str,
        definitions: &str,
        panel_state_references: &str,
        overlay_state_references: &str,
        panel_body: &str,
    ) {
        self.write(
            "shell.json",
            format!(
                r#"{{
                  "version":2,
                  "package":{{"id":"org.example.shell","kind":"shell","version":"1.0.0"}},
                  "dependencies":[],
                  "components":{components},
                  "surfaces":[
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":96,"reserveSpace":true,"stateReferences":{panel_state_references}}},
                    {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false,"stateReferences":{overlay_state_references}}}
                  ]
                }}"#
            ),
        );
        self.write(
            "panel.html",
            format!(
                "<!doctype html><html><body><main id=\"panel-root\">{panel_body}<button id=\"overlay-toggle\">Open</button></main></body></html>"
            ),
        );
        self.write(
            "overlay.html",
            "<!doctype html><html><body><main id=\"overlay-card\"><p id=\"overlay-status\">Closed</p><button id=\"overlay-close\">Close</button><button id=\"overlay-action\">Act</button></main></body></html>",
        );
        self.write("components/components.html", definitions);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn state_input(name: &str, value_type: &str) -> String {
    format!(
        r#"{{"name":"{name}","type":"state-reference","valueType":"{value_type}","required":true}}"#
    )
}

fn export(name: &str, inputs: &str) -> String {
    format!(
        r#"{{"name":"{name}","source":"components/components.html","inputs":{inputs},"slots":[],"styles":[],"resources":[]}}"#
    )
}

fn load_error(fixture: &Fixture) -> PackageErrorKind {
    PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap_err()
        .kind()
}

#[test]
fn state_reference_declarations_are_required_and_exactly_typed() {
    assert_eq!(
        ComponentInputType::parse("state-reference").unwrap(),
        ComponentInputType::StateReference
    );
    for value_type in ["string", "number", "boolean", "token"] {
        assert_eq!(
            ComponentStateValueType::parse(value_type).unwrap().as_str(),
            value_type
        );
    }

    let cases = [
        (
            r#"[{"name":"value","type":"state-reference","required":true}]"#,
            PackageErrorKind::ComponentStateReferenceValueTypeMissing,
        ),
        (
            r#"[{"name":"value","type":"state-reference","valueType":"color","required":true}]"#,
            PackageErrorKind::ComponentStateReferenceValueTypeUnsupported,
        ),
        (
            r#"[{"name":"value","type":"state-reference","valueType":"string"}]"#,
            PackageErrorKind::ComponentStateReferenceRequiredFlagInvalid,
        ),
        (
            r#"[{"name":"value","type":"state-reference","valueType":"string","required":false}]"#,
            PackageErrorKind::ComponentStateReferenceRequiredFlagInvalid,
        ),
        (
            r#"[{"name":"value","type":"state-reference","valueType":"string","required":true,"default":"x"}]"#,
            PackageErrorKind::ComponentStateReferenceDefaultForbidden,
        ),
    ];
    for (inputs, expected) in cases {
        let fixture = Fixture::new();
        fixture.package(
            &format!("[{}]", export("state-label", inputs)),
            r#"<template data-htm-component="state-label"><span data-htm-element="state-text" data-htm-bind="input.value"></span></template>"#,
            "[]",
            "[]",
            r#"<htm-use component="state-label"></htm-use>"#,
        );
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn surface_assignment_and_two_hop_forwarding_use_existing_live_state_path() {
    let fixture = Fixture::new();
    let input = format!("[{}]", state_input("time", "string"));
    fixture.package(
        &format!(
            "[{},{},{}]",
            export("state-outer", &input),
            export("state-middle", &input),
            export("state-leaf", &input)
        ),
        r#"
          <template data-htm-component="state-outer"><htm-use component="state-middle" input-time="input:time"></htm-use></template>
          <template data-htm-component="state-middle"><htm-use component="state-leaf" input-time="input:time"></htm-use></template>
          <template data-htm-component="state-leaf"><span data-htm-element="state-text" data-htm-bind="input.time"></span><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>
        "#,
        r#"[{"name":"clock-time","source":"clock.time","valueType":"string"}]"#,
        "[]",
        r#"<htm-use component="state-outer" input-time="state:clock-time"></htm-use>"#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    assert_eq!(panel.state_references().len(), 1);
    assert_eq!(
        panel.state_references()[0].source(),
        StateBindingKey::ClockTime
    );
    assert_eq!(
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_reference_values,
        3
    );
    assert_eq!(
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_consumer_bindings,
        2
    );
    let mut live = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();
    assert_eq!(
        live.text_binding_target_count(StateBindingKey::ClockTime),
        2
    );
    assert_eq!(
        live.component_input_consumers()
            .iter()
            .filter(|consumer| consumer.is_live_state_reference())
            .count(),
        2
    );
    let first = live
        .apply_bound_text(&[(StateBindingKey::ClockTime, "10:01".to_owned())])
        .unwrap();
    assert_eq!(first.changed_elements, 2);
    let equal = live
        .apply_bound_text(&[(StateBindingKey::ClockTime, "10:01".to_owned())])
        .unwrap();
    assert_eq!(equal.changed_elements, 0);
    let changed = live
        .apply_bound_text(&[(StateBindingKey::ClockTime, "10:02".to_owned())])
        .unwrap();
    assert_eq!(changed.changed_elements, 2);
}

#[test]
fn all_four_state_projections_use_the_existing_finite_consumers() {
    let fixture = Fixture::new();
    let inputs = [
        state_input("text", "string"),
        state_input("value", "number"),
        state_input("enabled", "boolean"),
        state_input("status", "token"),
    ]
    .join(",");
    fixture.package(
        &format!("[{}]", export("state-projections", &format!("[{inputs}]"))),
        r#"
          <template data-htm-component="state-projections">
            <span data-htm-element="state-text" data-htm-bind="input.text"></span>
            <data data-htm-element="state-value" data-htm-bind="input.value"></data>
            <button data-htm-enabled-bind="input.enabled">Enabled by provider</button>
            <span data-htm-element="state-token" data-htm-bind="input.status"></span>
          </template>
        "#,
        r#"[
          {"name":"text","source":"clock.time","valueType":"string"},
          {"name":"value","source":"battery.percentage","valueType":"number"},
          {"name":"enabled","source":"power_profile.availability","valueType":"boolean"},
          {"name":"status","source":"overlay.status","valueType":"token"}
        ]"#,
        "[]",
        r#"<htm-use component="state-projections" input-text="state:text" input-value="state:value" input-enabled="state:enabled" input-status="state:status"></htm-use>"#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let mut live = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();
    assert_eq!(
        live.text_binding_target_count(StateBindingKey::ClockTime),
        1
    );
    assert_eq!(
        live.value_binding_target_count(StateBindingKey::BatteryPercentage),
        1
    );
    assert_eq!(
        live.boolean_binding_target_count(StateBindingKey::PowerProfileAvailability),
        1
    );
    assert_eq!(
        live.token_binding_target_count(StateBindingKey::OverlayStatus),
        1
    );
    assert_eq!(
        live.apply_bound_values(&[(
            StateBindingKey::BatteryPercentage,
            NumericValue::Decimal(82.5),
        )])
        .unwrap()
        .changed_elements,
        1
    );
    assert_eq!(
        live.apply_bound_values(&[(StateBindingKey::BatteryPercentage, NumericValue::Unknown,)])
            .unwrap()
            .changed_elements,
        1
    );
    assert_eq!(
        live.apply_bound_booleans(&[(StateBindingKey::PowerProfileAvailability, Some(true))])
            .unwrap()
            .changed_elements,
        1
    );
    assert_eq!(
        live.apply_bound_booleans(&[(StateBindingKey::PowerProfileAvailability, None)])
            .unwrap()
            .changed_elements,
        1
    );
    assert_eq!(
        live.apply_bound_tokens(&[(StateBindingKey::OverlayStatus, StateToken::Open)])
            .unwrap()
            .changed_elements,
        1
    );
}

#[test]
fn boolean_state_consumers_preserve_authored_children_for_every_availability() {
    let fixture = Fixture::new();
    let declaration = export(
        "state-button",
        &format!("[{}]", state_input("enabled", "boolean")),
    )
    .replace(
        r#""resources":[]"#,
        r#""resources":[{"name":"icon","type":"svg","source":"assets/icon.svg"}]"#,
    );
    fixture.package(
        &format!("[{declaration}]"),
        r#"<template data-htm-component="state-button"><button data-htm-enabled-bind="input.enabled">Provider button <span>Nested label</span><img src="resource:icon" alt="Static icon"></button></template>"#,
        r#"[{"name":"enabled","source":"power_profile.availability","valueType":"boolean"}]"#,
        "[]",
        r#"<htm-use component="state-button" input-enabled="state:enabled"></htm-use>"#,
    );
    fixture.write(
        "assets/icon.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12"><rect width="12" height="12" fill="#ff0000"/></svg>"##,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let panel = &snapshot.root_manifest().unwrap().surfaces[0];
    let mut live = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();
    assert_eq!(live.component_resource_usages().len(), 1);
    let source = live.component_resource_usages()[0].source().id().clone();
    for enabled in [Some(true), Some(false), None, Some(true)] {
        live.apply_bound_booleans(&[(StateBindingKey::PowerProfileAvailability, enabled)])
            .unwrap();
        assert!(
            live.element_text("panel-root")
                .unwrap()
                .contains("Provider button Nested label")
        );
        assert_eq!(live.component_resource_usages().len(), 1);
        assert_eq!(live.component_resource_usages()[0].source().id(), &source);
        live.render().unwrap();
    }
}

#[test]
fn surface_aliases_and_forwarding_fail_closed() {
    let input = format!("[{}]", state_input("time", "string"));
    let cases = [
        (
            "[]",
            r#"<htm-use component="state-leaf" input-time="state:missing"></htm-use>"#,
            r#"<span data-htm-element="state-text" data-htm-bind="input.time"></span>"#,
            PackageErrorKind::ComponentStateReferenceAliasUnknown,
        ),
        (
            r#"[{"name":"time","source":"clock.time","valueType":"token"}]"#,
            r#"<htm-use component="state-leaf" input-time="state:time"></htm-use>"#,
            r#"<span data-htm-element="state-text" data-htm-bind="input.time"></span>"#,
            PackageErrorKind::SurfaceStateSourceTypeMismatch,
        ),
        (
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            r#"<htm-use component="state-leaf"></htm-use>"#,
            r#"<span data-htm-element="state-text" data-htm-bind="input.time"></span>"#,
            PackageErrorKind::ComponentStateReferenceAssignmentMissing,
        ),
        (
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            r#"<htm-use component="state-leaf" input-time="state:time"></htm-use>"#,
            r#"<span data-htm-element="state-token" data-htm-bind="input.time"></span>"#,
            PackageErrorKind::ComponentStateReferenceConsumerWrongType,
        ),
    ];
    for (aliases, body, consumer, expected) in cases {
        let fixture = Fixture::new();
        fixture.package(
            &format!("[{}]", export("state-leaf", &input)),
            &format!(r#"<template data-htm-component="state-leaf">{consumer}</template>"#),
            aliases,
            "[]",
            body,
        );
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn surface_authorizations_are_bounded_finite_and_context_free() {
    let aliases = (0..MAX_SURFACE_STATE_REFERENCES)
        .map(|index| {
            format!(r#"{{"name":"alias-{index}","source":"clock.time","valueType":"string"}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let maximum = Fixture::new();
    maximum.package("[]", "", &format!("[{aliases}]"), "[]", "");
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(maximum.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    assert_eq!(panel.state_references().len(), MAX_SURFACE_STATE_REFERENCES);
    assert_eq!(
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_reference_values,
        0
    );
    assert_eq!(
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_consumer_bindings,
        0
    );

    let overflow = Fixture::new();
    let overflow_aliases = format!(
        "{aliases},{}",
        r#"{"name":"alias-over","source":"clock.time","valueType":"string"}"#
    );
    overflow.package("[]", "", &format!("[{overflow_aliases}]"), "[]", "");
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::SurfaceStateReferenceAliasLimit
    );

    for (aliases, expected) in [
        (
            r#"[{"name":"same","source":"clock.time","valueType":"string"},{"name":"same","source":"clock.time","valueType":"string"}]"#,
            PackageErrorKind::DuplicateSurfaceStateReferenceAlias,
        ),
        (
            r#"[{"name":"items","source":"pipewire.nodes","valueType":"number"}]"#,
            PackageErrorKind::SurfaceStateSourceIneligible,
        ),
        (
            r#"[{"name":"missing","source":"provider.unknown","valueType":"string"}]"#,
            PackageErrorKind::SurfaceStateSourceUnknown,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.package("[]", "", aliases, "[]", "");
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn direct_assignment_grammar_is_exact_and_literal_strings_remain_literal() {
    let state = format!("[{}]", state_input("time", "string"));
    for invalid in [
        "state:",
        "state:time?zone=utc",
        "state:time#part",
        "state:time/path",
        "state:%74ime",
        "State:time",
        "state://time",
    ] {
        let fixture = Fixture::new();
        fixture.package(
            &format!("[{}]", export("state-leaf", &state)),
            r#"<template data-htm-component="state-leaf"><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>"#,
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            "[]",
            &format!(
                r#"<htm-use component="state-leaf" input-time="{invalid}"></htm-use>"#
            ),
        );
        assert_eq!(
            load_error(&fixture),
            PackageErrorKind::ComponentStateReferenceAssignmentMalformed,
            "unexpected classification for {invalid}"
        );
    }

    let literal = Fixture::new();
    literal.package(
        &format!(
            "[{}]",
            export(
                "literal-label",
                r#"[{"name":"label","type":"string","required":true}]"#
            )
        ),
        r#"<template data-htm-component="literal-label"><span data-htm-element="state-text" data-htm-bind="input.label"></span></template>"#,
        "[]",
        "[]",
        r#"<htm-use component="literal-label" input-label="state:ordinary-string"></htm-use>"#,
    );
    PackageSnapshotLoader::new()
        .load_manifest(literal.root.join("shell.json"))
        .unwrap();
}

#[test]
fn components_cannot_establish_state_authority_and_forwarding_is_exactly_typed() {
    let string_input = format!("[{}]", state_input("time", "string"));
    let token_input = format!("[{}]", state_input("time", "token"));
    let cases = [
        (
            format!(
                "[{},{}]",
                export("state-parent", &string_input),
                export("state-child", &string_input)
            ),
            r#"
              <template data-htm-component="state-parent"><htm-use component="state-child" input-time="state:time"></htm-use></template>
              <template data-htm-component="state-child"><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>
            "#,
            PackageErrorKind::ComponentStateReferenceAliasWrongSurface,
        ),
        (
            format!(
                "[{},{}]",
                export("state-parent", &string_input),
                export("state-child", &token_input)
            ),
            r#"
              <template data-htm-component="state-parent"><htm-use component="state-child" input-time="input:time"></htm-use></template>
              <template data-htm-component="state-child"><span data-htm-element="state-token" data-htm-bind="input.time"></span></template>
            "#,
            PackageErrorKind::ComponentStateReferenceForwardingTypeMismatch,
        ),
        (
            format!(
                "[{},{}]",
                export(
                    "literal-parent",
                    r#"[{"name":"label","type":"string","required":true}]"#
                ),
                export("state-child", &string_input)
            ),
            r#"
              <template data-htm-component="literal-parent"><htm-use component="state-child" input-time="input:label"></htm-use></template>
              <template data-htm-component="state-child"><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>
            "#,
            PackageErrorKind::ComponentStateReferenceForwardingSourceWrongType,
        ),
    ];
    for (components, definitions, expected) in cases {
        let fixture = Fixture::new();
        fixture.package(
            &components,
            definitions,
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            "[]",
            if components.contains("literal-parent") {
                r#"<htm-use component="literal-parent" input-label="fixed"></htm-use>"#
            } else {
                r#"<htm-use component="state-parent" input-time="state:time"></htm-use>"#
            },
        );
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn state_values_and_consumer_bindings_accept_the_exact_limits() {
    let aliases = (0..64)
        .map(|index| {
            format!(r#"{{"name":"alias-{index}","source":"clock.time","valueType":"string"}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let inputs = (0..64)
        .map(|index| state_input(&format!("value-{index}"), "string"))
        .collect::<Vec<_>>()
        .join(",");
    let assignments = (0..64)
        .map(|index| format!(r#"input-value-{index}="state:alias-{index}""#))
        .collect::<Vec<_>>()
        .join(" ");
    let uses = (0..(MAX_STATE_REFERENCE_VALUES_PER_PREPARED_ROOT / 64))
        .map(|_| format!(r#"<htm-use component="state-values" {assignments}></htm-use>"#))
        .collect::<String>();
    let maximum_values = Fixture::new();
    maximum_values.package(
        &format!("[{}]", export("state-values", &format!("[{inputs}]"))),
        r#"<template data-htm-component="state-values"></template>"#,
        &format!("[{aliases}]"),
        "[]",
        &uses,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(maximum_values.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    assert_eq!(
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_reference_values,
        MAX_STATE_REFERENCE_VALUES_PER_PREPARED_ROOT
    );

    let overflow = Fixture::new();
    overflow.package(
        &format!(
            "[{},{}]",
            export("state-values", &format!("[{inputs}]")),
            export(
                "state-one",
                &format!("[{}]", state_input("value", "string"))
            )
        ),
        r#"
          <template data-htm-component="state-values"></template>
          <template data-htm-component="state-one"></template>
        "#,
        &format!("[{aliases}]"),
        "[]",
        &format!(r#"{uses}<htm-use component="state-one" input-value="state:alias-0"></htm-use>"#),
    );
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentStateReferenceValueLimit
    );

    // Component hosts and root nodes also count toward the independent expanded-node limit.
    // The counter's exact 50,000/50,001 boundary is covered in the component unit tests.
    let consumer_count = 24;
    let consumer_instances = 1_000;
    let consumers = (0..consumer_count)
        .map(|_| r#"<span data-htm-element="state-text" data-htm-bind="input.time"></span>"#)
        .collect::<String>();
    let consumer_uses = (0..consumer_instances)
        .map(|_| r#"<htm-use component="state-consumers" input-time="state:time"></htm-use>"#)
        .collect::<String>();
    let maximum_bindings = Fixture::new();
    maximum_bindings.package(
        &format!(
            "[{}]",
            export(
                "state-consumers",
                &format!("[{}]", state_input("time", "string"))
            )
        ),
        &format!(r#"<template data-htm-component="state-consumers">{consumers}</template>"#),
        r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
        "[]",
        &consumer_uses,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(maximum_bindings.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    assert_eq!(
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_consumer_bindings,
        consumer_instances * consumer_count
    );
    // Candidate acceptance must cover the constructed DOM, not just binding counts.
    LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();

    let overflowing_consumers = consumers.repeat(2);
    let mut loader = PackageSnapshotLoader::new();
    let first = loader
        .load_manifest(maximum_bindings.root.join("shell.json"))
        .unwrap();
    maximum_bindings.write(
        "components/components.html",
        format!(
            r#"<template data-htm-component="state-consumers">{overflowing_consumers}</template>"#
        ),
    );
    assert_eq!(
        loader
            .load_manifest(maximum_bindings.root.join("shell.json"))
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentExpandedNodeLimit
    );
    assert!(Arc::ptr_eq(loader.current().unwrap(), &first));
}

#[test]
fn failed_state_reference_candidate_retains_last_known_good() {
    let fixture = Fixture::new();
    let input = format!("[{}]", state_input("time", "string"));
    let components = format!("[{}]", export("state-label", &input));
    let definitions = r#"<template data-htm-component="state-label"><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>"#;
    let aliases = r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#;
    fixture.package(
        &components,
        definitions,
        aliases,
        "[]",
        r#"<htm-use component="state-label" input-time="state:time"></htm-use>"#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let first = loader
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let generation = first.generation();

    fixture.package(
        &components,
        definitions,
        aliases,
        "[]",
        r#"<htm-use component="state-label" input-time="state:missing"></htm-use>"#,
    );
    assert_eq!(
        loader
            .load_manifest(fixture.root.join("shell.json"))
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentStateReferenceAliasUnknown
    );
    assert_eq!(loader.current().unwrap().generation(), generation);
    assert!(Arc::ptr_eq(loader.current().unwrap(), &first));

    fixture.package(
        &components,
        definitions,
        aliases,
        "[]",
        r#"<htm-use component="state-label" input-time="state:time"></htm-use>"#,
    );
    let second = loader
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    assert_eq!(second.generation().get(), generation.get() + 1);
}

#[test]
fn state_reference_identities_share_sources_but_are_output_local() {
    let fixture = Fixture::new();
    let input = format!("[{}]", state_input("time", "string"));
    fixture.package(
        &format!("[{}]", export("state-label", &input)),
        r#"
          <template data-htm-component="state-label">
            <span data-htm-element="state-text" data-htm-bind="input.time"></span>
            <span data-htm-element="state-text" data-htm-bind="input.time"></span>
          </template>
        "#,
        r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
        "[]",
        r#"
          <htm-use component="state-label" input-time="state:time"></htm-use>
          <htm-use component="state-label" input-time="state:time"></htm-use>
        "#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let first = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();
    let second = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();
    let first_consumers = first
        .component_input_consumers()
        .iter()
        .filter(|consumer| consumer.is_live_state_reference())
        .collect::<Vec<_>>();
    let second_consumers = second
        .component_input_consumers()
        .iter()
        .filter(|consumer| consumer.is_live_state_reference())
        .collect::<Vec<_>>();
    assert_eq!(first_consumers.len(), 4);
    assert_eq!(second_consumers.len(), 4);
    assert_eq!(
        first_consumers[0]
            .state_reference()
            .unwrap()
            .source_identity(),
        first_consumers[2]
            .state_reference()
            .unwrap()
            .source_identity()
    );
    assert_eq!(
        first_consumers[0]
            .state_reference()
            .unwrap()
            .source_identity(),
        second_consumers[0]
            .state_reference()
            .unwrap()
            .source_identity()
    );
    assert_ne!(
        first_consumers[0]
            .state_reference()
            .unwrap()
            .deterministic_id(),
        first_consumers[2]
            .state_reference()
            .unwrap()
            .deterministic_id()
    );
    assert_ne!(
        first_consumers[0]
            .state_reference()
            .unwrap()
            .deterministic_id(),
        second_consumers[0]
            .state_reference()
            .unwrap()
            .deterministic_id()
    );
    assert_ne!(
        first_consumers[0].state_binding_id().unwrap(),
        second_consumers[0].state_binding_id().unwrap()
    );
    assert_eq!(
        first_consumers[0]
            .state_reference()
            .unwrap()
            .authorization(),
        first_consumers[2]
            .state_reference()
            .unwrap()
            .authorization()
    );
}

#[cfg(feature = "gpu-renderer")]
#[test]
fn component_state_mutations_request_a_conservative_gpu_repaint() {
    let fixture = Fixture::new();
    let input = format!("[{}]", state_input("time", "string"));
    fixture.package(
        &format!("[{}]", export("state-label", &input)),
        r#"<template data-htm-component="state-label"><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>"#,
        r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
        "[]",
        r#"<htm-use component="state-label" input-time="state:time"></htm-use>"#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let mut live = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        480,
        96,
    )
    .unwrap();
    live.apply_bound_text(&[(StateBindingKey::ClockTime, "10:01".to_owned())])
        .unwrap();
    let initial = live
        .prepare_gpu_pending_for(
            htm_runtime::LiveRenderRequest::new(480, 96, 120).unwrap(),
            41,
            1,
        )
        .unwrap()
        .unwrap();
    live.accept_gpu_frame(initial);

    live.apply_bound_text(&[(StateBindingKey::ClockTime, "10:02".to_owned())])
        .unwrap();
    let updated = live
        .prepare_gpu_pending_for(
            htm_runtime::LiveRenderRequest::new(480, 96, 120).unwrap(),
            41,
            1,
        )
        .unwrap()
        .unwrap();
    assert_eq!(updated.damage_estimate.x, 0.0);
    assert_eq!(updated.damage_estimate.y, 0.0);
    assert_eq!(updated.damage_estimate.width, 480.0);
    assert_eq!(updated.damage_estimate.height, 96.0);
}

#[test]
#[ignore = "release-only state-reference input measurements and bounded stress"]
fn state_reference_release_measurement_and_stress_probe() {
    fn micros<T>(operation: impl FnOnce() -> T) -> (u128, T) {
        let started = Instant::now();
        let result = operation();
        (started.elapsed().as_micros(), result)
    }

    fn process_counts() -> (usize, usize, Option<u64>) {
        let descriptors = fs::read_dir("/proc/self/fd")
            .map(|entries| entries.count())
            .unwrap_or_default();
        let threads = fs::read_dir("/proc/self/task")
            .map(|entries| entries.count())
            .unwrap_or_default();
        let rss_kib = fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    line.strip_prefix("VmRSS:")
                        .and_then(|value| value.split_whitespace().next())
                        .and_then(|value| value.parse().ok())
                })
            });
        (descriptors, threads, rss_kib)
    }

    fn load(fixture: &Fixture) -> Arc<htm_runtime::PackageSnapshot> {
        PackageSnapshotLoader::new()
            .load_manifest(fixture.root.join("shell.json"))
            .unwrap()
    }

    fn panel_stats(snapshot: &htm_runtime::PackageSnapshot) -> htm_runtime::PreparedDocumentStats {
        snapshot
            .root_manifest()
            .unwrap()
            .surfaces
            .iter()
            .find(|surface| surface.id() == "panel")
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
    }

    fn simple_fixture(value_type: &str, source: &str, consumer: &str) -> Fixture {
        let fixture = Fixture::new();
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "state-leaf",
                    &format!("[{}]", state_input("value", value_type))
                )
            ),
            &format!(r#"<template data-htm-component="state-leaf">{consumer}</template>"#),
            &format!(r#"[{{"name":"value","source":"{source}","valueType":"{value_type}"}}]"#),
            "[]",
            r#"<htm-use component="state-leaf" input-value="state:value"></htm-use>"#,
        );
        fixture
    }

    fn forwarding_fixture(depth: usize) -> Fixture {
        let fixture = Fixture::new();
        let input = format!("[{}]", state_input("time", "string"));
        let components = (0..depth)
            .map(|index| export(&format!("level-{index}"), &input))
            .collect::<Vec<_>>()
            .join(",");
        let definitions = (0..depth)
            .map(|index| {
                if index + 1 == depth {
                    format!(
                        r#"<template data-htm-component="level-{index}"><span data-htm-element="state-text" data-htm-bind="input.time"></span></template>"#
                    )
                } else {
                    format!(
                        r#"<template data-htm-component="level-{index}"><htm-use component="level-{}" input-time="input:time"></htm-use></template>"#,
                        index + 1
                    )
                }
            })
            .collect::<String>();
        fixture.package(
            &format!("[{components}]"),
            &definitions,
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            "[]",
            r#"<htm-use component="level-0" input-time="state:time"></htm-use>"#,
        );
        fixture
    }

    fn consumer_fixture(count: usize) -> Fixture {
        let fixture = Fixture::new();
        let consumers = (0..count)
            .map(|_| r#"<span data-htm-element="state-text" data-htm-bind="input.time"></span>"#)
            .collect::<String>();
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "state-consumers",
                    &format!("[{}]", state_input("time", "string"))
                )
            ),
            &format!(r#"<template data-htm-component="state-consumers">{consumers}</template>"#),
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            "[]",
            r#"<htm-use component="state-consumers" input-time="state:time"></htm-use>"#,
        );
        fixture
    }

    fn maximum_value_fixture() -> Fixture {
        let fixture = Fixture::new();
        let aliases = (0..64)
            .map(|index| {
                format!(r#"{{"name":"alias-{index}","source":"clock.time","valueType":"string"}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        let inputs = (0..64)
            .map(|index| state_input(&format!("value-{index}"), "string"))
            .collect::<Vec<_>>()
            .join(",");
        let assignments = (0..64)
            .map(|index| format!(r#"input-value-{index}="state:alias-{index}""#))
            .collect::<Vec<_>>()
            .join(" ");
        let uses = (0..(MAX_STATE_REFERENCE_VALUES_PER_PREPARED_ROOT / 64))
            .map(|_| format!(r#"<htm-use component="state-values" {assignments}></htm-use>"#))
            .collect::<String>();
        fixture.package(
            &format!("[{}]", export("state-values", &format!("[{inputs}]"))),
            r#"<template data-htm-component="state-values"></template>"#,
            &format!("[{aliases}]"),
            "[]",
            &uses,
        );
        fixture
    }

    fn large_binding_fixture() -> Fixture {
        let fixture = Fixture::new();
        let consumers = (0..48)
            .map(|_| r#"<span data-htm-element="state-text" data-htm-bind="input.time"></span>"#)
            .collect::<String>();
        let uses = (0..1_000)
            .map(|_| r#"<htm-use component="state-consumers" input-time="state:time"></htm-use>"#)
            .collect::<String>();
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "state-consumers",
                    &format!("[{}]", state_input("time", "string"))
                )
            ),
            &format!(r#"<template data-htm-component="state-consumers">{consumers}</template>"#),
            r#"[{"name":"time","source":"clock.time","valueType":"string"}]"#,
            "[]",
            &uses,
        );
        fixture
    }

    let before = process_counts();
    let string_fixture = simple_fixture(
        "string",
        "clock.time",
        r#"<span data-htm-element="state-text" data-htm-bind="input.value"></span>"#,
    );
    let number_fixture = simple_fixture(
        "number",
        "battery.percentage",
        r#"<data data-htm-element="state-value" data-htm-bind="input.value"></data>"#,
    );
    let boolean_fixture = simple_fixture(
        "boolean",
        "power_profile.availability",
        r#"<button data-htm-enabled-bind="input.value">Value</button>"#,
    );
    let token_fixture = simple_fixture(
        "token",
        "overlay.status",
        r#"<span data-htm-element="state-token" data-htm-bind="input.value"></span>"#,
    );
    let one_hop_fixture = forwarding_fixture(2);
    let depth_32_fixture = forwarding_fixture(32);
    let thousand_consumer_fixture = consumer_fixture(1_000);
    let maximum_value_fixture = maximum_value_fixture();
    let large_binding_fixture = large_binding_fixture();

    let (string_us, string_snapshot) = micros(|| load(&string_fixture));
    let (number_us, _) = micros(|| load(&number_fixture));
    let (boolean_us, _) = micros(|| load(&boolean_fixture));
    let (token_us, _) = micros(|| load(&token_fixture));
    let (forwarding_one_us, _) = micros(|| load(&one_hop_fixture));
    let (forwarding_depth_32_us, depth_32_snapshot) = micros(|| load(&depth_32_fixture));
    let (consumers_1000_us, thousand_snapshot) = micros(|| load(&thousand_consumer_fixture));
    let (values_16384_us, maximum_value_snapshot) = micros(|| load(&maximum_value_fixture));
    let (bindings_48000_us, large_binding_snapshot) = micros(|| load(&large_binding_fixture));

    let panel = string_snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let (live_activation_us, mut live) = micros(|| {
        LiveDocument::load_surface_snapshot(
            Arc::clone(&string_snapshot),
            panel,
            LiveDocumentKind::Panel,
            480,
            96,
        )
        .unwrap()
    });
    let (changed_update_us, changed) = micros(|| {
        live.apply_bound_text(&[(StateBindingKey::ClockTime, "10:01".to_owned())])
            .unwrap()
    });
    assert_eq!(changed.changed_elements, 1);
    let (equal_update_us, equal) = micros(|| {
        live.apply_bound_text(&[(StateBindingKey::ClockTime, "10:01".to_owned())])
            .unwrap()
    });
    assert_eq!(equal.changed_elements, 0);
    let (coalesced_batch_us, batch) = micros(|| {
        live.apply_bound_text(&[
            (StateBindingKey::ClockTime, "10:02".to_owned()),
            (StateBindingKey::OutputLabel, "output".to_owned()),
        ])
        .unwrap()
    });
    assert_eq!(batch.changed_elements, 1);

    let (three_outputs_us, outputs) = micros(|| {
        (0..3)
            .map(|_| {
                LiveDocument::load_surface_snapshot(
                    Arc::clone(&string_snapshot),
                    panel,
                    LiveDocumentKind::Panel,
                    480,
                    96,
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(outputs.len(), 3);

    let candidate_loader = PackageSnapshotLoader::new();
    let (candidate_us, candidate) = micros(|| {
        candidate_loader
            .build_manifest_candidate(string_fixture.root.join("shell.json"))
            .unwrap()
    });
    let mut publication_loader = PackageSnapshotLoader::new();
    let (publication_us, published) = micros(|| publication_loader.publish(candidate).unwrap());
    let (diagnostic_us, diagnostic) = micros(|| published.deterministic_json().unwrap());
    assert!(!diagnostic.is_empty());

    let invalid_declaration = Fixture::new();
    invalid_declaration.package(
        &format!(
            "[{}]",
            export(
                "state-leaf",
                r#"[{"name":"value","type":"state-reference","valueType":"color","required":true}]"#
            )
        ),
        "",
        "[]",
        "[]",
        "",
    );
    let invalid_alias = Fixture::new();
    invalid_alias.package(
        "[]",
        "",
        r#"[{"name":"value","source":"pipewire.nodes","valueType":"number"}]"#,
        "[]",
        "",
    );
    let missing_assignment = Fixture::new();
    missing_assignment.package(
        &format!(
            "[{}]",
            export(
                "state-leaf",
                &format!("[{}]", state_input("value", "string"))
            )
        ),
        r#"<template data-htm-component="state-leaf"><span data-htm-element="state-text" data-htm-bind="input.value"></span></template>"#,
        r#"[{"name":"value","source":"clock.time","valueType":"string"}]"#,
        "[]",
        r#"<htm-use component="state-leaf"></htm-use>"#,
    );
    let type_mismatch = simple_fixture(
        "token",
        "clock.time",
        r#"<span data-htm-element="state-token" data-htm-bind="input.value"></span>"#,
    );

    let (valid_candidates_1000_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..1_000 {
            loader
                .build_manifest_candidate(string_fixture.root.join("shell.json"))
                .unwrap();
        }
    });
    let (invalid_declarations_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(invalid_declaration.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::ComponentStateReferenceValueTypeUnsupported
            );
        }
    });
    let (invalid_aliases_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(invalid_alias.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::SurfaceStateSourceIneligible
            );
        }
    });
    let (missing_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(missing_assignment.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::ComponentStateReferenceAssignmentMissing
            );
        }
    });
    let (type_mismatch_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(type_mismatch.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::SurfaceStateSourceTypeMismatch
            );
        }
    });
    let (forwarding_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            loader
                .build_manifest_candidate(one_hop_fixture.root.join("shell.json"))
                .unwrap();
        }
    });
    let (publications_500_us, ()) = micros(|| {
        let mut loader = PackageSnapshotLoader::new();
        for generation in 1..=500 {
            let snapshot = loader
                .load_manifest(string_fixture.root.join("shell.json"))
                .unwrap();
            assert_eq!(snapshot.generation().get(), generation);
        }
    });
    let (activation_cycles_500_us, ()) = micros(|| {
        for _ in 0..500 {
            let mut live = LiveDocument::load_surface_snapshot(
                Arc::clone(&string_snapshot),
                panel,
                LiveDocumentKind::Panel,
                480,
                96,
            )
            .unwrap();
            assert_eq!(
                live.apply_bound_text(&[(StateBindingKey::ClockTime, "10:01".to_owned())])
                    .unwrap()
                    .changed_elements,
                1
            );
        }
    });
    let (multi_output_500_us, ()) = micros(|| {
        for _ in 0..500 {
            for _ in 0..3 {
                let live = LiveDocument::load_surface_snapshot(
                    Arc::clone(&string_snapshot),
                    panel,
                    LiveDocumentKind::Panel,
                    480,
                    96,
                )
                .unwrap();
                assert_eq!(
                    live.component_input_consumers()
                        .iter()
                        .filter(|consumer| consumer.is_live_state_reference())
                        .count(),
                    1
                );
            }
        }
    });

    assert_eq!(panel_stats(&depth_32_snapshot).state_reference_values, 32);
    assert_eq!(
        panel_stats(&thousand_snapshot).state_consumer_bindings,
        1_000
    );
    assert_eq!(
        panel_stats(&maximum_value_snapshot).state_reference_values,
        MAX_STATE_REFERENCE_VALUES_PER_PREPARED_ROOT
    );
    assert_eq!(
        panel_stats(&large_binding_snapshot).state_consumer_bindings,
        48_000
    );
    let after = process_counts();
    eprintln!(
        "state_reference_measurements_us string={string_us} number={number_us} boolean={boolean_us} token={token_us} forwarding_one={forwarding_one_us} forwarding_depth_32={forwarding_depth_32_us} consumers_1000={consumers_1000_us} values_16384={values_16384_us} bindings_48000={bindings_48000_us} three_outputs={three_outputs_us} candidate={candidate_us} publication={publication_us} diagnostic={diagnostic_us} live_activation={live_activation_us} changed_update={changed_update_us} equal_update={equal_update_us} coalesced_batch={coalesced_batch_us} valid_candidates_1000={valid_candidates_1000_us} invalid_declarations_500={invalid_declarations_500_us} invalid_aliases_500={invalid_aliases_500_us} missing_500={missing_500_us} type_mismatch_500={type_mismatch_500_us} forwarding_500={forwarding_500_us} publications_500={publications_500_us} activation_cycles_500={activation_cycles_500_us} multi_output_500={multi_output_500_us} authorizations={} assignments={} forwarding_values={} consumers={} before_fd={} after_fd={} before_threads={} after_threads={} before_rss_kib={:?} after_rss_kib={:?}",
        panel.state_references().len(),
        panel
            .prepared_document()
            .unwrap()
            .stats()
            .state_reference_values,
        panel_stats(&depth_32_snapshot)
            .state_reference_values
            .saturating_sub(1),
        panel_stats(&thousand_snapshot).state_consumer_bindings,
        before.0,
        after.0,
        before.1,
        after.1,
        before.2,
        after.2
    );
    assert!(after.0 <= before.0.saturating_add(4));
    assert!(after.1 <= before.1.saturating_add(1));
}
