# Static components

HTMShell schema version 2 packages can export inert, reusable HTML fragments. A component definition is parsed and validated once while an immutable package snapshot candidate is built. An explicit `htm-use` then creates a fresh instance by cloning normalized template nodes. No source text is substituted or reparsed per instance.

This initial profile is intentionally static. It provides deterministic composition without component inputs, slots, local IDs, local state, actions, service state, repeat integration, scoped styles, or external component-owned resources.

## Export a definition

Both `shell` and `library` packages may declare an optional ordered `components` array:

```json
{
  "version": 2,
  "package": {
    "id": "org.example.controls",
    "kind": "library",
    "version": "1.0.0"
  },
  "dependencies": [],
  "components": [
    {
      "name": "status-card",
      "source": "components/status-card.html"
    }
  ]
}
```

Each entry has exactly `name` and `source`. The manifest owns the public export table. A template found in a file is not exported implicitly.

Only the root shell package owns surfaces and topology. A library component always renders inside the root-owned document that explicitly instantiates it.

A component name contains 3 through 64 lowercase ASCII bytes, starts with a letter, ends with a letter or digit, and contains at least one single interior hyphen. The remaining characters are lowercase letters, digits, and hyphens. Dots, whitespace, uppercase letters, leading or trailing hyphens, and consecutive hyphens are invalid.

Names used by the eight built-in behavior declarations are reserved. `htm-use`, every `htm-` prefix, every `xml-` prefix, and every `xlink-` prefix are also reserved. Components and `htm-use` are composition directives, so they do not add entries to the built-in visual registry.

The source path is a normalized package-relative UTF-8 path of at most 512 bytes. It must remain inside both the owning package and composition root. Absolute paths, parent traversal, backslashes, URLs, symbolic links, special files, and directories are rejected. A source document is at most 2 MiB and contributes to the 256 MiB package-candidate read budget.

## Define a template

A source document contains one or more top-level declarations:

```html
<!doctype html>
<template data-htm-component="status-card">
  <article class="status-card">
    <strong>Ready</strong>
  </article>
</template>
```

Whitespace, comments, and ordinary parser metadata may surround declarations. Renderable content outside a declaration is invalid. Every manifest export must match exactly one declaration, every declaration must be exported, and duplicate names are invalid. Nested declaration templates and scripts are invalid.

A definition may contain ordinary text and layout elements, ordinary classes, inline SVG without external references, non-resource inline styles, foreground filters, and nested `htm-use` directives. Definitions are inert: loading or leaving one unused creates no surface, document instance, CSS load, asset load, service demand, renderer resource, frame, or Wayland object.

The static profile rejects:

- all eight built-in behavior declarations, including `state-text`, `state-token`, `state-value`, `action-button`, `clock-text`, `repeat`, `range-control`, and `peak-monitor`;
- contextual repeat forms, state references, action references, service references, and other runtime `data-htm-*` behavior;
- `slot` elements and attributes, component input attributes, and renderable invocation children;
- `id`, `for`, fragment references, and supported ARIA local-reference attributes;
- scripts, style elements, stylesheet links, `@import`, `url()`, and URL-valued CSS;
- external images, SVG references, fonts, media, data files, or other component-owned resources.

Root shell documents retain their existing built-ins, state, actions, repeats, stylesheets, and resources outside component definitions.

## Instantiate a component

A root schema version 2 document can use a same-package export by name:

```html
<htm-use component="status-card"></htm-use>
```

It can use a direct library dependency through the declaring package's alias:

```html
<htm-use component="controls.status-card"></htm-use>
```

A nested component resolves references in its definition owner's package scope. Bare references select an export in that package. Qualified references use one direct dependency alias from that package. Parent aliases, transitive aliases, package IDs, filesystem paths, `self`, and `root` do not leak into the scope.

`htm-use` requires exactly one `component` attribute. It accepts no `id`, `class`, `style`, input, or slot attributes. Only whitespace and comments may be children. Unknown references, attributes, or renderable children reject the complete candidate. Schema version 1 and manifestless headless roots cannot use `htm-use`.

## Host and identity

Every invocation creates an internal non-rendering component host. The host retains the invocation provenance, definition identity, and component instance identity, but creates no layout box, paint, input region, accessibility node, or public CSS target. Expanded children participate in the parent layout and current root global cascade in invocation order.

Definitions are identified by the package snapshot generation, owning logical package ID, and component name. Each live or headless document creates distinct instance and descendant identities. Definitions may be shared across outputs, while document, component instance, descendant DOM, scene, surface, scale, damage, and presentation identities remain output-local.

Replacing a package snapshot, document, or output creates fresh generation-safe identities. Identity never derives from a memory address, render order, source path, or HTML `id`.

## Validation and limits

All package manifests, component sources, component references, component cycles, and root entry documents validate before a snapshot is published or a surface is created. A failed candidate leaves the last successfully published snapshot current. Headless and live loading consume the same prepared roots and expansion rules.

| Resource | Limit |
| --- | ---: |
| Component name | 64 bytes |
| Component exports per package | 256 |
| Component exports per graph | 4,096 |
| Component source document | 2 MiB |
| Source nodes per definition | 10,000 |
| Component instances per prepared document | 4,096 |
| Referenced definitions per prepared document | 256 |
| Component nesting depth | 32 |
| Expanded nodes per prepared document | 50,000 |

Component references form a separately validated dependency graph. Direct, indirect, and cross-package recursion cannot become current. The dependency-first definition order is deterministic, shared definitions are parsed once, and diamonds reuse one immutable definition.

See the [package graph example](../../examples/package-graph/shell.json), the [local package guide](packages.md), and the [`HTMShell.Component`](../types/HTMShell.Component/README.md) reference.

Component inputs, slots, local ID scoping, state and action access, repeat integration, component-scoped CSS, external component resources, and hot reload remain unavailable.
