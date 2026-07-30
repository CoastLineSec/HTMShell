use htm_runtime::{
    ComponentStylesheetPath, ExperimentOptions, MAX_COMPONENT_STYLESHEET_BYTES,
    MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE, MAX_COMPONENT_STYLESHEET_PATH_BYTES,
    MAX_COMPONENT_STYLESHEETS, PackageErrorKind, PackageSnapshotLoader, Phase, ViewportSpec,
    run_package_with_options,
};
use std::fs;
use std::path::PathBuf;
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
            "htmshell-component-style-test-{}-{serial}",
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

    fn write_package(&self, exports: &str, definitions: &str, index: &str) {
        self.write(
            "shell.json",
            format!(
                r#"{{
                  "version":2,
                  "package":{{"id":"org.example.shell","kind":"shell","version":"1.0.0"}},
                  "dependencies":[],
                  "components":{exports},
                  "surfaces":[
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":96,"reserveSpace":true}},
                    {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}}
                  ]
                }}"#
            ),
        );
        self.write("index.html", document(index));
        self.write("panel.html", document(index));
        self.write(
            "overlay.html",
            r#"<!doctype html><html><body><main id="overlay-card"><p id="overlay-status">Closed</p><button id="overlay-close">Close</button><button id="overlay-action">Act</button></main></body></html>"#,
        );
        self.write("components/components.html", definitions);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn document(content: &str) -> String {
    format!(
        r#"<!doctype html><html><head><link rel="stylesheet" href="style.css"></head><body>{content}</body></html>"#
    )
}

fn export(name: &str, slots: &str, styles: &str) -> String {
    format!(
        r#"{{"name":"{name}","source":"components/components.html","inputs":[],"slots":{slots},"styles":{styles}}}"#
    )
}

