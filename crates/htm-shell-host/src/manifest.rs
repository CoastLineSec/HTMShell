use crate::ShellHostError;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

const SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_SURFACE_TEMPLATES: usize = 16;
const MAX_ID_BYTES: usize = 64;
const MAX_DOCUMENT_PATH_BYTES: usize = 512;
const MAX_PANEL_THICKNESS: u32 = 512;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManifestMeasurements {
    pub parse_us: u64,
    pub validation_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputScope {
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Panel,
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTemplate {
    pub edge: PanelEdge,
    pub thickness: u32,
    pub reserve_space: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEdge {
    Top,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayTemplate {
    pub initially_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfacePreset {
    Panel(PanelTemplate),
    Overlay(OverlayTemplate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTemplate {
    id: String,
    document: PathBuf,
    canonical_document: PathBuf,
    outputs: OutputScope,
    preset: SurfacePreset,
    namespace: String,
}

impl SurfaceTemplate {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn document(&self) -> &Path {
        &self.document
    }

    pub fn canonical_document(&self) -> &Path {
        &self.canonical_document
    }

    pub fn outputs(&self) -> OutputScope {
        self.outputs
    }

    pub fn kind(&self) -> SurfaceKind {
        match self.preset {
            SurfacePreset::Panel(_) => SurfaceKind::Panel,
            SurfacePreset::Overlay(_) => SurfaceKind::Overlay,
        }
    }

    pub fn panel(&self) -> Option<&PanelTemplate> {
        match &self.preset {
            SurfacePreset::Panel(panel) => Some(panel),
            SurfacePreset::Overlay(_) => None,
        }
    }

    pub fn overlay(&self) -> Option<&OverlayTemplate> {
        match &self.preset {
            SurfacePreset::Overlay(overlay) => Some(overlay),
            SurfacePreset::Panel(_) => None,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellManifest {
    pub version: u32,
    pub id: String,
    pub surfaces: Vec<SurfaceTemplate>,
}

#[derive(Debug, Clone)]
pub struct ValidatedManifest {
    source: PathBuf,
    package_root: PathBuf,
    manifest: ShellManifest,
    parse_count: u32,
    measurements: ManifestMeasurements,
}

impl ValidatedManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ShellHostError> {
        let requested = path.as_ref();
        let source = requested.canonicalize().map_err(|error| {
            manifest_error(format!(
                "cannot resolve manifest `{}`: {error}",
                requested.display()
            ))
        })?;
        let metadata = source.metadata().map_err(|error| {
            manifest_error(format!(
                "cannot inspect manifest `{}`: {error}",
                source.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(manifest_error(format!(
                "manifest `{}` is not a regular file",
                source.display()
            )));
        }
        if metadata.len() > MAX_MANIFEST_BYTES {
            return Err(manifest_error(format!(
                "manifest is {} bytes; limit is {MAX_MANIFEST_BYTES}",
                metadata.len()
            )));
        }
        let package_root = source
            .parent()
            .ok_or_else(|| manifest_error("manifest has no package directory"))?
            .canonicalize()
            .map_err(|error| {
                manifest_error(format!(
                    "cannot resolve manifest package directory: {error}"
                ))
            })?;
        let bytes = std::fs::read(&source).map_err(|error| {
            manifest_error(format!(
                "cannot read manifest `{}`: {error}",
                source.display()
            ))
        })?;
        let parse_started = Instant::now();
        let raw: RawShellManifest = serde_json::from_slice(&bytes)
            .map_err(|error| manifest_error(format!("invalid JSON at {error}")))?;
        let parse_us = elapsed_us(parse_started);
        let validation_started = Instant::now();
        let manifest = validate(raw, &package_root)?;
        let validation_us = elapsed_us(validation_started);
        Ok(Self {
            source,
            package_root,
            manifest,
            parse_count: 1,
            measurements: ManifestMeasurements {
                parse_us,
                validation_us,
            },
        })
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn manifest(&self) -> &ShellManifest {
        &self.manifest
    }

    pub fn parse_count(&self) -> u32 {
        self.parse_count
    }

    pub fn measurements(&self) -> ManifestMeasurements {
        self.measurements
    }

    pub fn surface(&self, id: &str) -> Option<&SurfaceTemplate> {
        self.manifest
            .surfaces
            .iter()
            .find(|surface| surface.id == id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawShellManifest {
    version: u32,
    id: String,
    surfaces: Vec<RawSurfaceTemplate>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum RawSurfaceTemplate {
    Panel(RawPanelTemplate),
    Overlay(RawOverlayTemplate),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPanelTemplate {
    id: String,
    document: String,
    outputs: RawOutputScope,
    edge: RawPanelEdge,
    thickness: u32,
    reserve_space: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawOverlayTemplate {
    id: String,
    document: String,
    outputs: RawOutputScope,
    initially_open: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawOutputScope {
    All,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawPanelEdge {
    Top,
}

fn validate(raw: RawShellManifest, package_root: &Path) -> Result<ShellManifest, ShellHostError> {
    if raw.version != SCHEMA_VERSION {
        return Err(manifest_error(format!(
            "unsupported schema version {}; expected {SCHEMA_VERSION}",
            raw.version
        )));
    }
    validate_id("manifest id", &raw.id)?;
    if raw.surfaces.is_empty() {
        return Err(manifest_error("surfaces must not be empty"));
    }
    if raw.surfaces.len() > MAX_SURFACE_TEMPLATES {
        return Err(manifest_error(format!(
            "manifest has {} surfaces; limit is {MAX_SURFACE_TEMPLATES}",
            raw.surfaces.len()
        )));
    }
    let mut ids = BTreeSet::new();
    let mut panel_count = 0usize;
    let mut overlay_count = 0usize;
    let mut surfaces = Vec::with_capacity(raw.surfaces.len());
    for raw_surface in raw.surfaces {
        let (id, document, outputs, preset) = match raw_surface {
            RawSurfaceTemplate::Panel(panel) => {
                panel_count += 1;
                if panel.thickness == 0 || panel.thickness > MAX_PANEL_THICKNESS {
                    return Err(manifest_error(format!(
                        "surface `{}` thickness {} is outside 1..={MAX_PANEL_THICKNESS}",
                        panel.id, panel.thickness
                    )));
                }
                (
                    panel.id,
                    panel.document,
                    output_scope(panel.outputs),
                    SurfacePreset::Panel(PanelTemplate {
                        edge: match panel.edge {
                            RawPanelEdge::Top => PanelEdge::Top,
                        },
                        thickness: panel.thickness,
                        reserve_space: panel.reserve_space,
                    }),
                )
            }
            RawSurfaceTemplate::Overlay(overlay) => {
                overlay_count += 1;
                (
                    overlay.id,
                    overlay.document,
                    output_scope(overlay.outputs),
                    SurfacePreset::Overlay(OverlayTemplate {
                        initially_open: overlay.initially_open,
                    }),
                )
            }
        };
        validate_id("surface id", &id)?;
        if !ids.insert(id.clone()) {
            return Err(manifest_error(format!("duplicate surface id `{id}`")));
        }
        let relative = validate_document_path(&id, &document)?;
        let canonical_document = package_root
            .join(&relative)
            .canonicalize()
            .map_err(|error| {
                manifest_error(format!(
                    "surface `{id}` document `{}` cannot be resolved: {error}",
                    relative.display()
                ))
            })?;
        if !canonical_document.starts_with(package_root) {
            return Err(manifest_error(format!(
                "surface `{id}` document resolves outside the manifest package"
            )));
        }
        if !canonical_document.is_file() {
            return Err(manifest_error(format!(
                "surface `{id}` document `{}` is not a regular file",
                relative.display()
            )));
        }
        let namespace = format!("htmshell-{}-{id}", raw.id);
        surfaces.push(SurfaceTemplate {
            id,
            document: relative,
            canonical_document,
            outputs,
            preset,
            namespace,
        });
    }
    if panel_count != 1 || overlay_count != 1 {
        return Err(manifest_error(format!(
            "schema version 1 requires exactly one panel and one overlay; found {panel_count} panel(s) and {overlay_count} overlay(s)"
        )));
    }
    surfaces.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ShellManifest {
        version: raw.version,
        id: raw.id,
        surfaces,
    })
}

fn output_scope(raw: RawOutputScope) -> OutputScope {
    match raw {
        RawOutputScope::All => OutputScope::All,
    }
}

fn validate_id(field: &str, id: &str) -> Result<(), ShellHostError> {
    if id.is_empty() {
        return Err(manifest_error(format!("{field} must not be empty")));
    }
    if id.len() > MAX_ID_BYTES {
        return Err(manifest_error(format!(
            "{field} `{id}` exceeds {MAX_ID_BYTES} bytes"
        )));
    }
    let valid = id.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || (byte == b'-' && index > 0 && index + 1 < id.len())
    });
    if !valid {
        return Err(manifest_error(format!(
            "{field} `{id}` must use lowercase ASCII letters, digits, and interior hyphens"
        )));
    }
    Ok(())
}

fn validate_document_path(surface_id: &str, value: &str) -> Result<PathBuf, ShellHostError> {
    if value.is_empty() || value.len() > MAX_DOCUMENT_PATH_BYTES {
        return Err(manifest_error(format!(
            "surface `{surface_id}` document path must contain 1..={MAX_DOCUMENT_PATH_BYTES} bytes"
        )));
    }
    if value.contains("://") || value.starts_with("//") {
        return Err(manifest_error(format!(
            "surface `{surface_id}` document must be a local relative path"
        )));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(manifest_error(format!(
            "surface `{surface_id}` document must remain inside the manifest package"
        )));
    }
    Ok(path.to_path_buf())
}

fn manifest_error(message: impl Into<String>) -> ShellHostError {
    ShellHostError::Manifest(message.into())
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(manifest: &str) -> Self {
            let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "htmshell-manifest-test-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("panel.html"), "<main>panel</main>").unwrap();
            fs::write(root.join("overlay.html"), "<main>overlay</main>").unwrap();
            fs::write(root.join("shell.json"), manifest).unwrap();
            Self { root }
        }

        fn load(&self) -> Result<ValidatedManifest, ShellHostError> {
            ValidatedManifest::load(self.root.join("shell.json"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn valid() -> &'static str {
        r#"{
          "version": 1,
          "id": "portable-shell-demo",
          "surfaces": [
            {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true},
            {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}
          ]
        }"#
    }

    #[test]
    fn valid_manifest_is_normalized_and_counted_once() {
        let fixture = Fixture::new(valid());
        let manifest = fixture.load().unwrap();
        assert_eq!(manifest.parse_count(), 1);
        assert_eq!(manifest.manifest().version, 1);
        assert_eq!(manifest.manifest().surfaces.len(), 2);
        assert_eq!(manifest.manifest().surfaces[0].id(), "overlay");
        assert_eq!(manifest.manifest().surfaces[1].id(), "panel");
        let panel = manifest.surface("panel").unwrap();
        assert_eq!(panel.namespace(), "htmshell-portable-shell-demo-panel");
        assert_eq!(panel.panel().unwrap().thickness, 52);
        assert!(panel.panel().unwrap().reserve_space);
    }

    #[test]
    fn schema_ids_and_duplicate_ids_are_rejected() {
        for replacement in [
            ("\"version\": 1", "\"version\": 2"),
            ("portable-shell-demo", ""),
            ("portable-shell-demo", "Invalid_ID"),
            ("\"id\":\"overlay\"", "\"id\":\"panel\""),
        ] {
            let fixture = Fixture::new(&valid().replacen(replacement.0, replacement.1, 1));
            assert!(fixture.load().is_err());
        }
    }

    #[test]
    fn unknown_fields_kinds_and_scopes_are_rejected() {
        let cases = [
            valid().replace("\"version\": 1,", "\"version\": 1, \"extra\": true,"),
            valid().replace("\"kind\":\"panel\"", "\"kind\":\"background\""),
            valid().replace("\"outputs\":\"all\"", "\"outputs\":\"primary\""),
            valid().replace("\"edge\":\"top\"", "\"edge\":\"bottom\""),
        ];
        for manifest in cases {
            let fixture = Fixture::new(&manifest);
            assert!(fixture.load().is_err());
        }
    }

    #[test]
    fn document_paths_are_local_and_contained() {
        for document in [
            "/tmp/panel.html",
            "../panel.html",
            "https://example/panel.html",
        ] {
            let manifest = valid().replace("panel.html", document);
            let fixture = Fixture::new(&manifest);
            assert!(fixture.load().is_err(), "accepted {document}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new(&valid().replace("panel.html", "escaped.html"));
        let outside = fixture.root.parent().unwrap().join(format!(
            "htmshell-outside-{}",
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&outside, "<main>outside</main>").unwrap();
        symlink(&outside, fixture.root.join("escaped.html")).unwrap();
        assert!(fixture.load().is_err());
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn malformed_limits_and_invalid_panel_state_are_rejected() {
        let cases = [
            valid().replace("\"thickness\":52", "\"thickness\":0"),
            valid().replace("\"thickness\":52", "\"thickness\":513"),
            valid().replace("\"initiallyOpen\":false", "\"initiallyOpen\":\"no\""),
            valid().replace("overlay.html", "missing.html"),
        ];
        for manifest in cases {
            let fixture = Fixture::new(&manifest);
            assert!(fixture.load().is_err());
        }
    }

    #[test]
    fn manifest_and_field_size_limits_are_enforced() {
        let fixture = Fixture::new(valid());
        fs::write(
            fixture.root.join("shell.json"),
            vec![b' '; MAX_MANIFEST_BYTES as usize + 1],
        )
        .unwrap();
        assert!(fixture.load().unwrap_err().to_string().contains("limit"));

        let long_id = "a".repeat(MAX_ID_BYTES + 1);
        let fixture = Fixture::new(&valid().replace("portable-shell-demo", &long_id));
        assert!(fixture.load().is_err());

        let long_path = format!("{}.html", "a".repeat(MAX_DOCUMENT_PATH_BYTES));
        let fixture = Fixture::new(&valid().replace("panel.html", &long_path));
        assert!(fixture.load().is_err());
    }

    #[test]
    fn excessive_surface_count_is_rejected_before_semantic_expansion() {
        let panel = r#"{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true}"#;
        let mut surfaces = vec![panel.to_owned()];
        for index in 0..MAX_SURFACE_TEMPLATES {
            surfaces.push(format!(
                r#"{{"id":"overlay-{index}","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}}"#
            ));
        }
        let manifest = format!(
            r#"{{"version":1,"id":"portable-shell-demo","surfaces":[{}]}}"#,
            surfaces.join(",")
        );
        let fixture = Fixture::new(&manifest);
        assert!(fixture.load().unwrap_err().to_string().contains("limit"));
    }

    #[test]
    fn missing_fields_and_malformed_utf8_are_rejected() {
        let fixture = Fixture::new(&valid().replace("\"id\": \"portable-shell-demo\",", ""));
        assert!(fixture.load().is_err());

        let fixture = Fixture::new(valid());
        fs::write(fixture.root.join("shell.json"), [0xff, 0xfe]).unwrap();
        assert!(fixture.load().is_err());
    }

    #[test]
    fn surface_order_does_not_change_normalized_identity() {
        let fixture = Fixture::new(valid());
        let first = fixture.load().unwrap();
        let reversed = valid().replace(
            r#"{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true},
            {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}"#,
            r#"{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false},
            {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true}"#,
        );
        fs::write(fixture.root.join("shell.json"), reversed).unwrap();
        let second = fixture.load().unwrap();
        let left: Vec<_> = first
            .manifest()
            .surfaces
            .iter()
            .map(|surface| (surface.id(), surface.namespace()))
            .collect();
        let right: Vec<_> = second
            .manifest()
            .surfaces
            .iter()
            .map(|surface| (surface.id(), surface.namespace()))
            .collect();
        assert_eq!(left, right);
    }
}
