# `ShellManifest`

**Module:** `HTMShell` | **Kind:** JSON manifest | **Status:** Experimental

`ShellManifest` defines the root local package and the documents used for portable shell surfaces. Schema version 1 remains supported. Schema version 2 adds package identity, optional version metadata, and local library dependencies without changing the supported surface topology.

## Usage

```json
{
  "version": 1,
  "id": "static-panel-demo",
  "surfaces": [
    {
      "id": "panel",
      "kind": "panel",
      "document": "panel.html",
      "outputs": "all",
      "edge": "top",
      "thickness": 62,
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

## Members

Root fields:

| Field | Requirement |
| --- | --- |
| `version` | Integer `1`. |
| `id` | Lowercase ASCII letters, digits, and interior hyphens. Maximum 64 bytes. |
| `surfaces` | Exactly one `panel` and one `overlay`. IDs must be unique. |

Schema version 2 replaces the root `id` with package metadata and may add local dependencies and component exports:

```json
{
  "version": 2,
  "package": {
    "id": "org.example.shell",
    "kind": "shell",
    "version": "1.0.0"
  },
  "dependencies": [],
  "components": [
    {
      "name": "status-card",
      "source": "components/status-card.html"
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

The `package.id` uses the bounded reverse-DNS syntax, `package.kind` must be `shell` at the root, and `package.version` is optional SemVer 2.0.0 metadata. The existing surface fields and constraints are identical. The optional ordered `components` array contains `name`, `source`, and optional literal `inputs`. See [`HTMShell.Package`](../HTMShell.Package/README.md) for dependency fields and graph rules and [`HTMShell.Component`](../HTMShell.Component/README.md) for component syntax.

Shared surface fields:

| Field | Requirement |
| --- | --- |
| `id` | Stable surface ID with the root ID character rules. |
| `kind` | `panel` or `overlay`. |
| `document` | Local relative HTML path, maximum 512 bytes. |
| `outputs` | Only `all`. |

Panel fields are `edge`, `thickness`, and `reserveSpace`. Overlay fields use `initiallyOpen`. Unknown fields and unsupported values are rejected.

## Notes

The manifest is limited to 256 KiB. Document paths cannot be absolute, remote, or escape the package directory through parent components or symbolic links. Validation completes before the Wayland connection.

Dimensions are logical pixels. Scale is compositor-provided and cannot be selected by the manifest. Output names are not manifest selectors.

Manifest hot reload, dynamic component bindings, named slots, component-scoped styles, component-owned resources, persistent output selection, scale overrides, additional surface kinds, and more than one panel or overlay template are unavailable. Component exports may declare one default slot.

See the tracked [static panel manifest](../../../examples/static-panel/shell.json), [`PanelSurface`](PanelSurface.md), and [`OverlaySurface`](OverlaySurface.md).
