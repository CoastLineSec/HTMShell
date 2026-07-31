# `PanelSurface`

**Module:** `HTMShell` | **Kind:** Surface | **Manifest kind:** `panel`

A panel is a persistent top-edge layer-shell surface created on every eligible output.

## Usage

```json
{
  "id": "panel",
  "kind": "panel",
  "document": "panel.html",
  "outputs": "all",
  "edge": "top",
  "thickness": 62,
  "reserveSpace": true,
  "resources": [
    {
      "name": "status-symbol",
      "type": "svg",
      "source": "assets/status-symbol.svg"
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
| `edge` | Must be `top`. |
| `thickness` | Logical height from 1 through 512. |
| `reserveSpace` | Reserves `thickness` when true, otherwise reserves no space. |
| `resources` | Optional ordered strict raster or simple SVG catalog, at most 32 declarations. |

## Notes

The surface uses the top layer and top, left, and right anchors. Width is compositor-selected. Keyboard interactivity is disabled.

Every output owns an independent parsed document, protocol role, input state, buffer pool, and frame schedule. The panel remains mapped while its overlay opens or closes.

Scale 1 is the fallback. Compositor-preferred fractional presentation is used when fractional-scale and viewporter are both available.

Only one top panel template is supported. Other edges, multiple panels, keyboard focus, and persistent output selection are unavailable.

The surface catalog is assignment-only. The panel root may pass `resource:name` to a declared component `resource-reference` input. Ordinary root images keep the existing document-relative provider and cannot use `resource:name`. The source remains panel-template-owned while the receiving component image owns its usage. See [resource-reference inputs](../HTMShell.Component/ResourceReferenceInput.md).

See [`ShellManifest`](ShellManifest.md) and [`OverlaySurface`](OverlaySurface.md).
