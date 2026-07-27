use htm_runtime::{
    DiagnosticNode, ExperimentOptions, Phase, RuntimeError, ViewportSpec, run_package_with_options,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new(html: &str, css: Option<&str>) -> Self {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("htmshell-gate-a-{}-{sequence}", std::process::id()));
        std::fs::create_dir_all(root.join("assets")).expect("create temporary fixture");
        std::fs::write(root.join("index.html"), html).expect("write temporary index.html");
        if let Some(css) = css {
            std::fs::write(root.join("style.css"), css).expect("write temporary style.css");
        }
        Self { root }
    }

    fn write_asset(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create asset parent");
        }
        std::fs::write(path, bytes).expect("write temporary asset");
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn options(render_png: bool, interaction: bool) -> ExperimentOptions {
    ExperimentOptions {
        viewport: ViewportSpec {
            logical_width: 480,
            logical_height: 320,
            ..ViewportSpec::default()
        },
        render_png,
        run_interaction: interaction,
        output_directory: None,
    }
}

fn report_for(run: &htm_runtime::ExperimentRun, phase: Phase) -> &htm_runtime::DiagnosticReport {
    &run.artifacts
        .iter()
        .find(|artifact| artifact.phase == phase)
        .expect("requested phase artifact")
        .report
}

fn node_by_id<'a>(root: &'a DiagnosticNode, id: &str) -> Option<&'a DiagnosticNode> {
    if root.html_id.as_deref() == Some(id) {
        return Some(root);
    }
    root.children.iter().find_map(|child| node_by_id(child, id))
}

fn nodes_with_class<'a>(root: &'a DiagnosticNode, class: &str, out: &mut Vec<&'a DiagnosticNode>) {
    if root.classes.iter().any(|value| value == class) {
        out.push(root);
    }
    for child in &root.children {
        nodes_with_class(child, class, out);
    }
}

fn basic_document(body: &str) -> String {
    format!(
        "<!doctype html><html><head><link rel=\"stylesheet\" href=\"style.css\"></head><body>{body}</body></html>"
    )
}

#[test]
fn representative_fixture_exercises_required_layout_and_interaction() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic-shell");
    let run = run_package_with_options(
        root,
        ExperimentOptions {
            render_png: false,
            run_interaction: true,
            output_directory: None,
            ..ExperimentOptions::default()
        },
    )
    .expect("representative fixture should resolve");
    let initial = report_for(&run, Phase::Initial);

    let desktop = node_by_id(&initial.tree, "desktop-root").expect("desktop root");
    assert!(desktop.logical_bounds.width > 0.0);
    assert!(desktop.logical_bounds.height > 0.0);

    let panel = node_by_id(&initial.tree, "top-panel").expect("top panel");
    assert_eq!(panel.display, "flex");
    assert!(
        panel
            .border_radii
            .as_ref()
            .is_some_and(|r| r.top_left[0] > 0.0)
    );

    let grid = node_by_id(&initial.tree, "quick-grid").expect("quick grid");
    assert_eq!(grid.display, "grid");
    let mut tiles = Vec::new();
    nodes_with_class(grid, "quick-tile", &mut tiles);
    assert_eq!(tiles.len(), 4);
    assert!(tiles[1].logical_bounds.x > tiles[0].logical_bounds.x);
    assert!(tiles[2].logical_bounds.y > tiles[0].logical_bounds.y);

    let clip = node_by_id(&initial.tree, "clip-card")
        .and_then(|card| {
            let mut nodes = Vec::new();
            nodes_with_class(card, "clip-window", &mut nodes);
            nodes.into_iter().next()
        })
        .expect("clip window");
    assert!(clip.overflow.establishes_clip);

    let absolute = {
        let mut nodes = Vec::new();
        nodes_with_class(&initial.tree, "absolute-badge", &mut nodes);
        nodes.into_iter().next().expect("absolute badge")
    };
    assert_eq!(absolute.position, "absolute");
    assert!(
        initial
            .fonts
            .iter()
            .any(|font| font.family == "DejaVu Sans")
    );
    assert!(
        initial
            .resources
            .iter()
            .any(|record| { record.resource_kind == "svg" && record.decision == "loaded" })
    );
    assert!(initial.tree.tree_has_measured_text());

    let hover = report_for(&run, Phase::Hover)
        .interaction
        .as_ref()
        .expect("hover evidence");
    assert!(hover.target_hovered);
    assert!(hover.node_identity_retained);
    assert!(!hover.dom_tree_rebuilt);
    assert!(hover.observably_changed_style_nodes > 0);
    assert!(hover.animation_running_after_state_change);

    let active = report_for(&run, Phase::Active)
        .interaction
        .as_ref()
        .expect("active evidence");
    assert!(active.target_active);
    assert!(active.node_identity_retained);
}

