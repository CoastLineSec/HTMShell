# Local packages

HTMShell loads a shell from a local, read-only package graph. A graph contains one root `shell` package and zero or more `library` packages. The complete graph is validated before any root surface document is instantiated.

Package loading is offline. Manifests cannot select network locations, global package search roots, registries, environment expansions, or renderer backends.

## Package kinds

A `shell` package owns the graph root, entry documents, and surface topology. It may depend on libraries. A shell cannot be imported.

A `library` package may depend on other libraries and export inert static component definitions. Loading a library cannot declare surfaces, create service demand, load presentation CSS or assets, or affect root topology. An unused definition creates no document or visual work.

## Schema version 2

The `shell.json` manifest uses a `package` object and an ordered `dependencies` array:

```json
{
  "version": 2,
  "package": {
    "id": "org.example.shell",
    "kind": "shell",
    "version": "0.1.0"
  },
  "dependencies": [
    {
      "alias": "controls",
      "id": "org.example.controls",
      "path": "packages/controls"
    }
  ],
  "components": [
    {
      "name": "shell-heading",
      "source": "components/shell-heading.html",
      "inputs": [
        {
          "name": "label",
          "type": "string",
          "required": true
        }
      ],
      "slots": [
        {
          "name": "default",
          "required": false
        },
        {
          "name": "icon",
          "required": false
        }
      ],
      "styles": [
        "components/shell-heading.css"
      ]
    }
  ],
  "surfaces": [
    {
      "id": "panel",
      "kind": "panel",
      "document": "panel.html",
      "outputs": "all",
      "edge": "top",
      "thickness": 52,
      "reserveSpace": true
    },
    {
      "id": "overlay",
      "kind": "overlay",
      "document": "overlay.html",
      "outputs": "all",
      "initiallyOpen": false
    }
  ]
}
```

The corresponding library manifest is:

```json
{
  "version": 2,
  "package": {
    "id": "org.example.controls",
    "kind": "library",
    "version": "0.1.0"
  },
  "dependencies": [],
  "components": [
    {
      "name": "status-card",
      "source": "components/status-card.html"
    }
  ]
}
```

Package versions are optional SemVer 2.0.0 metadata. HTMShell records a declared version but does not solve ranges or select among versions.

Unknown fields are rejected. Library manifests must omit `surfaces`.

The optional ordered `components` array is accepted only by schema version 2. Each entry has a `name`, a package-relative `source`, an optional ordered `inputs` array, an optional ordered `slots` array with up to 32 unique default or named declarations, and an optional ordered `styles` array with up to 16 component-owned stylesheet paths. The manifest export, input, slot, and stylesheet association tables are authoritative. Every exported name must match exactly one inert template declaration, and every declaration must be exported. Inputs, slot projections, contained stylesheet sources, and selector ownership resolve before publication. See [components](components.md).

Component stylesheet paths are relative to the package that owns the export. They are bounded to 512 UTF-8 bytes and 1 MiB per regular non-symlink file. One package may declare at most 64 unique component stylesheet files. A shared path is read and parsed once per package snapshot candidate, while each definition retains its own ordered association. Invalid CSS, imports, URL assets, font sources, or unsupported scope selectors reject the complete candidate without fetching resources.

## Package IDs and aliases

A package ID is a lowercase ASCII reverse-DNS name:

- Total length is at most 255 bytes.
- At least two dot-separated segments are required.
- Each segment contains 1 through 63 bytes.
- A segment starts with `a` through `z`.
- Remaining characters are lowercase letters, digits, or interior hyphens.
- The `local.` prefix is reserved for compatibility packages.

Each dependency has an alias, expected package ID, and local path. Aliases contain 1 through 64 lowercase ASCII letters, digits, or interior hyphens, start with a letter, contain no dots, and are unique within the declaring package. These aliases are reserved:

```text
self root input state action service surface slot htm
```

The resolved library must declare the exact expected package ID. Dependency versions and aliases do not change logical package identity.

## Local resolution

The root shell directory is the composition root. A dependency path is interpreted relative to the package that declares it, while every resolved directory must remain beneath the composition root.

Dependency paths must be normalized, relative UTF-8 paths. Absolute paths, empty components, parent traversal, backslashes, URL syntax, special files, and containment escapes are rejected. Symbolic links are rejected for the composition root, dependency directories, traversed dependency components, and package manifest files.

Search directories and network retrieval are not consulted. A manifest names the exact local directory to validate.

## Graph behavior

Dependencies retain declaration order. The validated graph uses deterministic dependency-first order, with the root shell last. A shared dependency at the same canonical directory, with the same package ID and version, appears once even when multiple parents reference it.

Package dependency cycles are rejected. The complete graph is also rejected when:

- one package ID is claimed by different directories;
- one package ID is associated with conflicting versions;
- one directory is associated with conflicting package IDs;
- a dependency resolves to a shell package;
- a library declares surface topology.

No partial graph is used. Candidate construction finishes before an immutable snapshot generation becomes current. A failed replacement leaves the last successfully published snapshot unchanged. Headless and live loading use this same graph boundary.

## Compatibility

Schema version 1 manifests remain valid without edits. A schema version 1 shell ID is represented internally as `local.<shell-id>`, with no version or dependencies. Its panel, overlay, namespaces, resources, and multi-output behavior remain unchanged.

A manifestless headless directory containing `index.html` remains valid and uses the reserved compatibility identity `local.headless-root`. A manifest-backed headless package validates the same schema version 2 graph as live loading and still renders its root `index.html`.

## Limits

| Resource | Limit |
| --- | ---: |
| Packages per graph | 64 |
| Direct dependencies per package | 32 |
| Dependency depth | 16 |
| Package ID | 255 bytes |
| Dependency alias | 64 bytes |
| Package manifest | 256 KiB |
| Total candidate bytes read | 256 MiB |
| Component exports per package | 256 |
| Component exports per graph | 4,096 |
| Component source document | 2 MiB |
| Component source nodes per definition | 10,000 |
| Component instances per prepared document | 4,096 |
| Referenced definitions per prepared document | 256 |
| Component nesting depth | 32 |
| Expanded nodes per prepared document | 50,000 |
| Input declarations per component | 64 |
| Supplied inputs per invocation | 64 |
| String input | 4,096 UTF-8 bytes |
| Supplied literal bytes per invocation | 16 KiB |
| Slots per component | 32 |
| Slot name | 64 bytes |

Manifest, package, component, input, slot, stylesheet, and graph errors reject the candidate rather than truncating it. Component definitions, literal typed inputs, caller-owned default or named slot projection, and scoped component stylesheets are supported. Component-local IDs, dynamic state or action bindings, repeat integration, component-owned external resources, and hot reload are not implemented.

See the [package graph example](../../examples/package-graph/shell.json), the [`HTMShell.Package`](../types/HTMShell.Package/README.md) reference, and the [`HTMShell.Component`](../types/HTMShell.Component/README.md) reference.

Validate that example without creating Wayland surfaces:

```sh
cargo run -p htmshell-live --locked --offline -- \
  manifest examples/package-graph/shell.json --validate-only
```

The diagnostic lists the immutable snapshot generation and dependency-first package order. Its JSON is intended for development and deterministic testing, not as a stable package interchange format.
