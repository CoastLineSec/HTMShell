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
  "initiallyOpen": false,
  "resources": [
    {
      "name": "profile-photo",
      "type": "raster",
      "source": "assets/profile.webp"
    }
  ]
}
```

## Members

| Field | Behavior |
| --- | --- |
| `id` | Identifies the template and layer-shell namespace. |
| `document` | Selects the local HTML document. |
| `outputs` | Must be `all`. |
| `initiallyOpen` | Controls initial mapping. |
| `resources` | Optional ordered strict raster or simple SVG catalog, at most 32 declarations. |

## Notes

The surface uses the overlay layer, all four anchors, no exclusive zone, and no keyboard interactivity. The compositor selects its full-output logical size.

The input region follows visible overlay content. Transparent space outside that region remains click-through.

Closing attaches a null buffer and removes the transient role after presentation resources are released. The parsed document and its element index remain available. Reopening creates a fresh Wayland role and configure lifecycle for that document.

Each output has an independent overlay document and open state. A panel action affects only the overlay on the same output.

Scale 1 and compositor-preferred fractional presentation follow the same rules as panels. Multiple overlays, keyboard focus, and moving a live role between outputs are unavailable.

The surface catalog is visible only to resource-reference assignments made by this overlay root. It does not affect ordinary root image, CSS, or external SVG loading and is not visible to the panel or another overlay. See [resource-reference inputs](../HTMShell.Component/ResourceReferenceInput.md).

See [`ShellManifest`](ShellManifest.md), [`PanelSurface`](PanelSurface.md), and [overlay actions](../HTMShell.Actions/Overlay.md).
