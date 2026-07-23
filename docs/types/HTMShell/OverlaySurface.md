# `OverlaySurface`

**Module:** `HTMShell` | **Kind:** Surface | **Manifest kind:** `overlay`

An overlay is a transient, output-local layer-shell surface.

## Usage

```json
{
  "id": "overlay",
  "kind": "overlay",
  "document": "overlay.html",
  "outputs": "all",
  "initiallyOpen": false
}
```

## Members

| Field | Behavior |
| --- | --- |
| `id` | Identifies the template and layer-shell namespace. |
| `document` | Selects the local HTML document. |
| `outputs` | Must be `all`. |
| `initiallyOpen` | Controls initial mapping. |

## Notes

The surface uses the overlay layer, all four anchors, no exclusive zone, and no keyboard interactivity. The compositor selects its full-output logical size.

The input region follows visible overlay content. Transparent space outside that region remains click-through.

Closing attaches a null buffer and removes the transient role after presentation resources are released. The parsed document and its element index remain available. Reopening creates a fresh Wayland role and configure lifecycle for that document.

Each output has an independent overlay document and open state. A panel action affects only the overlay on the same output.

Scale 1 and compositor-preferred fractional presentation follow the same rules as panels. Multiple overlays, keyboard focus, and moving a live role between outputs are unavailable.

See [`ShellManifest`](ShellManifest.md), [`PanelSurface`](PanelSurface.md), and [overlay actions](../HTMShell.Actions/Overlay.md).
