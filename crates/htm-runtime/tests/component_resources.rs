use htm_runtime::{
    ComponentRasterFormat, ComponentResourceName, ExperimentOptions, LiveDocument,
    LiveDocumentKind, MAX_COMPONENT_RASTER_SOURCE_BYTES, MAX_COMPONENT_RESOURCE_DECLARATIONS,
    MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE, PackageErrorKind, PackageSnapshotLoader,
    ViewportSpec, run_package_with_options,
};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};
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
            "htmshell-component-resource-test-{}-{serial}",
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

    fn package(&self, exports: &str, definitions: &str, body: &str) {
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
        let document = format!("<!doctype html><html><body>{body}</body></html>");
        self.write("index.html", &document);
        self.write(
            "panel.html",
            format!(
                "<!doctype html><html><body><main id=\"panel-root\">{body}<button id=\"overlay-toggle\">Open</button></main></body></html>"
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

fn resource(name: &str, source: &str) -> String {
    format!(
        r#"{{"name":{},"type":"raster","source":{}}}"#,
        serde_json::to_string(name).unwrap(),
        serde_json::to_string(source).unwrap()
    )
}

fn export(name: &str, resources: &str) -> String {
    format!(
        r#"{{"name":"{name}","source":"components/components.html","inputs":[],"slots":[],"styles":[],"resources":{resources}}}"#
    )
}

fn definition(name: &str, body: &str) -> String {
    format!(r#"<template data-htm-component="{name}">{body}</template>"#)
}

fn rgba_pixels() -> Vec<u8> {
    vec![
        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255,
    ]
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&rgba_pixels(), 2, 2, ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

fn jpeg() -> Vec<u8> {
    let rgb = rgba_pixels()
        .chunks_exact(4)
        .flat_map(|pixel| pixel[..3].iter().copied())
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 95)
        .write_image(&rgb, 2, 2, ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

fn webp() -> Vec<u8> {
    let mut bytes = Vec::new();
    WebPEncoder::new_lossless(&mut bytes)
        .write_image(&rgba_pixels(), 2, 2, ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

fn animated_webp() -> Vec<u8> {
    let still = webp();
    let image_chunk = &still[12..];
    let mut chunks = Vec::new();
    chunks.extend_from_slice(b"VP8X");
    chunks.extend_from_slice(&10u32.to_le_bytes());
    chunks.extend_from_slice(&[
        0x12, 0, 0, 0, // animation and alpha flags
        1, 0, 0, // canvas width minus one
        1, 0, 0, // canvas height minus one
    ]);
    chunks.extend_from_slice(b"ANIM");
    chunks.extend_from_slice(&6u32.to_le_bytes());
    chunks.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let mut frame = Vec::new();
    frame.extend_from_slice(&[
        0, 0, 0, // x
        0, 0, 0, // y
        1, 0, 0, // width minus one
        1, 0, 0, // height minus one
        10, 0, 0, // duration
        0, // blend and disposal flags
    ]);
    frame.extend_from_slice(image_chunk);
    chunks.extend_from_slice(b"ANMF");
    chunks.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&frame);
    if frame.len() % 2 == 1 {
        chunks.push(0);
    }
    let mut encoded = Vec::new();
    encoded.extend_from_slice(b"RIFF");
    encoded.extend_from_slice(&(4u32 + chunks.len() as u32).to_le_bytes());
    encoded.extend_from_slice(b"WEBP");
    encoded.extend_from_slice(&chunks);
    encoded
}

fn animated_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(1, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&rgba_pixels()).unwrap();
    }
    bytes
}

fn load_error(fixture: &Fixture) -> PackageErrorKind {
    PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap_err()
        .kind()
}

#[test]
fn component_resource_name_grammar_and_reservations_are_exact() {
    for valid in ["speaker", "speaker-icon", "album-placeholder", "status-2"] {
        assert_eq!(ComponentResourceName::parse(valid).unwrap().as_str(), valid);
    }
    for invalid in [
        "",
        "Speaker",
        "2-status",
        "-speaker",
        "speaker-",
        "speaker--icon",
        "speaker.icon",
        "speaker/icon",
        "speaker:icon",
        "speaker icon",
        "spéaker",
        "resource",
        "component",
        "input",
        "slot",
        "style",
        "state",
        "action",
        "service",
        "surface",
        "host",
        "repeat",
        "htm-speaker",
        "xml-speaker",
        "xlink-speaker",
    ] {
        assert!(ComponentResourceName::parse(invalid).is_err(), "{invalid}");
    }
    assert!(ComponentResourceName::parse(&"a".repeat(65)).is_err());
}

#[test]
fn png_jpeg_and_static_webp_decode_once_and_materialize_owned_usages() {
    let fixture = Fixture::new();
    fixture.write("assets/shared.png", png());
    fixture.write("assets/photo.jpg", jpeg());
    fixture.write("assets/alpha.webp", webp());
    let resources = format!(
        "[{},{},{},{}]",
        resource("shared", "assets/shared.png"),
        resource("shared-alias", "assets/shared.png"),
        resource("photo", "assets/photo.jpg"),
        resource("alpha", "assets/alpha.webp")
    );
    fixture.package(
        &format!(
            "[{},{}]",
            export("media-card", &resources),
            export(
                "badge-card",
                &format!("[{}]", resource("shared", "assets/shared.png"))
            )
        ),
        &format!(
            "{}{}",
            definition(
                "media-card",
                r#"<article><img src="resource:shared" alt=""><img src="resource:shared-alias" alt=""><img src="resource:photo" alt=""><img src="resource:alpha" alt=""></article>"#,
            ),
            definition(
                "badge-card",
                r#"<span><img src="resource:shared" alt=""></span>"#
            )
        ),
        r#"<htm-use component="media-card"></htm-use><htm-use component="media-card"></htm-use><htm-use component="badge-card"></htm-use>"#,
    );
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&fixture.root)
        .unwrap();
    assert_eq!(snapshot.component_resources().sources().len(), 3);
    assert_eq!(snapshot.component_resources().associations().len(), 5);
    assert_eq!(snapshot.component_resources().totals().source_read_count, 3);
    assert_eq!(
        snapshot.component_resources().totals().source_decode_count,
        3
    );
    assert_eq!(
        snapshot
            .component_resources()
            .sources()
            .iter()
            .map(|source| source.format())
            .collect::<Vec<_>>(),
        vec![
            ComponentRasterFormat::WebP,
            ComponentRasterFormat::Jpeg,
            ComponentRasterFormat::Png
        ]
    );
    for source in snapshot.component_resources().sources() {
        assert_eq!((source.width(), source.height()), (2, 2));
        assert_eq!(source.decoded_bytes(), 16);
        assert!(
            source
                .semantic_version()
                .deterministic_string()
                .starts_with("component-raster-v1:")
        );
    }
    let png_source = snapshot
        .component_resources()
        .sources()
        .iter()
        .find(|source| source.format() == ComponentRasterFormat::Png)
        .unwrap();
    assert_eq!(png_source.rgba8().as_slice(), rgba_pixels());
    let jpeg_source = snapshot
        .component_resources()
        .sources()
        .iter()
        .find(|source| source.format() == ComponentRasterFormat::Jpeg)
        .unwrap();
    assert!(
        jpeg_source
            .rgba8()
            .chunks_exact(4)
            .all(|pixel| pixel[3] == 255)
    );
    let webp_source = snapshot
        .component_resources()
        .sources()
        .iter()
        .find(|source| source.format() == ComponentRasterFormat::WebP)
        .unwrap();
    assert_eq!(
        webp_source
            .rgba8()
            .chunks_exact(4)
            .map(|pixel| pixel[3])
            .collect::<Vec<_>>(),
        vec![255, 128, 0, 255]
    );

    let run = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            viewport: ViewportSpec {
                logical_width: 128,
                logical_height: 96,
                scale_factor: 1.0,
                color_space: "sRGB",
                dynamic_range: "SDR",
            },
            render_png: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(run.component_resource_usages.len(), 9);
    assert_eq!(
        run.component_resource_usages
            .iter()
            .map(|usage| usage.source().id())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert!(run.artifacts.iter().any(|artifact| artifact.png.is_some()));
}

#[test]
fn declaration_limits_duplicates_and_types_reject_atomically() {
    let maximum = Fixture::new();
    maximum.write("assets/pixel.png", png());
    let declarations = (0..MAX_COMPONENT_RESOURCE_DECLARATIONS)
        .map(|index| resource(&format!("resource-{index}"), "assets/pixel.png"))
        .collect::<Vec<_>>()
        .join(",");
    maximum.package(
        &format!("[{}]", export("maximum-card", &format!("[{declarations}]"))),
        &definition("maximum-card", r#"<img src="resource:resource-0">"#),
        r#"<htm-use component="maximum-card"></htm-use>"#,
    );
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&maximum.root)
            .is_ok()
    );

    let overflow = Fixture::new();
    overflow.write("assets/pixel.png", png());
    let declarations = (0..=MAX_COMPONENT_RESOURCE_DECLARATIONS)
        .map(|index| resource(&format!("resource-{index}"), "assets/pixel.png"))
        .collect::<Vec<_>>()
        .join(",");
    overflow.package(
        &format!(
            "[{}]",
            export("overflow-card", &format!("[{declarations}]"))
        ),
        &definition("overflow-card", "<div>overflow</div>"),
        "",
    );
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentResourceDeclarationLimit
    );

    let duplicate = Fixture::new();
    duplicate.package(
        &format!(
            "[{}]",
            export(
                "duplicate-card",
                &format!(
                    "[{},{}]",
                    resource("icon", "assets/a.png"),
                    resource("icon", "assets/b.png")
                )
            )
        ),
        &definition("duplicate-card", "<div>duplicate</div>"),
        "",
    );
    assert_eq!(
        load_error(&duplicate),
        PackageErrorKind::DuplicateComponentResourceName
    );

    let unknown = Fixture::new();
    unknown.package(
        r#"[{"name":"unknown-card","source":"components/components.html","resources":[{"name":"icon","type":"svg","source":"assets/icon.svg"}]}]"#,
        &definition("unknown-card", "<div>unknown</div>"),
        "",
    );
    assert_eq!(
        load_error(&unknown),
        PackageErrorKind::UnsupportedComponentResourceType
    );
}

#[test]
fn manifest_entry_shape_and_compatibility_documents_remain_strict() {
    let unknown_field = Fixture::new();
    unknown_field.package(
        r#"[{"name":"shape-card","source":"components/components.html","resources":[{"name":"icon","type":"raster","source":"assets/icon.png","renderer":"gpu"}]}]"#,
        &definition("shape-card", "<div>unused</div>"),
        "",
    );
    assert_eq!(load_error(&unknown_field), PackageErrorKind::UnknownField);

    let schema_v1 = Fixture::new();
    schema_v1.write(
        "shell.json",
        r#"{
          "version":1,
          "id":"legacy-shell",
          "components":[{
            "name":"legacy-card",
            "source":"components/components.html",
            "resources":[{"name":"icon","type":"raster","source":"assets/icon.png"}]
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
        r#"<!doctype html><html><body><img src="resource:icon"></body></html>"#,
    );
    assert_eq!(
        PackageSnapshotLoader::new()
            .load_headless(&legacy.root)
            .unwrap_err()
            .kind(),
        PackageErrorKind::ComponentResourceReferenceWrongOwner
    );
}

#[test]
fn package_association_and_unique_source_limits_are_exact() {
    fn association_package(fixture: &Fixture, count: usize) {
        fixture.write("a", png());
        let mut exports = Vec::new();
        let mut definitions = String::new();
        let mut remaining = count;
        let mut component_index = 0usize;
        while remaining > 0 {
            let declarations = remaining.min(MAX_COMPONENT_RESOURCE_DECLARATIONS);
            let resources = (0..declarations)
                .map(|index| resource(&format!("r{index}"), "a"))
                .collect::<Vec<_>>()
                .join(",");
            let name = format!("card-{component_index}");
            exports.push(export(&name, &format!("[{resources}]")));
            definitions.push_str(&definition(&name, "<div>bounded</div>"));
            component_index += 1;
            remaining -= declarations;
        }
        fixture.package(&format!("[{}]", exports.join(",")), &definitions, "");
    }

    let maximum = Fixture::new();
    association_package(&maximum, 4_096);
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&maximum.root)
        .unwrap();
    assert_eq!(snapshot.component_resources().associations().len(), 4_096);
    assert_eq!(snapshot.component_resources().sources().len(), 1);

    let overflow = Fixture::new();
    association_package(&overflow, 4_097);
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentResourceAssociationLimit
    );

    fn source_package(fixture: &Fixture, count: usize) {
        let mut exports = Vec::new();
        let mut definitions = String::new();
        let mut remaining = count;
        let mut source_index = 0usize;
        let mut component_index = 0usize;
        while remaining > 0 {
            let declarations = remaining.min(MAX_COMPONENT_RESOURCE_DECLARATIONS);
            let resources = (0..declarations)
                .map(|local_index| {
                    let name = format!("r{local_index}");
                    let path = format!("r{source_index}");
                    fixture.write(&path, png());
                    source_index += 1;
                    resource(&name, &path)
                })
                .collect::<Vec<_>>()
                .join(",");
            let name = format!("source-card-{component_index}");
            exports.push(export(&name, &format!("[{resources}]")));
            definitions.push_str(&definition(&name, "<div>bounded</div>"));
            component_index += 1;
            remaining -= declarations;
        }
        fixture.package(&format!("[{}]", exports.join(",")), &definitions, "");
    }

    let maximum = Fixture::new();
    source_package(&maximum, MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE);
    let snapshot = PackageSnapshotLoader::new()
        .load_headless(&maximum.root)
        .unwrap();
    assert_eq!(
        snapshot.component_resources().sources().len(),
        MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE
    );
    assert_eq!(
        snapshot.component_resources().totals().source_decode_count,
        MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE
    );

    let overflow = Fixture::new();
    source_package(&overflow, MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE + 1);
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentResourceUniqueSourceLimit
    );
}

#[test]
fn component_resource_paths_are_normalized_bounded_and_package_relative() {
    for invalid in [
        "",
        ".",
        "./assets/icon.png",
        "../icon.png",
        "assets/../icon.png",
        "/assets/icon.png",
        "assets\\icon.png",
        "assets//icon.png",
        "assets/icon.png?size=2",
        "assets/icon.png#fragment",
        "assets/%69con.png",
        "https://example.invalid/icon.png",
        "file:assets/icon.png",
        "$HOME/icon.png",
    ] {
        let fixture = Fixture::new();
        fixture.package(
            &format!(
                "[{}]",
                export("path-card", &format!("[{}]", resource("icon", invalid)))
            ),
            &definition("path-card", "<div>path</div>"),
            "",
        );
        assert!(
            matches!(
                load_error(&fixture),
                PackageErrorKind::InvalidComponentResourcePath
                    | PackageErrorKind::ComponentResourcePathLengthLimit
            ),
            "accepted `{invalid}`"
        );
    }

    let valid_deep = (0..31)
        .map(|index| format!("d{index}"))
        .chain(std::iter::once("icon.png".to_owned()))
        .collect::<Vec<_>>()
        .join("/");
    let fixture = Fixture::new();
    fixture.write(&valid_deep, png());
    fixture.package(
        &format!(
            "[{}]",
            export("path-card", &format!("[{}]", resource("icon", &valid_deep)))
        ),
        &definition("path-card", "<div>path</div>"),
        "",
    );
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .is_ok()
    );

    let deep = (0..32)
        .map(|index| format!("d{index}"))
        .chain(std::iter::once("icon.png".to_owned()))
        .collect::<Vec<_>>()
        .join("/");
    let fixture = Fixture::new();
    fixture.package(
        &format!(
            "[{}]",
            export("path-card", &format!("[{}]", resource("icon", &deep)))
        ),
        &definition("path-card", "<div>path</div>"),
        "",
    );
    assert_eq!(
        load_error(&fixture),
        PackageErrorKind::ComponentResourcePathDepthLimit
    );

    let overlong = format!("{}.png", "a".repeat(509));
    let fixture = Fixture::new();
    fixture.package(
        &format!(
            "[{}]",
            export("path-card", &format!("[{}]", resource("icon", &overlong)))
        ),
        &definition("path-card", "<div>path</div>"),
        "",
    );
    assert_eq!(
        load_error(&fixture),
        PackageErrorKind::ComponentResourcePathLengthLimit
    );

    let exact = format!("{}/{}/{}", "a".repeat(250), "b".repeat(250), "c".repeat(10));
    assert_eq!(exact.len(), 512);
    let fixture = Fixture::new();
    fixture.write(&exact, png());
    fixture.package(
        &format!(
            "[{}]",
            export("path-card", &format!("[{}]", resource("icon", &exact)))
        ),
        &definition("path-card", "<div>path</div>"),
        "",
    );
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .is_ok()
    );
}

