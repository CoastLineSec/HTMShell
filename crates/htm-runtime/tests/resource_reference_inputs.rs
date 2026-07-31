use htm_runtime::{
    ComponentInputType, ComponentResourceKind, ComponentResourceKindSet, LiveDocument,
    LiveDocumentKind, MAX_COMPONENT_INPUTS, MAX_RESOURCE_REFERENCE_VALUES_PER_PREPARED_ROOT,
    PackageErrorKind, PackageSnapshotLoader,
};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::collections::BTreeSet;
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
            "htmshell-resource-reference-input-test-{}-{serial}",
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
        panel_resources: &str,
        overlay_resources: &str,
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
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":96,"reserveSpace":true,"resources":{panel_resources}}},
                    {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false,"resources":{overlay_resources}}}
                  ]
                }}"#
            ),
        );
        self.write(
            "index.html",
            "<!doctype html><html><body><p>Headless root uses no surface catalog.</p></body></html>",
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

fn png() -> Vec<u8> {
    let pixels = [
        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255,
    ];
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&pixels, 2, 2, ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

fn svg() -> &'static str {
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 3"><rect x="0" y="0" width="4" height="3" fill="#55aaff"/></svg>"##
}

fn resource_input(name: &str, kinds: &str) -> String {
    format!(
        r#"{{"name":"{name}","type":"resource-reference","resourceTypes":{kinds},"required":true}}"#
    )
}

fn export(name: &str, inputs: &str, resources: &str) -> String {
    format!(
        r#"{{"name":"{name}","source":"components/components.html","inputs":{inputs},"slots":[],"styles":[],"resources":{resources}}}"#
    )
}

fn load_error(fixture: &Fixture) -> PackageErrorKind {
    PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap_err()
        .kind()
}

fn forwarding_chain_fixture(depth: usize, source_kind: &str) -> Fixture {
    assert!(depth > 0);
    let fixture = Fixture::new();
    let (source_path, source_bytes): (&str, Vec<u8>) = match source_kind {
        "raster" => ("assets/icon.png", png()),
        "svg" => ("assets/icon.svg", svg().as_bytes().to_vec()),
        _ => panic!("unsupported forwarding fixture resource kind"),
    };
    fixture.write(source_path, source_bytes);
    let mut exports = Vec::with_capacity(depth);
    let mut definitions = String::new();
    for index in 0..depth {
        let name = format!("forward-{index}");
        exports.push(export(
            &name,
            &format!(
                "[{}]",
                resource_input("icon", &format!(r#"["{source_kind}"]"#))
            ),
            "[]",
        ));
        if index + 1 == depth {
            definitions.push_str(&format!(
                r#"<template data-htm-component="{name}"><img src="input:icon" alt=""></template>"#
            ));
        } else {
            definitions.push_str(&format!(
                r#"<template data-htm-component="{name}"><htm-use component="forward-{}" input-icon="input:icon"></htm-use></template>"#,
                index + 1
            ));
        }
    }
    fixture.package(
        &format!("[{}]", exports.join(",")),
        &definitions,
        &format!(r#"[{{"name":"icon","type":"{source_kind}","source":"{source_path}"}}]"#),
        "[]",
        r#"<htm-use component="forward-0" input-icon="resource:icon"></htm-use>"#,
    );
    fixture
}

fn many_value_fixture(instance_count: usize) -> Fixture {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    let inputs = (0..MAX_COMPONENT_INPUTS)
        .map(|index| resource_input(&format!("icon-{index}"), r#"["raster"]"#))
        .collect::<Vec<_>>()
        .join(",");
    let assignments = (0..MAX_COMPONENT_INPUTS)
        .map(|index| format!(r#" input-icon-{index}="resource:icon""#))
        .collect::<String>();
    let uses = (0..instance_count)
        .map(|_| format!(r#"<htm-use component="many-inputs"{assignments}></htm-use>"#))
        .collect::<String>();
    fixture.package(
        &format!("[{}]", export("many-inputs", &format!("[{inputs}]"), "[]")),
        r#"<template data-htm-component="many-inputs"><img src="input:icon-0" alt=""></template>"#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        &uses,
    );
    fixture
}

fn direct_surface_fixture(kind: &str, instances: usize) -> Fixture {
    let fixture = Fixture::new();
    let (source_path, source_bytes): (&str, Vec<u8>) = match kind {
        "raster" => ("assets/icon.png", png()),
        "svg" => ("assets/icon.svg", svg().as_bytes().to_vec()),
        _ => panic!("unsupported direct surface fixture resource kind"),
    };
    fixture.write(source_path, source_bytes);
    let inputs = format!("[{}]", resource_input("icon", &format!(r#"["{kind}"]"#)));
    fixture.package(
        &format!("[{}]", export("image-leaf", &inputs, "[]")),
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        &format!(r#"[{{"name":"icon","type":"{kind}","source":"{source_path}"}}]"#),
        "[]",
        &r#"<htm-use component="image-leaf" input-icon="resource:icon"></htm-use>"#
            .repeat(instances),
    );
    fixture
}

fn component_direct_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    let inputs = format!("[{}]", resource_input("icon", r#"["raster"]"#));
    fixture.package(
        &format!(
            "[{},{}]",
            export("image-leaf", &inputs, "[]"),
            export(
                "image-owner",
                "[]",
                r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#
            )
        ),
        r#"
          <template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>
          <template data-htm-component="image-owner"><htm-use component="image-leaf" input-icon="resource:icon"></htm-use></template>
        "#,
        "[]",
        "[]",
        r#"<htm-use component="image-owner"></htm-use>"#,
    );
    fixture
}

#[test]
fn resource_reference_declaration_kind_sets_are_finite_and_canonical() {
    assert_eq!(
        ComponentInputType::parse("resource-reference").unwrap(),
        ComponentInputType::ResourceReference
    );
    let raster_svg =
        ComponentResourceKindSet::new([ComponentResourceKind::Raster, ComponentResourceKind::Svg]);
    let svg_raster =
        ComponentResourceKindSet::new([ComponentResourceKind::Svg, ComponentResourceKind::Raster]);
    assert_eq!(raster_svg, svg_raster);
    assert_eq!(raster_svg.canonical_string(), "raster,svg");

    let cases = [
        (
            r#"[{"name":"icon","type":"resource-reference","required":true}]"#,
            PackageErrorKind::ComponentResourceReferenceKindsMissing,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":[],"required":true}]"#,
            PackageErrorKind::ComponentResourceReferenceKindsMissing,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":["raster","raster"],"required":true}]"#,
            PackageErrorKind::ComponentResourceReferenceKindDuplicate,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":["font"],"required":true}]"#,
            PackageErrorKind::ComponentResourceReferenceKindUnsupported,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":["raster"]}]"#,
            PackageErrorKind::ComponentResourceReferenceRequiredFlagInvalid,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":["raster"],"required":false}]"#,
            PackageErrorKind::ComponentResourceReferenceRequiredFlagInvalid,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":["raster"],"required":true,"default":"resource:icon"}]"#,
            PackageErrorKind::ComponentResourceReferenceDefaultForbidden,
        ),
        (
            r#"[{"name":"icon","type":"resource-reference","resourceTypes":["raster"],"required":true,"optional":false}]"#,
            PackageErrorKind::UnknownField,
        ),
    ];
    for (inputs, expected) in cases {
        let fixture = Fixture::new();
        fixture.package(
            &format!("[{}]", export("icon-card", inputs, "[]")),
            r#"<template data-htm-component="icon-card"><img src="input:icon" alt=""></template>"#,
            "[]",
            "[]",
            r#"<htm-use component="icon-card"></htm-use>"#,
        );
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn surface_direct_component_direct_and_forwarded_resources_share_sources() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    fixture.write("assets/icon.svg", svg());
    let image_input = format!("[{}]", resource_input("icon", r#"["raster","svg"]"#));
    let fallback_export = format!(
        r#"{{"name":"fallback-image","source":"components/components.html","inputs":{image_input},"slots":[{{"name":"default","required":false}}],"styles":[],"resources":[]}}"#
    );
    let components = format!(
        "[{},{},{},{}]",
        export("image-leaf", &image_input, "[]"),
        export("image-forwarder", &image_input, "[]"),
        export(
            "owned-image",
            "[]",
            r#"[{"name":"owned","type":"raster","source":"assets/icon.png"}]"#
        ),
        fallback_export
    );
    let definitions = r#"
      <template data-htm-component="image-leaf"><img class="leaf" src="input:icon" alt=""></template>
      <template data-htm-component="image-forwarder"><htm-use component="image-leaf" input-icon="input:icon"></htm-use></template>
      <template data-htm-component="owned-image"><htm-use component="image-leaf" input-icon="resource:owned"></htm-use></template>
      <template data-htm-component="fallback-image"><slot><img class="fallback" src="input:icon" alt=""></slot></template>
    "#;
    let surface_resources = r#"[
      {"name":"surface-raster","type":"raster","source":"assets/icon.png"},
      {"name":"surface-svg","type":"svg","source":"assets/icon.svg"}
    ]"#;
    fixture.package(
        &components,
        definitions,
        surface_resources,
        r#"[{"name":"surface-raster","type":"raster","source":"assets/icon.png"}]"#,
        r#"
          <htm-use component="image-forwarder" input-icon="resource:surface-raster"></htm-use>
          <htm-use component="image-leaf" input-icon="resource:surface-svg"></htm-use>
          <htm-use component="owned-image"></htm-use>
          <htm-use component="fallback-image" input-icon="resource:surface-raster"></htm-use>
        "#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    assert_eq!(snapshot.component_resources().sources().len(), 2);
    assert_eq!(snapshot.component_resources().totals().source_read_count, 2);
    assert_eq!(
        snapshot.component_resources().totals().source_decode_count,
        1
    );
    assert_eq!(
        snapshot.component_resources().totals().source_parse_count,
        1
    );
    assert_eq!(
        snapshot.component_resources().surface_associations().len(),
        3
    );
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
            .resource_reference_values,
        5
    );
    let first = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        160,
        96,
    )
    .unwrap();
    let second = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        160,
        96,
    )
    .unwrap();
    assert_eq!(first.component_resource_usages().len(), 4);
    assert!(
        first
            .component_resource_usages()
            .iter()
            .all(|usage| usage.input_value_id().is_some())
    );
    assert!(
        first
            .component_resource_usages()
            .iter()
            .any(|usage| usage.origin().owner_diagnostic().contains("surface:"))
    );
    assert!(
        first
            .component_resource_usages()
            .iter()
            .any(|usage| usage.origin().owner_diagnostic().contains("component:"))
    );
    assert!(Arc::ptr_eq(
        first.component_resource_usages()[0].source(),
        second.component_resource_usages()[0].source()
    ));
    assert_ne!(
        first.component_resource_usages()[0]
            .input_value_id()
            .unwrap(),
        second.component_resource_usages()[0]
            .input_value_id()
            .unwrap()
    );
    let value_ids = first
        .component_instances()
        .iter()
        .flat_map(|instance| instance.inputs().values())
        .filter_map(|input| match input.value() {
            htm_runtime::ComponentInputValue::ResourceReference(value) => {
                Some(value.deterministic_id())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(value_ids.len(), 5);
    assert_eq!(
        first
            .component_resource_usages()
            .iter()
            .map(|usage| usage.id().deterministic_string())
            .collect::<BTreeSet<_>>()
            .len(),
        first.component_resource_usages().len()
    );
    let diagnostic = snapshot.deterministic_json().unwrap();
    assert!(diagnostic.contains("\"forwarding\""));
    assert!(diagnostic.contains("resource-input-forward:"));
    assert!(diagnostic.contains("\"resource_reference_values\": 5"));
}

#[test]
fn required_assignments_kinds_forwarding_consumers_and_surface_scopes_reject() {
    let cases = [
        (
            r#"<htm-use component="image-leaf"></htm-use>"#,
            r#"["raster","svg"]"#,
            r#"<img src="input:icon" alt="">"#,
            PackageErrorKind::ComponentResourceReferenceAssignmentMissing,
        ),
        (
            r#"<htm-use component="image-leaf" input-icon="resource:missing"></htm-use>"#,
            r#"["raster","svg"]"#,
            r#"<img src="input:icon" alt="">"#,
            PackageErrorKind::ComponentResourceReferenceDirectResourceUnknown,
        ),
        (
            r#"<htm-use component="image-leaf" input-icon="resource:surface-svg"></htm-use>"#,
            r#"["raster"]"#,
            r#"<img src="input:icon" alt="">"#,
            PackageErrorKind::ComponentResourceReferenceDirectResourceWrongKind,
        ),
        (
            r#"<htm-use component="image-leaf" input-icon="resource:surface-raster"></htm-use>"#,
            r#"["raster","svg"]"#,
            r#"<img src="input:unknown" alt="">"#,
            PackageErrorKind::ComponentResourceReferenceConsumerUnknown,
        ),
        (
            r#"<img src="resource:surface-raster" alt="">"#,
            r#"["raster","svg"]"#,
            r#"<img src="input:icon" alt="">"#,
            PackageErrorKind::ComponentResourceReferenceWrongOwner,
        ),
    ];
    for (body, kinds, definition_body, expected) in cases {
        let fixture = Fixture::new();
        fixture.write("assets/icon.png", png());
        fixture.write("assets/icon.svg", svg());
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "image-leaf",
                    &format!("[{}]", resource_input("icon", kinds)),
                    "[]"
                )
            ),
            &format!(r#"<template data-htm-component="image-leaf">{definition_body}</template>"#),
            r#"[
              {"name":"surface-raster","type":"raster","source":"assets/icon.png"},
              {"name":"surface-svg","type":"svg","source":"assets/icon.svg"}
            ]"#,
            "[]",
            body,
        );
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn assignment_and_consumer_syntax_is_exact() {
    for assignment in [
        "",
        "resource:",
        "resource:icon/path",
        "resource:icon?size=2",
        "resource:icon#part",
        "resource:%69con",
        "Resource:icon",
        "input:",
        "input:icon/path",
        "input:icon?size=2",
        "input:icon#part",
        "input:%69con",
        "Input:icon",
        "assets/icon.png",
    ] {
        let fixture = Fixture::new();
        fixture.write("assets/icon.png", png());
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "image-leaf",
                    &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                    "[]"
                )
            ),
            r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
            r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
            "[]",
            &format!(r#"<htm-use component="image-leaf" input-icon="{assignment}"></htm-use>"#),
        );
        assert_eq!(
            load_error(&fixture),
            PackageErrorKind::ComponentResourceReferenceAssignmentMalformed,
            "assignment `{assignment}`"
        );
    }

    for consumer in [
        "input:",
        "input:icon/path",
        "input:icon?size=2",
        "input:icon#part",
        "input:%69con",
    ] {
        let fixture = Fixture::new();
        fixture.write("assets/icon.png", png());
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "image-leaf",
                    &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                    "[]"
                )
            ),
            &format!(
                r#"<template data-htm-component="image-leaf"><img src="{consumer}" alt=""></template>"#
            ),
            r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
            "[]",
            r#"<htm-use component="image-leaf" input-icon="resource:icon"></htm-use>"#,
        );
        assert_eq!(
            load_error(&fixture),
            PackageErrorKind::ComponentResourceReferenceConsumerMalformed,
            "consumer `{consumer}`"
        );
    }
}

#[test]
fn surface_catalog_declaration_boundaries_are_exact() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    let resources = (0..32)
        .map(|index| {
            format!(r#"{{"name":"icon-{index}","type":"raster","source":"assets/icon.png"}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    fixture.package("[]", "", &format!("[{resources}]"), "[]", "");
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    assert_eq!(
        snapshot.component_resources().surface_associations().len(),
        32
    );
    assert_eq!(snapshot.component_resources().sources().len(), 1);
    assert_eq!(snapshot.component_resources().totals().source_read_count, 1);
    assert_eq!(
        snapshot.component_resources().totals().source_decode_count,
        1
    );

    let overflow = Fixture::new();
    overflow.write("assets/icon.png", png());
    let resources = (0..33)
        .map(|index| {
            format!(r#"{{"name":"icon-{index}","type":"raster","source":"assets/icon.png"}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    overflow.package("[]", "", &format!("[{resources}]"), "[]", "");
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::SurfaceResourceDeclarationLimit
    );

    let duplicate = Fixture::new();
    duplicate.write("assets/icon.png", png());
    duplicate.package(
        "[]",
        "",
        r#"[
          {"name":"icon","type":"raster","source":"assets/icon.png"},
          {"name":"icon","type":"raster","source":"assets/icon.png"}
        ]"#,
        "[]",
        "",
    );
    assert_eq!(
        load_error(&duplicate),
        PackageErrorKind::DuplicateSurfaceResourceName
    );
}

#[test]
fn literal_inputs_and_owner_scopes_do_not_acquire_resource_semantics() {
    let literal = Fixture::new();
    literal.package(
        &format!(
            "[{}]",
            export(
                "literal-card",
                r#"[{"name":"label","type":"string","required":true}]"#,
                "[]"
            )
        ),
        r#"<template data-htm-component="literal-card"><span data-htm-element="state-text" data-htm-bind="input.label"></span></template>"#,
        "[]",
        "[]",
        r#"<htm-use component="literal-card" input-label="resource:ordinary-string"></htm-use>"#,
    );
    PackageSnapshotLoader::new()
        .load_manifest(literal.root.join("shell.json"))
        .unwrap();

    let wrong_consumer = Fixture::new();
    wrong_consumer.package(
        &format!(
            "[{}]",
            export(
                "literal-card",
                r#"[{"name":"label","type":"string","required":true}]"#,
                "[]"
            )
        ),
        r#"<template data-htm-component="literal-card"><img src="input:label" alt=""></template>"#,
        "[]",
        "[]",
        r#"<htm-use component="literal-card" input-label="ordinary"></htm-use>"#,
    );
    assert_eq!(
        load_error(&wrong_consumer),
        PackageErrorKind::ComponentResourceReferenceConsumerWrongType
    );

    let cross_surface = Fixture::new();
    cross_surface.write("assets/icon.png", png());
    cross_surface.package(
        &format!(
            "[{}]",
            export(
                "image-leaf",
                &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                "[]"
            )
        ),
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        "[]",
        r#"[{"name":"overlay-icon","type":"raster","source":"assets/icon.png"}]"#,
        r#"<htm-use component="image-leaf" input-icon="resource:overlay-icon"></htm-use>"#,
    );
    assert_eq!(
        load_error(&cross_surface),
        PackageErrorKind::ComponentResourceReferenceDirectResourceUnknown
    );

    let root_consumer = Fixture::new();
    root_consumer.package("[]", "", "[]", "[]", r#"<img src="input:icon" alt="">"#);
    assert_eq!(
        load_error(&root_consumer),
        PackageErrorKind::ComponentResourceReferenceConsumerWrongType
    );
}

#[test]
fn forwarding_requires_a_safe_kind_subset() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    let parent_input = format!("[{}]", resource_input("icon", r#"["raster","svg"]"#));
    let child_input = format!("[{}]", resource_input("icon", r#"["raster"]"#));
    fixture.package(
        &format!(
            "[{},{}]",
            export("parent-card", &parent_input, "[]"),
            export("child-card", &child_input, "[]")
        ),
        r#"
          <template data-htm-component="parent-card"><htm-use component="child-card" input-icon="input:icon"></htm-use></template>
          <template data-htm-component="child-card"><img src="input:icon" alt=""></template>
        "#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        r#"<htm-use component="parent-card" input-icon="resource:icon"></htm-use>"#,
    );
    assert_eq!(
        load_error(&fixture),
        PackageErrorKind::ComponentResourceReferenceForwardingKindsIncompatible
    );
}

#[test]
fn value_limit_is_exact_and_checked_before_publication() {
    let exact =
        many_value_fixture(MAX_RESOURCE_REFERENCE_VALUES_PER_PREPARED_ROOT / MAX_COMPONENT_INPUTS);
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(exact.root.join("shell.json"))
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
            .resource_reference_values,
        MAX_RESOURCE_REFERENCE_VALUES_PER_PREPARED_ROOT
    );

    let overflow = many_value_fixture(
        MAX_RESOURCE_REFERENCE_VALUES_PER_PREPARED_ROOT / MAX_COMPONENT_INPUTS + 1,
    );
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentResourceReferenceValueLimit
    );
}

#[test]
fn forwarding_uses_the_existing_exact_nesting_boundary() {
    let exact = forwarding_chain_fixture(32, "svg");
    let snapshot = PackageSnapshotLoader::new()
        .load_manifest(exact.root.join("shell.json"))
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
            .resource_reference_values,
        32
    );
    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(&snapshot),
        panel,
        LiveDocumentKind::Panel,
        160,
        96,
    )
    .unwrap();
    assert_eq!(live.component_instances().len(), 32);
    assert_eq!(live.component_resource_usages().len(), 1);
    assert_eq!(
        live.component_resource_usages()[0].source().kind(),
        ComponentResourceKind::Svg
    );

    let overflow = forwarding_chain_fixture(33, "svg");
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentNestingLimit
    );
}

#[test]
fn failed_resource_input_candidate_retains_last_known_good() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    let components = format!(
        "[{}]",
        export(
            "image-leaf",
            &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
            "[]"
        )
    );
    fixture.package(
        &components,
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        r#"<htm-use component="image-leaf" input-icon="resource:icon"></htm-use>"#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let first = loader
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let generation = first.generation();
    fixture.package(
        &components,
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        r#"<htm-use component="image-leaf" input-icon="resource:missing"></htm-use>"#,
    );
    assert_eq!(
        loader
            .load_manifest(fixture.root.join("shell.json"))
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentResourceReferenceDirectResourceUnknown
    );
    assert_eq!(loader.current().unwrap().generation(), generation);
    assert!(Arc::ptr_eq(loader.current().unwrap(), &first));
}

#[test]
fn resource_reference_identities_are_snapshot_and_output_local() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    fixture.package(
        &format!(
            "[{}]",
            export(
                "image-leaf",
                &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                "[]"
            )
        ),
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        r#"
          <htm-use component="image-leaf" input-icon="resource:icon"></htm-use>
          <htm-use component="image-leaf" input-icon="resource:icon"></htm-use>
        "#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let first_snapshot = loader
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let first_panel = first_snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let first = LiveDocument::load_surface_snapshot(
        Arc::clone(&first_snapshot),
        first_panel,
        LiveDocumentKind::Panel,
        160,
        96,
    )
    .unwrap();
    assert_eq!(first.component_resource_usages().len(), 2);
    assert!(Arc::ptr_eq(
        first.component_resource_usages()[0].source(),
        first.component_resource_usages()[1].source()
    ));
    assert_ne!(
        first.component_resource_usages()[0]
            .input_value_id()
            .unwrap(),
        first.component_resource_usages()[1]
            .input_value_id()
            .unwrap()
    );
    assert_ne!(
        first.component_resource_usages()[0]
            .id()
            .deterministic_string(),
        first.component_resource_usages()[1]
            .id()
            .deterministic_string()
    );

    let second_snapshot = loader
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let second_panel = second_snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let second = LiveDocument::load_surface_snapshot(
        Arc::clone(&second_snapshot),
        second_panel,
        LiveDocumentKind::Panel,
        160,
        96,
    )
    .unwrap();
    assert_ne!(first_snapshot.generation(), second_snapshot.generation());
    assert_eq!(
        first.component_resource_usages()[0]
            .source()
            .semantic_version(),
        second.component_resource_usages()[0]
            .source()
            .semantic_version()
    );
    assert_ne!(
        first.component_resource_usages()[0]
            .source()
            .id()
            .deterministic_string(first_snapshot.generation()),
        second.component_resource_usages()[0]
            .source()
            .id()
            .deterministic_string(second_snapshot.generation())
    );
    assert_ne!(
        first.component_resource_usages()[0]
            .input_value_id()
            .unwrap(),
        second.component_resource_usages()[0]
            .input_value_id()
            .unwrap()
    );
}

#[test]
fn schema_v1_and_manifestless_roots_cannot_add_resource_input_authority() {
    let schema_v1 = Fixture::new();
    schema_v1.write(
        "shell.json",
        r#"{
          "version":1,
          "id":"legacy-shell",
          "components":[{
            "name":"legacy-card",
            "source":"components/components.html",
            "inputs":[{
              "name":"icon",
              "type":"resource-reference",
              "resourceTypes":["raster"],
              "required":true
            }]
          }],
          "surfaces":[]
        }"#,
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_manifest(schema_v1.root.join("shell.json"))
            .unwrap_err()
            .kind(),
        PackageErrorKind::UnknownField
    );

    let legacy = Fixture::new();
    legacy.write(
        "index.html",
        r#"<!doctype html><html><body><img src="input:icon" alt=""></body></html>"#,
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&legacy.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentResourceReferenceConsumerWrongType
    );
}

#[test]
#[ignore = "release-only resource-reference input measurements and bounded stress"]
fn resource_reference_release_measurement_and_stress_probe() {
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

    fn snapshot(fixture: &Fixture) -> Arc<htm_runtime::PackageSnapshot> {
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

    let before = process_counts();
    let raster = direct_surface_fixture("raster", 1);
    let (surface_raster_us, raster_snapshot) = micros(|| snapshot(&raster));
    let svg_fixture = direct_surface_fixture("svg", 1);
    let (surface_svg_us, svg_snapshot) = micros(|| snapshot(&svg_fixture));
    let component = component_direct_fixture();
    let (component_direct_us, component_snapshot) = micros(|| snapshot(&component));
    let one_hop = forwarding_chain_fixture(2, "raster");
    let (forwarding_one_us, forwarding_one_snapshot) = micros(|| snapshot(&one_hop));
    let depth_32 = forwarding_chain_fixture(32, "svg");
    let (forwarding_depth_32_us, forwarding_depth_32_snapshot) = micros(|| snapshot(&depth_32));
    let sixty_four = many_value_fixture(1);
    let (sixty_four_inputs_us, sixty_four_snapshot) = micros(|| snapshot(&sixty_four));
    let thousand = direct_surface_fixture("raster", 1_000);
    let (thousand_instances_us, thousand_snapshot) = micros(|| snapshot(&thousand));
    let maximum =
        many_value_fixture(MAX_RESOURCE_REFERENCE_VALUES_PER_PREPARED_ROOT / MAX_COMPONENT_INPUTS);
    let (maximum_values_us, maximum_snapshot) = micros(|| snapshot(&maximum));

    let loader = PackageSnapshotLoader::new();
    let (candidate_us, candidate) = micros(|| {
        loader
            .build_manifest_candidate(raster.root.join("shell.json"))
            .unwrap()
    });
    let mut publication_loader = PackageSnapshotLoader::new();
    let (publication_us, published) = micros(|| publication_loader.publish(candidate).unwrap());
    let (diagnostic_us, diagnostic) = micros(|| published.deterministic_json().unwrap());
    assert!(!diagnostic.is_empty());

    let panel = raster_snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let (three_outputs_us, outputs) = micros(|| {
        (0..3)
            .map(|_| {
                LiveDocument::load_surface_snapshot(
                    Arc::clone(&raster_snapshot),
                    panel,
                    LiveDocumentKind::Panel,
                    192,
                    96,
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(outputs.len(), 3);
    assert!(Arc::ptr_eq(
        outputs[0].component_resource_usages()[0].source(),
        outputs[1].component_resource_usages()[0].source()
    ));

    let missing = Fixture::new();
    missing.write("assets/icon.png", png());
    missing.package(
        &format!(
            "[{}]",
            export(
                "image-leaf",
                &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                "[]"
            )
        ),
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        r#"<htm-use component="image-leaf"></htm-use>"#,
    );
    let wrong_kind = Fixture::new();
    wrong_kind.write("assets/icon.svg", svg());
    wrong_kind.package(
        &format!(
            "[{}]",
            export(
                "image-leaf",
                &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                "[]"
            )
        ),
        r#"<template data-htm-component="image-leaf"><img src="input:icon" alt=""></template>"#,
        r#"[{"name":"icon","type":"svg","source":"assets/icon.svg"}]"#,
        "[]",
        r#"<htm-use component="image-leaf" input-icon="resource:icon"></htm-use>"#,
    );
    let invalid_forwarding = Fixture::new();
    invalid_forwarding.write("assets/icon.png", png());
    invalid_forwarding.package(
        &format!(
            "[{},{}]",
            export(
                "forward-parent",
                r#"[{"name":"icon","type":"string","required":true}]"#,
                "[]"
            ),
            export(
                "forward-child",
                &format!("[{}]", resource_input("icon", r#"["raster"]"#)),
                "[]"
            )
        ),
        r#"
          <template data-htm-component="forward-parent"><htm-use component="forward-child" input-icon="input:icon"></htm-use></template>
          <template data-htm-component="forward-child"><img src="input:icon" alt=""></template>
        "#,
        r#"[{"name":"icon","type":"raster","source":"assets/icon.png"}]"#,
        "[]",
        r#"<htm-use component="forward-parent" input-icon="literal"></htm-use>"#,
    );
    let bad_surface = Fixture::new();
    bad_surface.package(
        "[]",
        "",
        r#"[{"name":"icon","type":"raster","source":"assets/missing.png"}]"#,
        "[]",
        "",
    );

    let (valid_direct_1000_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..1_000 {
            loader
                .build_manifest_candidate(component.root.join("shell.json"))
                .unwrap();
        }
    });
    let (surface_direct_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            loader
                .build_manifest_candidate(raster.root.join("shell.json"))
                .unwrap();
        }
    });
    let (forwarding_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            loader
                .build_manifest_candidate(one_hop.root.join("shell.json"))
                .unwrap();
        }
    });
    let (missing_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(missing.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::ComponentResourceReferenceAssignmentMissing
            );
        }
    });
    let (wrong_kind_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(wrong_kind.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::ComponentResourceReferenceDirectResourceWrongKind
            );
        }
    });
    let (invalid_forwarding_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert_eq!(
                loader
                    .build_manifest_candidate(invalid_forwarding.root.join("shell.json"))
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::ComponentResourceReferenceForwardingSourceWrongType
            );
        }
    });
    let (surface_failures_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert!(
                loader
                    .build_manifest_candidate(bad_surface.root.join("shell.json"))
                    .is_err()
            );
        }
    });
    let (publications_500_us, ()) = micros(|| {
        let mut loader = PackageSnapshotLoader::new();
        for generation in 1..=500 {
            let snapshot = loader
                .load_manifest(raster.root.join("shell.json"))
                .unwrap();
            assert_eq!(snapshot.generation().get(), generation);
        }
    });
    let (multi_output_500_us, ()) = micros(|| {
        for _ in 0..500 {
            for _ in 0..3 {
                let live = LiveDocument::load_surface_snapshot(
                    Arc::clone(&raster_snapshot),
                    panel,
                    LiveDocumentKind::Panel,
                    192,
                    96,
                )
                .unwrap();
                assert_eq!(live.component_resource_usages().len(), 1);
            }
        }
    });

    assert_eq!(
        raster_snapshot
            .component_resources()
            .totals()
            .source_decode_count,
        1
    );
    assert_eq!(
        svg_snapshot
            .component_resources()
            .totals()
            .source_parse_count,
        1
    );
    assert_eq!(
        component_snapshot
            .component_resources()
            .totals()
            .source_decode_count,
        1
    );
    assert_eq!(
        panel_stats(&forwarding_one_snapshot).resource_reference_values,
        2
    );
    assert_eq!(
        panel_stats(&forwarding_depth_32_snapshot).resource_reference_values,
        32
    );
    assert_eq!(
        panel_stats(&sixty_four_snapshot).resource_reference_values,
        64
    );
    assert_eq!(panel_stats(&thousand_snapshot).component_instances, 1_000);
    assert_eq!(
        panel_stats(&maximum_snapshot).resource_reference_values,
        MAX_RESOURCE_REFERENCE_VALUES_PER_PREPARED_ROOT
    );
    let after = process_counts();
    eprintln!(
        "resource_reference_measurements_us surface_raster={surface_raster_us} surface_svg={surface_svg_us} component_direct={component_direct_us} forwarding_one={forwarding_one_us} forwarding_depth_32={forwarding_depth_32_us} inputs_64={sixty_four_inputs_us} instances_1000={thousand_instances_us} values_16384={maximum_values_us} candidate={candidate_us} publication={publication_us} diagnostic={diagnostic_us} three_outputs={three_outputs_us} valid_direct_1000={valid_direct_1000_us} surface_direct_500={surface_direct_500_us} forwarding_500={forwarding_500_us} missing_500={missing_500_us} wrong_kind_500={wrong_kind_500_us} invalid_forwarding_500={invalid_forwarding_500_us} surface_failures_500={surface_failures_500_us} publications_500={publications_500_us} multi_output_500={multi_output_500_us} source_reads={} raster_decodes={} svg_parses={} values={} usages={} before_fd={} after_fd={} before_threads={} after_threads={} before_rss_kib={:?} after_rss_kib={:?}",
        raster_snapshot
            .component_resources()
            .totals()
            .source_read_count,
        raster_snapshot
            .component_resources()
            .totals()
            .source_decode_count,
        svg_snapshot
            .component_resources()
            .totals()
            .source_parse_count,
        panel_stats(&maximum_snapshot).resource_reference_values,
        outputs
            .iter()
            .map(|output| output.component_resource_usages().len())
            .sum::<usize>(),
        before.0,
        after.0,
        before.1,
        after.1,
        before.2,
        after.2
    );
    assert!(
        after.0 <= before.0.saturating_add(4),
        "file descriptor growth exceeded allowance: {before:?} -> {after:?}"
    );
    assert!(
        after.1 <= before.1.saturating_add(1),
        "thread growth exceeded allowance: {before:?} -> {after:?}"
    );
}
