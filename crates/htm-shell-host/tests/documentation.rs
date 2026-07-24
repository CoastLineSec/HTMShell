use htm_runtime::{
    CLOCK_FORMAT_CONVERSIONS, CLOCK_FORMAT_FLAGS, CLOCK_PUBLIC_ATTRIBUTES, ClockFormat,
    ItemBindingKey, MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT, MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM,
    MAX_PIPEWIRE_PERCEPTUAL_VOLUME, MAX_RANGE_CONTROLS_PER_DOCUMENT, MAX_RANGE_CONTROLS_PER_ITEM,
    RepeatSource, ShellAction, StateBindingKey, StateToken, StateValueFormat,
    built_in_registry_names,
};
use htm_shell_host::{
    PerformanceDegradationReason, PipeWireNodeDirection, PipeWireNodeType, PowerProfile,
    SurfaceKind, UPowerDeviceState, UPowerDeviceType, ValidatedManifest,
};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    fn visit(directory: &Path, files: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(root, &mut files);
    files
}

fn public_reference_files(root: &Path) -> Vec<PathBuf> {
    let docs = root.join("docs");
    let mut files = vec![docs.join("README.md")];
    files.extend(markdown_files(&docs.join("guide")));
    files.extend(markdown_files(&docs.join("types")));
    files.sort();
    files
}

fn read_joined(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn documented_name(text: &str, name: &str) -> bool {
    text.contains(&format!("`{name}`")) || text.lines().any(|line| line.trim() == name)
}

fn markdown_targets(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("](") {
        let after = &remaining[start + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        targets.push(after[..end].trim());
        remaining = &after[end + 1..];
    }
    targets
}

#[test]
fn public_documentation_links_and_example_paths_resolve() {
    let root = workspace_root();
    let docs = root.join("docs");
    let mut files = markdown_files(&docs);
    files.push(root.join("README.md"));

    for file in files {
        let text = fs::read_to_string(&file).expect("Markdown is UTF-8");
        for target in markdown_targets(&text) {
            if target.is_empty()
                || target.starts_with('#')
                || target.starts_with("https://")
                || target.starts_with("http://")
                || target.starts_with("mailto:")
            {
                continue;
            }
            assert!(
                !target.contains(".internal"),
                "{} links to private material: {target}",
                file.display()
            );
            let target = target.split('#').next().expect("link target");
            let resolved = file.parent().expect("Markdown parent").join(target);
            assert!(
                resolved.exists(),
                "{} has unresolved link {target}",
                file.display()
            );
        }
    }

    for path in [
        "examples/static-panel/shell.json",
        "examples/static-panel/panel.html",
        "examples/static-panel/overlay.html",
        "examples/static-panel/style.css",
        "examples/static-panel/assets/shell.svg",
        "examples/static-panel/assets/overlay.svg",
        "examples/clock-panel/shell.json",
        "examples/formatted-clock/shell.json",
        "examples/formatted-clock/panel.html",
        "examples/formatted-clock/overlay.html",
        "examples/formatted-clock/style.css",
        "examples/battery-panel/shell.json",
        "examples/battery-panel/assets/battery.svg",
        "examples/power/shell.json",
        "examples/power/panel.html",
        "examples/power/overlay.html",
        "examples/power/style.css",
        "examples/power/assets/power.svg",
        "examples/audio-inspector/shell.json",
        "examples/audio-inspector/panel.html",
        "examples/audio-inspector/overlay.html",
        "examples/audio-inspector/style.css",
    ] {
        assert!(
            root.join(path).is_file(),
            "documented path is missing: {path}"
        );
    }
}

#[test]
fn public_documentation_style_is_safe_and_page_shape_is_stable() {
    let root = workspace_root();
    for file in public_reference_files(&root) {
        let text = fs::read_to_string(&file).expect("Markdown is UTF-8");
        for forbidden in [
            "\u{2013}",
            "\u{2014}",
            ".internal/",
            "Gate A",
            "Gate B",
            "Gate C",
            "Gate D",
            "/home/james/",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} contains forbidden public documentation text: {forbidden}",
                file.display()
            );
        }
        assert_eq!(
            text.lines().filter(|line| line.starts_with("# ")).count(),
            1,
            "{} must contain exactly one H1",
            file.display()
        );
    }
}

