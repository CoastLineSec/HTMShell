# HTMShell

**HTMShell (Hypertext Theming & Markup Shell)** is an experimental native desktop-shell platform that uses HTML and CSS as authoring languages for Linux desktop interfaces.

The goal is to make it possible to build panels, launchers, widgets, overlays, notifications, and other shell components using familiar web-style markup and styling without embedding a browser, WebView, Electron, GTK, Qt, or QML.

[Developer documentation](docs/README.md)

## Vision

HTML and CSS provide a mature and flexible language for designing interfaces. HTMShell applies that authoring model to the desktop while keeping rendering, input, and Wayland presentation native.

```text
HTML and CSS
      │
      ▼
HTMShell runtime
      │
      ▼
Wayland layer-shell presentation
      │
      ▼
Wayland compositor
```

## Design goals

* Real HTML and CSS authoring
* Native rendering without an embedded browser
* Layer-shell panels and overlays
* Standard Wayland surface and input lifecycles
* Incremental document updates
* Extensible theming and component development

## Project status

The authoring API, runtime, and supported CSS profile are experimental and may change as development continues.
