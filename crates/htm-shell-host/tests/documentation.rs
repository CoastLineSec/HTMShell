use htm_runtime::{
    CLOCK_FORMAT_CONVERSIONS, CLOCK_FORMAT_FLAGS, CLOCK_PUBLIC_ATTRIBUTES, ClockFormat,
    ContextualRepeatSource, ItemBindingKey, LiveDocument, LiveDocumentKind, LiveRenderRequest,
    MAX_CANDIDATE_BYTES, MAX_COMPONENT_EXPANDED_NODES, MAX_COMPONENT_EXPORTS_PER_GRAPH,
    MAX_COMPONENT_EXPORTS_PER_PACKAGE, MAX_COMPONENT_INPUT_ATTRIBUTES,
    MAX_COMPONENT_INPUT_LITERAL_BYTES, MAX_COMPONENT_INPUT_NAME_BYTES,
    MAX_COMPONENT_INPUT_STRING_BYTES, MAX_COMPONENT_INPUTS, MAX_COMPONENT_INSTANCES_PER_DOCUMENT,
    MAX_COMPONENT_NAME_BYTES, MAX_COMPONENT_NESTING_DEPTH, MAX_COMPONENT_REFERENCES_PER_DOCUMENT,
    MAX_COMPONENT_SLOTS, MAX_COMPONENT_SOURCE_BYTES, MAX_COMPONENT_SOURCE_NODES,
    MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT, MAX_CONTEXTUAL_LINK_GROUP_REPEATS_PER_NODE_TEMPLATE,
    MAX_CONTEXTUAL_LINK_REPEATS_PER_GROUP_TEMPLATE, MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT,
    MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE, MAX_DEPENDENCY_DEPTH, MAX_DIRECT_DEPENDENCIES,
    MAX_PACKAGE_ALIAS_BYTES, MAX_PACKAGE_ID_BYTES, MAX_PACKAGE_MANIFEST_BYTES,
    MAX_PACKAGES_PER_GRAPH, MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS,
    MAX_PIPEWIRE_AUDIO_CONTROLS_PER_DOCUMENT, MAX_PIPEWIRE_AUDIO_CONTROLS_PER_ITEM,
    MAX_PIPEWIRE_BINDINGS_PER_ITEM, MAX_PIPEWIRE_CHANNEL_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT, MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM,
    MAX_PIPEWIRE_CHANNELS_PER_NODE, MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT,
    MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM, MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT,
    MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM, MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT,
    MAX_PIPEWIRE_LINK_GROUPS_PER_PROCESS, MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT,
    MAX_PIPEWIRE_LINKS_PER_PROCESS, MAX_PIPEWIRE_NODES_PER_PROCESS,
    MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR, MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR,
    MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM, MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR,
    MAX_PIPEWIRE_PEAK_CHANNELS_PER_STREAM, MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET,
    MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT, MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM,
    MAX_PIPEWIRE_PERCEPTUAL_VOLUME, MAX_PIPEWIRE_PROPERTY_KEY_BYTES,
    MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT, MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS,
    MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM, MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM,
    MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT, MAX_RANGE_CONTROLS_PER_DOCUMENT,
    MAX_RANGE_CONTROLS_PER_ITEM, PeakBindingKey, RepeatSource, ShellAction, StateBindingKey,
    StateToken, StateValueFormat, built_in_registry_names,
};
use htm_shell_host::{
    PerformanceDegradationReason, PipeWireAudioChannelPosition, PipeWireNodeDirection,
    PipeWireNodeType, PowerProfile, SurfaceKind, UPowerDeviceState, UPowerDeviceType,
    ValidatedManifest,
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
    let mut files = vec![root.join("README.md"), docs.join("README.md")];
    files.extend(markdown_files(&docs.join("architecture")));
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
        "examples/audio-basic/shell.json",
        "examples/audio-basic/index.html",
        "examples/audio-basic/overlay.html",
        "examples/audio-basic/style.css",
        "examples/audio-control-center/shell.json",
        "examples/audio-control-center/panel.html",
        "examples/audio-control-center/overlay.html",
        "examples/audio-control-center/style.css",
        "examples/color-filters/index.html",
        "examples/color-filters/style.css",
        "examples/color-filters/assets/palette.svg",
        "examples/color-filters/assets/alpha-grid.png",
        "examples/package-graph/shell.json",
        "examples/package-graph/index.html",
        "examples/package-graph/panel.html",
        "examples/package-graph/overlay.html",
        "examples/package-graph/style.css",
        "examples/package-graph/packages/controls/shell.json",
        "examples/package-graph/packages/controls/shared/shell.json",
        "examples/package-graph/packages/controls/components/status-card.html",
        "examples/package-graph/packages/controls/shared/components/badge-label.html",
    ] {
        assert!(
            root.join(path).is_file(),
            "documented path is missing: {path}"
        );
    }
}

