# `HTMShell.Component`

**Kind:** Declarative composition | **Status:** Experimental

`HTMShell.Component` describes manifest-owned inert templates, literal typed inputs, bounded default and named content slots, explicit references, and deterministic component uses in schema version 2 packages.

## Component definition

A package exports a definition from its ordered manifest `components` table:

```json
{
  "components": [
    {
      "name": "status-card",
      "source": "components/status-card.html",
      "inputs": [
        {
          "name": "label",
          "type": "string",
          "required": true
        }
      ],
      "slots": [
        {
          "name": "default",
          "required": false
        },
        {
          "name": "icon",
          "required": false
        }
      ]
    }
  ]
}
```

The entry has `name`, `source`, an optional ordered `inputs` array, and an optional ordered `slots` array with up to 32 unique declarations. Shell and library packages may export definitions. Schema version 1 and manifestless headless packages cannot.

The source declares the exported definition exactly once:

```html
<template data-htm-component="status-card">
  <article class="status-card">
    <strong>Ready</strong>
    <slot><p>No content</p></slot>
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
<htm-use
  component="controls.status-card"
  input-label="Connected">
</htm-use>
```

It requires one `component` attribute. Its only additional attributes are `input-<declared-name>` literals. Renderable direct children are accepted only when they route to a declared slot. Unqualified children route to `default`; direct child `slot="<name>"` attributes route to named slots. Unprefixed inputs, undeclared inputs, host attributes, unknown slots, nested routing attributes, and content without a matching slot are invalid. Invalid uses reject the complete package candidate before rendering.

Each use creates an internal `ComponentInstance` host with one resolved definition and cloned normalized template children. The source is not reparsed and no string substitution occurs.

The host has no visual box, paint, input region, accessibility node, or public CSS selector. Its ordinary expanded children participate directly in the current root document layout and global cascade.

## Ownership and identity

A definition belongs to one immutable package snapshot. Its logical identity contains the package snapshot generation, owning package ID, and component name. Its source path is diagnostic ownership metadata, not identity.

An instance identity additionally contains its document generation, parent component instance when nested, invocation position, and finite host role. Descendant provenance contains the instance identity, template source-node ordinal, and fresh DOM slot generation. Separate uses and separate outputs cannot alias identities. Input and slot content have separate deterministic semantic versions and never define instance or descendant identity.

Library definitions do not own or create surfaces. Loading or instantiating this static profile creates no native-service demand, state subscription, action lookup, stylesheet load, asset load, external resource, renderer object, or Wayland object.

## Content profile

Definitions may contain ordinary static HTML geometry and text, classes, non-resource inline styles, self-contained inline SVG, and nested component uses. Existing root package CSS may style expanded ordinary nodes through the current global cascade.

Definitions cannot contain:

- action, clock, repeat, range, peak, or contextual-repeat declarations;
- arbitrary state, action, service, or resource-reference behavior;
- undeclared or duplicate slots, invalid named routing, nested `slot` attributes, and invocation children without a matching slot;
- component-local IDs or local-reference attributes;
- scripts, component style elements, stylesheet links, `@import`, or `url()`;
- external images, SVG references, fonts, media, or other component-owned resources.

The existing `state-text`, `state-token`, and `state-value` declarations may consume compatible values from the nearest `input.*` host namespace. They do not create process-global state subscriptions or native-service demand. See [component inputs](Input.md).

Up to 32 standard-like default or named `slot` insertion points are available when the manifest declares them. Caller children retain caller input, state, action, ID, resource, and CSS ownership. Fallback children belong to the callee. Internal slot boundaries create no box or paint. See [slots](Slot.md).

Component-scoped CSS, `:host`, Shadow DOM, dynamic bindings, local state, action exports, repeat integration, external component resources, and hot reload are unavailable.

## Limits and errors

| Resource | Limit |
| --- | ---: |
| Exports per package | 256 |
| Exports per package graph | 4,096 |
| Instances per prepared document | 4,096 |
| Referenced definitions per prepared document | 256 |
| Nesting depth | 32 |
| Expanded nodes per prepared document | 50,000 |
| Inputs per component | 64 |
| Supplied inputs per use | 64 |
| String input | 4,096 UTF-8 bytes |
| Supplied literal bytes per use | 16 KiB |
| Slots per component | 32 |
| Slot name | 64 bytes |

All definitions, dependencies, and root invocations validate in the package-candidate transaction. Missing or duplicate declarations, invalid names, invalid sources, unknown references, cycles, forbidden content, and limit failures reject the candidate. No partial definition table or subtree is published, and a failed replacement retains the last successfully published snapshot.

Headless and live loading use the same immutable definitions and prepared root documents. Multi-output live loading shares definition data but creates output-local document, instance, descendant, scene, and surface identities.

See [components](../../guide/components.md), [component inputs](Input.md), [slots](Slot.md), [local packages](../../guide/packages.md), and [`HTMShell.Package`](../HTMShell.Package/README.md).
