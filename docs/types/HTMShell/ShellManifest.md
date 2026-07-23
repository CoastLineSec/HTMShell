# `ShellManifest`

**Module:** `HTMShell` | **Kind:** JSON manifest | **Status:** Experimental

`ShellManifest` defines the local documents used for portable shell surfaces.

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

Manifest hot reload, persistent output selection, scale overrides, additional surface kinds, and more than one panel or overlay template are unavailable.

See the tracked [static panel manifest](../../../examples/static-panel/shell.json), [`PanelSurface`](PanelSurface.md), and [`OverlaySurface`](OverlaySurface.md).
