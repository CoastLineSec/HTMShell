# `HTMShell.Component.Slot`

**Kind:** Declarative content projection | **Status:** Experimental

`HTMShell.Component.Slot` projects caller-owned children into one definition-owned insertion point. The current profile supports exactly one slot named `default`.

## Declaration

The component export declares an optional or required default slot:

```json
{
  "name": "content-frame",
  "source": "components/content-frame.html",
  "slots": [
    {
      "name": "default",
      "required": false
    }
  ]
}
```

`slots` is optional and contains at most one object. The object contains exactly `name` and `required`. `name` must be `default`, and `required` is a JSON boolean. Named slots, shorthand strings, duplicate entries, and unknown fields are invalid.

Schema version 1 and manifestless packages cannot declare component exports or slots.

## Definition

The matching component template contains exactly one attribute-free `slot`:

```html
<template data-htm-component="content-frame">
  <article class="content-frame">
    <p>Before</p>
    <slot>
      <p>No content was supplied.</p>
    </slot>
    <p>After</p>
  </article>
</template>
```

A declared slot without a matching element is invalid. An undeclared slot, duplicate slot, `name` attribute, any other slot attribute, nested slot fallback, root-document slot, or slot in caller content is invalid.

The `slot` element is an internal insertion directive. It does not add a built-in registry entry or remain as a public rendered element.

## Assignment

Caller children are assigned through the existing `htm-use`:

```html
<htm-use component="controls.content-frame">
  <strong>Projected content</strong>
</htm-use>
```

Non-whitespace text, ordinary HTML elements, self-contained inline SVG, nested component uses, and built-ins permitted by the caller scope are assignable. Whitespace-only text and comments are ignored for assignment. Caller order is preserved.

Children are valid only when the target declares the default slot. Caller `slot` attributes and named routing are unsupported. Each caller node is inserted once at the slot position and is not also rendered at the invocation site.

## Optional and required behavior

For an optional slot:

- assignable caller content is projected;
- otherwise definition-owned fallback is instantiated;
- otherwise the slot contributes no children.

Assigned and fallback content never render together.

For a required slot, every invocation supplies at least one assignable node. Whitespace and comments do not satisfy the requirement. A required slot cannot declare fallback content.

## Ownership and scope

Projection changes placement, not ownership.

Root-document content retains root state, action, ID, reference, package, resource, CSS, and semantic ownership. Content originating in a parent component retains that parent component instance and nearest `input.*` host. Projected content never gains callee inputs or the callee package resource base.

Fallback belongs to the callee definition. It is cloned per callee instance, uses callee literal inputs, and remains subject to the component static profile. It cannot load component-owned external resources or declare component-local IDs.

Repeat and contextual-repeat declarations cannot cross the projection boundary in the current profile.

## Identity and layout

The slot definition is identified by package snapshot generation, component definition identity, and the finite default-slot role. Each invocation owns a distinct projection identity and semantic projection version.

Assigned-node provenance retains caller source and component ownership plus the projection placement. Fallback provenance derives from the callee component instance and fallback source node. Package, document, or output replacement creates fresh live identities. Slot content is not part of component instance identity.

The component host and slot/projection boundaries create no layout box and no paint. They also create no input region, accessibility node, stacking context, or public CSS selector. Children occupy the slot location directly, so flex, grid, absolute positioning, clipping, hit testing, and foreground effects see the same ordinary nodes as equivalent handwritten HTML.

This contract does not implement Shadow DOM.

## Failure and limits

Slot declaration, template matching, required content, assignment, caller scope, nested component expansion, and node accounting complete before package snapshot publication. A failure rejects the complete candidate and retains the last successfully published snapshot.

| Resource | Limit |
| --- | ---: |
| Default slot declarations per component | 1 |
| Slot elements per component definition | 1 |
| Component nesting depth | 32 |
| Component instances per prepared document | 4,096 |
| Referenced definitions per prepared document | 256 |
| Source nodes per definition | 10,000 |
| Expanded nodes per prepared document | 50,000 |

Assigned nodes count once. Fallback nodes count only when fallback is selected.

Named slots, caller `slot` attributes, component-local IDs, scoped component CSS, repeat projection, component-owned resources, dynamic slot switching, and hot reload are unavailable.

See [components](../../guide/components.md), [`HTMShell.Component`](README.md), and [component inputs](Input.md).