trait DiagnosticTreeExt {
    fn tree_has_measured_text(&self) -> bool;
}

impl DiagnosticTreeExt for DiagnosticNode {
    fn tree_has_measured_text(&self) -> bool {
        self.text.as_ref().is_some_and(|text| {
            text.measured_bounds.width > 0.0 && text.measured_bounds.height > 0.0
        }) || self
            .children
            .iter()
            .any(DiagnosticTreeExt::tree_has_measured_text)
    }
}

#[test]
fn malformed_html_and_css_do_not_crash() {
    let malformed_html = TempFixture::new(
        "<html><head><link rel=stylesheet href=style.css><body><div><span>repair me",
        Some("body { color: ; .broken { width: calc( }"),
    );
    let run = run_package_with_options(&malformed_html.root, options(false, false))
        .expect("html5ever and Stylo should recover without a process crash");
    assert!(report_for(&run, Phase::Initial).node_count > 0);
}

#[test]
fn local_stylesheet_and_svg_are_loaded() {
    let fixture = TempFixture::new(
        &basic_document("<div id=box><img id=icon src=\"assets/icon.svg\"></div>"),
        Some(
            "#box { display:block; width:120px; height:80px; background:#123456; border-radius:9px; } #icon { width:20px; height:20px; }",
        ),
    );
    fixture.write_asset(
        "assets/icon.svg",
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" rx="4" fill="#abcdef"/></svg>"##,
    );
    let run = run_package_with_options(&fixture.root, options(false, false)).expect("local assets");
    let report = report_for(&run, Phase::Initial);
    let box_node = node_by_id(&report.tree, "box").expect("styled box");
    assert_eq!(box_node.display, "block");
    assert!(box_node.logical_bounds.width >= 120.0);
    assert!(box_node.border_radii.is_some());
    let icon = node_by_id(&report.tree, "icon").expect("SVG image node");
    assert_eq!(icon.image.as_ref().unwrap().decoded_kind, "svg");
    assert_eq!(
        report
            .resources
            .iter()
            .filter(|record| record.decision == "loaded")
            .count(),
        2
    );
}

#[test]
fn missing_stylesheet_and_image_are_diagnostics_not_panics() {
    let fixture = TempFixture::new(
        "<!doctype html><html><head><link rel=stylesheet href=missing.css></head><body><img src=assets/missing.svg></body></html>",
        None,
    );
    let run = run_package_with_options(&fixture.root, options(false, false))
        .expect("missing resources should not panic");
    let report = report_for(&run, Phase::Initial);
    assert_eq!(
        report
            .resources
            .iter()
            .filter(|record| record.decision == "missing")
            .count(),
        2
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|message| message.code == "resource.missing")
    );
}

#[test]
fn network_imports_images_and_protocol_relative_urls_are_rejected() {
    let fixture = TempFixture::new(
        &basic_document(
            "<img src=\"http://example.invalid/a.png\"><img src=\"https://example.invalid/b.svg\"><img src=\"//example.invalid/c.png\">",
        ),
        Some("@import \"https://example.invalid/remote.css\"; body { color: white; }"),
    );
    let run = run_package_with_options(&fixture.root, options(false, false))
        .expect("remote references should be contained");
    let report = report_for(&run, Phase::Initial);
    let rejected: Vec<_> = report
        .resources
        .iter()
        .filter(|record| record.decision == "rejected")
        .collect();
    assert!(
        rejected
            .iter()
            .any(|record| record.url.starts_with("http://"))
    );
    assert!(
        rejected
            .iter()
            .any(|record| record.url.starts_with("https://"))
    );
    assert!(
        rejected
            .iter()
            .any(|record| record.detail.contains("foreign package host"))
    );
    assert!(rejected.len() >= 4);
}

#[test]
fn traversal_and_absolute_filesystem_urls_are_rejected() {
    let fixture = TempFixture::new(
        &basic_document("<img src=\"../outside.svg\"><img src=\"file:///etc/passwd\">"),
        Some("body { display:block; }"),
    );
    let run = run_package_with_options(&fixture.root, options(false, false))
        .expect("escaping paths should be contained");
    let report = report_for(&run, Phase::Initial);
    assert!(report.resources.iter().any(|record| {
        record.decision == "rejected" && record.detail.contains("package-root escape")
    }));
    assert!(report.resources.iter().any(|record| {
        record.decision == "rejected" && record.detail.contains("filesystem URL")
    }));
}