#[test]
fn typed_public_names_are_covered_by_the_reference() {
    let root = workspace_root();
    let reference = read_joined(&markdown_files(&root.join("docs/types")));
    assert_eq!(built_in_registry_names().len(), 7);

    for name in built_in_registry_names() {
        assert!(
            documented_name(&reference, name),
            "built-in element is undocumented: {name}"
        );
    }
    for key in StateBindingKey::ALL {
        assert!(
            documented_name(&reference, key.as_str()),
            "state key is undocumented: {}",
            key.as_str()
        );
    }
    for action in ShellAction::ALL {
        assert!(
            documented_name(&reference, action.as_str()),
            "action is undocumented: {}",
            action.as_str()
        );
    }
    for token in StateToken::ALL {
        assert!(
            documented_name(&reference, token.as_str()),
            "state token is undocumented: {}",
            token.as_str()
        );
    }
    for source in RepeatSource::ALL {
        assert!(
            documented_name(&reference, source.as_str()),
            "repeat source is undocumented: {}",
            source.as_str()
        );
    }
    for binding in ItemBindingKey::ALL {
        assert!(
            documented_name(&reference, binding.as_str()),
            "repeat item binding is undocumented: {}",
            binding.as_str()
        );
    }
    for format in StateValueFormat::ALL {
        assert!(
            documented_name(&reference, format.as_str()),
            "state-value format is undocumented: {}",
            format.as_str()
        );
    }
    for attribute in [
        "data-htm-source",
        "data-htm-local-id",
        "data-htm-format",
        "data-htm-enabled-bind",
        "data-htm-property-key",
        "data-htm-state",
        "min",
        "max",
        "step",
        "disabled",
        "value",
    ] {
        assert!(
            documented_name(&reference, attribute),
            "power declaration attribute is undocumented: {attribute}"
        );
    }

    for (limit, label) in [
        (
            MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT,
            "PipeWire audio controls per document",
        ),
        (
            MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM,
            "PipeWire audio controls per repeated item",
        ),
        (
            MAX_RANGE_CONTROLS_PER_DOCUMENT,
            "range controls per document",
        ),
        (
            MAX_RANGE_CONTROLS_PER_ITEM,
            "range controls per repeated item",
        ),
    ] {
        assert!(
            reference.contains(&limit.to_string()),
            "{label} limit is undocumented: {limit}"
        );
    }
    assert!(
        reference.contains(&format!(
            "runtime maximum is `{MAX_PIPEWIRE_PERCEPTUAL_VOLUME:.1}`"
        )),
        "PipeWire amplification maximum is undocumented"
    );
    assert!(
        reference.contains("clip or distort"),
        "PipeWire amplification warning is undocumented"
    );
    for device_type in UPowerDeviceType::ALL {
        assert!(
            documented_name(&reference, device_type.token().as_str()),
            "UPower device type is undocumented: {}",
            device_type.token().as_str()
        );
    }
    for state in UPowerDeviceState::ALL {
        assert!(
            documented_name(&reference, state.token().as_str()),
            "UPower device state is undocumented: {}",
            state.token().as_str()
        );
    }
    for profile in PowerProfile::ALL {
        assert!(
            documented_name(&reference, profile.wire()),
            "power profile is undocumented: {}",
            profile.wire()
        );
    }
    for degradation in PerformanceDegradationReason::ALL {
        assert!(
            documented_name(&reference, degradation.token().as_str()),
            "degradation reason is undocumented: {}",
            degradation.token().as_str()
        );
    }
    for node_type in PipeWireNodeType::ALL {
        assert!(
            documented_name(&reference, node_type.token().as_str()),
            "PipeWire node type is undocumented: {}",
            node_type.token().as_str()
        );
    }
    for direction in PipeWireNodeDirection::ALL {
        assert!(
            documented_name(&reference, direction.token().as_str()),
            "PipeWire direction is undocumented: {}",
            direction.token().as_str()
        );
    }
    for attribute in CLOCK_PUBLIC_ATTRIBUTES {
        assert!(
            documented_name(&reference, attribute),
            "clock attribute is undocumented: {attribute}"
        );
    }
    for conversion in CLOCK_FORMAT_CONVERSIONS {
        assert!(
            documented_name(&reference, conversion),
            "clock format conversion is undocumented: {conversion}"
        );
        ClockFormat::compile(conversion).unwrap_or_else(|error| {
            panic!("documented conversion {conversion} is invalid: {error}")
        });
    }
    for flag in CLOCK_FORMAT_FLAGS {
        assert!(
            documented_name(&reference, &flag.to_string()),
            "clock format flag is undocumented: {flag}"
        );
    }

    for (kind, name) in [
        (SurfaceKind::Panel, "panel"),
        (SurfaceKind::Overlay, "overlay"),
    ] {
        assert!(
            documented_name(&reference, name),
            "surface kind is undocumented: {kind:?}"
        );
    }
}

#[test]
fn documented_manifests_validate_without_wayland() {
    let root = workspace_root();
    for path in [
        "examples/static-panel/shell.json",
        "examples/clock-panel/shell.json",
        "examples/formatted-clock/shell.json",
        "examples/battery-panel/shell.json",
        "examples/power/shell.json",
        "examples/audio-inspector/shell.json",
    ] {
        let manifest = ValidatedManifest::load(root.join(path))
            .unwrap_or_else(|error| panic!("documented manifest {path} is invalid: {error}"));
        assert_eq!(manifest.parse_count(), 1);
        assert_eq!(manifest.manifest().version, 1);
        assert_eq!(manifest.manifest().surfaces.len(), 2);
    }
}