#[test]
fn local_package_documentation_matches_the_loader_contract() {
    let root = workspace_root();
    let guide = fs::read_to_string(root.join("docs/guide/packages.md")).unwrap();
    let reference = fs::read_to_string(root.join("docs/types/HTMShell.Package/README.md")).unwrap();
    let manifest_reference =
        fs::read_to_string(root.join("docs/types/HTMShell/ShellManifest.md")).unwrap();
    let public = format!("{guide}\n{reference}\n{manifest_reference}");

    for statement in [
        "`shell` package",
        "`library` package",
        "Only that package may declare",
        "reverse-DNS",
        "`local.`",
        "SemVer 2.0.0",
        "dependency-first",
        "Symbolic links are rejected",
        "network",
        "global package search",
        "cycles",
        "immutable generation",
        "last successfully published snapshot",
        "Headless and live",
        "component definitions",
        "hot reload",
    ] {
        assert!(
            public.contains(statement),
            "missing local package contract: {statement}"
        );
    }
    for (value, unit) in [
        (MAX_PACKAGES_PER_GRAPH as u64, "Packages per graph"),
        (
            MAX_DIRECT_DEPENDENCIES as u64,
            "Direct dependencies per package",
        ),
        (MAX_DEPENDENCY_DEPTH as u64, "Dependency depth"),
        (MAX_PACKAGE_ID_BYTES as u64, "Package ID"),
        (MAX_PACKAGE_ALIAS_BYTES as u64, "Dependency alias"),
    ] {
        assert!(
            guide.contains(&format!("| {unit} | {value}")),
            "package guide does not match source limit {unit}={value}"
        );
    }
    assert!(guide.contains(&format!(
        "| Package manifest | {} KiB |",
        MAX_PACKAGE_MANIFEST_BYTES / 1024
    )));
    assert!(guide.contains(&format!(
        "| Total candidate bytes read | {} MiB |",
        MAX_CANDIDATE_BYTES / (1024 * 1024)
    )));

    let manifest = ValidatedManifest::load(root.join("examples/package-graph/shell.json")).unwrap();
    let package_ids = manifest
        .snapshot()
        .packages()
        .iter()
        .map(|package| package.id().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        package_ids,
        [
            "dev.coastlinesec.htmshell.shared",
            "dev.coastlinesec.htmshell.controls",
            "dev.coastlinesec.htmshell.package-graph",
        ]
    );
    assert_eq!(manifest.manifest().surfaces.len(), 2);
    assert_eq!(manifest.snapshot().root_package().dependencies().len(), 2);
    assert!(
        !manifest
            .deterministic_package_graph_json()
            .unwrap()
            .contains(root.to_str().unwrap())
    );
}

