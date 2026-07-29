# Components

HTMShell schema version 2 packages can export inert, reusable HTML fragments. A component definition is parsed and validated once while an immutable package snapshot candidate is built. An explicit `htm-use` then creates a fresh instance by cloning normalized template nodes. No source text is substituted or reparsed per instance.

Components may declare bounded literal inputs and up to 32 default or named content slots. They still have no local IDs, local state, implicit actions or service state, repeat integration, scoped styles, or external component-owned resources.

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
      "source": "components/status-card.html",
      "inputs": [
        {
          "name": "label",
          "type": "string",
          "required": true
        },
        {
          "name": "count",
          "type": "number",
          "default": 0
        }
      ],
      "slots": [
        {
          "name": "default",
          "required": false
        }
      ]
    }
  ]
}
```

Each entry has `name`, `source`, an optional ordered `inputs` array, and an optional ordered `slots` array. The manifest owns the public export, input, and slot tables. A template found in a file is not exported implicitly.

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
    <slot><p>No content was supplied.</p></slot>
  </article>
</template>
```

Whitespace, comments, and ordinary parser metadata may surround declarations. Renderable content outside a declaration is invalid. Every manifest export must match exactly one declaration, every declaration must be exported, and duplicate names are invalid. Nested declaration templates and scripts are invalid.

A definition may contain ordinary text and layout elements, ordinary classes, inline SVG without external references, non-resource inline styles, foreground filters, and nested `htm-use` directives. Definitions are inert: loading or leaving one unused creates no surface, document instance, CSS load, asset load, service demand, renderer resource, frame, or Wayland object.

The component profile rejects:

- `action-button`, `clock-text`, `repeat`, `range-control`, `peak-monitor`, and contextual repeat forms;
- arbitrary state references, action references, service references, and other runtime `data-htm-*` behavior;
- undeclared or duplicate `slot` elements, invalid slot names or routing, nested slot fallback, and slot elements outside component definitions;
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

`htm-use` requires one `component` attribute. Its only other accepted attributes are declared `input-*` literals. It accepts no `id`, `class`, `style`, or unprefixed input. Renderable direct children are accepted only when they route to a declared slot. Unknown references, attributes, inputs, slots, or unroutable content reject the complete candidate. Schema version 1 and manifestless headless roots cannot use `htm-use`.

## Literal inputs

An export may declare at most 64 inputs. Input names contain 1 through 64 lowercase ASCII bytes, start with a letter, and use lowercase letters, digits, and single interior hyphens. They cannot end with a hyphen. These names are reserved:

```text
component slot id class style input state action service resource repeat surface host
```

The six input types are `string`, `number`, `boolean`, `token`, `color`, and `length`. A declaration is either required or supplies a typed default:

```json
{
  "name": "status-card",
  "source": "components/status-card.html",
  "inputs": [
    {
      "name": "label",
      "type": "string",
      "required": true
    },
    {
      "name": "enabled",
      "type": "boolean",
      "default": true
    },
    {
      "name": "accent",
      "type": "color",
      "default": "#7cc4ff"
    }
  ]
}
```

Pass literals with `input-<name>`:

```html
<htm-use
  component="controls.status-card"
  input-label="Connected"
  input-enabled="false"
  input-accent="rgb(124 196 255)">
</htm-use>
```

Strings preserve the HTML-decoded Unicode value and whitespace. Numbers are complete finite decimal literals, normalize negative zero, and do not accept units. Booleans accept exactly `true` or `false`. Tokens use the existing single state-token vocabulary. Colors use context-free CSS color syntax and normalize to encoded-sRGB RGBA. Lengths accept finite `px` values and unitless zero; percentages and font-, viewport-, container-, variable-, or calculation-dependent lengths are invalid.

Each instance owns an immutable input map in declaration order. Required inputs must be supplied, defaults are normalized before publication, and a defaulted value has the same semantic input version as an explicitly supplied equivalent value. Invocation attribute order does not affect the version. Input values do not define component instance or descendant identity.

Components consume a value only through their nearest host's `input.<name>` namespace. `state-text` can display the canonical value of all six types, `state-token` accepts `token` and `boolean`, and `state-value` accepts `number` with raw formatting. These local consumers create no global state key, subscription, action lookup, or native-service demand:

```html
<template data-htm-component="status-card">
  <article class="status-card">
    <span data-htm-element="state-text" data-htm-bind="input.label"></span>
    <span data-htm-element="state-token" data-htm-bind="input.enabled"></span>
  </article>
</template>
```

Nested components receive only their own declared literals and defaults. Parent inputs are not inherited or forwarded. Placeholder scanning, interpolation, expressions, state-reference inputs, action-reference inputs, and resource-reference inputs do not exist.

## Content slots