#[test]
fn malformed_unknown_and_wrong_owner_references_reject() {
    for value in [
        "resource:",
        "resource:unknown",
        "resource:speaker/icon",
        "resource:speaker?size=2",
        "resource:speaker#fragment",
        "resource:%73peaker",
        "Resource:speaker",
        "assets/speaker.png",
        "https://example.invalid/speaker.png",
        "data:image/png;base64,AA==",
    ] {
        let fixture = Fixture::new();
        fixture.write("assets/speaker.png", png());
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "reference-card",
                    &format!("[{}]", resource("speaker", "assets/speaker.png"))
                )
            ),
            &definition(
                "reference-card",
                &format!(r#"<img src="{}">"#, html_escape(value)),
            ),
            r#"<htm-use component="reference-card"></htm-use>"#,
        );
        assert!(
            PackageSnapshotLoader::new()
                .load_headless(&fixture.root)
                .is_err(),
            "accepted `{value}`"
        );
    }

    let root = Fixture::new();
    root.package("[]", "", r#"<img src="resource:speaker">"#);
    assert_eq!(
        load_error(&root),
        PackageErrorKind::ComponentResourceReferenceWrongOwner
    );

    for body in [
        r#"<img src="resource:speaker" srcset="resource:speaker 1x">"#,
        r#"<svg><image href="resource:speaker"></image></svg>"#,
        r#"<video src="resource:speaker"></video>"#,
    ] {
        let fixture = Fixture::new();
        fixture.write("assets/speaker.png", png());
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "reference-card",
                    &format!("[{}]", resource("speaker", "assets/speaker.png"))
                )
            ),
            &definition("reference-card", body),
            r#"<htm-use component="reference-card"></htm-use>"#,
        );
        assert_eq!(
            load_error(&fixture),
            PackageErrorKind::ComponentResourceNotSupported
        );
    }
}