fn definition(name: &str, content: &str) -> String {
    format!(r#"<template data-htm-component="{name}">{content}</template>"#)
}

fn load_error(fixture: &Fixture) -> PackageErrorKind {
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap_err()
        .kind()
}

#[test]
fn manifest_styles_are_ordered_bounded_and_share_sources() {
    let fixture = Fixture::new();
    let styles = (0..MAX_COMPONENT_STYLESHEETS)
        .map(|index| format!(r#""components/sheet-{index:02}.css""#))
        .collect::<Vec<_>>()
        .join(",");
    fixture.write_package(
        &format!(
            "[{},{}]",
            export("first-card", "[]", &format!("[{styles}]")),
            export("second-card", "[]", r#"["components/sheet-00.css"]"#)
        ),
        &format!(
            "{}{}",
            definition("first-card", "<div>first</div>"),
            definition("second-card", "<div>second</div>")
        ),
        r#"<htm-use component="first-card"></htm-use><htm-use component="second-card"></htm-use>"#,
    );
    fixture.write("style.css", "");
    for index in 0..MAX_COMPONENT_STYLESHEETS {
        fixture.write(
            &format!("components/sheet-{index:02}.css"),
            format!(".value {{ padding: {index}px; }}"),
        );
    }
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();
    let component_export = &snapshot.packages()[0].components()[0];
    assert_eq!(
        component_export
            .styles()
            .iter()
            .map(ComponentStylesheetPath::as_str)
            .collect::<Vec<_>>(),
        (0..MAX_COMPONENT_STYLESHEETS)
            .map(|index| format!("components/sheet-{index:02}.css"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot.component_styles().sources().len(),
        MAX_COMPONENT_STYLESHEETS
    );
    assert_eq!(
        snapshot.component_styles().totals().source_read_count,
        MAX_COMPONENT_STYLESHEETS
    );
    assert_eq!(
        snapshot.component_styles().totals().source_parse_count,
        MAX_COMPONENT_STYLESHEETS
    );
    assert_eq!(
        snapshot.component_styles().associations().len(),
        MAX_COMPONENT_STYLESHEETS + 1
    );

    let overflow = Fixture::new();
    let styles = (0..=MAX_COMPONENT_STYLESHEETS)
        .map(|index| format!(r#""components/sheet-{index:02}.css""#))
        .collect::<Vec<_>>()
        .join(",");
    overflow.write_package(
        &format!(
            "[{}]",
            export("overflow-card", "[]", &format!("[{styles}]"))
        ),
        &definition("overflow-card", "<div>overflow</div>"),
        "",
    );
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentStylesheetDeclarationLimit
    );

    let invalid = Fixture::new();
    invalid.write_package(
        r#"[{"name":"invalid-card","source":"components/components.html","inputs":[],"slots":[],"styles":[{"path":"components/style.css"}]}]"#,
        &definition("invalid-card", "<div>invalid</div>"),
        "",
    );
    assert_eq!(
        load_error(&invalid),
        PackageErrorKind::InvalidComponentStylesheetDeclaration
    );

    let duplicate = Fixture::new();
    duplicate.write_package(
        &format!(
            "[{}]",
            export(
                "duplicate-card",
                "[]",
                r#"["components/style.css","components/style.css"]"#
            )
        ),
        &definition("duplicate-card", "<div>duplicate</div>"),
        "",
    );
    assert_eq!(
        load_error(&duplicate),
        PackageErrorKind::DuplicateComponentStylesheet
    );
}

#[test]
fn package_unique_source_limit_counts_shared_paths_once() {
    fn package(unique: usize) -> Fixture {
        let fixture = Fixture::new();
        let mut exports = Vec::new();
        let mut definitions = String::new();
        let mut index = String::new();
        for group in 0..unique.div_ceil(MAX_COMPONENT_STYLESHEETS) {
            let start = group * MAX_COMPONENT_STYLESHEETS;
            let end = unique.min(start + MAX_COMPONENT_STYLESHEETS);
            let styles = (start..end)
                .map(|index| format!(r#""components/source-{index:02}.css""#))
                .collect::<Vec<_>>()
                .join(",");
            let name = format!("group-{group:02}");
            exports.push(export(&name, "[]", &format!("[{styles}]")));
            definitions.push_str(&definition(&name, "<div>group</div>"));
            index.push_str(&format!(r#"<htm-use component="{name}"></htm-use>"#));
        }
        fixture.write_package(&format!("[{}]", exports.join(",")), &definitions, &index);
        fixture.write("style.css", "");
        for index in 0..unique {
            fixture.write(
                &format!("components/source-{index:02}.css"),
                format!(".source-{index:02} {{ opacity: 1; }}"),
            );
        }
        fixture
    }

    let maximum = package(MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE);
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&maximum.root)
        .unwrap();
    assert_eq!(
        snapshot.component_styles().sources().len(),
        MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE
    );

    let overflow = package(MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE + 1);
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentStylesheetPackageFileLimit
    );
}

#[test]
fn stylesheet_paths_are_normalized_local_and_length_bounded() {
    for invalid in [
        "",
        ".",
        "../style.css",
        "components/../style.css",
        "/style.css",
        "components\\style.css",
        "components//style.css",
        "https://example.invalid/style.css",
        "//server/style.css",
    ] {
        let fixture = Fixture::new();
        fixture.write_package(
            &format!(
                "[{}]",
                export(
                    "path-card",
                    "[]",
                    &format!("[{}]", serde_json::to_string(invalid).unwrap())
                )
            ),
            &definition("path-card", "<div>path</div>"),
            "",
        );
        assert_eq!(
            load_error(&fixture),
            PackageErrorKind::InvalidComponentStylesheetPath,
            "accepted `{invalid}`"
        );
    }

    let overlong = "a".repeat(MAX_COMPONENT_STYLESHEET_PATH_BYTES + 1);
    let fixture = Fixture::new();
    fixture.write_package(
        &format!(
            "[{}]",
            export(
                "path-card",
                "[]",
                &format!("[{}]", serde_json::to_string(&overlong).unwrap())
            )
        ),
        &definition("path-card", "<div>path</div>"),
        "",
    );
    assert_eq!(
        load_error(&fixture),
        PackageErrorKind::InvalidComponentStylesheetPath
    );

    let first = "a".repeat(200);
    let second = "b".repeat(200);
    let third = format!("{}.css", "c".repeat(106));
    let maximum_path = format!("{first}/{second}/{third}");
    assert_eq!(maximum_path.len(), MAX_COMPONENT_STYLESHEET_PATH_BYTES);
    let fixture = Fixture::new();
    fixture.write_package(
        &format!(
            "[{}]",
            export(
                "path-card",
                "[]",
                &format!("[{}]", serde_json::to_string(&maximum_path).unwrap())
            )
        ),
        &definition("path-card", "<div>path</div>"),
        "<htm-use component=\"path-card\"></htm-use>",
    );
    fixture.write("style.css", "");
    fixture.write(&maximum_path, ".path { color: red; }");
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .is_ok()
    );
}

#[test]
fn stylesheet_files_reject_missing_directory_symlink_and_oversize_sources() {
    let missing = Fixture::new();
    missing.write_package(
        &format!(
            "[{}]",
            export("file-card", "[]", r#"["components/missing.css"]"#)
        ),
        &definition("file-card", "<div>file</div>"),
        "",
    );
    assert_eq!(
        load_error(&missing),
        PackageErrorKind::ComponentStylesheetMissing
    );

    let directory = Fixture::new();
    directory.write_package(
        &format!(
            "[{}]",
            export("file-card", "[]", r#"["components/directory"]"#)
        ),
        &definition("file-card", "<div>file</div>"),
        "",
    );
    fs::create_dir_all(directory.root.join("components/directory")).unwrap();
    assert_eq!(
        load_error(&directory),
        PackageErrorKind::ComponentStylesheetSpecialFile
    );

    #[cfg(unix)]
    {
        let symlink = Fixture::new();
        symlink.write_package(
            &format!(
                "[{}]",
                export("file-card", "[]", r#"["components/link.css"]"#)
            ),
            &definition("file-card", "<div>file</div>"),
            "",
        );
        symlink.write("outside.css", ".file { color: red; }");
        fs::create_dir_all(symlink.root.join("components")).unwrap();
        std::os::unix::fs::symlink(
            symlink.root.join("outside.css"),
            symlink.root.join("components/link.css"),
        )
        .unwrap();
        assert_eq!(
            load_error(&symlink),
            PackageErrorKind::ComponentStylesheetSymlink
        );

        let symlink_component = Fixture::new();
        symlink_component.write_package(
            &format!(
                "[{}]",
                export("file-card", "[]", r#"["components/link/style.css"]"#)
            ),
            &definition("file-card", "<div>file</div>"),
            "",
        );
        symlink_component.write("real/style.css", ".file { color: red; }");
        std::os::unix::fs::symlink(
            symlink_component.root.join("real"),
            symlink_component.root.join("components/link"),
        )
        .unwrap();
        assert_eq!(
            load_error(&symlink_component),
            PackageErrorKind::ComponentStylesheetSymlink
        );
    }

    let maximum = Fixture::new();
    maximum.write_package(
        &format!(
            "[{}]",
            export("file-card", "[]", r#"["components/maximum.css"]"#)
        ),
        &definition("file-card", "<div>file</div>"),
        "<htm-use component=\"file-card\"></htm-use>",
    );
    maximum.write("style.css", "");
    maximum.write(
        "components/maximum.css",
        vec![b' '; MAX_COMPONENT_STYLESHEET_BYTES as usize],
    );
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&maximum.root)
            .is_ok()
    );

    let oversize = Fixture::new();
    oversize.write_package(
        &format!(
            "[{}]",
            export("file-card", "[]", r#"["components/large.css"]"#)
        ),
        &definition("file-card", "<div>file</div>"),
        "",
    );
    oversize.write(
        "components/large.css",
        vec![b' '; MAX_COMPONENT_STYLESHEET_BYTES as usize + 1],
    );
    assert_eq!(
        load_error(&oversize),
        PackageErrorKind::ComponentStylesheetTooLarge
    );
}

#[test]
fn forbidden_component_css_is_typed_and_never_fetched() {
    let cases = [
        (
            "@import \"theme.css\";",
            PackageErrorKind::ComponentStylesheetForbiddenImport,
        ),
        (
            ".card { background-image: url(asset.png); }",
            PackageErrorKind::ComponentStylesheetForbiddenUrlResource,
        ),
        (
            "@font-face { font-family: demo; src: url(font.woff2); }",
            PackageErrorKind::ComponentStylesheetForbiddenFontResource,
        ),
        (
            ":host { color: red; }",
            PackageErrorKind::ComponentStylesheetForbiddenHostSelector,
        ),
        (
            "::slotted(.item) { color: red; }",
            PackageErrorKind::ComponentStylesheetForbiddenSlottedSelector,
        ),
        (
            "::part(label) { color: red; }",
            PackageErrorKind::ComponentStylesheetForbiddenShadowSelector,
        ),
        (
            ".card { color: ; }",
            PackageErrorKind::ComponentStylesheetParseFailure,
        ),
    ];
    for (css, expected) in cases {
        let fixture = Fixture::new();
        fixture.write_package(
            &format!(
                "[{}]",
                export("style-card", "[]", r#"["components/style.css"]"#)
            ),
            &definition("style-card", "<div>style</div>"),
            "",
        );
        fixture.write("components/style.css", css);
        assert_eq!(load_error(&fixture), expected, "CSS was `{css}`");
    }
}

fn scoped_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.write_package(
        &format!(
            "[{},{},{}]",
            export(
                "outer-card",
                r#"[{"name":"default","required":false}]"#,
                r#"["components/outer.css","components/outer-override.css"]"#
            ),
            export(
                "child-card",
                r#"[{"name":"content","required":false}]"#,
                r#"["components/child.css"]"#
            ),
            export("plain-card", "[]", "[]")
        ),
        &format!(
            "{}{}{}",
            definition(
                "outer-card",
                r#"<section class="outer-wrapper"><div class="shared case-outer">outer</div><slot><div class="shared case-fallback">fallback</div></slot><htm-use component="child-card"><span class="shared case-parent-projected" slot="content">parent projected</span></htm-use><htm-use component="plain-card"></htm-use></section>"#
            ),
            definition(
                "child-card",
                r#"<div class="shared child-shared case-child">child<slot name="content"></slot></div>"#
            ),
            definition(
                "plain-card",
                r#"<div class="shared case-plain">plain boundary</div>"#
            )
        ),
        r#"<main><div class="shared case-root">root</div><htm-use component="outer-card"></htm-use><htm-use component="outer-card"><div class="shared case-projected">projected</div></htm-use></main>"#,
    );
    fixture.write(
        "style.css",
        r#".shared { display:block; width:40px; height:20px; background:rgb(255,0,0); }"#,
    );
    fixture.write(
        "components/outer.css",
        r#".shared { display:block; width:40px; height:20px; background:rgb(0,0,200); }.outer-wrapper .child-shared { background:rgb(255,255,0); }.case-parent-projected[slot] { background:rgb(255,0,255); }"#,
    );
    fixture.write(
        "components/outer-override.css",
        r#".shared { background:rgb(0,0,255); }"#,
    );
    fixture.write(
        "components/child.css",
        r#".shared { display:block; width:40px; height:20px; background:rgb(0,255,0); }"#,
    );
    fixture
}

fn nodes_with_class<'a>(
    node: &'a htm_runtime::DiagnosticNode,
    class: &str,
    result: &mut Vec<&'a htm_runtime::DiagnosticNode>,
) {
    if node.classes.iter().any(|candidate| candidate == class) {
        result.push(node);
    }
    for child in &node.children {
        nodes_with_class(child, class, result);
    }
}

fn assert_background(
    root: &htm_runtime::DiagnosticNode,
    class: &str,
    expected_count: usize,
    expected: [f32; 4],
) {
    let mut nodes = Vec::new();
    nodes_with_class(root, class, &mut nodes);
    assert_eq!(nodes.len(), expected_count);
    let backgrounds = nodes
        .into_iter()
        .map(|node| node.background_srgba.unwrap())
        .collect::<Vec<_>>();
    assert!(
        backgrounds.iter().all(|background| *background == expected),
        "class `{class}` backgrounds were {backgrounds:?}"
    );
}

#[test]
fn public_component_styles_isolate_root_nested_projection_and_fallback_nodes() {
    let fixture = scoped_fixture();
    let run = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            viewport: ViewportSpec {
                logical_width: 320,
                logical_height: 200,
                ..Default::default()
            },
            render_png: true,
            ..Default::default()
        },
    )
    .unwrap();
    let report = &run
        .artifacts
        .iter()
        .find(|artifact| artifact.phase == Phase::Initial)
        .unwrap()
        .report;
    assert_background(&report.tree, "case-root", 1, [1.0, 0.0, 0.0, 1.0]);
    assert_background(&report.tree, "case-projected", 1, [1.0, 0.0, 0.0, 1.0]);
    assert_background(&report.tree, "case-outer", 2, [0.0, 0.0, 1.0, 1.0]);
    assert_background(&report.tree, "case-fallback", 1, [0.0, 0.0, 1.0, 1.0]);
    assert_background(
        &report.tree,
        "case-parent-projected",
        2,
        [0.0, 0.0, 1.0, 1.0],
    );
    assert_background(&report.tree, "case-child", 2, [0.0, 1.0, 0.0, 1.0]);
    assert_background(&report.tree, "case-plain", 2, [0.0, 0.0, 0.0, 0.0]);
    assert!(
        run.artifacts[0]
            .png
            .as_ref()
            .is_some_and(|png| !png.is_empty())
    );
}

#[test]
fn stylesheet_candidate_failure_retains_last_known_good_generation() {
    let fixture = scoped_fixture();
    let mut loader = PackageSnapshotLoader::new();
    let current = loader.load_headless(&fixture.root).unwrap();
    let generation = current.generation();
    fixture.write("components/outer.css", ":host { color:red; }");
    assert_eq!(
        loader.load_headless(&fixture.root).unwrap_err().kind(),
        PackageErrorKind::ComponentStylesheetForbiddenHostSelector
    );
    assert_eq!(loader.current().unwrap().generation(), generation);
    assert!(std::sync::Arc::ptr_eq(loader.current().unwrap(), &current));

    fixture.write(
        "components/outer.css",
        ".shared { background:rgb(0,0,200); }",
    );
    let replacement = loader.load_headless(&fixture.root).unwrap();
    assert_ne!(replacement.generation(), generation);
}

#[test]
#[ignore = "release-only component stylesheet measurements and bounded stress"]
fn component_stylesheet_release_measurement_and_stress_probe() {
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

    fn repeated_fixture(instances: usize) -> Fixture {
        let fixture = Fixture::new();
        fixture.write_package(
            &format!(
                "[{}]",
                export("style-item", "[]", r#"["components/item.css"]"#)
            ),
            &definition("style-item", r#"<span class="stress-node">styled</span>"#),
            &format!(
                "<main>{}</main>",
                r#"<htm-use component="style-item"></htm-use>"#.repeat(instances)
            ),
        );
        fixture.write("style.css", "");
        fixture.write(
            "components/item.css",
            ".stress-node { display:block; color:blue; }",
        );
        fixture
    }

    fn stylesheet_count_fixture(count: usize) -> Fixture {
        let fixture = Fixture::new();
        let styles = (0..count)
            .map(|index| format!(r#""components/sheet-{index:02}.css""#))
            .collect::<Vec<_>>()
            .join(",");
        fixture.write_package(
            &format!("[{}]", export("style-item", "[]", &format!("[{styles}]"))),
            &definition("style-item", "<span>styled</span>"),
            r#"<htm-use component="style-item"></htm-use>"#,
        );
        fixture.write("style.css", "");
        for index in 0..count {
            fixture.write(
                &format!("components/sheet-{index:02}.css"),
                format!(".sheet-{index:02} {{ opacity:1; }}"),
            );
        }
        fixture
    }

    fn unique_source_fixture(count: usize) -> Fixture {
        let fixture = Fixture::new();
        let mut exports = Vec::new();
        let mut definitions = String::new();
        let mut uses = String::new();
        for group in 0..count.div_ceil(MAX_COMPONENT_STYLESHEETS) {
            let start = group * MAX_COMPONENT_STYLESHEETS;
            let end = count.min(start + MAX_COMPONENT_STYLESHEETS);
            let styles = (start..end)
                .map(|index| format!(r#""components/source-{index:02}.css""#))
                .collect::<Vec<_>>()
                .join(",");
            let name = format!("style-group-{group:02}");
            exports.push(export(&name, "[]", &format!("[{styles}]")));
            definitions.push_str(&definition(&name, "<span>styled</span>"));
            uses.push_str(&format!(r#"<htm-use component="{name}"></htm-use>"#));
        }
        fixture.write_package(&format!("[{}]", exports.join(",")), &definitions, &uses);
        fixture.write("style.css", "");
        for index in 0..count {
            fixture.write(
                &format!("components/source-{index:02}.css"),
                format!(".source-{index:02} {{ opacity:1; }}"),
            );
        }
        fixture
    }

    fn nested_fixture(depth: usize) -> Fixture {
        let fixture = Fixture::new();
        let exports = (0..depth)
            .map(|index| {
                export(
                    &format!("level-{index:02}"),
                    "[]",
                    r#"["components/shared.css"]"#,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let definitions = (0..depth)
            .map(|index| {
                let child = if index + 1 == depth {
                    "<span class=\"nested\">leaf</span>".to_owned()
                } else {
                    format!("<htm-use component=\"level-{:02}\"></htm-use>", index + 1)
                };
                definition(&format!("level-{index:02}"), &child)
            })
            .collect::<String>();
        fixture.write_package(
            &format!("[{exports}]"),
            &definitions,
            r#"<htm-use component="level-00"></htm-use>"#,
        );
        fixture.write("style.css", "");
        fixture.write("components/shared.css", ".nested { color:green; }");
        fixture
    }

    let before = process_counts();
    let one = stylesheet_count_fixture(1);
    let (one_us, one_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&one.root)
            .unwrap()
    });
    let sixteen = stylesheet_count_fixture(MAX_COMPONENT_STYLESHEETS);
    let (sixteen_us, sixteen_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&sixteen.root)
            .unwrap()
    });
    let sixty_four = unique_source_fixture(MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE);
    let (sixty_four_us, sixty_four_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&sixty_four.root)
            .unwrap()
    });
    let thousand = repeated_fixture(1_000);
    let (thousand_us, thousand_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&thousand.root)
            .unwrap()
    });
    let nested = nested_fixture(32);
    let (nested_us, nested_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&nested.root)
            .unwrap()
    });
    let loader = PackageSnapshotLoader::new();
    let (candidate_us, candidate) = micros(|| loader.build_headless_candidate(&one.root).unwrap());
    let mut loader = PackageSnapshotLoader::new();
    let (publication_us, published) = micros(|| loader.publish(candidate).unwrap());
    let (serialization_us, serialized) = micros(|| published.deterministic_json().unwrap());
    assert!(!serialized.is_empty());
    assert_eq!(one_snapshot.component_styles().sources().len(), 1);
    assert_eq!(
        sixteen_snapshot.component_styles().sources().len(),
        MAX_COMPONENT_STYLESHEETS
    );
    assert_eq!(
        sixty_four_snapshot.component_styles().sources().len(),
        MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE
    );
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
    assert_eq!(
        thousand_snapshot
            .component_styles()
            .totals()
            .source_parse_count,
        1
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
    assert_eq!(nested_snapshot.component_styles().sources().len(), 1);
    assert_eq!(nested_snapshot.component_styles().associations().len(), 32);
    let after = process_counts();
    eprintln!(
        "component_style_measurements_us one={one_us} sixteen={sixteen_us} sixty_four={sixty_four_us} thousand_instances={thousand_us} nested_32={nested_us} candidate={candidate_us} publication={publication_us} serialization={serialization_us} before_fd={} after_fd={} before_threads={} after_threads={} before_rss_kib={:?} after_rss_kib={:?}",
        before.0, after.0, before.1, after.1, before.2, after.2
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
