# Components

HTMShell schema version 2 packages can export inert, reusable HTML fragments. A component definition is parsed and validated once while an immutable package snapshot candidate is built. An explicit `htm-use` then creates a fresh instance by cloning normalized template nodes. No source text is substituted or reparsed per instance.

Components may declare bounded literal or required resource-reference inputs, up to 32 default or named content slots, up to 16 package-owned stylesheets, and up to 32 static raster or simple SVG resources. Surfaces may declare up to 32 strict local resources for typed assignment. Components still have no local IDs, local state, implicit actions or service state, repeat integration, SVG subresources, CSS URL assets, or fonts.

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
      ],
      "styles": [
        "components/status-card.css"
      ],
      "resources": [
        {
          "name": "status-icon",
          "type": "raster",
          "source": "assets/status-icon.png"
        }
      ]
    }
  ]
}
```

Each entry has `name`, `source`, an optional ordered `inputs` array of literal or required resource-reference declarations, an optional ordered `slots` array, an optional ordered `styles` array, and an optional ordered `resources` array. The manifest owns the public export, input, slot, stylesheet association, and resource association tables. A template found in a file is not exported implicitly.

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

A definition may contain ordinary text and layout elements, ordinary classes, inline SVG without external references, non-resource inline styles, foreground filters, declared raster or simple SVG `<img>` elements, input-supplied resource images, and nested `htm-use` directives. Declared component stylesheets and resources are read during package candidate validation. Raster sources are decoded and simple SVG sources are parsed. Leaving a definition unused creates no surface, document instance, resource-reference value, prepared resource usage, computed-style work, renderer resource, GPU upload, service demand, frame, or Wayland object.

The component profile rejects:

- `action-button`, `clock-text`, `repeat`, `range-control`, `peak-monitor`, and contextual repeat forms;
- arbitrary state references, action references, service references, and other runtime `data-htm-*` behavior;
- undeclared or duplicate `slot` elements, invalid slot names or routing, nested slot fallback, and slot elements outside component definitions;
- `id`, `for`, fragment references, and supported ARIA local-reference attributes;
- scripts, style elements, stylesheet links, `@import`, `url()`, and URL-valued CSS;
- undeclared images, ordinary relative component image paths, SVG subresources or advanced references, fonts, media, data files, or other component-owned resources.

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

`htm-use` requires one `component` attribute. Its only other accepted attributes are declared `input-*` assignments. Literal inputs accept typed literals. Resource-reference inputs accept caller-local `resource:name` or static `input:name` forwarding. It accepts no `id`, `class`, `style`, or unprefixed input. Renderable direct children are accepted only when they route to a declared slot. Unknown references, attributes, inputs, slots, or unroutable content reject the complete candidate. Schema version 1 and manifestless headless roots cannot use `htm-use`.

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

Nested components receive only their own declared values. Literal parent inputs are not inherited or forwarded. Placeholder scanning, interpolation, expressions, state-reference inputs, and action-reference inputs do not exist.

## Resource-reference inputs

A reusable component may require one caller-authorized raster or simple SVG source:

```json
{
  "name": "icon",
  "type": "resource-reference",
  "resourceTypes": ["raster", "svg"],
  "required": true
}
```

`resourceTypes` contains one or two unique entries from `raster` and `svg`. Order does not change the semantic kind set. The input is required-only: `required: true` must be present and defaults, optional values, and null are invalid.

A schema version 2 panel or overlay can declare an ordered strict local `resources` array with at most 32 entries. The entries use the same exact `name`, `type`, and `source` shape, secure package-root-relative loader, formats, and limits as component resources. This catalog is used only for typed assignments from that surface root:

```html
<htm-use
  component="controls.status-row"
  input-icon="resource:warning-icon">
</htm-use>
```

A component caller uses the same syntax to pass an intrinsic resource from its own definition catalog. The name always resolves in the caller scope. It never grants the callee another catalog lookup.

A component statically forwards a received reference:

```html
<htm-use
  component="nested-icon"
  input-icon="input:icon">