#[test]
fn fallback_nested_and_projected_resources_keep_their_definition_owner() {
    let fixture = Fixture::new();
    fixture.write("assets/parent.png", png());
    fixture.write("assets/child.webp", webp());
    fixture.write("assets/root.png", png());
    let exports = format!(
        r#"[{{
          "name":"parent-card",
          "source":"components/components.html",
          "inputs":[],
          "slots":[],
          "styles":[],
          "resources":[{}]
        }},{{
          "name":"child-card",
          "source":"components/components.html",
          "inputs":[],
          "slots":[{{"name":"default","required":false}}],
          "styles":[],
          "resources":[{}]
        }}]"#,
        resource("icon", "assets/parent.png"),
        resource("icon", "assets/child.webp")
    );
    let definitions = format!(
        "{}{}",
        definition(
            "parent-card",
            r#"<section>
              <img src="resource:icon" alt="parent">
              <htm-use component="child-card"><img src="resource:icon" alt="projected"></htm-use>
              <htm-use component="child-card"></htm-use>
            </section>"#
        ),
        r#"<template data-htm-component="child-card"><div><slot><img src="resource:icon" alt="fallback"></slot><img src="resource:icon" alt="child"></div></template>"#
    );
    fixture.package(
        &exports,
        &definitions,
        r#"<htm-use component="parent-card"></htm-use>"#,
    );
    let run = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            render_png: false,
            run_interaction: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(run.component_resource_usages.len(), 5);
    let paths = run
        .component_resource_usages
        .iter()
        .map(|usage| usage.source().path().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths
            .iter()
            .filter(|path| **path == "assets/parent.png")
            .count(),
        2
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| **path == "assets/child.webp")
            .count(),
        3
    );
    let projected_parent = run
        .component_resource_usages
        .iter()
        .find(|usage| {
            usage.source().path().as_str() == "assets/parent.png"
                && usage.template_source_ordinal() > 1
        })
        .unwrap();
    assert_eq!(
        projected_parent.instance().definition().name().as_str(),
        "parent-card"
    );
}

