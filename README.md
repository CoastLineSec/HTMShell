# HTMShell

HTMShell expands to **Hypertext Theming & Markup Shell**. It is an experimental native desktop-shell runtime intended to use real HTML and a documented CSS desktop profile as authoring languages without embedding a conventional browser or WebView.

Gate A is a reversible feasibility spike. It evaluates whether modular DioxusLabs Blitz crates can parse a local document, resolve CSS, perform layout and text shaping, produce structured diagnostics, and paint a headless image while remaining isolated behind an HTMShell-owned adapter.

```sh
cargo run -p htm-headless -- examples/basic-shell
```

The command uses a fixed 1440 × 900 logical-pixel, scale-1.0 SDR/sRGB viewport and writes experimental artifacts below `examples/basic-shell/output/`.

There is no Wayland, layer-shell, Hyprland, or compositor integration in this phase. Blitz is under evaluation and is not the permanent HTMShell engine unless later gates justify that decision.
