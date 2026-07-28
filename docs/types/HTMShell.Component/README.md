# `HTMShell.Component`

**Kind:** Static declarative composition | **Status:** Experimental

`HTMShell.Component` describes manifest-owned inert templates, explicit references, and deterministic component uses in schema version 2 packages.

## Component definition

A package exports a definition from its ordered manifest `components` table:

```json
{
  "components": [
    {
      "name": "status-card",
      "source": "components/status-card.html"
    }
  ]
}
```

The entry has exactly `name` and `source`. Shell and library packages may export definitions. Schema version 1 and manifestless headless packages cannot.

The source declares the exported definition exactly once:

```html
<template data-htm-component="status-card">
  <article class="status-card">
    <strong>Ready</strong>
  </article>
</template>
```

The template wrapper is inert and is not part of rendered content. Every declaration must be exported and every export must have one matching declaration. Renderable content outside top-level declarations, nested declarations, scripts, and unsupported static-profile features reject the package candidate.

One definition may contain at most 10,000 normalized source nodes. A component source document is at most 2 MiB. It is read and parsed once per immutable package snapshot candidate, regardless of its instance count.

## Component name

`ComponentName` has this grammar:

```text
letter (lowercase-letter | digit | "-")* "-" (lowercase-letter | digit | "-")*
```

The complete value contains 3 through 64 ASCII bytes, starts with a lowercase letter, ends with a lowercase letter or digit, contains at least one hyphen, and has no consecutive hyphens. Uppercase letters, dots, whitespace, and empty hyphen-separated segments are invalid.

The eight built-in element names, `htm-use`, and names beginning with `htm-`, `xml-`, or `xlink-` are reserved.

## Component reference

`ComponentReference` accepts either:

```text
status-card
controls.status-card
```

A bare name resolves in the package that owns the root document or current definition. A qualified name resolves through one direct dependency alias owned by that same package. Exactly zero or one dot is permitted. Package IDs, paths, inherited parent aliases, and transitive aliases are not accepted.

References are resolved before publication. Unknown aliases, unknown exports, direct recursion, indirect cycles, cross-package recursion, or nesting beyond 32 reject the candidate.

## Component use

`ComponentUse` is the public `htm-use` directive:

```html
<htm-use component="controls.status-card"></htm-use>
```

It requires exactly one `component` attribute, accepts no other attributes, and permits only whitespace and comments as children. Inputs and slots are not inferred from extra attributes or children. Invalid uses reject the complete package candidate before rendering.

Each use creates an internal `ComponentInstance` host with one resolved definition and cloned normalized template children. The source is not reparsed and no string substitution occurs.

The host has no visual box, paint, input region, accessibility node, or public CSS selector. Its ordinary expanded children participate directly in the current root document layout and global cascade.

## Ownership and identity

A definition belongs to one immutable package snapshot. Its logical identity contains the package snapshot generation, owning package ID, and component name. Its source path is diagnostic ownership metadata, not identity.

An instance identity additionally contains its document generation, parent component instance when nested, invocation position, and finite host role. Descendant provenance contains the instance identity, template source-node ordinal, and fresh DOM slot generation. Separate uses and separate outputs cannot alias identities.

Library definitions do not own or create surfaces. Loading or instantiating this static profile creates no native-service demand, state subscription, action lookup, stylesheet load, asset load, external resource, renderer object, or Wayland object.

## Content profile

Definitions may contain ordinary static HTML geometry and text, classes, non-resource inline styles, self-contained inline SVG, and nested component uses. Existing root package CSS may style expanded ordinary nodes through the current global cascade.

Definitions cannot contain:

- the eight built-in state, action, clock, repeat, range, or peak declarations;
- state, action, service, repeat, contextual-repeat, or input behavior;
- slots or invocation children;
- component-local IDs or local-reference attributes;
- scripts, component style elements, stylesheet links, `@import`, or `url()`;
- external images, SVG references, fonts, media, or other component-owned resources.

Component-scoped CSS, `:host`, Shadow DOM, inputs, slots, local state, action exports, repeat integration, external component resources, and hot reload are unavailable.

## Limits and errors

| Resource | Limit |
| --- | ---: |
| Exports per package | 256 |
| Exports per package graph | 4,096 |
| Instances per prepared document | 4,096 |
| Referenced definitions per prepared document | 256 |
| Nesting depth | 32 |
| Expanded nodes per prepared document | 50,000 |

All definitions, dependencies, and root invocations validate in the package-candidate transaction. Missing or duplicate declarations, invalid names, invalid sources, unknown references, cycles, forbidden content, and limit failures reject the candidate. No partial definition table or subtree is published, and a failed replacement retains the last successfully published snapshot.

Headless and live loading use the same immutable definitions and prepared root documents. Multi-output live loading shares definition data but creates output-local document, instance, descendant, scene, and surface identities.

See [static components](../../guide/components.md), [local packages](../../guide/packages.md), and [`HTMShell.Package`](../HTMShell.Package/README.md).