#[test]
fn gif_other_formats_and_corrupt_sources_reject_even_when_unused() {
    let animated = animated_webp();
    let apng = animated_png();
    for (bytes, expected) in [
        (
            b"GIF89a\x01\x00\x01\x00".as_slice(),
            PackageErrorKind::ComponentResourceFormatUnsupported,
        ),
        (
            b"BM\x00\x00\x00\x00".as_slice(),
            PackageErrorKind::ComponentResourceFormatUnsupported,
        ),
        (
            b"\x89PNG\r\n\x1a\ntruncated".as_slice(),
            PackageErrorKind::ComponentResourceDecodeFailure,
        ),
        (
            b"\xff\xd8\xfftruncated".as_slice(),
            PackageErrorKind::ComponentResourceDecodeFailure,
        ),
        (
            b"RIFF\x04\x00\x00\x00WEBP".as_slice(),
            PackageErrorKind::ComponentResourceDecodeFailure,
        ),
        (
            animated.as_slice(),
            PackageErrorKind::ComponentResourceAnimatedFormatUnsupported,
        ),
        (
            apng.as_slice(),
            PackageErrorKind::ComponentResourceAnimatedFormatUnsupported,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write("assets/image.bin", bytes);
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "unused-card",
                    &format!("[{}]", resource("image", "assets/image.bin"))
                )
            ),
            &definition("unused-card", "<div>unused</div>"),
            "",
        );
        assert_eq!(load_error(&fixture), expected);
    }
}

