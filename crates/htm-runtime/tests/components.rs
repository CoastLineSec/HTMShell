use htm_runtime::{
    ComponentName, ComponentReference, ExperimentOptions, LiveDocument, LiveDocumentKind,
    MAX_COMPONENT_EXPANDED_NODES, MAX_COMPONENT_EXPORTS_PER_GRAPH,
    MAX_COMPONENT_EXPORTS_PER_PACKAGE, MAX_COMPONENT_INSTANCES_PER_DOCUMENT,
    MAX_COMPONENT_NESTING_DEPTH, MAX_COMPONENT_REFERENCES_PER_DOCUMENT, MAX_COMPONENT_SOURCE_BYTES,
    MAX_COMPONENT_SOURCE_NODES, PackageErrorKind, PackageSnapshotLoader, ValidatedManifest,
    ViewportSpec, run_package_with_options,
};
use std::fs;
use std::path::{Path, PathBuf};
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
            "htmshell-component-test-{}-{serial}",
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

    fn write_shell(&self, dependencies: &str, components: &str, index: &str) {
        self.write(
            "shell.json",
            schema_v2_shell("org.example.shell", dependencies, components),
        );
        self.write("index.html", index);
        self.write("panel.html", panel_document(""));
        self.write("overlay.html", overlay_document(""));
    }

    fn write_library(&self, relative: &str, id: &str, dependencies: &str, components: &str) {
        self.write(
            &format!("{relative}/shell.json"),
            schema_v2_library(id, dependencies, components),
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

fn schema_v2_shell(id: &str, dependencies: &str, components: &str) -> String {
    format!(
        r#"{{
          "version": 2,
          "package": {{"id": "{id}", "kind": "shell", "version": "1.0.0"}},
          "dependencies": {dependencies},
          "components": {components},
          "surfaces": [
            {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true}},
            {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}}
          ]
        }}"#
    )
}

fn schema_v2_library(id: &str, dependencies: &str, components: &str) -> String {
    format!(
        r#"{{
          "version": 2,
          "package": {{"id": "{id}", "kind": "library", "version": "1.0.0"}},
          "dependencies": {dependencies},
          "components": {components}
        }}"#
    )
}

