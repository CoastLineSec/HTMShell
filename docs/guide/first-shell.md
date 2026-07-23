# Create the first shell

The tracked [`examples/static-panel`](../../examples/static-panel/shell.json) package provides a panel, an overlay, CSS, and local SVG assets.

## Project files

```text
examples/static-panel/
├── shell.json
├── panel.html
├── overlay.html
├── style.css
└── assets/
    ├── overlay.svg
    └── shell.svg
```

## Manifest

The manifest defines one top panel and one initially closed overlay:

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

## Panel HTML

The panel binds its output label and opens its output-local overlay:

```html
<!doctype html>
<html lang="en">
  <head>
    <link rel="stylesheet" href="style.css">
  </head>
  <body>
    <main class="panel">
      <img src="assets/shell.svg" alt="">
      <span id="output-label"
            data-htm-element="state-text"
            data-htm-bind="output.label"></span>
      <button id="overlay-toggle"
              data-htm-element="action-button"
              data-htm-action="overlay.toggle">
        Open overlay
      </button>
    </main>
  </body>
</html>
```

## Overlay HTML

The overlay exposes its state and a close action:

```html
<!doctype html>
<html lang="en">
  <head>
    <link rel="stylesheet" href="style.css">
  </head>
  <body>
    <main id="overlay-root">
      <section id="overlay-card" class="overlay-card">
        <span id="overlay-status"
              data-htm-element="state-text"
              data-htm-bind="overlay.status"></span>
        <button id="overlay-close"
                data-htm-element="action-button"
                data-htm-action="overlay.close">
          Close
        </button>
      </section>
    </main>
  </body>
</html>
```

## CSS

CSS owns the internal layout and appearance:

```css
html, body {
  width: 100%;
  height: 100%;
  margin: 0;
  background: transparent;
}

.panel {
  display: flex;
  height: 100%;
  align-items: center;
  gap: 12px;
  background: #111827;
  color: white;
}

#overlay-root {
  display: flex;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: center;
}

.overlay-card {
  padding: 24px;
  border-radius: 20px;
  background: rgba(17, 24, 39, 0.94);
  color: white;
}
```

## Run

```sh
cargo run -p htmshell-live --release --locked -- \
  manifest examples/static-panel/shell.json
```

Each eligible output receives an independent panel and overlay document. The panel reserves its configured height. Clicking its button opens only the overlay on that output.

See [`ShellManifest`](../types/HTMShell/ShellManifest.md), [`PanelSurface`](../types/HTMShell/PanelSurface.md), [`OverlaySurface`](../types/HTMShell/OverlaySurface.md), [`state-text`](../types/HTMShell.Elements/state-text.md), and [`action-button`](../types/HTMShell.Elements/action-button.md).