#[test]
fn audio_example_uses_only_the_typed_control_surface() {
    let root = workspace_root();
    let panel = fs::read_to_string(root.join("examples/audio-inspector/panel.html")).unwrap();
    let overlay = fs::read_to_string(root.join("examples/audio-inspector/overlay.html")).unwrap();
    let style = fs::read_to_string(root.join("examples/audio-inspector/style.css")).unwrap();
    let example = format!("{panel}\n{overlay}\n{style}");
    for name in [
        "range-control",
        "pipewire.audio.set_volume",
        "pipewire.audio.toggle_mute",
        "pipewire.default_sink",
        "pipewire.default_source",
        "item.volume",
        "item.mute_state",
        "item.audio_status",
        "pending",
        "failed",
    ] {
        assert!(example.contains(name), "audio example omits `{name}`");
    }
    for forbidden in [
        "pipewire.configured_sink\"",
        "pipewire.configured_source\"",
        "pipewire.links",
        "pipewire.peaks",
    ] {
        assert!(
            !overlay.contains(forbidden),
            "audio example uses forbidden control surface `{forbidden}`"
        );
    }
}

#[test]
fn private_clock_parity_matrix_is_resolved_when_present() {
    let path = workspace_root().join(".internal/research/system-clock-parity.md");
    if !path.is_file() {
        return;
    }
    let text = fs::read_to_string(&path).expect("private parity matrix is UTF-8");
    for capability in [
        "`date`",
        "`hours`",
        "`minutes`",
        "`seconds`",
        "`precision`",
        "Hours precision",
        "Minutes precision",
        "Seconds precision",
        "`enabled` default",
        "`enabled: false`",
        "Runtime enable",
        "Multiple independent instances",
        "Clock-change behavior",
        "Formatted date and time",
    ] {
        assert!(
            text.contains(capability),
            "SystemClock parity row is missing: {capability}"
        );
    }
    for unresolved in ["| PLANNED |", "UNRESOLVED", "TODO"] {
        assert!(
            !text.contains(unresolved),
            "SystemClock parity matrix remains unresolved: {unresolved}"
        );
    }
    for row in text
        .lines()
        .filter(|line| line.starts_with('|') && !line.starts_with("| ---"))
    {
        if row.contains("Quickshell capability") {
            continue;
        }
        assert!(
            row.ends_with("| EQUIVALENT |")
                || row.ends_with("| IMPLEMENTED |")
                || row.ends_with("| NOT APPLICABLE |")
                || row.ends_with("| DEFERRED BY JAMES |"),
            "invalid final parity classification: {row}"
        );
    }
}

#[test]
fn private_upower_parity_matrix_is_complete_when_present() {
    let path = workspace_root().join(".internal/research/upower-parity.md");
    if !path.is_file() {
        return;
    }
    let text = fs::read_to_string(&path).expect("private parity matrix is UTF-8");
    for capability in [
        "`UPower.displayDevice`",
        "`UPower.onBattery`",
        "`UPower.devices`",
        "`UPowerDevice.ready`",
        "`UPowerDevice.model`",
        "`UPowerDeviceState.toString`",
        "`UPowerDeviceType.toString`",
        "`PowerProfiles.profile`",
        "`PowerProfiles.hasPerformanceProfile`",
        "`PowerProfiles.holds`",
        "`PowerProfiles.degradationReason`",
        "`PowerProfile.toString`",
        "`PerformanceDegradationReason.toString`",
    ] {
        assert!(
            text.contains(capability),
            "UPower parity row is missing: {capability}"
        );
    }
    for device_type in UPowerDeviceType::ALL {
        assert!(
            text.contains(&format!("`{}`", device_type.token().as_str()))
                || text.contains(&format!("Type `{:?}`", device_type)),
            "UPower device type parity row is missing: {device_type:?}"
        );
    }
    for unresolved in ["| PENDING |", "UNRESOLVED", "TODO"] {
        assert!(
            !text.contains(unresolved),
            "UPower parity matrix remains unresolved: {unresolved}"
        );
    }
    let rows: Vec<_> = text
        .lines()
        .filter(|line| line.starts_with('|') && !line.starts_with("| ---"))
        .filter(|line| !line.contains("Quickshell capability"))
        .collect();
    assert_eq!(rows.len(), 73, "UPower parity row count changed");
    for row in rows {
        assert_eq!(
            row.matches('|').count(),
            10,
            "parity row must contain all nine fields: {row}"
        );
        assert!(
            row.ends_with("| EQUIVALENT |")
                || row.ends_with("| IMPLEMENTED |")
                || row.ends_with("| NOT APPLICABLE |")
                || row.ends_with("| DEFERRED BY JAMES |"),
            "invalid final parity classification: {row}"
        );
    }
}