fn dependency(alias: &str, id: &str, path: &str) -> String {
    format!(r#"{{"alias":"{alias}","id":"{id}","path":"{path}"}}"#)
}

fn export(name: &str, source: &str) -> String {
    format!(
        r#"{{"name":{},"source":{}}}"#,
        serde_json::to_string(name).unwrap(),
        serde_json::to_string(source).unwrap()
    )
}

fn definition(name: &str, content: &str) -> String {
    format!(r#"<!doctype html><template data-htm-component="{name}">{content}</template>"#)
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

fn component_graph_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.write_shell(
        &format!(
            "[{},{}]",
            dependency("controls", "org.example.controls", "packages/controls"),
            dependency("shared", "org.example.shared", "packages/controls/shared")
        ),
        "[]",
        r#"<!doctype html><html><body><main><htm-use component="controls.status-card"></htm-use></main></body></html>"#,
    );
    fixture.write(
        "panel.html",
        panel_document(r#"<htm-use component="controls.status-card"></htm-use>"#),
    );
    fixture.write(
        "overlay.html",
        overlay_document(r#"<htm-use component="controls.status-card"></htm-use>"#),
    );
    fixture.write_library(
        "packages/controls",
        "org.example.controls",
        &format!("[{}]", dependency("shared", "org.example.shared", "shared")),
        &format!("[{}]", export("status-card", "components/cards.html")),
    );
    fixture.write_library(
        "packages/controls/shared",
        "org.example.shared",
        "[]",
        &format!("[{}]", export("badge-label", "components/badge.html")),
    );
    fixture.write(
        "packages/controls/components/cards.html",
        definition(
            "status-card",
            r#"<article class="card"><strong>Status</strong><htm-use component="shared.badge-label"></htm-use></article>"#,
        ),
    );
    fixture.write(
        "packages/controls/shared/components/badge.html",
        definition("badge-label", r#"<span class="badge">Ready</span>"#),
    );
    fixture
}

#[test]
fn component_name_and_reference_grammars_are_exact() {
    for valid in [
        "media-card",
        "status-row",
        "audio-output-card",
        "workspace-2d-preview",
    ] {
        assert_eq!(ComponentName::parse(valid).unwrap().as_str(), valid);
    }
    for invalid in [
        "card",
        "Media-card",
        "media--card",
        "-media-card",
        "media-card-",
        "media.card",
        "media card",
        "htm-use",
        "htm-private",
        "xml-node",
        "xlink-node",
        "state-text",
        &format!("a-{}", "b".repeat(63)),
    ] {
        assert!(
            ComponentName::parse(invalid).is_err(),
            "accepted `{invalid}`"
        );
    }

    let bare = ComponentReference::parse("media-card").unwrap();
    assert!(bare.alias().is_none());
    assert_eq!(bare.name().as_str(), "media-card");
    let qualified = ComponentReference::parse("controls.media-card").unwrap();
    assert_eq!(qualified.alias().unwrap().as_str(), "controls");
    assert_eq!(qualified.name().as_str(), "media-card");
    for invalid in [
        "",
        "controls.shared.media-card",
        "org.example.media-card",
        "controls.Media-card",
        " controls.media-card",
        "root.media-card",
    ] {
        assert!(
            ComponentReference::parse(invalid).is_err(),
            "accepted `{invalid}`"
        );
    }
}

#[test]
fn nested_library_components_prepare_once_and_instantiate_without_a_host_box() {
    let fixture = component_graph_fixture();
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let snapshot = manifest.snapshot();
    assert_eq!(snapshot.components().definitions().len(), 2);
    assert_eq!(snapshot.components().totals().source_document_count, 2);
    assert_eq!(snapshot.components().totals().source_read_count, 2);
    assert_eq!(snapshot.components().totals().source_parse_count, 2);
    assert_eq!(
        snapshot
            .components()
            .dependency_first_order()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "org.example.shared:badge-label",
            "org.example.controls:status-card"
        ]
    );

    let panel = manifest.surface("panel").unwrap();
    let prepared = panel.prepared_document().unwrap();
    assert_eq!(prepared.stats().component_instances, 2);
    assert_eq!(prepared.stats().referenced_definitions, 2);
    assert_eq!(prepared.stats().maximum_nesting_depth, 2);

    let live = LiveDocument::load_surface_snapshot(
        Arc::clone(snapshot),
        panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_eq!(live.component_instances().len(), 2);
    assert_eq!(
        live.component_instances()[0].reference().to_string(),
        "controls.status-card"
    );
    assert_eq!(
        live.component_instances()[1].reference().to_string(),
        "shared.badge-label"
    );
    assert!(
        live.component_instances()
            .iter()
            .all(|instance| instance.top_level_slots().len() == 1)
    );
    assert_eq!(live.resource_request_count(), 0);
}

#[test]
fn headless_and_live_share_component_semantics_but_not_instance_identity() {
    let fixture = component_graph_fixture();
    let mut loader = PackageSnapshotLoader::new();
    let snapshot = loader.load_headless(&fixture.root).unwrap();
    let entry = snapshot.headless_entry().unwrap();
    assert_eq!(
        entry
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        2
    );

    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    let panel = manifest.surface("panel").unwrap();
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
    let overlay = manifest.surface("overlay").unwrap();
    let overlay_document = LiveDocument::load_surface_snapshot(
        Arc::clone(manifest.snapshot()),
        overlay,
        LiveDocumentKind::TransientOverlay,
        800,
        600,
    )
    .unwrap();
    assert_eq!(
        first.component_instances()[0].definition_id(),
        second.component_instances()[0].definition_id()
    );
    assert_eq!(
        first.component_instances()[0].definition_id(),
        overlay_document.component_instances()[0].definition_id()
    );
    assert_ne!(
        first.component_instances()[0].id(),
        second.component_instances()[0].id()
    );
    assert_ne!(
        first.component_instances()[0].id(),
        overlay_document.component_instances()[0].id()
    );
    assert_ne!(
        first.component_instances()[0].id().document_serial(),
        second.component_instances()[0].id().document_serial()
    );
    assert_eq!(
        first.component_instances()[0].id().snapshot_generation(),
        manifest.snapshot().generation()
    );
}

#[test]
fn component_identities_are_generation_safe_across_replacements() {
    let fixture = component_graph_fixture();
    let mut loader = PackageSnapshotLoader::new();
    let first_snapshot = loader.load_manifest(fixture.manifest()).unwrap();
    let first_definition = first_snapshot
        .components()
        .definitions()
        .iter()
        .find(|definition| definition.key().name().as_str() == "status-card")
        .unwrap();
    let first_definition_id = first_snapshot
        .component_definition_id(first_definition.key())
        .unwrap();
    let first_panel = first_snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let first_document = LiveDocument::load_surface_snapshot(
        Arc::clone(&first_snapshot),
        first_panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    let first_instance_id = first_document.component_instances()[0].id().clone();

    let second_snapshot = loader
        .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
        .unwrap();
    let second_definition = second_snapshot
        .components()
        .definitions()
        .iter()
        .find(|definition| definition.key().name().as_str() == "status-card")
        .unwrap();
    let second_definition_id = second_snapshot
        .component_definition_id(second_definition.key())
        .unwrap();
    assert_eq!(first_definition_id.key(), second_definition_id.key());
    assert_ne!(first_definition_id, second_definition_id);

    fixture.write_library(
        "packages/controls",
        "org.example.controls",
        &format!("[{}]", dependency("shared", "org.example.shared", "shared")),
        &format!("[{}]", export("status-card", "components/moved-card.html")),
    );
    fixture.write(
        "packages/controls/components/moved-card.html",
        definition(
            "status-card",
            r#"<htm-use component="shared.badge-label"></htm-use>"#,
        ),
    );
    let moved_snapshot = loader
        .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
        .unwrap();
    let moved_definition = moved_snapshot
        .components()
        .definitions()
        .iter()
        .find(|definition| definition.key().name().as_str() == "status-card")
        .unwrap();
    assert_eq!(moved_definition.key(), second_definition.key());
    assert_eq!(
        moved_definition.logical_source(),
        "components/moved-card.html"
    );
    let moved_panel = moved_snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap();
    let moved_document = LiveDocument::load_surface_snapshot(
        Arc::clone(&moved_snapshot),
        moved_panel,
        LiveDocumentKind::Panel,
        800,
        52,
    )
    .unwrap();
    assert_ne!(
        moved_document.component_instances()[0].id(),
        &first_instance_id
    );

    fixture.write_library(
        "packages/controls",
        "org.example.controls",
        &format!("[{}]", dependency("shared", "org.example.shared", "shared")),
        "[]",
    );
    fixture.write("index.html", "<main>without component</main>");
    fixture.write("panel.html", panel_document(""));
    fixture.write("overlay.html", overlay_document(""));
    let removed_snapshot = loader
        .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
        .unwrap();
    assert!(
        removed_snapshot
            .components()
            .definitions()
            .iter()
            .all(|definition| definition.key().name().as_str() != "status-card")
    );

    fixture.write_library(
        "packages/controls",
        "org.example.controls",
        &format!("[{}]", dependency("shared", "org.example.shared", "shared")),
        &format!("[{}]", export("status-card", "components/moved-card.html")),
    );
    fixture.write(
        "panel.html",
        panel_document(r#"<htm-use component="controls.status-card"></htm-use>"#),
    );
    let readded_snapshot = loader
        .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
        .unwrap();
    let readded = readded_snapshot
        .components()
        .definitions()
        .iter()
        .find(|definition| definition.key().name().as_str() == "status-card")
        .unwrap();
    let readded_id = readded_snapshot
        .component_definition_id(readded.key())
        .unwrap();
    assert_ne!(readded_id, second_definition_id);
    assert_ne!(readded_id.generation(), moved_snapshot.generation());
}

#[test]
fn component_pixels_match_equivalent_hand_written_markup() {
    let component = Fixture::new();
    component.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        r#"<!doctype html><html><head><style>html,body{margin:0;background:#112;color:white}.card{width:120px;height:60px;padding:8px;background:#357}</style></head><body><htm-use component="static-card"></htm-use></body></html>"#,
    );
    component.write(
        "components/card.html",
        definition("static-card", r#"<section class="card">Static</section>"#),
    );
    let handwritten = Fixture::new();
    handwritten.write_shell(
        "[]",
        "[]",
        r#"<!doctype html><html><head><style>html,body{margin:0;background:#112;color:white}.card{width:120px;height:60px;padding:8px;background:#357}</style></head><body><section class="card">Static</section></body></html>"#,
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
    assert_eq!(component_run.component_instances.len(), 1);
    assert!(!component_run.component_descendants.is_empty());
}

#[test]
fn manifest_exports_are_authoritative_and_source_documents_are_declaration_only() {
    let cases = [
        (
            format!("[{}]", export("status-card", "components/card.html")),
            definition("other-card", "<p>Other</p>"),
            PackageErrorKind::ComponentTemplateUnexported,
        ),
        (
            format!("[{}]", export("status-card", "components/card.html")),
            "<p>outside</p>".to_owned(),
            PackageErrorKind::ComponentSourceRenderedContent,
        ),
        (
            format!("[{}]", export("status-card", "components/card.html")),
            "<template data-htm-component=\"status-card\"><template></template></template>"
                .to_owned(),
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            format!("[{}]", export("status-card", "components/card.html")),
            "<template data-htm-component = \"status-card\" DATA-HTM-COMPONENT=\"status-card\"><p>x</p></template>".to_owned(),
            PackageErrorKind::InvalidComponentExport,
        ),
        (
            format!("[{}]", export("status-card", "components/card.html")),
            "<template data-htm-component=\"status-card\"><script></script></template>".to_owned(),
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
    ];
    for (exports, source, kind) in cases {
        let fixture = Fixture::new();
        fixture.write_shell("[]", &exports, "<main>root</main>");
        fixture.write("components/card.html", source);
        assert_eq!(
            PackageSnapshotLoader::new()
                .load_headless(&fixture.root)
                .unwrap_err()
                .kind(),
            kind
        );
    }
}

#[test]
fn static_profile_rejects_dynamic_identity_style_and_resource_features() {
    let cases = [
        (
            "<p id=\"local\">x</p>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<label for=\"local\">x</label>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<a href=\"#local\">x</a>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<p aria-labelledby=\"local\">x</p>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<p aria-describedby=\"local\">x</p>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<p aria-controls=\"local\">x</p>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<slot></slot>",
            PackageErrorKind::ComponentSlotDefinitionUndeclared,
        ),
        (
            "<p slot=\"name\">x</p>",
            PackageErrorKind::ComponentSlotAttributePlacement,
        ),
        (
            "<style>p{color:red}</style>",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<link rel=\"stylesheet\" href=\"x.css\">",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<img src=\"x.png\">",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<svg><image href=\"x.svg\"></image></svg>",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<p style=\"background:url(x.png)\">x</p>",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<p style=\"background:u\\72l(x.png)\">x</p>",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<p style=\"@import 'x.css'\">x</p>",
            PackageErrorKind::ComponentResourceNotSupported,
        ),
        (
            "<script>alert(1)</script>",
            PackageErrorKind::ComponentFeatureNotSupported,
        ),
        (
            "<p data-htm-element=\"state-text\">x</p>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<p data-htm-element=\"state-token\">x</p>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<p data-htm-element=\"state-value\">x</p>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<button data-htm-element=\"action-button\">x</button>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<time data-htm-element=\"clock-text\">x</time>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<div data-htm-element=\"repeat\"></div>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<input data-htm-element=\"range-control\">",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
        (
            "<div data-htm-element=\"peak-monitor\"></div>",
            PackageErrorKind::ComponentStateActionNotSupported,
        ),
    ];
    for (content, kind) in cases {
        let fixture = Fixture::new();
        fixture.write_shell(
            "[]",
            &format!("[{}]", export("status-card", "components/card.html")),
            "<main>root</main>",
        );
        fixture.write("components/card.html", definition("status-card", content));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            kind,
            "unexpected result for {content}"
        );
    }
}

#[test]
fn static_profile_accepts_text_layout_inline_style_svg_and_nested_use() {
    let fixture = Fixture::new();
    fixture.write_shell(
        "[]",
        &format!(
            "[{},{}]",
            export("badge-label", "components/all.html"),
            export("status-card", "components/all.html")
        ),
        r#"<main><htm-use component="status-card"></htm-use></main>"#,
    );
    fixture.write(
        "components/all.html",
        concat!(
            "<template data-htm-component=\"badge-label\"><span>Ready</span></template>",
            "<template data-htm-component=\"status-card\"><article class=\"card\" style=\"color:#fff\">",
            "<svg viewBox=\"0 0 10 10\"><path d=\"M0 0L10 10\"></path></svg>",
            "<htm-use component=\"badge-label\"></htm-use></article></template>"
        ),
    );
    let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
    assert_eq!(manifest.snapshot().components().definitions().len(), 2);
    assert_eq!(
        manifest.snapshot().headless_entry().map(|entry| entry
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances),
        None
    );
}

#[test]
fn invocation_contract_rejects_attributes_children_and_compatibility_documents() {
    let invalid = [
        (
            "<htm-use></htm-use>",
            PackageErrorKind::ComponentInvocationAttributes,
        ),
        (
            r#"<htm-use component=""></htm-use>"#,
            PackageErrorKind::InvalidComponentReference,
        ),
        (
            r#"<htm-use component="static-card" class="x"></htm-use>"#,
            PackageErrorKind::ComponentInvocationAttributes,
        ),
        (
            r#"<htm-use component="static-card">text</htm-use>"#,
            PackageErrorKind::ComponentInvocationContentWithoutSlot,
        ),
        (
            r#"<htm-use component="static-card"><span></span></htm-use>"#,
            PackageErrorKind::ComponentInvocationContentWithoutSlot,
        ),
        (
            r#"<HTM-USE component = "static-card" COMPONENT="static-card"></HTM-USE>"#,
            PackageErrorKind::ComponentInvocationAttributes,
        ),
    ];
    for (use_markup, kind) in invalid {
        let fixture = Fixture::new();
        fixture.write_shell(
            "[]",
            &format!("[{}]", export("static-card", "components/card.html")),
            &format!("<main>{use_markup}</main>"),
        );
        fixture.write(
            "components/card.html",
            definition("static-card", "<p>Static</p>"),
        );
        assert_eq!(
            PackageSnapshotLoader::new()
                .load_headless(&fixture.root)
                .unwrap_err()
                .kind(),
            kind
        );
    }

    let v1 = Fixture::new();
    v1.write(
        "shell.json",
        r#"{"version":1,"id":"legacy","surfaces":[{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true},{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}]}"#,
    );
    v1.write("index.html", "<main>legacy</main>");
    v1.write(
        "panel.html",
        r#"<main><htm-use component="static-card"></htm-use></main>"#,
    );
    v1.write("overlay.html", "<main>overlay</main>");
    assert_eq!(
        ValidatedManifest::load(v1.manifest()).unwrap_err().kind(),
        PackageErrorKind::ComponentFeatureNotSupported
    );

    let legacy = Fixture::new();
    legacy.write(
        "index.html",
        r#"<htm-use component="static-card"></htm-use>"#,
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&legacy.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentFeatureNotSupported
    );
}

#[test]
fn component_references_are_package_scoped_and_cycles_are_atomic() {
    let fixture = component_graph_fixture();
    let mut loader = PackageSnapshotLoader::new();
    let current = loader.load_manifest(fixture.manifest()).unwrap();
    let generation = current.generation();

    fixture.write(
        "packages/controls/components/cards.html",
        definition(
            "status-card",
            r#"<htm-use component="controls.badge-label"></htm-use>"#,
        ),
    );
    assert_eq!(
        loader
            .build_manifest_candidate(fixture.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentAliasUnknown
    );
    assert!(Arc::ptr_eq(loader.current().unwrap(), &current));
    assert_eq!(loader.current().unwrap().generation(), generation);

    fixture.write(
        "packages/controls/components/cards.html",
        definition(
            "status-card",
            r#"<htm-use component="shared.badge-label"></htm-use>"#,
        ),
    );
    fixture.write(
        "packages/controls/shared/components/badge.html",
        definition(
            "badge-label",
            r#"<htm-use component="controls.status-card"></htm-use>"#,
        ),
    );
    assert_eq!(
        loader
            .build_manifest_candidate(fixture.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentAliasUnknown
    );
    assert!(Arc::ptr_eq(loader.current().unwrap(), &current));
}

#[test]
fn component_candidate_failures_preserve_last_known_good() {
    let fixture = component_graph_fixture();
    let mut loader = PackageSnapshotLoader::new();
    let current = loader.load_manifest(fixture.manifest()).unwrap();
    let generation = current.generation();
    let source = "packages/controls/components/cards.html";
    let valid = definition(
        "status-card",
        r#"<htm-use component="shared.badge-label"></htm-use>"#,
    );

    let assert_preserved = |loader: &PackageSnapshotLoader, expected: PackageErrorKind| {
        let error = loader
            .build_manifest_candidate(fixture.manifest())
            .unwrap_err();
        assert_eq!(error.kind(), expected);
        assert!(Arc::ptr_eq(loader.current().unwrap(), &current));
        assert_eq!(loader.current().unwrap().generation(), generation);
    };

    fs::remove_file(fixture.root.join(source)).unwrap();
    assert_preserved(&loader, PackageErrorKind::ComponentSourceMissing);

    fixture.write(source, "<template");
    assert_preserved(&loader, PackageErrorKind::ComponentSourceParse);

    fixture.write(
        source,
        format!("{valid}{}", definition("status-card", "<p>duplicate</p>")),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentTemplateDuplicate);

    fixture.write(
        source,
        definition(
            "status-card",
            r#"<htm-use component="missing-card"></htm-use>"#,
        ),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentExportUnknown);

    fixture.write(
        source,
        definition(
            "status-card",
            r#"<htm-use component="status-card"></htm-use>"#,
        ),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentDependencyCycle);

    fixture.write(
        source,
        definition(
            "status-card",
            r#"<span data-htm-element="state-text" data-htm-bind="shell.id"></span>"#,
        ),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentStateActionNotSupported);

    fixture.write(
        source,
        definition("status-card", r#"<img src="external.png">"#),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentResourceNotSupported);

    fixture.write(source, &valid);
    fixture.write(
        "panel.html",
        panel_document(r#"<htm-use component="missing-card"></htm-use>"#),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentExportUnknown);

    fixture.write(
        "panel.html",
        panel_document(r#"<htm-use component="controls.status-card" class="invalid"></htm-use>"#),
    );
    assert_preserved(&loader, PackageErrorKind::ComponentInvocationAttributes);

    fixture.write(
        "panel.html",
        panel_document(r#"<htm-use component="controls.status-card"></htm-use>"#),
    );
    let replacement = loader
        .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
        .unwrap();
    assert_eq!(replacement.generation().get(), generation.get() + 1);
    assert!(!Arc::ptr_eq(&replacement, &current));
}

#[test]
fn direct_and_cross_package_component_cycles_are_rejected() {
    let direct = Fixture::new();
    direct.write_shell(
        "[]",
        &format!("[{}]", export("status-card", "components/card.html")),
        "<main>root</main>",
    );
    direct.write(
        "components/card.html",
        definition(
            "status-card",
            r#"<htm-use component="status-card"></htm-use>"#,
        ),
    );
    assert_eq!(
        ValidatedManifest::load(direct.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentDependencyCycle
    );

    for (exports, source) in [
        (
            format!(
                "[{},{}]",
                export("first-card", "components/cycle.html"),
                export("second-card", "components/cycle.html")
            ),
            format!(
                "{}{}",
                definition(
                    "first-card",
                    r#"<htm-use component="second-card"></htm-use>"#
                ),
                definition(
                    "second-card",
                    r#"<htm-use component="first-card"></htm-use>"#
                )
            ),
        ),
        (
            format!(
                "[{},{},{}]",
                export("first-card", "components/cycle.html"),
                export("second-card", "components/cycle.html"),
                export("third-card", "components/cycle.html")
            ),
            format!(
                "{}{}{}",
                definition(
                    "first-card",
                    r#"<htm-use component="second-card"></htm-use>"#
                ),
                definition(
                    "second-card",
                    r#"<htm-use component="third-card"></htm-use>"#
                ),
                definition(
                    "third-card",
                    r#"<htm-use component="first-card"></htm-use>"#
                )
            ),
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write_shell("[]", &exports, "<main>root</main>");
        fixture.write("components/cycle.html", source);
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentDependencyCycle
        );
    }

    let cycle = Fixture::new();
    cycle.write_shell(
        &format!(
            "[{}]",
            dependency("first", "org.example.first", "packages/first")
        ),
        "[]",
        "<main>root</main>",
    );
    cycle.write_library(
        "packages/first",
        "org.example.first",
        &format!(
            "[{}]",
            dependency("second", "org.example.second", "../second")
        ),
        &format!("[{}]", export("first-card", "components/card.html")),
    );
    cycle.write_library(
        "packages/second",
        "org.example.second",
        "[]",
        &format!("[{}]", export("second-card", "components/card.html")),
    );
    // Package paths reject parent traversal before a cross-package component
    // cycle can exist. Direct package dependency aliases are the only route.
    assert_eq!(
        ValidatedManifest::load(cycle.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::InvalidDependencyPath
    );
}

#[test]
fn component_source_paths_and_symlinks_are_contained() {
    let unicode = Fixture::new();
    unicode.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/état.html")),
        "<htm-use component=\"static-card\"></htm-use>",
    );
    unicode.write(
        "components/état.html",
        definition("static-card", "<p>ready</p>"),
    );
    PackageSnapshotLoader::new()
        .load_headless(&unicode.root)
        .unwrap();

    for source in [
        "",
        "../card.html",
        "/card.html",
        "./card.html",
        "components//card.html",
        r"components\card.html",
        "https://example.invalid/card.html",
    ] {
        let fixture = Fixture::new();
        fixture.write_shell(
            "[]",
            &format!("[{}]", export("status-card", source)),
            "<main>root</main>",
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::InvalidComponentExport,
            "unexpected source result for `{source}`"
        );
    }

    let missing = Fixture::new();
    missing.write_shell(
        "[]",
        &format!("[{}]", export("status-card", "components/missing.html")),
        "<main>root</main>",
    );
    assert_eq!(
        ValidatedManifest::load(missing.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentSourceMissing
    );

    let directory = Fixture::new();
    directory.write_shell(
        "[]",
        &format!("[{}]", export("status-card", "components/card.html")),
        "<main>root</main>",
    );
    fs::create_dir_all(directory.root.join("components/card.html")).unwrap();
    assert_eq!(
        ValidatedManifest::load(directory.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentSourceInvalidType
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        fixture.write_shell(
            "[]",
            &format!("[{}]", export("status-card", "components/card.html")),
            "<main>root</main>",
        );
        fixture.write("target.html", definition("status-card", "<p>Static</p>"));
        fs::create_dir_all(fixture.root.join("components")).unwrap();
        symlink(
            fixture.root.join("target.html"),
            fixture.root.join("components/card.html"),
        )
        .unwrap();
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentSourceSymlink
        );

        let directory_link = Fixture::new();
        directory_link.write_shell(
            "[]",
            &format!("[{}]", export("status-card", "components/card.html")),
            "<main>root</main>",
        );
        directory_link.write(
            "target/card.html",
            definition("status-card", "<p>Static</p>"),
        );
        symlink(
            directory_link.root.join("target"),
            directory_link.root.join("components"),
        )
        .unwrap();
        assert_eq!(
            ValidatedManifest::load(directory_link.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentSourceSymlink
        );
    }
}

#[test]
fn component_source_byte_and_node_limits_are_exact() {
    let base = definition("status-card", "<span></span>");
    let comment_prefix = "<!--";
    let comment_suffix = "-->";
    let filler_len = MAX_COMPONENT_SOURCE_BYTES as usize
        - base.len()
        - comment_prefix.len()
        - comment_suffix.len();
    let exact_source = format!(
        "{comment_prefix}{}{comment_suffix}{base}",
        "x".repeat(filler_len)
    );
    assert_eq!(exact_source.len(), MAX_COMPONENT_SOURCE_BYTES as usize);
    let exact = Fixture::new();
    exact.write_shell(
        "[]",
        &format!("[{}]", export("status-card", "components/card.html")),
        "<main>root</main>",
    );
    exact.write("components/card.html", &exact_source);
    ValidatedManifest::load(exact.manifest()).unwrap();
    exact.write("components/card.html", format!("{exact_source}x"));
    assert_eq!(
        ValidatedManifest::load(exact.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentSourceTooLarge
    );

    for (nodes, expected) in [
        (MAX_COMPONENT_SOURCE_NODES, None),
        (
            MAX_COMPONENT_SOURCE_NODES + 1,
            Some(PackageErrorKind::ComponentSourceNodeLimit),
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write_shell(
            "[]",
            &format!("[{}]", export("status-card", "components/card.html")),
            "<main>root</main>",
        );
        fixture.write(
            "components/card.html",
            definition("status-card", &"<i></i>".repeat(nodes)),
        );
        let result = ValidatedManifest::load(fixture.manifest());
        match expected {
            Some(kind) => assert_eq!(result.unwrap_err().kind(), kind),
            None => {
                let manifest = result.unwrap();
                assert_eq!(
                    manifest.snapshot().components().definitions()[0].source_node_count(),
                    nodes
                );
            }
        }
    }
}

#[test]
fn export_instance_reference_and_nesting_limits_are_enforced() {
    let exports = (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
        .map(|index| export(&format!("item-{index:04}"), "components/all.html"))
        .collect::<Vec<_>>()
        .join(",");
    let templates = (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
        .map(|index| definition(&format!("item-{index:04}"), "<span></span>"))
        .collect::<String>();
    let fixture = Fixture::new();
    fixture.write_shell("[]", &format!("[{exports}]"), "<main>root</main>");
    fixture.write("components/all.html", templates);
    assert_eq!(
        ValidatedManifest::load(fixture.manifest())
            .unwrap()
            .snapshot()
            .components()
            .definitions()
            .len(),
        MAX_COMPONENT_EXPORTS_PER_PACKAGE
    );

    let over_exports = format!(
        "{exports},{}",
        export("overflow-item", "components/all.html")
    );
    fixture.write(
        "shell.json",
        schema_v2_shell("org.example.shell", "[]", &format!("[{over_exports}]")),
    );
    assert_eq!(
        ValidatedManifest::load(fixture.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::InvalidComponentExport
    );

    let repeated = Fixture::new();
    repeated.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        &format!(
            "<main>{}</main>",
            r#"<htm-use component="static-card"></htm-use>"#
                .repeat(MAX_COMPONENT_INSTANCES_PER_DOCUMENT)
        ),
    );
    repeated.write(
        "components/card.html",
        definition("static-card", "<span></span>"),
    );
    let valid = PackageSnapshotLoader::new()
        .load_headless(&repeated.root)
        .unwrap();
    assert_eq!(
        valid
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        MAX_COMPONENT_INSTANCES_PER_DOCUMENT
    );
    repeated.write(
        "index.html",
        format!(
            "<main>{}</main>",
            r#"<htm-use component="static-card"></htm-use>"#
                .repeat(MAX_COMPONENT_INSTANCES_PER_DOCUMENT + 1)
        ),
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&repeated.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentInstanceLimit
    );

    let referenced = Fixture::new();
    let library_exports = (0..MAX_COMPONENT_REFERENCES_PER_DOCUMENT)
        .map(|index| export(&format!("item-{index:04}"), "components/all.html"))
        .collect::<Vec<_>>()
        .join(",");
    let library_templates = (0..MAX_COMPONENT_REFERENCES_PER_DOCUMENT)
        .map(|index| definition(&format!("item-{index:04}"), "<span></span>"))
        .collect::<String>();
    let library_uses = (0..MAX_COMPONENT_REFERENCES_PER_DOCUMENT)
        .map(|index| format!(r#"<htm-use component="library.item-{index:04}"></htm-use>"#))
        .collect::<String>();
    referenced.write_shell(
        &format!(
            "[{}]",
            dependency("library", "org.example.library", "packages/library")
        ),
        "[]",
        &format!("<main>{library_uses}</main>"),
    );
    referenced.write_library(
        "packages/library",
        "org.example.library",
        "[]",
        &format!("[{library_exports}]"),
    );
    referenced.write("packages/library/components/all.html", library_templates);
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&referenced.root)
            .unwrap()
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .referenced_definitions,
        MAX_COMPONENT_REFERENCES_PER_DOCUMENT
    );
    referenced.write(
        "shell.json",
        schema_v2_shell(
            "org.example.shell",
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
            &format!("[{}]", export("extra-card", "components/extra.html")),
        ),
    );
    referenced.write(
        "components/extra.html",
        definition("extra-card", "<span></span>"),
    );
    referenced.write(
        "index.html",
        format!("<main>{library_uses}<htm-use component=\"extra-card\"></htm-use></main>"),
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&referenced.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentReferencedDefinitionLimit
    );

    for (depth, expected) in [
        (MAX_COMPONENT_NESTING_DEPTH, None),
        (
            MAX_COMPONENT_NESTING_DEPTH + 1,
            Some(PackageErrorKind::ComponentNestingLimit),
        ),
    ] {
        let nested = Fixture::new();
        let exports = (0..depth)
            .map(|index| export(&format!("level-{index:02}"), "components/chain.html"))
            .collect::<Vec<_>>()
            .join(",");
        let templates = (0..depth)
            .map(|index| {
                let child = if index + 1 == depth {
                    "<span>leaf</span>".to_owned()
                } else {
                    format!("<htm-use component=\"level-{:02}\"></htm-use>", index + 1)
                };
                definition(&format!("level-{index:02}"), &child)
            })
            .collect::<String>();
        nested.write_shell(
            "[]",
            &format!("[{exports}]"),
            r#"<main><htm-use component="level-00"></htm-use></main>"#,
        );
        nested.write("components/chain.html", templates);
        let result = PackageSnapshotLoader::new().load_headless(&nested.root);
        match expected {
            Some(kind) => assert_eq!(result.unwrap_err().kind(), kind),
            None => assert_eq!(
                result
                    .unwrap()
                    .headless_entry()
                    .unwrap()
                    .prepared_document()
                    .unwrap()
                    .stats()
                    .maximum_nesting_depth,
                depth
            ),
        }
    }
}

#[test]
fn component_graph_export_limit_is_exact() {
    let fixture = Fixture::new();
    let dependency_count = MAX_COMPONENT_EXPORTS_PER_GRAPH / MAX_COMPONENT_EXPORTS_PER_PACKAGE;
    let dependencies = (0..dependency_count)
        .map(|index| {
            dependency(
                &format!("library-{index}"),
                &format!("org.example.library-{index}"),
                &format!("packages/library-{index}"),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    fixture.write_shell(&format!("[{dependencies}]"), "[]", "<main>root</main>");
    for package in 0..dependency_count {
        let exports = (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
            .map(|index| export(&format!("item-{index:04}"), "components/all.html"))
            .collect::<Vec<_>>()
            .join(",");
        let templates = (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
            .map(|index| definition(&format!("item-{index:04}"), "<span></span>"))
            .collect::<String>();
        fixture.write_library(
            &format!("packages/library-{package}"),
            &format!("org.example.library-{package}"),
            "[]",
            &format!("[{exports}]"),
        );
        fixture.write(
            &format!("packages/library-{package}/components/all.html"),
            templates,
        );
    }
    assert_eq!(
        ValidatedManifest::load(fixture.manifest())
            .unwrap()
            .snapshot()
            .components()
            .definitions()
            .len(),
        MAX_COMPONENT_EXPORTS_PER_GRAPH
    );

    fixture.write(
        "shell.json",
        schema_v2_shell(
            "org.example.shell",
            &format!("[{dependencies}]"),
            &format!("[{}]", export("overflow-card", "components/overflow.html")),
        ),
    );
    assert_eq!(
        ValidatedManifest::load(fixture.manifest())
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentGraphExportLimit
    );
}

#[test]
fn maximum_expanded_node_boundary_is_enforced() {
    fn source(node_counts: &[usize]) -> String {
        node_counts
            .iter()
            .enumerate()
            .map(|(index, count)| {
                definition(&format!("group-{index}"), &"<span></span>".repeat(*count))
            })
            .collect()
    }
    fn exports(count: usize) -> String {
        (0..count)
            .map(|index| export(&format!("group-{index}"), "components/all.html"))
            .collect::<Vec<_>>()
            .join(",")
    }
    fn uses(count: usize) -> String {
        format!(
            "<main>{}</main>",
            (0..count)
                .map(|index| { format!(r#"<htm-use component="group-{index}"></htm-use>"#) })
                .collect::<String>()
        )
    }

    let fixture = Fixture::new();
    fixture.write_shell("[]", &format!("[{}]", exports(5)), &uses(5));
    fixture.write(
        "components/all.html",
        source(&[9_999, 9_999, 9_999, 9_999, 9_995]),
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();
    assert_eq!(
        snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .expanded_nodes,
        MAX_COMPONENT_EXPANDED_NODES
    );

    fixture.write(
        "components/all.html",
        source(&[9_999, 9_999, 9_999, 9_999, 9_996]),
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentExpandedNodeLimit
    );
}

#[test]
fn unused_library_definitions_are_inert() {
    let fixture = component_graph_fixture();
    fixture.write("index.html", "<main>unused libraries</main>");
    fixture.write("panel.html", panel_document(""));
    let run = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            output_directory: None,
            run_interaction: false,
            render_png: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(run.component_instances.is_empty());
    assert!(run.component_descendants.is_empty());
    assert!(run.artifacts[0].report.resources.is_empty());
}

#[test]
fn package_graph_example_exercises_inputs_and_named_slot_projection() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let package = root.join("examples/package-graph");
    let run = run_package_with_options(
        package,
        ExperimentOptions {
            output_directory: None,
            run_interaction: false,
            render_png: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(run.package_snapshot.components().definitions().len(), 9);
    assert_eq!(run.component_instances.len(), 15);
    assert_eq!(run.component_input_consumers.len(), 24);
    assert_eq!(run.component_slot_projections.len(), 11);
    assert!(run.component_slot_projections.iter().any(|projection| {
        projection.id().slot_definition().name().as_str() == "icon"
            && projection.outcome() == htm_runtime::ComponentSlotProjectionOutcome::Assigned
    }));
    assert!(run.component_slot_projections.iter().any(|projection| {
        projection.id().slot_definition().name().as_str() == "content"
            && projection.outcome() == htm_runtime::ComponentSlotProjectionOutcome::Assigned
    }));
    assert!(!run.projected_component_nodes.is_empty());
    assert!(!run.component_fallback_nodes.is_empty());
    assert_eq!(
        run.component_instances[0].inputs().values()[0]
            .value()
            .canonical_string(),
        "Defaulted package snapshot"
    );
    assert!(
        run.component_instances.iter().all(
            |instance| instance.id().snapshot_generation() == run.package_snapshot.generation()
        )
    );
}

#[test]
#[ignore = "release-only component measurements and stress"]
fn component_release_measurement_and_stress_probe() {
    fn micros<T>(operation: impl FnOnce() -> T) -> (u128, T) {
        let started = Instant::now();
        let result = operation();
        (started.elapsed().as_micros(), result)
    }

    fn repeated_uses(count: usize, reference: &str) -> String {
        format!(
            "<main>{}</main>",
            (0..count)
                .map(|_| format!(r#"<htm-use component="{reference}"></htm-use>"#))
                .collect::<String>()
        )
    }

    fn nested_fixture(depth: usize) -> Fixture {
        let fixture = Fixture::new();
        let exports = (0..depth)
            .map(|index| export(&format!("level-{index:02}"), "components/all.html"))
            .collect::<Vec<_>>()
            .join(",");
        let definitions = (0..depth)
            .map(|index| {
                let child = if index + 1 == depth {
                    "<span>leaf</span>".to_owned()
                } else {
                    format!("<htm-use component=\"level-{:02}\"></htm-use>", index + 1)
                };
                definition(&format!("level-{index:02}"), &child)
            })
            .collect::<String>();
        fixture.write_shell(
            "[]",
            &format!("[{exports}]"),
            r#"<htm-use component="level-00"></htm-use>"#,
        );
        fixture.write("components/all.html", definitions);
        fixture
    }

    let one = Fixture::new();
    one.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        &repeated_uses(1, "static-card"),
    );
    one.write(
        "components/card.html",
        definition("static-card", "<article>static</article>"),
    );
    let (one_definition_us, one_candidate) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&one.root)
            .unwrap()
    });
    let mut publication_loader = PackageSnapshotLoader::new();
    let (publication_us, one_snapshot) =
        micros(|| publication_loader.publish(one_candidate).unwrap());
    let one_instance_us = one_definition_us;

    let hundred = Fixture::new();
    hundred.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        &repeated_uses(100, "static-card"),
    );
    hundred.write(
        "components/card.html",
        definition("static-card", "<article>static</article>"),
    );
    let (hundred_instances_us, hundred_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&hundred.root)
            .unwrap()
    });
    assert_eq!(
        hundred_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        100
    );

    let thousand = Fixture::new();
    thousand.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        &repeated_uses(1_000, "static-card"),
    );
    thousand.write(
        "components/card.html",
        definition("static-card", "<article>static</article>"),
    );
    let (thousand_instances_us, thousand_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&thousand.root)
            .unwrap()
    });
    assert_eq!(
        thousand_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        1_000
    );

    let depth = nested_fixture(MAX_COMPONENT_NESTING_DEPTH);
    let (maximum_depth_us, depth_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&depth.root)
            .unwrap()
    });
    assert_eq!(
        depth_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .maximum_nesting_depth,
        MAX_COMPONENT_NESTING_DEPTH
    );

    let definitions = Fixture::new();
    let definition_exports = (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
        .map(|index| export(&format!("item-{index:04}"), "components/all.html"))
        .collect::<Vec<_>>()
        .join(",");
    let definition_sources = (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
        .map(|index| definition(&format!("item-{index:04}"), "<span></span>"))
        .collect::<String>();
    definitions.write_shell(
        "[]",
        &format!("[{definition_exports}]"),
        "<main>root</main>",
    );
    definitions.write("components/all.html", definition_sources);
    let (definitions_256_us, definitions_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&definitions.root)
            .unwrap()
    });
    assert_eq!(
        definitions_snapshot.components().definitions().len(),
        MAX_COMPONENT_EXPORTS_PER_PACKAGE
    );

    let graph = Fixture::new();
    let dependency_count = MAX_COMPONENT_EXPORTS_PER_GRAPH / MAX_COMPONENT_EXPORTS_PER_PACKAGE;
    let dependencies = (0..dependency_count)
        .map(|index| {
            dependency(
                &format!("library-{index}"),
                &format!("org.example.library-{index}"),
                &format!("packages/library-{index}"),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    graph.write_shell(&format!("[{dependencies}]"), "[]", "<main>root</main>");
    for package in 0..dependency_count {
        graph.write_library(
            &format!("packages/library-{package}"),
            &format!("org.example.library-{package}"),
            "[]",
            &format!("[{definition_exports}]"),
        );
        graph.write(
            &format!("packages/library-{package}/components/all.html"),
            (0..MAX_COMPONENT_EXPORTS_PER_PACKAGE)
                .map(|index| definition(&format!("item-{index:04}"), "<span></span>"))
                .collect::<String>(),
        );
    }
    let (graph_4096_us, graph_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&graph.root)
            .unwrap()
    });
    assert_eq!(
        graph_snapshot.components().definitions().len(),
        MAX_COMPONENT_EXPORTS_PER_GRAPH
    );

    let expansion = Fixture::new();
    let expansion_exports = (0..5)
        .map(|index| export(&format!("group-{index}"), "components/all.html"))
        .collect::<Vec<_>>()
        .join(",");
    expansion.write_shell(
        "[]",
        &format!("[{expansion_exports}]"),
        &format!(
            "<main>{}</main>",
            (0..5)
                .map(|index| { format!(r#"<htm-use component="group-{index}"></htm-use>"#) })
                .collect::<String>()
        ),
    );
    expansion.write(
        "components/all.html",
        [9_999, 9_999, 9_999, 9_999, 9_995]
            .iter()
            .enumerate()
            .map(|(index, count)| {
                definition(&format!("group-{index}"), &"<span></span>".repeat(*count))
            })
            .collect::<String>(),
    );
    let (maximum_expansion_us, expansion_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&expansion.root)
            .unwrap()
    });
    assert_eq!(
        expansion_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .expanded_nodes,
        MAX_COMPONENT_EXPANDED_NODES
    );

    let cycle = Fixture::new();
    cycle.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        "<main>root</main>",
    );
    cycle.write(
        "components/card.html",
        definition(
            "static-card",
            r#"<htm-use component="static-card"></htm-use>"#,
        ),
    );
    let (cycle_rejection_us, cycle_error) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&cycle.root)
            .unwrap_err()
    });
    assert_eq!(
        cycle_error.kind(),
        PackageErrorKind::ComponentDependencyCycle
    );

    let unknown = Fixture::new();
    unknown.write_shell(
        "[]",
        &format!("[{}]", export("static-card", "components/card.html")),
        r#"<htm-use component="missing-card"></htm-use>"#,
    );
    unknown.write(
        "components/card.html",
        definition("static-card", "<span>static</span>"),
    );
    let (unknown_rejection_us, unknown_error) = micros(|| {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&unknown.root)
            .unwrap_err()
    });
    assert_eq!(
        unknown_error.kind(),
        PackageErrorKind::ComponentExportUnknown
    );

    let (serialization_us, serialized) = micros(|| one_snapshot.deterministic_json().unwrap());
    assert!(serialized.contains("\"component_definition_count\": 1"));

    for _ in 0..1_000 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&one.root)
            .unwrap();
    }
    let multi = component_graph_fixture();
    for _ in 0..500 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&multi.root)
            .unwrap();
    }
    for _ in 0..500 {
        assert_eq!(
            publication_loader
                .build_headless_candidate(&cycle.root)
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentDependencyCycle
        );
        assert!(Arc::ptr_eq(
            publication_loader.current().unwrap(),
            &one_snapshot
        ));
    }
    for _ in 0..500 {
        assert_eq!(
            publication_loader
                .build_headless_candidate(&unknown.root)
                .unwrap_err()
                .kind(),
            PackageErrorKind::ComponentExportUnknown
        );
    }
    for _ in 0..500 {
        let candidate = publication_loader
            .build_headless_candidate(&one.root)
            .unwrap();
        publication_loader.publish(candidate).unwrap();
    }
    let live_manifest = ValidatedManifest::load(multi.manifest()).unwrap();
    let panel = live_manifest.surface("panel").unwrap();
    for _ in 0..500 {
        let live = LiveDocument::load_surface_snapshot(
            Arc::clone(live_manifest.snapshot()),
            panel,
            LiveDocumentKind::Panel,
            800,
            52,
        )
        .unwrap();
        assert_eq!(live.component_instances().len(), 2);
    }
    for _ in 0..500 {
        assert!(
            !live_manifest
                .deterministic_package_graph_json()
                .unwrap()
                .is_empty()
        );
    }
    for _ in 0..10 {
        PackageSnapshotLoader::new()
            .build_headless_candidate(&depth.root)
            .unwrap();
        PackageSnapshotLoader::new()
            .build_headless_candidate(&thousand.root)
            .unwrap();
    }

    println!(
        "component_measurements_us one_definition={one_definition_us} one_instance={one_instance_us} hundred_instances={hundred_instances_us} thousand_instances={thousand_instances_us} maximum_depth={maximum_depth_us} definitions_256={definitions_256_us} graph_4096={graph_4096_us} maximum_expansion={maximum_expansion_us} cycle_rejection={cycle_rejection_us} unknown_rejection={unknown_rejection_us} publication={publication_us} serialization={serialization_us} source_reads={} source_parses={} instances={} expanded_nodes={} bytes_read={}",
        one_snapshot.components().totals().source_read_count,
        one_snapshot.components().totals().source_parse_count,
        thousand_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .component_instances,
        expansion_snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap()
            .stats()
            .expanded_nodes,
        graph_snapshot.bytes_read(),
    );
}