#[test]
fn component_documentation_matches_the_literal_input_contract() {
    let root = workspace_root();
    let package_guide = fs::read_to_string(root.join("docs/guide/packages.md")).unwrap();
    let guide = fs::read_to_string(root.join("docs/guide/components.md")).unwrap();
    let reference =
        fs::read_to_string(root.join("docs/types/HTMShell.Component/README.md")).unwrap();
    let input_reference =
        fs::read_to_string(root.join("docs/types/HTMShell.Component/Input.md")).unwrap();
    let slot_reference =
        fs::read_to_string(root.join("docs/types/HTMShell.Component/Slot.md")).unwrap();
    let manifest_reference =
        fs::read_to_string(root.join("docs/types/HTMShell/ShellManifest.md")).unwrap();
    let public = format!(
        "{package_guide}\n{guide}\n{reference}\n{input_reference}\n{slot_reference}\n{manifest_reference}"
    );

    for statement in [
        "`components`",
        "`name`, `source`",
        "data-htm-component",
        "`htm-use`",
        "Bare references",
        "direct dependency alias",
        "transitive aliases",
        "Nested",
        "cycles",
        "non-rendering component host",
        "no layout box",
        "inert",
        "Only the root",
        "literal inputs",
        "`string`, `number`, `boolean`, `token`, `color`, and `length`",
        "`input-*`",
        "`input.<name>`",
        "`state-text`",
        "`state-token`",
        "`state-value`",
        "instance-local",
        "explicitly supplied equivalent value",
        "state-reference",
        "action-reference",
        "resource-reference",
        "interpolation",
        "slots",
        "one default content slot",
        "`required`",
        "fallback",
        "caller ownership",
        "caller order",
        "no layout box",
        "no paint",
        "Named slots",
        "Shadow DOM",
        "component-local IDs",
        "component-scoped CSS",
        "state or action",
        "repeat integration",
        "external component",
        "hot reload",
        "Headless and live",
        "Multi-output",
    ] {
        assert!(
            public.contains(statement),
            "missing static component contract: {statement}"
        );
    }

    for (value, label) in [
        (MAX_COMPONENT_NAME_BYTES as u64, "Component name"),
        (
            MAX_COMPONENT_EXPORTS_PER_PACKAGE as u64,
            "Component exports per package",
        ),
        (
            MAX_COMPONENT_EXPORTS_PER_GRAPH as u64,
            "Component exports per graph",
        ),
        (
            MAX_COMPONENT_SOURCE_NODES as u64,
            "Source nodes per definition",
        ),
        (
            MAX_COMPONENT_INSTANCES_PER_DOCUMENT as u64,
            "Component instances per prepared document",
        ),
        (
            MAX_COMPONENT_REFERENCES_PER_DOCUMENT as u64,
            "Referenced definitions per prepared document",
        ),
        (
            MAX_COMPONENT_NESTING_DEPTH as u64,
            "Component nesting depth",
        ),
        (
            MAX_COMPONENT_EXPANDED_NODES as u64,
            "Expanded nodes per prepared document",
        ),
    ] {
        let documented_value = if value >= 1_000 {
            format!("{},{:03}", value / 1_000, value % 1_000)
        } else {
            value.to_string()
        };
        assert!(
            guide.contains(&format!("| {label} | {documented_value}")),
            "component guide does not match source limit {label}={value}"
        );
    }
    assert!(guide.contains(&format!(
        "| Component source document | {} MiB |",
        MAX_COMPONENT_SOURCE_BYTES / (1024 * 1024)
    )));
    for (value, label) in [
        (
            MAX_COMPONENT_INPUTS as u64,
            "Input declarations per component",
        ),
        (
            MAX_COMPONENT_INPUT_ATTRIBUTES as u64,
            "Supplied inputs per invocation",
        ),
        (MAX_COMPONENT_INPUT_NAME_BYTES as u64, "Input name bytes"),
        (
            MAX_COMPONENT_INPUT_STRING_BYTES as u64,
            "String input bytes",
        ),
    ] {
        let documented_value = if value >= 1_000 {
            format!("{},{:03}", value / 1_000, value % 1_000)
        } else {
            value.to_string()
        };
        assert!(
            input_reference.contains(&format!("| {label} | {documented_value}")),
            "component input reference does not match source limit {label}={value}"
        );
    }
    assert!(input_reference.contains(&format!(
        "| Supplied literal bytes per invocation | {} KiB |",
        MAX_COMPONENT_INPUT_LITERAL_BYTES / 1024
    )));
    assert!(slot_reference.contains(&format!(
        "| Default slot declarations per component | {MAX_COMPONENT_SLOTS} |"
    )));

    let manifest = ValidatedManifest::load(root.join("examples/package-graph/shell.json")).unwrap();
    let snapshot = manifest.snapshot();
    assert_eq!(snapshot.components().definitions().len(), 4);
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
            "dev.coastlinesec.htmshell.shared:badge-label",
            "dev.coastlinesec.htmshell.controls:status-card",
            "dev.coastlinesec.htmshell.controls:required-frame",
            "dev.coastlinesec.htmshell.controls:projected-label",
        ]
    );
    let diagnostic = manifest.deterministic_package_graph_json().unwrap();
    for expected in [
        "\"component_exports\"",
        "\"dependency_first_components\"",
        "\"component_instances\"",
        "\"instance_paths\"",
        "\"inputs\"",
        "\"semantic_version\"",
        "\"consumers\"",
        "\"default_slot\"",
        "\"projections\"",
        "\"projected_nodes\"",
        "\"fallback_nodes\"",
    ] {
        assert!(
            diagnostic.contains(expected),
            "component diagnostic omits {expected}"
        );
    }
    assert!(!diagnostic.contains(root.to_str().unwrap()));

    assert_eq!(built_in_registry_names().len(), 8);
    assert!(!built_in_registry_names().contains(&"htm-use"));
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
fn foreground_filter_docs_and_example_cover_cpu_and_native_vello_boundaries() {
    let root = workspace_root();
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let guide_index = fs::read_to_string(root.join("docs/guide/README.md")).unwrap();
    let guide = fs::read_to_string(root.join("docs/guide/visual-effects.md")).unwrap();
    let architecture = fs::read_to_string(root.join("docs/architecture/renderer.md")).unwrap();
    let reference = read_joined(&markdown_files(&root.join("docs/types/HTMShell.CSS")));
    let html = fs::read_to_string(root.join("examples/color-filters/index.html")).unwrap();
    let css = fs::read_to_string(root.join("examples/color-filters/style.css")).unwrap();
    let svg = fs::read_to_string(root.join("examples/color-filters/assets/palette.svg")).unwrap();
    let public =
        format!("{readme}\n{guide_index}\n{guide}\n{architecture}\n{reference}\n{html}\n{css}");

    for function in [
        "blur()",
        "brightness()",
        "contrast()",
        "drop-shadow()",
        "grayscale()",
        "hue-rotate()",
        "invert()",
        "opacity()",
        "saturate()",
        "sepia()",
    ] {
        assert!(
            public.contains(function),
            "missing rendered filter: {function}"
        );
    }
    for statement in [
        "left to right",
        "encoded sRGB",
        "complete SourceGraphic",
        "box shadows",
        "complete declaration",
        "1,024 UTF-8 bytes",
        "at most 16 functions",
        "All ten",
        "`backdrop-filter` are not supported",
        "distinct from the `opacity` property",
        "ceil(3 * sigma)",
        "transparent black",
        "premultiplied",
        "64 logical pixels",
        "all ten functions with bounded native GPU effect layers",
        "experimental native Vello execution",
        "complete CPU-frame fallback",
        "executes blur natively",
        "cumulative physical blur reach",
        "Backing-to-surface conversion remains full target",
        "native Vello drop shadow",
        "current-stage alpha",
        "Fractional physical offsets",
        "Vello remains optional and experimental",
        "256 distinct normalized foreground-filter declarations",
        "256 active filtered elements",
        "eight filtered ancestors",
        "512 logical pixels on each side",
        "32 finite",
    ] {
        assert!(
            public.contains(statement),
            "missing filter boundary: {statement}"
        );
    }
    for forbidden in [
        "native GPU backdrop",
        "renderer selector",
        "HTMSHELL_RENDERER",
        "foreground and\nbackdrop filters currently use CPU fallback",
        "color and blur subset",
        "blur pending on GPU",
        "drop shadow pending on GPU",
        "foreground filters always use CPU fallback",
    ] {
        assert!(
            !public.contains(forbidden),
            "public filter documentation overstates or exposes {forbidden}"
        );
    }
    for syntax in [
        "brightness(1.35)",
        "contrast(1.35)",
        "grayscale(1)",
        "hue-rotate(90deg)",
        "invert(1)",
        "opacity(55%)",
        "saturate(1.8)",
        "sepia(1)",
        "blur(1px)",
        "blur(3px)",
        "blur(8px)",
        "drop-shadow(5px 4px 2px rgb(0 0 0 / 80%))",
        "drop-shadow(5px 5px currentColor)",
        "drop-shadow(7px 6px 0 #071020)",
        "drop-shadow(-7px -5px 2px #071020)",
    ] {
        assert!(css.contains(syntax), "example omits {syntax}");
    }
    assert!(css.matches("brightness(1.1)").count() >= 2);
    assert!(html.contains("nested color filters"));
    assert!(html.contains("nested blur"));
    assert!(html.contains("nested element shadows"));
    assert!(html.contains("Filtered text"));
    assert!(html.contains("Blurred text"));
    assert!(html.contains("text silhouette"));
    assert!(html.contains("SVG silhouette"));
    assert!(html.contains("transparent raster holes"));
    assert!(html.contains("box-shadow geometry"));
    assert!(html.contains("drop-shadow silhouette"));
    assert!(html.contains("alpha-grid.png"));
    assert!(css.contains("box-shadow:"));
    assert!(css.contains("drop-shadow("));
    assert!(css.contains("blur(2px) drop-shadow("));
    assert!(css.contains("drop-shadow(7px 0 2px #ff557f) blur(2px)"));
    assert!(css.contains("overflow: hidden"));
    assert!(css.contains("transform:"));
    assert!(svg.contains("<svg"));
    for forbidden in ["backdrop-filter", "<script"] {
        assert!(!format!("{html}\n{css}").contains(forbidden));
    }

    let effect_source =
        fs::read_to_string(root.join("crates/htm-runtime/src/render/effects.rs")).unwrap();
    for constant in [
        "MAX_FOREGROUND_EFFECT_FUNCTIONS: usize = 16",
        "MAX_FOREGROUND_EFFECT_SERIALIZED_BYTES: usize = 1_024",
        "MAX_FILTER_DECLARATIONS_PER_DOCUMENT: usize = 256",
        "MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE: usize = 256",
        "MAX_FILTER_NESTING_DEPTH: usize = 8",
        "MAX_FOREGROUND_EFFECT_FACTOR: f32 = 8.0",
        "MAX_HUE_ROTATION_TURNS: f32 = 100.0",
        "MAX_FOREGROUND_BLUR_SIGMA: f32 = 64.0",
        "MAX_FOREGROUND_SHADOW_OFFSET: f32 = 256.0",
        "MAX_FOREGROUND_EFFECT_EXPANSION: f32 = 512.0",
        "MAX_EFFECT_LAYER_DIMENSION: u32 = 4_096",
        "MAX_EFFECT_IMAGE_BYTES: usize = 64 * 1024 * 1024",
        "MAX_EFFECT_SURFACE_BYTES: usize = 256 * 1024 * 1024",
        "MAX_EFFECT_PIPELINE_VARIANTS: usize = 32",
    ] {
        assert!(
            effect_source.contains(constant),
            "foreground documentation validation is stale relative to {constant}"
        );
    }

    htm_runtime::run_package_with_options(
        root.join("examples/color-filters"),
        htm_runtime::ExperimentOptions {
            viewport: htm_runtime::ViewportSpec {
                logical_width: 1440,
                logical_height: 900,
                ..htm_runtime::ViewportSpec::default()
            },
            render_png: true,
            run_interaction: false,
            output_directory: None,
        },
    )
    .expect("color-filter example CPU render");
}