</htm-use>
```

The forwarding input must be a resource-reference input. The parent accepted-kind set must be a subset of the child accepted-kind set. Each hop receives a distinct immutable value identity while retaining the original source, semantic version, and owner. Forwarding is bounded by the component nesting depth of 32 and adds no state cell, observer, callback, timer, or runtime mutation.

The callee consumes the value only through an ordinary component-owned or fallback image:

```html
<img src="input:icon" alt="">
```

The lowercase `input:` reference contains one input name and no slash, query, fragment, or percent encoding. No other element or attribute can consume it. Natural dimensions and ordinary image layout come from the underlying raster or simple SVG source.

The source stays owned by the original surface or component association. The callee image owns a distinct usage. Fallback keeps callee DOM and style ownership while using the caller source. Projected caller content does not gain callee inputs, siblings do not share values, and surface catalogs remain isolated.

All direct assignments, forwarding plans, kind checks, required values, and image consumers resolve before publication. A failed reference rejects the complete candidate and retains the last published snapshot. Assignment, forwarding, consumption, output instantiation, rendering, and device recovery perform no filesystem read, raster decode, or SVG parse. CPU and Vello reuse the existing immutable source paths. One prepared root may contain at most 16,384 concrete resource-reference values.

Strict surface resources do not replace ordinary root resource loading. Existing root-relative images, external SVG, CSS resources, fonts, caching, and symlink behavior remain unchanged. Root `<img src="resource:name">` and root `<img src="input:name">` are still invalid.

See the [resource-reference input reference](../types/HTMShell.Component/ResourceReferenceInput.md).

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

Projected nodes retain caller ownership. Root-owned content keeps root state, action, ID, local-reference, resource, and style ownership. Content projected by a parent component keeps that parent's nearest `input.*` and stylesheet scope plus its static component restrictions. It does not acquire callee inputs, callee styles, or the callee package resource base. Fallback content belongs to the callee and uses callee literal inputs and styles.

Template order determines rendered order. At each insertion point, assigned caller nodes appear in their caller order, otherwise fallback nodes appear in definition order. The component host and internal slot/projection boundaries create no layout box, paint, input region, accessibility node, stacking context, or public CSS selector. Projected or fallback children occupy the insertion point directly. This is declarative projection, not Shadow DOM.

Repeat projection, component-local IDs, and `::slotted()` are not supported. Slot routing is immutable for one prepared package snapshot.

## Component stylesheets

The optional `styles` array associates ordered CSS files with one component definition:

```json
{
  "name": "status-card",
  "source": "components/status-card.html",
  "styles": [
    "components/status-card.css",
    "components/status-card-density.css"
  ]
}
```

Paths are resolved from the package that owns the export. Each path is normalized, package-contained, non-symlink, at most 512 UTF-8 bytes, and names a regular file of at most 1 MiB. A component may declare 16 sheets, and one package may use 64 unique component stylesheet files. Shared files are read and parsed once per immutable snapshot candidate, then associated independently with every declaring definition. Manifest order controls equal-specificity conflicts between files; source order and ordinary selector specificity remain unchanged.

When a prepared root can reach any styled component, the complete root uses ownership-aware selector matching. Root styles match root-owned nodes and root-owned projected content, but do not directly match component internals or fallback. A component sheet matches ordinary and fallback nodes owned by that component instance. It does not match root nodes, sibling instances, nested child internals, or caller-owned assigned slot content. A component without its own sheets is still a selector boundary inside such a root.

Roots with no reachable styled component retain the legacy document-global cascade, so existing component packages require no migration until they use `styles`. An unused styled export does not activate unrelated prepared roots. Panel and overlay roots can therefore select different matching modes.

Selector isolation does not change the rendered tree. Inherited properties and supported custom properties continue across rendered ancestry from root to component, parent to nested child, and callee containers to projected caller content. Non-inherited properties do not cross. Inline style keeps its existing priority and node ownership. Existing `:hover` and `:active` matching is instance-local; invalidation may conservatively inspect more of the document while preserving scoped results.

Component sheets use the existing HTMShell CSS selector and property profile. `@import`, `@font-face`, any `url()` resource, `:host`, `::slotted()`, shadow-tree selectors, ID selectors, and unsupported CSS reject the complete candidate. No external asset is fetched. The scope is internal metadata: no public attribute, generated class, selector rewriting, layout wrapper, or Shadow DOM is introduced.

## Component image resources

The optional `resources` array declares definition-owned static raster or simple SVG sources:

```json
{
  "name": "status-card",
  "source": "components/status-card.html",
  "resources": [
    {
      "name": "status-icon",
      "type": "raster",
      "source": "assets/status-icon.png"
    },
    {
      "name": "status-symbol",
      "type": "svg",
      "source": "assets/status-symbol.svg"
    }
  ]
}
```

Component markup consumes the association by logical name:

```html
<img src="resource:status-icon" alt="">
```

Names contain 1 through 64 lowercase ASCII bytes and use letters, digits, and single interior hyphens. Paths resolve from the package root that owns the export. They are normalized, package-contained, opened without following symlinks, limited to 512 UTF-8 bytes and 32 components, and must identify a regular file.

Raster resources accept PNG, JPEG, and static WebP. GIF, animated WebP, animated PNG, and other raster formats reject. An encoded raster is at most 8 MiB. Width and height are at most 4,096 pixels, total pixels are at most 16,777,216, one canonical RGBA8 decode is at most 64 MiB, and all decoded component rasters in one snapshot are at most 256 MiB.

Simple SVG resources are geometry-only and self-contained. They allow only `svg`, `g`, `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, and `polygon`, with finite transforms, solid hexadecimal fill and stroke, opacity, stroke geometry, and the attributes required by those shapes. The root requires a positive finite `viewBox`. The source is at most 2 MiB, natural dimensions are at most 4,096 by 4,096 with area at most 16,777,216, and the tree is limited to 4,096 nodes, depth 64, and 65,536 normalized path segments.