#[test]
fn unsupported_elements_are_reported_without_panicking() {
    let fixture = TempFixture::new(
        &basic_document("<video id=media></video><script>ignored()</script>"),
        Some("#media { width:100px; height:20px; }"),
    );
    let run = run_package_with_options(&fixture.root, options(false, false))
        .expect("unsupported elements should remain inert");
    let report = report_for(&run, Phase::Initial);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|message| message.code == "html.element_unsupported")
            .count(),
        2
    );
}

#[test]
fn diagnostics_and_png_are_repeatable_in_one_environment() {
    let fixture = TempFixture::new(
        &basic_document("<div id=box>Deterministic fixture</div>"),
        Some(
            "body { margin:0; font-family:\"DejaVu Sans\",sans-serif; } #box { width:220px; height:90px; padding:10px; color:white; background:#223355; border-radius:12px; }",
        ),
    );
    let first = run_package_with_options(&fixture.root, options(true, false)).expect("first run");
    let second = run_package_with_options(&fixture.root, options(true, false)).expect("second run");
    let third = run_package_with_options(&fixture.root, options(true, false)).expect("third run");
    let first_artifact = &first.artifacts[0];
    assert_eq!(
        first_artifact.diagnostic_json,
        second.artifacts[0].diagnostic_json
    );
    assert_eq!(
        first_artifact.diagnostic_json,
        third.artifacts[0].diagnostic_json
    );
    assert_eq!(first_artifact.png, second.artifacts[0].png);
    assert_eq!(first_artifact.png, third.artifacts[0].png);
    assert_eq!(
        first_artifact.report.retained_scene_order,
        second.artifacts[0].report.retained_scene_order
    );
}

#[test]
fn color_filter_reference_matches_the_tracked_cpu_golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/color-filters");
    let options = ExperimentOptions {
        render_png: true,
        run_interaction: false,
        output_directory: None,
        ..ExperimentOptions::default()
    };
    let first = run_package_with_options(&root, options.clone()).expect("first filtered run");
    let second = run_package_with_options(&root, options).expect("second filtered run");
    assert_eq!(first.artifacts.len(), 1);
    assert_eq!(first.artifacts[0].png, second.artifacts[0].png);
    let expected =
        std::fs::read(root.join("output/initial.png")).expect("tracked color-filter CPU golden");
    assert_eq!(first.artifacts[0].png.as_deref(), Some(expected.as_slice()));
}

#[test]
fn retained_renderer_matches_the_tracked_basic_shell_pixels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic-shell");
    let run = run_package_with_options(
        &root,
        ExperimentOptions {
            render_png: true,
            run_interaction: true,
            output_directory: None,
            ..ExperimentOptions::default()
        },
    )
    .expect("retained renderer should render the tracked fixture");
    for artifact in &run.artifacts {
        let expected = std::fs::read(
            root.join("output")
                .join(format!("{}.png", artifact.phase.filename())),
        )
        .expect("tracked baseline PNG");
        assert_eq!(
            artifact.png.as_deref(),
            Some(expected.as_slice()),
            "{} retained pixels changed",
            artifact.phase.filename()
        );
    }
}

#[test]
fn invalid_dimensions_never_serialize_nonfinite_geometry() {
    let fixture = TempFixture::new(
        &basic_document("<div id=bad>finite fallback</div>"),
        Some("#bad { width: calc(1px / 0); height: 1e999px; transform: scale(1e999); }"),
    );
    let run = run_package_with_options(&fixture.root, options(false, false))
        .expect("invalid CSS dimensions should not crash");
    let json = std::str::from_utf8(&run.artifacts[0].diagnostic_json).unwrap();
    assert!(!json.contains("NaN"));
    assert!(!json.contains("Infinity"));
    assert!(!json.contains("-Infinity"));
}

#[test]
fn excessively_deep_dom_is_rejected_at_the_adapter_boundary() {
    let mut body = String::new();
    for _ in 0..300 {
        body.push_str("<div>");
    }
    body.push_str("deep");
    for _ in 0..300 {
        body.push_str("</div>");
    }
    let fixture = TempFixture::new(&basic_document(&body), Some("div { display:block; }"));
    let error = run_package_with_options(&fixture.root, options(false, false))
        .expect_err("adapter depth limit should reject the fixture");
    assert!(matches!(error, RuntimeError::LimitExceeded(_)));
}
