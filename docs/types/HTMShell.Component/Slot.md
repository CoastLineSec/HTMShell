# `HTMShell.Component.Slot`

**Kind:** Declarative content projection | **Status:** Experimental

`HTMShell.Component.Slot` projects caller-owned direct children into default or named definition-owned insertion points. A component may declare up to 32 slots.

## Declaration

The component export declares an ordered `slots` array:

```json
{
  "name": "content-frame",
  "source": "components/content-frame.html",
  "slots": [
    {
      "name": "default",
      "required": false
    },
    {
      "name": "icon",
      "required": false
    },
    {
      "name": "content",
      "required": true
    }
  ]
}
```

`slots` is optional. Each object contains exactly `name` and the JSON boolean `required`. Shorthand strings, duplicate names, missing fields, unknown fields, and more than 32 entries are invalid. Declaration order is preserved.

A slot name contains 1 through 64 ASCII bytes, starts with a lowercase letter, and contains lowercase letters, digits, and single interior hyphens. It cannot end with a hyphen. Uppercase, dots, whitespace, leading or trailing hyphens, and consecutive hyphens are invalid. `default` identifies the one unqualified slot.

Schema version 1 and manifestless packages cannot declare component exports or slots.

## Definition

The default insertion point is an attribute-free `slot`. A named insertion point has exactly one `name` attribute:

```html
<template data-htm-component="content-frame">
  <article class="content-frame">
    <slot name="icon">
      <span class="fallback-icon">●</span>
    </slot>
    <p>Before</p>
    <slot>
      <p>No default content was supplied.</p>
    </slot>
    <slot name="content"></slot>
    <p>After</p>
  </article>
</template>
```

Every declaration matches exactly one insertion point. A missing, undeclared, or duplicate insertion point is invalid. The default insertion point cannot use `name="default"`. A named insertion point accepts no attribute other than `name`. A slot nested in fallback, a root-document slot, or a slot in caller content is invalid.

The `slot` element is an internal insertion directive. It does not add a built-in registry entry or remain as a public rendered element.

## Assignment

Unqualified direct children route to `default`. A direct child element with `slot="<name>"` routes to that named slot:

```html
<htm-use component="controls.content-frame">
  <span slot="icon">✓</span>
  <strong>Default content</strong>
  <p slot="content">Named content</p>
</htm-use>
```

Non-whitespace text, ordinary HTML elements, self-contained inline SVG, nested component uses, and built-ins permitted by the caller scope are assignable. Text cannot carry a route and targets `default`. Whitespace-only text and comments are ignored for assignment.

Routing is accepted only on direct child elements. The default slot is always unqualified, so `slot="default"` is invalid. An unknown name, a `slot` attribute outside a direct invocation child, repeated `slot` attributes, or unqualified assignable content without a declared default slot is invalid. Each caller node is assigned once and is not also rendered at the invocation site. Caller order is preserved among nodes assigned to the same slot.

## Optional and required behavior

Each declared slot resolves independently to one outcome:

- `Assigned`: one or more caller nodes are projected;
- `Fallback`: no caller node is assigned and definition fallback is instantiated;
- `EmptyOptional`: no caller node or fallback exists.

Assigned and fallback content never render together for one slot.

For a required slot, every invocation supplies at least one assignable node for that exact name. Whitespace and comments do not satisfy the requirement. A required insertion point cannot declare fallback content.

## Ownership and scope

Projection changes placement, not ownership.

Root-document content retains root state, action, ID, reference, package, resource, stylesheet, and semantic ownership. Content originating in a parent component retains that parent component instance, nearest `input.*` host, and component stylesheet scope. Projected content never gains callee inputs, callee selector ownership, or the callee package resource base.

Fallback belongs to the callee definition. It is cloned per callee instance, uses callee literal or resource-reference inputs, component stylesheets, and declared raster or simple SVG resource catalog, and remains subject to the component static profile. A fallback `<img src="input:name">` remains callee-owned while consuming the caller-owned source. Fallback cannot use ordinary relative images, SVG subresources or advanced references, CSS URL assets, fonts, or component-local IDs.

Repeat and contextual-repeat declarations cannot cross a projection boundary.

## Identity, order, and layout

A slot definition is identified by package snapshot generation, component definition identity, and slot name. Each invocation owns one distinct projection identity and semantic projection version per declared slot.

Assigned-node provenance retains caller source and component ownership plus projection placement. Fallback provenance derives from the callee component instance and fallback source node. Content changes affect the corresponding projection version, not component instance identity. Package, document, or output replacement creates fresh live identities.

The component template determines rendered order. At each insertion point, assigned caller nodes appear in caller order, otherwise fallback nodes appear in definition order.

Component host and slot/projection boundaries create no layout box and no paint, input region, accessibility node, stacking context, or public CSS selector. Children occupy the insertion location directly, so flex, grid, absolute positioning, clipping, hit testing, and foreground effects see the same ordinary nodes as equivalent handwritten HTML.

This contract does not implement Shadow DOM.

## Failure and limits

Declaration, template matching, required content, direct-child routing, caller scope, nested expansion, and node accounting complete before package snapshot publication. A failure rejects the complete candidate and retains the last successfully published snapshot.

| Resource | Limit |
| --- | ---: |
| Slot declarations per component | 32 |
| Slot name bytes | 64 |
| Slot insertion points per component definition | 32 |
| Component nesting depth | 32 |
| Component instances per prepared document | 4,096 |
| Referenced definitions per prepared document | 256 |
| Source nodes per definition | 10,000 |
| Expanded nodes per prepared document | 50,000 |

Assigned nodes count once. Fallback nodes count only when fallback is selected.

Component-local IDs, `::slotted()`, repeat projection, advanced or subresource-bearing component SVG, CSS URL assets, optional or dynamic resource inputs, dynamic slot switching, and hot reload are unavailable. Declared raster and simple SVG images are supported on component-owned `<img src="resource:name">` nodes. Required resource-reference values may be consumed through `<img src="input:name">`. Projection never changes the caller or callee ownership described above.

See [components](../../guide/components.md), [`HTMShell.Component`](README.md), [component inputs](Input.md), [component styles](Style.md), and [component resources](Resource.md).
