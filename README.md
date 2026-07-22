# HTMShell

**HTMShell — Hypertext Theming & Markup Shell** is an experimental native desktop-shell platform that uses HTML and CSS as authoring languages for Linux desktop interfaces.

The goal is to make it possible to build panels, launchers, widgets, overlays, notifications, and other shell components using familiar web-style markup and styling—without embedding a browser, WebView, Electron, GTK, Qt, or QML.

## Vision

HTML and CSS provide a mature and flexible language for designing interfaces. HTMShell applies that authoring model to the desktop while keeping rendering, input, and compositor integration native.

```text
HTML and CSS
      │
      ▼
HTMShell runtime
      │
      ▼
Portable Wayland shell presentation
      │
      ▼
Supporting Wayland compositor

Optional later:
enhanced HTMShell compositor integration
```

HTMShell is not intended to render websites or recreate a full browser platform. It uses a focused desktop-oriented HTML and CSS profile designed specifically for shell interfaces.

## Design goals

* Real HTML and CSS authoring
* Native rendering without an embedded browser
* Compositor-neutral presentation through advertised Wayland protocols
* Standard Wayland surface and input lifecycles
* Semantic shell roles instead of compositor-specific render stages
* Incremental document updates
* Strong separation between the shell runtime and compositor
* Support for advanced compositor-native materials and effects
* Portable support across compatible Wayland compositors
* Extensible theming and component development

## Compositor integration

Layer shell is HTMShell's portable baseline presentation path. The runtime
discovers standard and existing extension protocols through the Wayland
registry; it does not identify the compositor or select a compositor-specific
implementation.

The experimental HTMShell compositor contract remains available as an optional
path for future integration that existing protocols cannot express. It is not
required for basic shell presentation. Hyprland is the first validation
environment, but neither the runtime nor the portable layer-shell path contains
Hyprland-specific behavior.

## Current status

HTMShell is in early experimental development.

The project has demonstrated:

* Headless parsing and rendering of local HTML and CSS
* Block, flexbox, and grid layouts
* Native text shaping
* Local images and SVG
* Host-driven document mutation
* Dynamic node insertion and removal
* Stylesheet replacement without rebuilding the document
* Deterministic scene diagnostics
* A compositor-neutral Wayland shell-surface protocol prototype
* Standard Wayland buffer and frame lifecycles
* Semantic compositor-controlled shell placement
* An experimental live layer-shell presentation path using shared-memory buffers
* One portable host process managing independent panel and overlay layer surfaces
* A validated local surface manifest expanded into independent instances for each eligible output
* Generation-safe output addition and removal through standard Wayland registry events

The portable live profile currently supports integer scale 1. HTMShell is not
yet a usable desktop shell, and no stable release is available.

## Non-goals

HTMShell is not:

* A web browser
* A WebView wrapper
* An Electron application
* A GTK or Qt shell
* A QML or QuickShell replacement layer
* A mechanism for rendering arbitrary websites on the desktop
* A compositor-specific shell tied permanently to one window manager

## Project status

The APIs, protocol, runtime architecture, and supported CSS profile are still experimental and may change substantially as development continues.

Contributions and compositor integration discussions will become more practical once the core contract and runtime boundaries are stable.