A component export may declare up to 32 ordered slots. `default` is the unqualified slot; every other declaration is named:

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

Slot names contain 1 through 64 lowercase ASCII bytes. They start with a letter and contain lowercase letters, digits, and single interior hyphens. A name cannot end with a hyphen. Dots, whitespace, uppercase, leading or trailing hyphens, and consecutive hyphens are invalid. Declarations are unique and preserve manifest order.

The matching template contains one insertion point for every declaration. The default insertion point is unqualified. A named insertion point has exactly one `name` attribute:

```html
<template data-htm-component="content-frame">
  <article class="content-frame">
    <header>
      <slot name="icon">
        <span class="fallback-icon">●</span>
      </slot>
    </header>
    <p>Before content</p>
    <slot>
      <p class="empty-message">No content was supplied.</p>
    </slot>
    <slot name="content"></slot>
    <p>After content</p>
  </article>
</template>
```

Every declared slot has exactly one matching insertion point. Missing, undeclared, or duplicate insertion points reject the candidate. The default insertion point cannot use `name="default"`. A named insertion point accepts no attribute other than `name`.

Unqualified direct children route to `default`. A direct child with `slot="<name>"` routes to that named declaration:

```html
<htm-use component="controls.content-frame">
  <span slot="icon">✓</span>
  <strong>Caller-owned default content</strong>
  <p slot="content">Caller-owned named content</p>
</htm-use>
```

Only direct child elements can carry `slot`. The default route is always unqualified, so `slot="default"` is invalid. A routing attribute outside a direct invocation child, unknown slot name, duplicate routing attribute, or unqualified assignable child when no default slot exists is invalid. Text cannot carry a route and therefore targets `default`. Caller order is preserved within each slot.

Non-whitespace text, ordinary elements, self-contained inline SVG, nested component uses, and caller-permitted root built-ins are assignable. Whitespace and comments do not count as assigned content. Each optional empty slot independently uses its definition-owned fallback, or produces no children when no fallback exists. Assigned content suppresses fallback for that slot only.

A required declaration uses `"required": true`. Every invocation must supply assignable content for that exact slot, and its matching insertion point cannot contain fallback content.

Projected nodes retain caller ownership. Root-owned content keeps root state, action, ID, local-reference, resource, and CSS behavior. Content projected by a parent component keeps that parent's nearest `input.*` scope and static component restrictions. It does not acquire callee inputs or the callee package resource base. Fallback content belongs to the callee and uses callee literal inputs.

Template order determines rendered order. At each insertion point, assigned caller nodes appear in their caller order, otherwise fallback nodes appear in definition order. The component host and internal slot/projection boundaries create no layout box, paint, input region, accessibility node, stacking context, or public CSS selector. Projected or fallback children occupy the insertion point directly. This is declarative projection, not Shadow DOM.

Repeat projection, component-local IDs, scoped CSS, and component-owned resources are not supported. Slot routing is immutable for one prepared package snapshot.

## Host and identity

Every invocation creates an internal non-rendering component host. The host retains the invocation provenance, definition identity, and component instance identity, but creates no layout box, paint, input region, accessibility node, or public CSS target. Expanded children participate in the parent layout and current root global cascade in invocation order.

Definitions are identified by the package snapshot generation, owning logical package ID, and component name. Each live or headless document creates distinct instance and descendant identities. Definitions may be shared across outputs, while document, component instance, descendant DOM, scene, surface, scale, damage, and presentation identities remain output-local.

Replacing a package snapshot, document, or output creates fresh generation-safe identities. Identity never derives from a memory address, render order, source path, or HTML `id`.

Each declared slot has a generation-scoped definition identity that includes its slot name. Every invocation has one distinct projection identity and semantic projection version per declared slot. Assigned node provenance retains caller identity, while fallback provenance derives from the callee instance. Changing projected content changes the corresponding projection version without making the content part of component instance identity.

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
| Input declarations per component | 64 |
| Supplied inputs per invocation | 64 |
| String input | 4,096 UTF-8 bytes |
| Supplied literal bytes per invocation | 16 KiB |
| Slots per component | 32 |
| Slot name | 64 bytes |

Component references form a separately validated dependency graph. Direct, indirect, and cross-package recursion cannot become current. The dependency-first definition order is deterministic, shared definitions are parsed once, and diamonds reuse one immutable definition.

See the [package graph example](../../examples/package-graph/shell.json), the [local package guide](packages.md), the [`HTMShell.Component`](../types/HTMShell.Component/README.md) reference, the [component input reference](../types/HTMShell.Component/Input.md), and the [slot reference](../types/HTMShell.Component/Slot.md).

Local ID scoping, dynamic state and action bindings, repeat integration, component-scoped CSS, external component resources, and hot reload remain unavailable.
