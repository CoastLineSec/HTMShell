# Surfaces

Manifest version 1 defines one panel template and one overlay template. HTMShell expands both templates for every eligible output.

## Panels

A panel uses the layer-shell top layer and the top edge. Its `thickness` is a logical height. When `reserveSpace` is true, the compositor reserves that height for the panel.

Panels remain mapped while their overlays open and close. Each output receives an independent document, buffer pool, input state, and frame schedule.

## Overlays

An overlay uses the overlay layer and covers the configured output. Its input region is restricted to visible overlay content, so transparent space remains click-through.

Overlay state is output-local. A panel action on one output does not open an overlay on another output. Closing an overlay unmaps its Wayland role while retaining the parsed document for a later reopen.

## Outputs and scaling

`outputs: "all"` is the only current output policy. Output names are diagnostic labels, not persistent identifiers.

New outputs receive fresh panel and overlay instances. Removing an output destroys only its instances. The process remains idle if no eligible output exists.

Manifest dimensions are logical pixels. When fractional-scale and viewporter are available, the compositor supplies a preferred scale and HTMShell renders a scaled physical buffer. Layout, input coordinates, and input regions remain logical. Scale 1 is the fallback when the complete protocol pair is unavailable.

See [`ShellManifest`](../types/HTMShell/ShellManifest.md), [`PanelSurface`](../types/HTMShell/PanelSurface.md), and [`OverlaySurface`](../types/HTMShell/OverlaySurface.md).
