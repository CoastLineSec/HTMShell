# `HTMShell.Component.Style`

**Kind:** Scoped author stylesheet | **Status:** Experimental

`HTMShell.Component.Style` associates package-contained CSS files with one component definition. HTMShell enforces selector ownership internally without adding public DOM attributes, generated classes, layout wrappers, selector rewriting, or Shadow DOM.

## Declaration

The component export contains an optional ordered `styles` array:

```json
{
  "name": "media-card",
  "source": "components/media-card.html",
  "styles": [
    "components/media-card.css",
    "components/media-card-density.css"
  ]
}
```

Each entry is one normalized package-relative UTF-8 path. A component may declare at most 16 entries. Duplicate normalized paths within one component are invalid. One owning package may use at most 64 unique component stylesheet files, even when a file is associated with several definitions.

Each path is at most 512 bytes and resolves beneath both the owning package root and the composition root. Absolute paths, empty components, parent traversal, backslashes, URL syntax, symlink files or path components, directories, and special files are invalid. Each source is at most 1 MiB and contributes to the existing 256 MiB package-candidate read budget.

HTMShell reads and parses each unique source once per immutable package snapshot candidate. Parsed sources are shared across definitions, component instances, prepared roots, outputs, headless documents, and live documents that retain the snapshot. Each definition keeps a separate immutable association in manifest declaration order.

## Activation

A prepared root uses ownership-aware selector matching when its reachable instantiated component graph contains at least one definition with a stylesheet association. Every component instance in that root is then a selector boundary, including components with no `styles` declaration.

A prepared root with no reachable styled component retains legacy document-global author matching. An unused styled export does not activate an unrelated root. Panel and overlay roots may therefore use different modes.

Activation and all owner associations validate before snapshot publication. A failure leaves the last published snapshot and generation unchanged and creates no surface or renderer work.

## Selector ownership

In an ownership-aware root:

- root styles match root-owned nodes and root-owned projected slot content;
- a component stylesheet matches ordinary and fallback nodes owned by an instance of its definition;
- parent component styles may match caller content that the parent projects into a child;
- a component stylesheet does not match caller-owned assigned slot content;
- parent selectors do not pierce nested child internals;
- child selectors do not escape into parent or root nodes;
- sibling instances and separate outputs remain isolated.

Same-scope type, class, universal, attribute, compound, descendant, child, `:hover`, and `:active` selectors retain the current HTMShell CSS behavior. Scope metadata adds no specificity. Normal specificity, stylesheet source order, manifest stylesheet order, and inline-style priority remain authoritative.

The component host and slot and ownership boundaries create no element, box, paint, input region, containing block, stacking context, or accessibility node. A template with several top-level nodes gives all of them the same component scope, but does not gain a synthetic common root. Authors may add an ordinary wrapper when a structural selector requires one.

## Inheritance and projection

Selector ownership does not change the rendered tree. Inherited properties and supported custom properties cross ordinary rendered ancestry:

- from a root ancestor into component top-level nodes;
- from a parent component into nested child top-level nodes;
- from a callee container into caller-owned projected content;
- from a component container into its fallback.

Non-inherited properties do not cross automatically. Inheritance does not change node ownership or allow selector piercing.

Assigned default and named slot content retains its caller stylesheet owner. Fallback uses the callee owner. Slot names and consumed `slot` routing attributes are not selector hooks. No `::slotted()` equivalent exists.

## CSS and resource profile

Component sheets use the existing HTMShell CSS property and selector profile. Unsupported CSS rejects the candidate rather than being silently omitted.

The following are unavailable:

- CSS `@import`;
- `@font-face` and font sources;
- external, relative, data, or foreign `url()` resources;
- `:host` and `:host(...)`;
- `::slotted(...)`, shadow parts, and shadow-tree traversal;
- component-local ID selectors;
- package-global library stylesheets;
- component external assets.

No referenced URL is fetched or resolved. Self-contained CSS values that require no external resource use remain subject to the existing CSS profile.

## Identity and lifecycle

Stylesheet source identity contains the package snapshot generation, owning package ID, normalized logical path, and source role. A deterministic semantic version derives from normalized parsed rules. Definition association identity also contains the component definition and manifest ordinal.

One definition-level association serves every instance, while selector scope instances remain document and output local. New package snapshots, document replacements, removed and re-added outputs, and fresh live documents receive fresh generation-safe owner and scope-instance identities. Closed surfaces perform no stylesheet presentation work, and idle surfaces add no polling or timers.

See [components](../../guide/components.md), [`HTMShell.Component`](README.md), [slots](Slot.md), and [local packages](../../guide/packages.md).
