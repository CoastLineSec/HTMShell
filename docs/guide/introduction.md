# Introduction

An HTMShell package is a local directory with a manifest, HTML documents, CSS, and optional assets:

```text
shell.json
panel.html
overlay.html
style.css
assets/
```

The manifest creates shell surfaces. HTML defines their structure. CSS controls layout and appearance. Validated built-in elements bind native state and typed actions to ordinary HTML.

HTMShell parses, lays out, paints, and presents each document through Wayland. It does not embed a browser or toolkit. JavaScript is not supported.

The runtime and its authoring interfaces are experimental. The tracked [static panel example](../../examples/static-panel/shell.json) shows the complete package shape.

Continue with [running from source](running-from-source.md) or [create the first shell](first-shell.md).
