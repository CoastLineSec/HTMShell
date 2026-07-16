# HTMShell

HTMShell expands to **Hypertext Theming & Markup Shell**. It is an experimental native desktop-shell runtime intended to use real HTML and a documented CSS desktop profile as authoring languages without embedding a conventional browser or WebView.

Gate A and Gate A.1 are reversible feasibility spikes. They evaluate whether modular DioxusLabs Blitz crates can parse a local document, resolve CSS, perform layout and text shaping, produce structured diagnostics, paint a headless image, and then sustain host-driven mutations without reconstructing that document. Blitz remains isolated behind an HTMShell-owned adapter.

```sh
cargo run -p htm-headless -- examples/basic-shell
cargo run -p htm-headless --release --locked -- mutate examples/basic-shell
```

Both commands use a fixed 1440 × 900 logical-pixel, scale-1.0 SDR/sRGB viewport and write experimental artifacts below `examples/basic-shell/output/`. The `mutate` command exercises in-place text and class changes, dynamic insertion/removal, generational diagnostic identities, author-stylesheet replacement, failed-reload recovery, deterministic scene snapshots/diffs, and approximately 120-, 1,000-, and 5,000-node fixtures.

The scene diff is an HTMShell-owned diagnostic artifact, not evidence of retained Blitz painting. Every accepted painted mutation phase currently invokes `blitz-paint` again and reconstructs the full AnyRender scene.

There is no Wayland, layer-shell, Hyprland, or compositor integration in this phase. Blitz is under evaluation and is not the permanent HTMShell engine unless later gates justify that decision.