#[test]
fn encoded_source_limit_is_enforced_before_decode() {
    let maximum = Fixture::new();
    let mut bytes = png();
    bytes.resize(MAX_COMPONENT_RASTER_SOURCE_BYTES as usize, 0);
    maximum.write("assets/image.png", bytes);
    maximum.package(
        &format!(
            "[{}]",
            export(
                "maximum-card",
                &format!("[{}]", resource("image", "assets/image.png"))
            )
        ),
        &definition("maximum-card", "<div>unused</div>"),
        "",
    );
    assert!(
        PackageSnapshotLoader::new()
            .load_headless(&maximum.root)
            .is_ok()
    );

    let overflow = Fixture::new();
    overflow.write(
        "assets/image.png",
        vec![0u8; MAX_COMPONENT_RASTER_SOURCE_BYTES as usize + 1],
    );
    overflow.package(
        &format!(
            "[{}]",
            export(
                "overflow-card",
                &format!("[{}]", resource("image", "assets/image.png"))
            )
        ),
        &definition("overflow-card", "<div>unused</div>"),
        "",
    );
    assert_eq!(
        load_error(&overflow),
        PackageErrorKind::ComponentResourceSourceTooLarge
    );
}

#[test]
fn failed_resource_candidate_retains_last_known_good_and_later_valid_publishes_once() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    let resources = format!("[{}]", resource("icon", "assets/icon.png"));
    fixture.package(
        &format!("[{}]", export("atomic-card", &resources)),
        &definition("atomic-card", r#"<img src="resource:icon">"#),
        r#"<htm-use component="atomic-card"></htm-use>"#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let first = loader.load_headless(&fixture.root).unwrap();
    let first_generation = first.generation();

    fixture.write("assets/icon.png", b"GIF89a\x01\x00\x01\x00");
    assert_eq!(
        loader.load_headless(&fixture.root).unwrap_err().kind(),
        PackageErrorKind::ComponentResourceFormatUnsupported
    );
    let current = loader.current().unwrap();
    assert!(std::sync::Arc::ptr_eq(current, &first));
    assert_eq!(current.generation(), first_generation);

    fixture.write("assets/icon.png", png());
    let second = loader.load_headless(&fixture.root).unwrap();
    assert_ne!(second.generation(), first_generation);
    assert_eq!(second.component_resources().totals().source_decode_count, 1);
}

#[test]
fn component_raster_pixels_and_natural_layout_match_the_root_image_path() {
    fn render(fixture: &Fixture) -> Vec<u8> {
        run_package_with_options(
            &fixture.root,
            ExperimentOptions {
                viewport: ViewportSpec {
                    logical_width: 48,
                    logical_height: 48,
                    scale_factor: 1.0,
                    color_space: "sRGB",
                    dynamic_range: "SDR",
                },
                render_png: true,
                run_interaction: false,
                output_directory: None,
            },
        )
        .unwrap()
        .artifacts
        .into_iter()
        .find_map(|artifact| artifact.png)
        .unwrap()
    }

    let root = Fixture::new();
    root.write("assets/icon.png", png());
    root.package("[]", "", r#"<img src="assets/icon.png" alt="">"#);

    let component = Fixture::new();
    component.write("assets/icon.png", png());
    component.package(
        &format!(
            "[{}]",
            export(
                "image-card",
                &format!("[{}]", resource("icon", "assets/icon.png"))
            )
        ),
        &definition("image-card", r#"<img src="resource:icon" alt="">"#),
        r#"<htm-use component="image-card"></htm-use>"#,
    );

    assert_eq!(render(&component), render(&root));
}

#[test]
fn component_raster_foreground_effects_render_through_the_existing_pipeline() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    fixture.package(
        &format!(
            "[{}]",
            export(
                "image-card",
                &format!("[{}]", resource("icon", "assets/icon.png"))
            )
        ),
        &definition(
            "image-card",
            r#"<div><img src="resource:icon" alt="" style="width:32px;height:32px;filter:contrast(1.04) drop-shadow(0 2px 4px rgba(4,8,18,0.45))"><img src="resource:icon" alt="" style="width:32px;height:32px;filter:contrast(1.04) drop-shadow(0 2px 4px rgba(4,8,18,0.45))"><img src="resource:icon" alt="" style="position:absolute;top:1000px;width:32px;height:32px;filter:contrast(1.04) drop-shadow(0 2px 4px rgba(4,8,18,0.45))"></div>"#,
        ),
        r#"<htm-use component="image-card"></htm-use>"#,
    );
    let run = run_package_with_options(
        &fixture.root,
        ExperimentOptions {
            viewport: ViewportSpec {
                logical_width: 64,
                logical_height: 64,
                scale_factor: 1.0,
                color_space: "sRGB",
                dynamic_range: "SDR",
            },
            render_png: true,
            run_interaction: false,
            output_directory: None,
        },
    )
    .unwrap();
    assert!(run.artifacts.iter().any(|artifact| artifact.png.is_some()));
}

#[test]
fn live_documents_share_sources_but_use_fresh_output_local_usage_identities() {
    let fixture = Fixture::new();
    fixture.write("assets/icon.png", png());
    fixture.package(
        &format!(
            "[{}]",
            export(
                "image-card",
                &format!("[{}]", resource("icon", "assets/icon.png"))
            )
        ),
        &definition("image-card", r#"<img src="resource:icon" alt="">"#),
        r#"<htm-use component="image-card"></htm-use>"#,
    );
    let mut loader = PackageSnapshotLoader::new();
    let snapshot = loader
        .load_manifest(fixture.root.join("shell.json"))
        .unwrap();
    let panel = snapshot
        .root_manifest()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| surface.id() == "panel")
        .unwrap()
        .clone();
    let first = LiveDocument::load_surface_snapshot(
        std::sync::Arc::clone(&snapshot),
        &panel,
        LiveDocumentKind::Panel,
        128,
        96,
    )
    .unwrap();
    let second = LiveDocument::load_surface_snapshot(
        std::sync::Arc::clone(&snapshot),
        &panel,
        LiveDocumentKind::Panel,
        128,
        96,
    )
    .unwrap();
    assert_eq!(first.component_resource_usages().len(), 1);
    assert_eq!(second.component_resource_usages().len(), 1);
    assert!(std::sync::Arc::ptr_eq(
        first.component_resource_usages()[0].source(),
        second.component_resource_usages()[0].source()
    ));
    assert_ne!(
        first.component_resource_usages()[0]
            .id()
            .deterministic_string(),
        second.component_resource_usages()[0]
            .id()
            .deterministic_string()
    );
}

#[cfg(unix)]
#[test]
fn symlink_files_and_path_components_reject() {
    use std::os::unix::fs::symlink;

    let file = Fixture::new();
    file.write("outside.png", png());
    fs::create_dir_all(file.root.join("assets")).unwrap();
    symlink(
        file.root.join("outside.png"),
        file.root.join("assets/icon.png"),
    )
    .unwrap();
    file.package(
        &format!(
            "[{}]",
            export(
                "symlink-card",
                &format!("[{}]", resource("icon", "assets/icon.png"))
            )
        ),
        &definition("symlink-card", "<div>unused</div>"),
        "",
    );
    assert_eq!(
        load_error(&file),
        PackageErrorKind::ComponentResourceSymlink
    );

    let component = Fixture::new();
    component.write("real/icon.png", png());
    symlink(component.root.join("real"), component.root.join("assets")).unwrap();
    component.package(
        &format!(
            "[{}]",
            export(
                "symlink-card",
                &format!("[{}]", resource("icon", "assets/icon.png"))
            )
        ),
        &definition("symlink-card", "<div>unused</div>"),
        "",
    );
    assert_eq!(
        load_error(&component),
        PackageErrorKind::ComponentResourceSymlink
    );
}

#[test]
fn directories_reject_as_special_files_without_blocking() {
    let directory = Fixture::new();
    fs::create_dir_all(directory.root.join("assets/icon.png")).unwrap();
    directory.package(
        &format!(
            "[{}]",
            export(
                "special-card",
                &format!("[{}]", resource("icon", "assets/icon.png"))
            )
        ),
        &definition("special-card", "<div>unused</div>"),
        "",
    );
    assert_eq!(
        load_error(&directory),
        PackageErrorKind::ComponentResourceSpecialFile
    );
}

#[test]
#[ignore = "release-only component raster measurements and bounded stress"]
fn component_raster_release_measurement_and_stress_probe() {
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

    fn single_source(encoded: Vec<u8>, path: &str) -> Fixture {
        let fixture = Fixture::new();
        fixture.write(path, encoded);
        fixture.package(
            &format!(
                "[{}]",
                export("image-card", &format!("[{}]", resource("image", path)))
            ),
            &definition("image-card", r#"<img src="resource:image">"#),
            r#"<htm-use component="image-card"></htm-use>"#,
        );
        fixture
    }

    fn repeated_fixture(instances: usize) -> Fixture {
        let fixture = Fixture::new();
        fixture.write("image.png", png());
        fixture.package(
            &format!(
                "[{}]",
                export(
                    "image-card",
                    &format!("[{}]", resource("image", "image.png"))
                )
            ),
            &definition("image-card", r#"<img src="resource:image">"#),
            &r#"<htm-use component="image-card"></htm-use>"#.repeat(instances),
        );
        fixture
    }

    let before = process_counts();
    let png_fixture = single_source(png(), "image.png");
    let (png_us, png_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&png_fixture.root)
            .unwrap()
    });
    let jpeg_fixture = single_source(jpeg(), "image.jpg");
    let (jpeg_us, jpeg_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&jpeg_fixture.root)
            .unwrap()
    });
    let webp_fixture = single_source(webp(), "image.webp");
    let (webp_us, webp_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&webp_fixture.root)
            .unwrap()
    });

    let declarations = Fixture::new();
    declarations.write("image.png", png());
    let resources = (0..MAX_COMPONENT_RESOURCE_DECLARATIONS)
        .map(|index| resource(&format!("r{index}"), "image.png"))
        .collect::<Vec<_>>()
        .join(",");
    declarations.package(
        &format!("[{}]", export("image-card", &format!("[{resources}]"))),
        &definition("image-card", r#"<img src="resource:r0">"#),
        r#"<htm-use component="image-card"></htm-use>"#,
    );
    let (declarations_us, declarations_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&declarations.root)
            .unwrap()
    });

    let sources = Fixture::new();
    let mut exports = Vec::new();
    let mut definitions = String::new();
    for group in
        0..MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE.div_ceil(MAX_COMPONENT_RESOURCE_DECLARATIONS)
    {
        let start = group * MAX_COMPONENT_RESOURCE_DECLARATIONS;
        let end = MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE
            .min(start + MAX_COMPONENT_RESOURCE_DECLARATIONS);
        let declarations = (start..end)
            .map(|index| {
                let path = format!("r{index}.png");
                sources.write(&path, png());
                resource(&format!("r{}", index - start), &path)
            })
            .collect::<Vec<_>>()
            .join(",");
        let name = format!("image-card-{group}");
        exports.push(export(&name, &format!("[{declarations}]")));
        definitions.push_str(&definition(&name, "<div>unused</div>"));
    }
    sources.package(&format!("[{}]", exports.join(",")), &definitions, "");
    let (sources_us, sources_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&sources.root)
            .unwrap()
    });

    let thousand = repeated_fixture(1_000);
    let (thousand_us, thousand_snapshot) = micros(|| {
        PackageSnapshotLoader::new()
            .load_headless(&thousand.root)
            .unwrap()
    });
    let loader = PackageSnapshotLoader::new();
    let (candidate_us, candidate) =
        micros(|| loader.build_headless_candidate(&png_fixture.root).unwrap());
    let mut loader = PackageSnapshotLoader::new();
    let (publication_us, published) = micros(|| loader.publish(candidate).unwrap());
    let (serialization_us, serialized) = micros(|| published.deterministic_json().unwrap());
    assert!(!serialized.is_empty());

    let invalid = single_source(animated_webp(), "image.webp");
    let (invalid_500_us, ()) = micros(|| {
        let loader = PackageSnapshotLoader::new();
        for _ in 0..500 {
            assert!(loader.build_headless_candidate(&invalid.root).is_err());
        }
    });
    let publications = single_source(png(), "image.png");
    let (publication_500_us, ()) = micros(|| {
        let mut loader = PackageSnapshotLoader::new();
        for generation in 1..=500 {
            let snapshot = loader.load_headless(&publications.root).unwrap();
            assert_eq!(snapshot.generation().get(), generation);
        }
    });

    assert_eq!(png_snapshot.component_resources().sources().len(), 1);
    assert_eq!(jpeg_snapshot.component_resources().sources().len(), 1);
    assert_eq!(webp_snapshot.component_resources().sources().len(), 1);
    assert_eq!(
        declarations_snapshot
            .component_resources()
            .associations()
            .len(),
        MAX_COMPONENT_RESOURCE_DECLARATIONS
    );
    assert_eq!(
        sources_snapshot.component_resources().sources().len(),
        MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE
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
            .component_resources()
            .totals()
            .source_decode_count,
        1
    );
    let after = process_counts();
    eprintln!(
        "component_raster_measurements_us png={png_us} jpeg={jpeg_us} webp={webp_us} declarations_32={declarations_us} sources_256={sources_us} thousand_instances={thousand_us} candidate={candidate_us} publication={publication_us} serialization={serialization_us} invalid_500={invalid_500_us} publications_500={publication_500_us} before_fd={} after_fd={} before_threads={} after_threads={} before_rss_kib={:?} after_rss_kib={:?}",
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

fn html_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}