#[test]
fn typed_public_names_are_covered_by_the_reference() {
    let root = workspace_root();
    let reference = read_joined(&markdown_files(&root.join("docs/types")));
    assert_eq!(built_in_registry_names().len(), 8);

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
    for source in ContextualRepeatSource::ALL {
        assert!(
            documented_name(&reference, source.as_str()),
            "contextual repeat source is undocumented: {}",
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
    for binding in PeakBindingKey::ALL {
        assert!(
            documented_name(&reference, binding.as_str()),
            "peak binding is undocumented: {}",
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
        "data-htm-enabled",
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
            MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_DOCUMENT,
            "PipeWire preferred-default controls per document",
        ),
        (
            MAX_PIPEWIRE_DEFAULT_CONTROLS_PER_ITEM,
            "PipeWire preferred-default controls per repeated item",
        ),
        (
            MAX_RANGE_CONTROLS_PER_DOCUMENT,
            "range controls per document",
        ),
        (
            MAX_RANGE_CONTROLS_PER_ITEM,
            "range controls per repeated item",
        ),
        (
            MAX_CONTEXTUAL_REPEATS_PER_NODE_TEMPLATE,
            "contextual repeats per node template",
        ),
        (
            MAX_CONTEXTUAL_REPEATS_PER_DOCUMENT,
            "contextual repeats per document",
        ),
        (MAX_PIPEWIRE_CHANNELS_PER_NODE, "public channels per node"),
        (
            MAX_PIPEWIRE_CHANNEL_BINDINGS_PER_ITEM,
            "channel bindings per item",
        ),
        (
            MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_ITEM,
            "channel range controls per item",
        ),
        (
            MAX_PIPEWIRE_CHANNEL_RANGE_CONTROLS_PER_DOCUMENT,
            "channel range controls per document",
        ),
        (
            MAX_PIPEWIRE_LINK_REPEAT_DECLARATIONS_PER_DOCUMENT,
            "PipeWire link repeats per document",
        ),
        (
            MAX_PIPEWIRE_LINK_GROUP_REPEAT_DECLARATIONS_PER_DOCUMENT,
            "PipeWire link-group repeats per document",
        ),
        (
            MAX_CONTEXTUAL_LINK_REPEATS_PER_GROUP_TEMPLATE,
            "member-link repeats per group template",
        ),
        (
            MAX_CONTEXTUAL_LINK_GROUP_REPEATS_PER_NODE_TEMPLATE,
            "link-group repeats per node template",
        ),
        (
            MAX_CONTEXTUAL_GRAPH_REPEATS_PER_DOCUMENT,
            "contextual graph repeats per document",
        ),
        (
            MAX_PIPEWIRE_GRAPH_BINDINGS_PER_ITEM,
            "PipeWire graph bindings per item",
        ),
        (
            MAX_PIPEWIRE_RELATION_BINDINGS_PER_ITEM,
            "PipeWire relation bindings per item",
        ),
        (
            MAX_PIPEWIRE_REPEAT_DECLARATIONS_PER_DOCUMENT,
            "PipeWire node repeats per document",
        ),
        (MAX_PIPEWIRE_NODES_PER_PROCESS, "public PipeWire nodes"),
        (
            MAX_PIPEWIRE_BINDINGS_PER_ITEM,
            "PipeWire node bindings per item",
        ),
        (
            MAX_PIPEWIRE_PROPERTY_LOOKUPS_PER_ITEM,
            "PipeWire property lookups per item",
        ),
        (
            MAX_PIPEWIRE_PROPERTY_KEYS_PER_DOCUMENT,
            "PipeWire property keys per document",
        ),
        (
            MAX_PIPEWIRE_PROPERTY_KEYS_PER_PROCESS,
            "PipeWire property keys per process",
        ),
        (
            MAX_PIPEWIRE_PROPERTY_KEY_BYTES,
            "PipeWire property key bytes",
        ),
        (MAX_PIPEWIRE_LINKS_PER_PROCESS, "public PipeWire links"),
        (
            MAX_PIPEWIRE_LINK_GROUPS_PER_PROCESS,
            "public PipeWire link groups",
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
    for position in PipeWireAudioChannelPosition::NAMED {
        assert!(
            documented_name(&reference, &position.token()),
            "PipeWire channel position is undocumented: {}",
            position.token()
        );
    }
    for position in [
        PipeWireAudioChannelPosition::AUXILIARY_FIRST,
        PipeWireAudioChannelPosition::AUXILIARY_LAST,
        PipeWireAudioChannelPosition::CUSTOM_FIRST,
        PipeWireAudioChannelPosition::CUSTOM_LAST,
    ] {
        assert!(
            documented_name(&reference, &position.token()),
            "PipeWire ranged channel position is undocumented: {}",
            position.token()
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
fn peak_monitor_reference_example_and_limits_are_complete() {
    let root = workspace_root();
    let reference = read_joined(&markdown_files(&root.join("docs/types")));
    let guide = fs::read_to_string(root.join("docs/guide/audio.md")).unwrap();
    let example = fs::read_to_string(root.join("examples/audio-inspector/overlay.html")).unwrap();
    let css = fs::read_to_string(root.join("examples/audio-inspector/style.css")).unwrap();
    let public = format!("{reference}\n{guide}");

    for name in [
        "peak-monitor",
        "peak.channels",
        "pipewire.peaks.enable",
        "pipewire.peaks.disable",
        "pipewire.peaks.toggle",
        "item.can_monitor_peaks",
        "pipewire.default_sink.can_monitor_peaks",
        "pipewire.default_source.can_monitor_peaks",
        "disabled",
        "suspended",
        "unavailable",
        "starting",
        "ready",
        "failed",
        "capture-adjacent",
        "raw samples",
        "60 per second",
    ] {
        assert!(public.contains(name), "missing peak documentation: {name}");
    }

    assert!(example.contains(r#"data-htm-target="pipewire.default_sink""#));
    assert!(example.contains(r#"data-htm-target="pipewire.default_source""#));
    assert!(example.contains(r#"data-htm-enabled="true""#));
    assert!(example.contains(r#"data-htm-enabled="false""#));
    assert!(example.contains("capture-adjacent"));
    assert!(css.contains(r#"[data-htm-state="starting"]"#));
    assert!(css.contains(r#"[data-htm-state="suspended"]"#));

    for limit in [
        MAX_PIPEWIRE_PEAK_MONITORS_PER_DOCUMENT,
        MAX_PIPEWIRE_PEAK_MONITORS_PER_ITEM,
        MAX_PIPEWIRE_ENABLED_PEAK_MONITORS_PER_DOCUMENT,
        MAX_PIPEWIRE_PEAK_ACTIONS_PER_MONITOR,
        MAX_PIPEWIRE_PEAK_CHANNEL_REPEATS_PER_MONITOR,
        MAX_PIPEWIRE_PEAK_BINDINGS_PER_MONITOR,
        MAX_PIPEWIRE_PEAK_CHANNEL_BINDINGS_PER_ITEM,
        MAX_PIPEWIRE_PEAK_CHANNELS_PER_STREAM,
        MAX_PIPEWIRE_ACTIVE_PEAK_STREAMS,
        MAX_PIPEWIRE_PEAK_DECLARATIONS_PER_TARGET,
    ] {
        assert!(
            public.contains(&limit.to_string()),
            "peak limit is undocumented: {limit}"
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
        "examples/audio-basic/shell.json",
        "examples/audio-control-center/shell.json",
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
        "item.channels",
        "item.channel_count",
        "item.channel_status",
        "item.position",
        "item.position_name",
        "item.index",
        "item.is_auxiliary",
        "item.is_custom",
        "pipewire.audio.set_channel_volume",
        "pipewire.link_count",
        "pipewire.link_group_count",
        "pipewire.links",
        "pipewire.link_groups",
        "item.links",
        "item.link_groups",
        "item.source.description",
        "item.target.description",
        "item.peer.description",
        "item.connection_direction",
        "pipewire.defaults.set_preferred_sink",
        "pipewire.defaults.set_preferred_source",
        "pipewire.defaults.clear_preferred_sink",
        "pipewire.defaults.clear_preferred_source",
        "item.can_set_preferred_sink",
        "item.can_set_preferred_source",
        "pipewire.configured_sink.can_clear",
        "pipewire.configured_source.can_clear",
        "peak-monitor",
        "peak.channels",
        "pipewire.peaks.enable",
        "pipewire.peaks.disable",
        "pipewire.peaks.toggle",
        "peak.maximum",
        "pending",
        "failed",
    ] {
        assert!(example.contains(name), "audio example omits `{name}`");
    }
    for forbidden in ["pipewire.configured_sink\"", "pipewire.configured_source\""] {
        assert!(
            !overlay.contains(forbidden),
            "audio example uses forbidden control surface `{forbidden}`"
        );
    }
}

#[test]
fn all_audio_examples_validate_built_ins_css_and_headless_rendering() {
    let root = workspace_root();
    for (package, document, kind, width, height) in [
        (
            "examples/audio-basic",
            "index.html",
            LiveDocumentKind::Panel,
            1280,
            72,
        ),
        (
            "examples/audio-basic",
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        ),
        (
            "examples/audio-control-center",
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            58,
        ),
        (
            "examples/audio-control-center",
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        ),
        (
            "examples/audio-inspector",
            "panel.html",
            LiveDocumentKind::Panel,
            1280,
            54,
        ),
        (
            "examples/audio-inspector",
            "overlay.html",
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        ),
    ] {
        let mut live =
            LiveDocument::load_surface_document(root.join(package), document, kind, width, height)
                .unwrap_or_else(|error| panic!("{package}/{document} is invalid: {error}"));
        live.render_for(LiveRenderRequest::new(width, height, 120).unwrap())
            .unwrap_or_else(|error| panic!("{package}/{document} did not render: {error}"));
        assert_eq!(live.snapshot().unwrap().document_parse_count, 1);
    }
}

#[test]
fn audio_examples_form_a_narrow_progression() {
    let root = workspace_root();
    let basic = read_joined(&[
        root.join("examples/audio-basic/index.html"),
        root.join("examples/audio-basic/overlay.html"),
        root.join("examples/audio-basic/style.css"),
    ]);
    for required in [
        "pipewire.availability",
        "pipewire.default_sink.description",
        "pipewire.default_sink.volume",
        "pipewire.default_sink.mute_state",
        "pipewire.audio.toggle_mute",
        "pipewire.audio.set_volume",
    ] {
        assert!(basic.contains(required), "audio-basic omits `{required}`");
    }
    for excluded in [
        "pipewire.nodes",
        "pipewire.links",
        "item.channels",
        "peak-monitor",
        "pipewire.defaults.",
    ] {
        assert!(
            !basic.contains(excluded),
            "audio-basic exceeds its onboarding scope with `{excluded}`"
        );
    }

    let control_center = read_joined(&[
        root.join("examples/audio-control-center/panel.html"),
        root.join("examples/audio-control-center/overlay.html"),
        root.join("examples/audio-control-center/style.css"),
    ]);
    for required in [
        "pipewire.configured_sink.description",
        "pipewire.configured_source.description",
        "pipewire.defaults.set_preferred_sink",
        "pipewire.defaults.set_preferred_source",
        "pipewire.nodes",
        "item.is_stream",
        "item.channels",
        "pipewire.audio.set_channel_volume",
        r#"[data-htm-state="pending"]"#,
        r#"[data-htm-state="failed"]"#,
    ] {
        assert!(
            control_center.contains(required),
            "audio-control-center omits `{required}`"
        );
    }
    for excluded in [
        "pipewire.links",
        "pipewire.link_groups",
        "peak-monitor",
        "peak.channels",
    ] {
        assert!(
            !control_center.contains(excluded),
            "audio-control-center exceeds its control scope with `{excluded}`"
        );
    }

    let inspector = read_joined(&[
        root.join("examples/audio-inspector/panel.html"),
        root.join("examples/audio-inspector/overlay.html"),
        root.join("examples/audio-inspector/style.css"),
    ]);
    for required in [
        "pipewire.links",
        "pipewire.link_groups",
        "item.link_groups",
        "item.peer.description",
        "peak-monitor",
        "peak.channels",
        "capture-adjacent",
    ] {
        assert!(
            inspector.contains(required),
            "audio-inspector omits `{required}`"
        );
    }
    assert!(
        inspector.contains(
            r#"data-htm-target="pipewire.default_source"
                 data-htm-enabled="false""#
        ),
        "the public inspector must not enable source monitoring by default"
    );
    for example in [&basic, &control_center, &inspector] {
        assert!(!example.contains(r#"max="2""#));
        assert!(!example.contains("/home/"));
        assert!(!example.contains(".internal"));
    }
}

#[test]
fn pipewire_reference_is_derived_from_the_typed_public_inventory() {
    let root = workspace_root();
    let reference = read_joined(&markdown_files(
        &root.join("docs/types/HTMShell.Services.PipeWire"),
    ));

    for key in StateBindingKey::ALL
        .into_iter()
        .filter(|key| key.as_str().starts_with("pipewire."))
    {
        assert!(
            documented_name(&reference, key.as_str()),
            "PipeWire state key is absent from the module reference: {}",
            key.as_str()
        );
        for token in key.token_values() {
            assert!(
                documented_name(&reference, token),
                "PipeWire token `{token}` for `{}` is undocumented",
                key.as_str()
            );
        }
    }
    for key in ItemBindingKey::ALL.into_iter().filter(|key| {
        key.supports_source(RepeatSource::PipeWireNodes)
            || key.supports_source(RepeatSource::PipeWireLinks)
            || key.supports_source(RepeatSource::PipeWireLinkGroups)
            || ContextualRepeatSource::ALL
                .into_iter()
                .any(|source| key.supports_contextual(source))
            || key.supports_peak_channel()
    }) {
        assert!(
            documented_name(&reference, key.as_str()),
            "PipeWire item key is absent from the module reference: {}",
            key.as_str()
        );
    }
    for key in PeakBindingKey::ALL {
        assert!(
            documented_name(&reference, key.as_str()),
            "PipeWire peak key is absent from the module reference: {}",
            key.as_str()
        );
    }
}

#[test]
fn private_pipewire_final_parity_is_complete_when_present() {
    let path = workspace_root().join(".internal/research/pipewire-final-parity.md");
    if !path.is_file() {
        return;
    }
    let text = fs::read_to_string(&path).expect("private parity matrix is UTF-8");
    let rows = text
        .lines()
        .filter(|line| line.starts_with("| PW-"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 101, "PipeWire parity row count changed");
    for (index, row) in rows.iter().enumerate() {
        assert!(
            row.starts_with(&format!("| PW-{:03} |", index + 1)),
            "PipeWire parity row ordering changed: {row}"
        );
        assert_eq!(
            row.matches('|').count(),
            12,
            "PipeWire parity row must contain all eleven fields: {row}"
        );
        let fields = row.split('|').map(str::trim).collect::<Vec<_>>();
        assert!(
            matches!(
                fields[10],
                "IMPLEMENTED" | "EQUIVALENT" | "NOT APPLICABLE" | "DEFERRED BY JAMES"
            ),
            "invalid final PipeWire classification: {row}"
        );
        assert!(
            !fields[11].is_empty(),
            "PipeWire parity row omits notes: {row}"
        );
    }
    for unresolved in ["UNRESOLVED", "| PLANNED |", "| PENDING |", "TODO"] {
        assert!(
            !text.contains(unresolved),
            "PipeWire parity matrix remains unresolved: {unresolved}"
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