SVG CSS, text, fonts, images, data URLs, external references, IDs, links, fragments, gradients, patterns, clips, masks, filters, markers, symbols, use, scripts, and animation reject. Explicit no-op resolvers and an empty font database prevent subresource and font access. Component or caller CSS does not style inside the SVG tree. Root external SVG behavior remains separate.

Every declared source, including one on an unused definition, is eagerly read and validated before publication. One owning-package, resource kind, and logical-path source is read once per candidate. Raster sources decode once into immutable neutral pixels. SVG sources parse once into immutable neutral trees. Sources are shared by associations, definitions, instances, roots, and outputs. CPU and Vello consume the same neutral data without filesystem access or reparsing after publication.

Definition content and fallback resolve against the callee definition catalog. Nested children use their own catalogs. Assigned slot content keeps caller ownership: root content keeps the root resource pipeline, and parent-component content keeps its parent definition catalog. Projection never grants access to a callee resource. Dependency resource aliases do not exist.

Intrinsic `resource:name` and passed `input:name` syntax applies only to `<img src>`. It does not add `srcset`, SVG `<image>` resources, CSS `url()`, fonts, media, data, optional or dynamic resources, animation, network loading, or data URLs. See the [component resource reference](../types/HTMShell.Component/Resource.md).

## Host and identity

Every invocation creates an internal non-rendering component host. The host retains the invocation provenance, definition identity, and component instance identity, but creates no layout box, paint, input region, accessibility node, or public CSS target. Expanded children participate in the parent layout and the prepared root's selected stylesheet ownership mode in invocation order.

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
| Concrete resource-reference values per prepared root | 16,384 |
| Slots per component | 32 |
| Slot name | 64 bytes |
| Stylesheets per component | 16 |
| Unique component stylesheet files per package | 64 |
| Component stylesheet path | 512 UTF-8 bytes |
| Component stylesheet file | 1 MiB |
| Image resources per component | 32 |
| Image resources per surface | 32 |
| Resource associations per package | 4,096 |
| Unique image sources per package | 256 |
| Resource name | 64 ASCII bytes |
| Resource path | 512 UTF-8 bytes and 32 components |
| Encoded raster source | 8 MiB |
| Raster dimensions | 4,096 by 4,096 pixels |
| Raster pixels | 16,777,216 |
| Decoded raster | 64 MiB |
| Snapshot decoded component resources | 256 MiB |
| Encoded SVG source | 2 MiB |
| SVG natural dimensions | 4,096 by 4,096 pixels |
| SVG natural area | 16,777,216 pixels |
| SVG allowed nodes | 4,096 |
| SVG element depth | 64 |
| SVG normalized path segments | 65,536 |

Component references form a separately validated dependency graph. Direct, indirect, and cross-package recursion cannot become current. The dependency-first definition order is deterministic, shared definitions are parsed once, and diamonds reuse one immutable definition.

See the [package graph example](../../examples/package-graph/shell.json), the [local package guide](packages.md), the [`HTMShell.Component`](../types/HTMShell.Component/README.md) reference, the [component input reference](../types/HTMShell.Component/Input.md), the [resource-reference input reference](../types/HTMShell.Component/ResourceReferenceInput.md), the [slot reference](../types/HTMShell.Component/Slot.md), the [component style reference](../types/HTMShell.Component/Style.md), and the [component resource reference](../types/HTMShell.Component/Resource.md).

Local ID scoping, host styling, slotted-content selectors, package-global library styles, dynamic state and action bindings, repeat integration, advanced or subresource-bearing component SVG, CSS URL assets, fonts, optional or dynamic resource inputs, and hot reload remain unavailable.
