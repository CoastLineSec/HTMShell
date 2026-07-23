use htm_runtime::{ShellAction, StateBindingKey, StateToken, built_in_registry_names};
use htm_shell_host::{SurfaceKind, ValidatedManifest};
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
        "examples/battery-panel/shell.json",
        "examples/battery-panel/assets/battery.svg",
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
        "examples/battery-panel/shell.json",
    ] {
        let manifest = ValidatedManifest::load(root.join(path))
            .unwrap_or_else(|error| panic!("documented manifest {path} is invalid: {error}"));
        assert_eq!(manifest.parse_count(), 1);
        assert_eq!(manifest.manifest().version, 1);
        assert_eq!(manifest.manifest().surfaces.len(), 2);
    }
}
