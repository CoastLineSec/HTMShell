# ADR 0001: Evaluate modular Blitz crates in a headless runtime spike

- Status: experimental and reversible
- Date: 2026-07-16

## Context

HTMShell needs evidence that real local HTML and a useful CSS subset can feed a native document, layout, text, paint, and diagnostic pipeline without embedding a browser shell. This gate tests only that headless foundation. It does not select the final engine or public API.

A browser, browser window, WebView, GTK, Qt, QML, WebKit, Chromium, and Electron are excluded because HTMShell is intended to own a desktop scene rather than host a conventional web application runtime. They would also import application-window, navigation, networking, and toolkit assumptions that this gate is explicitly intended to avoid.

## Decision for the experiment

Evaluate the modular `blitz-dom`, `blitz-html`, `blitz-paint`, and `blitz-traits` crates at exact upstream commit `389e3762fc0ac19f6de7c0cec7201d0c8bde393a`. Use a CPU headless AnyRender backend and a strict package-local resource provider. Hide all Blitz values inside an experimental `htm-runtime` adapter; the CLI consumes only HTMShell-owned inputs, reports, and artifact results.

The pin makes the experiment reproducible and prevents an internal pre-release API change from silently changing the evidence. `blitz-shell`, `blitz-net`, Dioxus, and `winit` are excluded because static document construction, manual event driving, and headless paint are available below those layers. No final public API is designed because the dependency and process boundaries have not passed this gate.

## Alternatives considered

- Direct Stylo integration would provide more control but requires substantially more DOM glue, invalidation integration, layout bridging, and text/paint work before answering the narrower feasibility question.
- Servo embedding supplies a much broader browser engine and lifecycle than this gate requires.
- Taffy plus a custom style stack would require building or assembling cascade and selector behavior that Blitz already integrates, weakening the reuse hypothesis being tested.
- WebKitGTK conflicts with the native-scene, no-WebView, and no-GTK constraints.

## Acceptance criteria

The gate must demonstrate real local HTML/CSS, block/flex/grid/positioned layout, shaped text, a local image or static SVG, rounded and clipped painting, deterministic structured diagnostics, and a practical headless paint artifact. Resource policy must reject remote and escaping references. The enabled and linked graph must exclude `blitz-shell`, `blitz-net`, Dioxus, `winit`, network clients, browser/toolkit runtimes, Wayland clients, and Hyprland libraries. Malformed fixtures must not panic, and the report must distinguish observable behavior from unavailable instrumentation.

## Stop conditions

Stop rather than expand scope if the experiment requires `blitz-shell`, `winit`, Dioxus, inseparable networking, a conventional UI toolkit, a WebView, a broad Blitz fork, copied Blitz source, a replacement CSS/layout/text engine, or changes to another CoastLineSec repository.

## Consequences

The workspace remains deliberately small: one adapter library, one CLI binary, one representative fixture, focused tests, and evidence documents. Even a successful result only authorizes another controlled prototype. It does not establish Blitz as the permanent engine or make its internal types part of HTMShell's API.

## Gate A result

The experiment passed its acceptance criteria for a deliberately narrow profile. Real local HTML/CSS, block, flex, grid, positioned layout, shaped text, local SVG, rounded/clipped paint, deterministic JSON, Vello CPU PNG output, and host-driven `:hover`/`:active` states worked without a prohibited dependency. Malformed inputs and rejected resources were contained by focused tests.

Important limitations remain: the convenience APIs do not expose detailed parser errors or exact invalidation counters, and `blitz-paint` rebuilds its AnyRender scene for every exported phase. The decision is therefore **CONTINUE WITH NARROWER PROFILE**, not adoption of Blitz as the permanent runtime.
