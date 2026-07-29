use htm_runtime::{
    ComponentInputName, ComponentInputProvenance, ComponentInputType, ComponentInputValue,
    ExperimentOptions, LiveDocument, LiveDocumentKind, MAX_COMPONENT_INPUT_ATTRIBUTES,
    MAX_COMPONENT_INPUT_LITERAL_BYTES, MAX_COMPONENT_INPUT_STRING_BYTES, MAX_COMPONENT_INPUTS,
    PackageErrorKind, PackageSnapshotLoader, ValidatedManifest, ViewportSpec,
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
            "htmshell-component-input-test-{}-{serial}",
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

    fn write_package(&self, export: &str, component: &str, invocation: &str) {
        self.write(
            "shell.json",
            format!(
                r#"{{
                  "version":2,
                  "package":{{"id":"org.example.shell","kind":"shell","version":"1.0.0"}},
                  "dependencies":[],
                  "components":[{export}],
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
        self.write("panel.html", panel_document(invocation));
        self.write("overlay.html", overlay_document(invocation));
        self.write(
            "components/card.html",
            format!(
                r#"<!doctype html><template data-htm-component="status-card">{component}</template>"#
            ),
        );
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

fn panel_document(content: &str) -> String {
    format!(
        r#"<!doctype html><html><body><main id="panel-root">{content}<button id="overlay-toggle">Open</button></main></body></html>"#
    )
}

fn overlay_document(content: &str) -> String {
    format!(
        r#"<!doctype html><html><body><main><section id="overlay-card">{content}<p id="overlay-status">Closed</p><button id="overlay-close">Close</button><button id="overlay-action">Act</button></section></main></body></html>"#
    )
}

fn export_with_inputs(inputs: &str) -> String {
    format!(r#"{{"name":"status-card","source":"components/card.html","inputs":{inputs}}}"#)
}

fn load_error(inputs: &str, component: &str, invocation: &str) -> PackageErrorKind {
    let fixture = Fixture::new();
    fixture.write_package(&export_with_inputs(inputs), component, invocation);
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap_err()
        .kind()
}

#[test]
fn input_name_and_type_grammars_are_exact() {
    for valid in ["label", "item-count", "enabled2", "accent-color"] {
        assert_eq!(ComponentInputName::parse(valid).unwrap().as_str(), valid);
    }
    for invalid in [
        "",
        "Label",
        "2label",
        "-label",
        "label-",
        "label--tone",
        "label.tone",
        "label tone",
        &"x".repeat(65),
        "component",
        "slot",
        "id",
        "class",
        "style",
        "input",
        "state",
        "action",
        "service",
        "resource",
        "repeat",
        "surface",
        "host",
    ] {
        assert!(
            ComponentInputName::parse(invalid).is_err(),
            "accepted `{invalid}`"
        );
    }
    for (value, expected) in [
        ("string", ComponentInputType::String),
        ("number", ComponentInputType::Number),
        ("boolean", ComponentInputType::Boolean),
        ("token", ComponentInputType::Token),
        ("color", ComponentInputType::Color),
        ("length", ComponentInputType::Length),
    ] {
        assert_eq!(ComponentInputType::parse(value).unwrap(), expected);
    }
    for value in [
        "str",
        "float",
        "bool",
        "css-color",
        "css-length",
        "state",
        "action",
        "resource",
    ] {
        assert!(ComponentInputType::parse(value).is_err());
    }
}

#[test]
fn all_literal_types_normalize_and_render_through_local_consumers() {
    let inputs = r##"[
      {"name":"label","type":"string","required":true},
      {"name":"count","type":"number","default":0},
      {"name":"enabled","type":"boolean","default":true},
      {"name":"tone","type":"token","default":"ready"},
      {"name":"accent","type":"color","default":"#dcebff"},
      {"name":"spacing","type":"length","default":"8px"}
    ]"##;
    let component = r#"
      <article class="card">
        <span data-htm-element="state-text" data-htm-bind="input.label"></span>
        <data data-htm-element="state-value" data-htm-bind="input.count"></data>
        <span data-htm-element="state-token" data-htm-bind="input.enabled"></span>
        <span data-htm-element="state-token" data-htm-bind="input.tone"></span>
        <span data-htm-element="state-text" data-htm-bind="input.accent"></span>
        <span data-htm-element="state-text" data-htm-bind="input.spacing"></span>
      </article>
    "#;
    let invocation = r##"<htm-use component="status-card" input-spacing="-0px" input-accent="rgb(124 196 255 / 50%)" input-tone="active" input-enabled="false" input-count="-0" input-label=" &#x2603; &lt;{{input.label}}&gt; "></htm-use>"##;
    let fixture = Fixture::new();
    fixture.write_package(&export_with_inputs(inputs), component, invocation);
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let definition = manifest.snapshot().components().definitions()[0].clone();
    assert_eq!(definition.inputs().len(), 6);
    assert_eq!(
        definition
            .inputs()
            .iter()
            .map(|input| input.name().as_str())
            .collect::<Vec<_>>(),
        ["label", "count", "enabled", "tone", "accent", "spacing"]
    );

    let panel = manifest.surface("panel").unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(live.component_instances().len(), 1);
    assert_eq!(live.component_input_consumers().len(), 6);
    assert_eq!(live.built_in_summary().registered_elements, 0);
    let values = live.component_instances()[0].inputs().values();
    assert_eq!(
        values
            .iter()
            .map(|input| input.value().canonical_string())
            .collect::<Vec<_>>(),
        [
            " ☃ <{{input.label}}> ",
            "0",
            "false",
            "active",
            "#7cc4ff80",
            "0px"
        ]
    );
    assert!(
        values
            .iter()
            .all(|value| value.provenance() == ComponentInputProvenance::Supplied)
    );
    assert!(matches!(values[1].value(), ComponentInputValue::Number(_)));
    assert!(matches!(values[4].value(), ComponentInputValue::Color(_)));
    let diagnostic = manifest.deterministic_package_graph_json().unwrap();
    assert!(diagnostic.contains("\"semantic_version\""));
    assert!(diagnostic.contains("\"value\": \"#7cc4ff80\""));
    assert!(diagnostic.contains("\"kind\": \"state-token\""));
}

#[test]
fn defaults_required_values_and_versions_are_semantic() {
    let inputs = r#"[
      {"name":"label","type":"string","required":true},
      {"name":"count","type":"number","default":0},
      {"name":"enabled","type":"boolean","default":true}
    ]"#;
    let component = r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#;
    let invocation = concat!(
        r#"<htm-use component="status-card" input-label="same"></htm-use>"#,
        r#"<htm-use input-enabled="true" input-count="-0" input-label="same" component="status-card"></htm-use>"#,
        r#"<htm-use component="status-card" input-label="different"></htm-use>"#
    );
    let fixture = Fixture::new();
    fixture.write_package(&export_with_inputs(inputs), component, invocation);
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        manifest.surface("panel").unwrap(),
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    let instances = live.component_instances();
    assert_eq!(instances.len(), 3);
    assert_eq!(
        instances[0].inputs().version(),
        instances[1].inputs().version()
    );
    assert_ne!(
        instances[0].inputs().version(),
        instances[2].inputs().version()
    );
    assert!(
        instances[0]
            .inputs()
            .is_structurally_compatible_with(instances[2].inputs())
    );
    assert_eq!(
        instances[0].inputs().values()[1].provenance(),
        ComponentInputProvenance::Defaulted
    );
    assert_eq!(
        instances[1].inputs().values()[1].provenance(),
        ComponentInputProvenance::Supplied
    );
    assert_ne!(instances[0].id(), instances[1].id());
}

#[test]
fn declarations_and_invocations_reject_invalid_contracts() {
    let declaration_cases = [
        (
            r#"[{"name":"label","type":"string","required":true,"default":"x"}]"#,
            PackageErrorKind::ComponentInputRequiredWithDefault,
        ),
        (
            r#"[{"name":"label","type":"string","required":false}]"#,
            PackageErrorKind::ComponentInputOptionalWithoutDefault,
        ),
        (
            r#"[{"name":"label","type":"string"}]"#,
            PackageErrorKind::ComponentInputOptionalWithoutDefault,
        ),
        (
            r#"[{"name":"label","type":"unknown","default":"x"}]"#,
            PackageErrorKind::UnsupportedComponentInputType,
        ),
        (
            r#"[{"name":"label","type":"string","default":"x"},{"name":"label","type":"string","default":"y"}]"#,
            PackageErrorKind::DuplicateComponentInputDeclaration,
        ),
        (
            r#"[{"name":"label","type":"number","default":"1"}]"#,
            PackageErrorKind::InvalidComponentInputDefault,
        ),
        (
            r#"[{"name":"enabled","type":"boolean","default":"true"}]"#,
            PackageErrorKind::InvalidComponentInputDefault,
        ),
        (
            r#"[{"name":"state","type":"string","default":"x"}]"#,
            PackageErrorKind::ReservedComponentInputName,
        ),
        (
            r#"[{"name":"binding","type":"state-reference","default":"x"}]"#,
            PackageErrorKind::ComponentStateReferenceInputNotSupported,
        ),
    ];
    for (inputs, expected) in declaration_cases {
        assert_eq!(
            load_error(inputs, "<p>static</p>", ""),
            expected,
            "unexpected declaration result for {inputs}"
        );
    }

    let inputs = r#"[{"name":"label","type":"string","required":true},{"name":"count","type":"number","default":0},{"name":"enabled","type":"boolean","default":true},{"name":"tone","type":"token","default":"ready"},{"name":"accent","type":"color","default":"red"},{"name":"spacing","type":"length","default":"0"}]"#;
    let invocation_cases = [
        (
            r#"<htm-use component="status-card"></htm-use>"#,
            PackageErrorKind::ComponentInputMissingRequired,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" input-other="x"></htm-use>"#,
            PackageErrorKind::ComponentInputUnknown,
        ),
        (
            r#"<htm-use component="status-card" label="x"></htm-use>"#,
            PackageErrorKind::ComponentInvocationAttributes,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" input-count="1px"></htm-use>"#,
            PackageErrorKind::InvalidComponentInputLiteral,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" input-enabled="TRUE"></htm-use>"#,
            PackageErrorKind::InvalidComponentInputLiteral,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" input-tone="not-a-runtime-token"></htm-use>"#,
            PackageErrorKind::InvalidComponentInputLiteral,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" input-accent="currentColor"></htm-use>"#,
            PackageErrorKind::InvalidComponentInputLiteral,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" input-spacing="50%"></htm-use>"#,
            PackageErrorKind::InvalidComponentInputLiteral,
        ),
        (
            r#"<htm-use component="status-card" input-label="x" class="host"></htm-use>"#,
            PackageErrorKind::ComponentInvocationAttributes,
        ),
        (
            r#"<htm-use component="status-card" input-label=">" INPUT-LABEL="y"></htm-use>"#,
            PackageErrorKind::ComponentInputDuplicate,
        ),
        (
            r#"<htm-use component="status-card" input-label="x">child</htm-use>"#,
            PackageErrorKind::ComponentInvocationContentWithoutSlot,
        ),
    ];
    for (invocation, expected) in invocation_cases {
        assert_eq!(
            load_error(inputs, "<p>static</p>", invocation),
            expected,
            "unexpected invocation result for {invocation}"
        );
    }

    assert_eq!(
        load_error(
            inputs,
            "<p>static</p>",
            "<htm-use component=\"status-card\" input-label=\"a\0b\"></htm-use>"
        ),
        PackageErrorKind::InvalidComponentInputLiteral
    );
}

#[test]
fn input_consumers_are_local_typed_and_non_subscribing() {
    let cases = [
        (
            r#"[{"name":"label","type":"string","default":"x"}]"#,
            r#"<span data-htm-element="state-token" data-htm-bind="input.label"></span>"#,
            PackageErrorKind::ComponentInputConsumerTypeMismatch,
        ),
        (
            r#"[{"name":"count","type":"number","default":1}]"#,
            r#"<span data-htm-element="state-text" data-htm-bind="input.unknown"></span>"#,
            PackageErrorKind::ComponentInputNamespaceUnknown,
        ),
        (
            r#"[{"name":"label","type":"string","default":"x"}]"#,
            r#"<span data-htm-element="state-text" data-htm-bind="clock.time"></span>"#,
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            r#"[{"name":"count","type":"number","default":1}]"#,
            r#"<data data-htm-element="state-value" data-htm-bind="input.count" data-htm-format="percent"></data>"#,
            PackageErrorKind::ComponentInputConsumerTypeMismatch,
        ),
    ];
    for (inputs, component, expected) in cases {
        assert_eq!(load_error(inputs, component, ""), expected);
    }

    let fixture = Fixture::new();
    fixture.write_package(
        &export_with_inputs(r#"[{"name":"label","type":"string","default":"x"}]"#),
        "<p>static</p>",
        r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#,
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentInputNamespaceOutsideComponent
    );
}

#[test]
fn nested_components_resolve_only_their_own_literal_inputs() {
    let fixture = Fixture::new();
    fixture.write(
        "shell.json",
        r#"{
          "version":2,
          "package":{"id":"org.example.shell","kind":"shell","version":"1.0.0"},
          "dependencies":[],
          "components":[
            {"name":"parent-card","source":"components/all.html","inputs":[{"name":"label","type":"string","required":true}]},
            {"name":"child-card","source":"components/all.html","inputs":[{"name":"label","type":"string","required":true}]}
          ],
          "surfaces":[
            {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true},
            {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}
          ]
        }"#,
    );
    fixture.write(
        "components/all.html",
        concat!(
            r#"<template data-htm-component="parent-card">"#,
            r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#,
            r#"<htm-use component="child-card" input-label="child"></htm-use>"#,
            r#"</template>"#,
            r#"<template data-htm-component="child-card">"#,
            r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#,
            r#"</template>"#
        ),
    );
    let invocation = r#"<htm-use component="parent-card" input-label="parent"></htm-use>"#;
    fixture.write("index.html", invocation);
    fixture.write("panel.html", panel_document(invocation));
    fixture.write("overlay.html", overlay_document(invocation));
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let first = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        manifest.surface("panel").unwrap(),
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    let second = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        manifest.surface("panel").unwrap(),
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(
        first
            .component_instances()
            .iter()
            .map(|instance| instance.inputs().values()[0].value().canonical_string())
            .collect::<Vec<_>>(),
        ["parent", "child"]
    );
    assert_ne!(
        first.component_instances()[0].id(),
        first.component_instances()[1].id()
    );
    assert_ne!(
        first.component_instances()[0].id(),
        second.component_instances()[0].id()
    );
    assert_eq!(
        first.component_instances()[0].inputs().version(),
        second.component_instances()[0].inputs().version()
    );
}

#[test]
fn input_limits_are_enforced_at_exact_boundaries() {
    let declarations = (0..MAX_COMPONENT_INPUTS)
        .map(|index| format!(r#"{{"name":"value{index}","type":"string","default":""}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let supplied = (0..MAX_COMPONENT_INPUT_ATTRIBUTES)
        .map(|index| format!(r#" input-value{index}="""#))
        .collect::<String>();
    let valid = format!(r#"<htm-use component="status-card"{supplied}></htm-use>"#);
    let fixture = Fixture::new();
    fixture.write_package(
        &export_with_inputs(&format!("[{declarations}]")),
        "<p>static</p>",
        &valid,
    );
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();

    let over_declarations = format!(
        "[{},{}]",
        declarations, r#"{"name":"overflow","type":"string","default":""}"#
    );
    assert_eq!(
        load_error(&over_declarations, "<p>static</p>", ""),
        PackageErrorKind::ComponentInputCountLimit
    );
    let over_supplied =
        format!(r#"<htm-use component="status-card"{supplied} input-overflow=""></htm-use>"#);
    assert_eq!(
        load_error(
            &format!("[{declarations}]"),
            "<p>static</p>",
            &over_supplied
        ),
        PackageErrorKind::ComponentInputCountLimit
    );

    let string_boundary = "x".repeat(MAX_COMPONENT_INPUT_STRING_BYTES);
    let invocation = format!(
        r#"<htm-use component="status-card" input-label="{}"></htm-use>"#,
        string_boundary
    );
    let required = r#"[{"name":"label","type":"string","required":true}]"#;
    let fixture = Fixture::new();
    fixture.write_package(&export_with_inputs(required), "<p>static</p>", &invocation);
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();
    let over_string = "x".repeat(MAX_COMPONENT_INPUT_STRING_BYTES + 1);
    assert_eq!(
        load_error(
            required,
            "<p>static</p>",
            &format!(r#"<htm-use component="status-card" input-label="{over_string}"></htm-use>"#)
        ),
        PackageErrorKind::ComponentInputStringLimit
    );

    let bytes_inputs = r#"[{"name":"a","type":"string","required":true},{"name":"b","type":"string","required":true},{"name":"c","type":"string","required":true},{"name":"d","type":"string","required":true}]"#;
    let quarter = "x".repeat(MAX_COMPONENT_INPUT_LITERAL_BYTES / 4);
    let boundary = format!(
        r#"<htm-use component="status-card" input-a="{quarter}" input-b="{quarter}" input-c="{quarter}" input-d="{quarter}"></htm-use>"#
    );
    let fixture = Fixture::new();
    fixture.write_package(
        &export_with_inputs(bytes_inputs),
        "<p>static</p>",
        &boundary,
    );
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .is_ok()
    );
    assert_eq!(
        load_error(
            bytes_inputs,
            "<p>static</p>",
            &format!(
                r#"<htm-use component="status-card" input-a="{quarter}x" input-b="{quarter}" input-c="{quarter}" input-d="{quarter}"></htm-use>"#
            )
        ),
        PackageErrorKind::ComponentInputLiteralByteLimit
    );

    let consumers =
        r#"<span data-htm-element="state-text" data-htm-bind="input.value"></span>"#.repeat(5_000);
    let uses = r#"<htm-use component="status-card"></htm-use>"#.repeat(6);
    assert_eq!(
        load_error(
            r#"[{"name":"value","type":"string","default":"x"}]"#,
            &consumers,
            &uses
        ),
        PackageErrorKind::ComponentExpandedNodeLimit
    );
}

#[test]
fn failed_input_candidate_preserves_last_known_good() {
    let fixture = Fixture::new();
    let inputs = r#"[{"name":"count","type":"number","default":0}]"#;
    fixture.write_package(
        &export_with_inputs(inputs),
        r#"<data data-htm-element="state-value" data-htm-bind="input.count"></data>"#,
        r#"<htm-use component="status-card" input-count="1"></htm-use>"#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let first = loader.load_headless(&fixture.root).unwrap();
    fixture.write(
        "index.html",
        r#"<htm-use component="status-card" input-count="not-number"></htm-use>"#,
    );
    assert_eq!(
        loader
            .build_headless_candidate(&fixture.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::InvalidComponentInputLiteral
    );
    assert!(Arc::ptr_eq(loader.current().unwrap(), &first));
    assert_eq!(loader.current().unwrap().generation(), first.generation());
    fixture.write(
        "index.html",
        r#"<htm-use component="status-card" input-count="2"></htm-use>"#,
    );
    let second = loader.load_headless(&fixture.root).unwrap();
    assert_ne!(second.generation(), first.generation());
}

#[test]
fn input_component_pixels_match_handwritten_markup() {
    let component = Fixture::new();
    component.write_package(
        &export_with_inputs(r#"[{"name":"label","type":"string","required":true}]"#),
        r#"<section class="card"><span data-htm-element="state-text" data-htm-bind="input.label"></span></section>"#,
        r#"<htm-use component="status-card" input-label="Connected"></htm-use>"#,
    );
    component.write(
        "index.html",
        r#"<!doctype html><html><head><style>html,body{margin:0;background:#112;color:white}.card{width:120px;height:60px;padding:8px;background:#357}</style></head><body><htm-use component="status-card" input-label="Connected"></htm-use></body></html>"#,
    );

    let handwritten = Fixture::new();
    handwritten.write(
        "shell.json",
        r#"{"version":2,"package":{"id":"org.example.shell","kind":"shell","version":"1.0.0"},"dependencies":[],"components":[],"surfaces":[{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true},{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}]}"#,
    );
    handwritten.write(
        "index.html",
        r#"<!doctype html><html><head><style>html,body{margin:0;background:#112;color:white}.card{width:120px;height:60px;padding:8px;background:#357}</style></head><body><section class="card"><span>Connected</span></section></body></html>"#,
    );
    handwritten.write("panel.html", panel_document(""));
    handwritten.write("overlay.html", overlay_document(""));

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
    assert_eq!(component_run.component_input_consumers.len(), 1);
}

#[test]
fn headless_and_live_resolve_identical_input_semantics() {
    let fixture = Fixture::new();
    fixture.write_package(
        &export_with_inputs(
            r#"[{"name":"label","type":"string","required":true},{"name":"count","type":"number","default":0}]"#,
        ),
        concat!(
            r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#,
            r#"<data data-htm-element="state-value" data-htm-bind="input.count"></data>"#
        ),
        r#"<htm-use component="status-card" input-label="ready"></htm-use>"#,
    );
    let headless = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            viewport: ViewportSpec::default(),
            output_directory: None,
            run_interaction: false,
            render_png: false,
        },
    )
    .unwrap();
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        manifest.surface("panel").unwrap(),
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(
        headless.component_instances[0].inputs().version(),
        live.component_instances()[0].inputs().version()
    );
    assert_eq!(
        headless.component_instances[0]
            .inputs()
            .values()
            .iter()
            .map(|value| value.value().canonical_string())
            .collect::<Vec<_>>(),
        live.component_instances()[0]
            .inputs()
            .values()
            .iter()
            .map(|value| value.value().canonical_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        headless.component_input_consumers.len(),
        live.component_input_consumers().len()
    );
}

#[test]
fn output_local_input_maps_are_isolated_and_create_no_runtime_demand() {
    let fixture = Fixture::new();
    fixture.write_package(
        &export_with_inputs(r#"[{"name":"label","type":"string","default":"default value"}]"#),
        r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#,
        r#"<htm-use component="status-card"></htm-use>"#,
    );
    fixture.write(
        "overlay.html",
        overlay_document(
            r#"<htm-use component="status-card" input-label="different value"></htm-use>"#,
        ),
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

    let panel_a_input = panel_a.component_instances()[0].inputs();
    let panel_b_input = panel_b.component_instances()[0].inputs();
    let overlay_input = overlay.component_instances()[0].inputs();
    assert_eq!(panel_a_input.version(), panel_b_input.version());
    assert_ne!(panel_a_input.version(), overlay_input.version());
    assert!(!std::ptr::eq(
        panel_a_input.values(),
        panel_b_input.values()
    ));
    assert_ne!(
        panel_a.component_instances()[0].id(),
        panel_b.component_instances()[0].id()
    );
    for document in [&panel_a, &panel_b, &overlay] {
        assert_eq!(
            document.package_snapshot_generation(),
            Some(manifest.snapshot().generation())
        );
        assert_eq!(document.built_in_summary().registered_elements, 0);
        assert!(document.clock_declarations().is_empty());
        assert!(!document.pipewire_demand().service);
        assert_eq!(document.resource_request_count(), 0);
    }
}

#[test]
#[ignore = "release-only component input measurements and stress"]
fn component_input_release_measurement_and_stress_probe() {
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
                    .find(|line| line.starts_with("VmRSS:"))
                    .and_then(|line| line.split_whitespace().nth(1))
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

    fn input_fixture(input_count: usize, invocation_count: usize, distinct: bool) -> Fixture {
        let fixture = Fixture::new();
        let inputs = (0..input_count)
            .map(|index| {
                format!(r#"{{"name":"value{index}","type":"string","default":"default-{index}"}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        let invocations = (0..invocation_count)
            .map(|instance| {
                let supplied = if distinct {
                    format!(r#" input-value0="instance-{instance}""#)
                } else {
                    String::new()
                };
                format!(r#"<htm-use component="status-card"{supplied}></htm-use>"#)
            })
            .collect::<String>();
        fixture.write_package(
            &export_with_inputs(&format!("[{inputs}]")),
            r#"<span data-htm-element="state-text" data-htm-bind="input.value0"></span>"#,
            &invocations,
        );
        fixture
    }

    fn nested_fixture(depth: usize) -> Fixture {
        let fixture = Fixture::new();
        let exports = (0..depth)
            .map(|index| {
                format!(
                    r#"{{"name":"level-{index:02}","source":"components/all.html","inputs":[{{"name":"label","type":"string","default":"level-{index:02}"}}]}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let definitions = (0..depth)
            .map(|index| {
                let body = if index + 1 == depth {
                    r#"<span data-htm-element="state-text" data-htm-bind="input.label"></span>"#
                        .to_owned()
                } else {
                    format!(
                        r#"<htm-use component="level-{:02}" input-label="nested-{index:02}"></htm-use>"#,
                        index + 1
                    )
                };
                format!(
                    r#"<template data-htm-component="level-{index:02}">{body}</template>"#
                )
            })
            .collect::<String>();
        fixture.write(
            "shell.json",
            format!(
                r#"{{
                  "version":2,
                  "package":{{"id":"org.example.shell","kind":"shell","version":"1.0.0"}},
                  "dependencies":[],
                  "components":[{exports}],
                  "surfaces":[
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true}},
                    {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}}
                  ]
                }}"#
            ),
        );
        fixture.write("components/all.html", definitions);
        let invocation = r#"<htm-use component="level-00"></htm-use>"#;
        fixture.write("index.html", invocation);
        fixture.write("panel.html", panel_document(invocation));
        fixture.write("overlay.html", overlay_document(invocation));
        fixture
    }

    fn maximum_expansion_fixture() -> Fixture {
        let fixture = Fixture::new();
        let consumer = r#"<span data-htm-element="state-text" data-htm-bind="input.value"></span>"#;
        let consumers = consumer.repeat(4_164);
        let uses = r#"<htm-use component="status-card"></htm-use>"#.repeat(6);
        fixture.write_package(
            &export_with_inputs(r#"[{"name":"value","type":"string","default":"maximum"}]"#),
            &consumers,
            &uses,
        );
        fixture.write(
            "index.html",
            format!(
                "<!doctype html><html><body>{uses}{}</body></html>",
                "<i></i>".repeat(23)
            ),
        );
        fixture.write("panel.html", panel_document(&uses));
        fixture.write("overlay.html", overlay_document(&uses));
        fixture
    }

    let one = input_fixture(1, 1, false);
    let sixty_four = input_fixture(64, 1, false);
    let supplied_sixty_four = {
        let fixture = input_fixture(64, 1, false);
        let supplied = (0..64)
            .map(|index| format!(r#" input-value{index}="supplied-{index}""#))
            .collect::<String>();
        let invocation = format!(r#"<htm-use component="status-card"{supplied}></htm-use>"#);
        fixture.write("index.html", &invocation);
        fixture.write("panel.html", panel_document(&invocation));
        fixture.write("overlay.html", overlay_document(&invocation));
        fixture
    };
    let thousand_shared = input_fixture(1, 1_000, false);
    let thousand_distinct = input_fixture(1, 1_000, true);
    let nested = nested_fixture(32);
    let maximum = maximum_expansion_fixture();
    let invalid = input_fixture(1, 1, false);
    invalid.write(
        "index.html",
        r#"<htm-use component="status-card" input-value0="x" input-unknown="x"></htm-use>"#,
    );
    let missing = {
        let fixture = Fixture::new();
        fixture.write_package(
            &export_with_inputs(r#"[{"name":"value","type":"string","required":true}]"#),
            r#"<span data-htm-element="state-text" data-htm-bind="input.value"></span>"#,
            r#"<htm-use component="status-card"></htm-use>"#,
        );
        fixture
    };
    let mismatch = {
        let fixture = Fixture::new();
        fixture.write_package(
            &export_with_inputs(r#"[{"name":"value","type":"string","default":"x"}]"#),
            r#"<span data-htm-element="state-token" data-htm-bind="input.value"></span>"#,
            r#"<htm-use component="status-card"></htm-use>"#,
        );
        fixture
    };

    let before = process_observation();
    let (one_us, one_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&one.root)
            .unwrap()
    });
    let one_bytes = one_candidate.bytes_read();
    let mut publication_loader = PackageSnapshotLoader::new();
    let (publication_us, one_snapshot) =
        micros(|| publication_loader.publish(one_candidate).unwrap());
    let (serialization_us, diagnostic) = micros(|| one_snapshot.deterministic_json().unwrap());
    assert!(diagnostic.contains("\"semantic_version\""));
    let (sixty_four_default_us, _) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&sixty_four.root)
            .unwrap()
    });
    let (sixty_four_supplied_us, _) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&supplied_sixty_four.root)
            .unwrap()
    });
    let (thousand_shared_us, shared_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&thousand_shared.root)
            .unwrap()
    });
    let (thousand_distinct_us, distinct_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&thousand_distinct.root)
            .unwrap()
    });
    let (nested_us, nested_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&nested.root)
            .unwrap()
    });
    let (maximum_us, maximum_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&maximum.root)
            .unwrap()
    });
    let maximum_stats = maximum_snapshot
        .headless_entry()
        .unwrap()
        .prepared_document()
        .unwrap()
        .stats();
    assert_eq!(maximum_stats.expanded_nodes, 50_000);
    let (invalid_us, invalid_error) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&invalid.root)
            .unwrap_err()
    });
    assert_eq!(
        invalid_error.kind(),
        PackageErrorKind::ComponentInputUnknown
    );

    eprintln!("component-input-stress stage=one-input-builds count=1000");
    for _ in 0..1_000 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&one.root)
            .unwrap();
    }
    eprintln!("component-input-stress stage=multi-input-and-failures count=500");
    for _ in 0..500 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&sixty_four.root)
            .unwrap();
        assert_eq!(
            PackageSnapshotLoader::new()
                .build_headless_candidate(&missing.root)
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentInputMissingRequired
        );
        assert_eq!(
            PackageSnapshotLoader::new()
                .build_headless_candidate(&mismatch.root)
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentInputConsumerTypeMismatch
        );
    }
    eprintln!("component-input-stress stage=publications count=500");
    let mut replacement_loader = PackageSnapshotLoader::new();
    for _ in 0..500 {
        let candidate = replacement_loader
            .build_headless_candidate(&one.root)
            .unwrap();
        replacement_loader.publish(candidate).unwrap();
    }
    eprintln!("component-input-stress stage=instantiation-and-diagnostics count=500");
    let shared_manifest = ValidatedManifest::load(one.manifest()).unwrap();
    let shared_panel = shared_manifest.surface("panel").unwrap();
    for _ in 0..500 {
        LiveDocument::load_surface_snapshot(
            Arc::clone(shared_manifest.snapshot()),
            shared_panel,
            LiveDocumentKind::Panel,
            800,
            52,
        )
        .unwrap();
        nested_snapshot.deterministic_json().unwrap();
    }
    assert_eq!(
        shared_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        1_000
    );
    assert_eq!(
        distinct_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        1_000
    );
    assert_eq!(
        nested_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .maximum_nesting_depth,
        32
    );
    let after = process_observation();
    assert_eq!(after.1, before.1, "file descriptor count changed");
    assert_eq!(after.2, before.2, "thread count changed");

    eprintln!(
        "component-input-measurement one_us={one_us} one_bytes={one_bytes} inputs64_default_us={sixty_four_default_us} inputs64_supplied_us={sixty_four_supplied_us} instances1000_shared_us={thousand_shared_us} instances1000_distinct_us={thousand_distinct_us} nested32_us={nested_us} maximum50000_us={maximum_us} invalid_us={invalid_us} publication_us={publication_us} serialization_us={serialization_us} rss_before_kib={} rss_after_kib={} fd_before={} fd_after={} threads_before={} threads_after={}",
        before.0, after.0, before.1, after.1, before.2, after.2
    );
}
