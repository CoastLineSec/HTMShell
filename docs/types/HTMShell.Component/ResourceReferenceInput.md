# `HTMShell.Component.ResourceReferenceInput`

**Kind:** Required static component value | **Status:** Experimental

`HTMShell.Component.ResourceReferenceInput` passes one already validated raster or simple SVG resource from a caller to a component. The value is an immutable reference to neutral package data. It is not a path, URL, resource name, catalog, or permission to load another resource.

## Declaration

A schema version 2 component declares a required resource-reference input:

```json
{
  "name": "icon",
  "type": "resource-reference",
  "resourceTypes": [
    "raster",
    "svg"
  ],
  "required": true
}
```

`resourceTypes` is required, contains one or two unique entries, and accepts only `raster` and `svg`. It is a semantic set: `["raster", "svg"]` and `["svg", "raster"]` produce the same definition semantics. Authored duplicates reject.

Every resource-reference input is required. The declaration must contain `required: true`. An omitted `required`, `required: false`, `default`, an empty kind set, or an unknown kind rejects the package candidate. Optional, nullable, and default resource values are not available.

Resource-reference inputs count toward the existing limit of 64 inputs per component.

## Surface resources

A schema version 2 panel or overlay template may declare up to 32 strict local resources:

```json
{
  "id": "panel",
  "kind": "panel",
  "document": "panel.html",
  "outputs": "all",
  "edge": "top",
  "thickness": 52,
  "reserveSpace": true,
  "resources": [
    {
      "name": "warning-icon",
      "type": "svg",
      "source": "assets/warning.svg"
    },
    {
      "name": "profile-photo",
      "type": "raster",
      "source": "assets/profile.webp"
    }
  ]
}
```

The entry shape, resource-name grammar, path rules, raster profile, simple SVG profile, and package limits are the same as component resources. Surface and component associations share one immutable package source when they declare the same owning-package path and resource kind. Every declared surface resource is read and validated before publication, including resources on an unused surface template.

A surface catalog is visible only when that surface root resolves a typed resource-reference assignment. It is not a general root resource provider. Another surface, ordinary root image or CSS loading, a component without an assignment, and a dependency package cannot see it.

## Assignment and forwarding

A surface root passes its own strict resource:

```html
<htm-use
  component="controls.status-row"
  input-icon="resource:warning-icon">
</htm-use>
```

A component caller uses the same syntax to pass one of its definition-owned resources:

```html
<htm-use
  component="nested-icon"
  input-icon="resource:warning-icon">
</htm-use>
```

The `resource:` name resolves in the caller's exact catalog. It never resolves in the callee, another surface, a sibling definition, or a dependency alias.

A component forwards a received resource without resolving a logical name:

```html
<htm-use
  component="nested-icon"
  input-icon="input:icon">
</htm-use>
```

The exact forwarded value syntax is `input:icon` for an input named `icon`.

The forwarding component must declare `icon` as a resource-reference input. Its accepted-kind set must be a subset of the child input's set. A parent accepting only raster may forward to a child accepting raster and SVG. A parent accepting raster and SVG cannot forward to a raster-only child because an SVG value could otherwise reach that child. Forwarding is immutable and bounded by the component nesting depth of 32.

Direct assignment and each forwarding hop create distinct generation-safe value identities while retaining the original source and authorization association. They perform no filesystem read, raster decode, SVG parse, or backend preparation.

## Image consumption

A component-owned or component-fallback HTML image consumes its local value:

```html
<img src="input:icon" alt="">
```

The lowercase `input:` form contains one declared input name and no slash, query, fragment, or percent encoding. The named input must be a resource-reference input. Only HTML `<img src>` supports this binding.

Root documents cannot consume `input:`. SVG `<image>`, `srcset`, CSS `url()`, `background-image`, masks, cursors, generated content, fonts, media elements, and generic attributes cannot consume a resource-reference input.

No unresolved `resource:` or `input:` typed value reaches the ordinary URL loader. Natural dimensions, layout, clipping, opacity, transforms, foreground effects, damage, and hit testing use the existing HTML image behavior.

## Ownership and isolation

The underlying resource source remains owned by its original surface or component association. The receiving component gains no path, resource name, package root, catalog, enumerator, URL provider, decoder, parser, or renderer handle.

The consuming image node and its scene usage belong to the callee component instance. Source ownership and DOM or style ownership are deliberately separate:

- a surface resource remains surface-owned when a component displays it;
- a parent component resource remains parent-owned when a nested child displays it;
- a forwarded value retains its original upstream owner through every hop;
- a fallback image is callee-owned while consuming the caller-owned source;
- projected caller content does not gain callee input values;
- siblings cannot inspect or consume one another's values;
- identical resource names in different surfaces remain isolated.

The source identity and semantic version stay unchanged during assignment and forwarding. Direct assignment, forwarding, and consumer usage identities remain distinct. Resource values do not alter component instance identity. Separate outputs receive distinct values and usages while sharing immutable neutral source data.

## Validation and lifetime

Resource catalogs, declarations, direct assignments, forwarding plans, kind compatibility, required inputs, consumers, and concrete prepared-root values validate before publication. Any missing assignment, malformed reference, unknown or wrong-owner resource, wrong kind, unsafe forwarding relation, invalid consumer, surface resource failure, or value-limit failure rejects the complete candidate. The last published snapshot and generation remain current.

Prepared-root specialization creates no resource I/O. After the strict catalog has been built:

- direct assignment performs zero filesystem reads, raster decodes, and SVG parses;
- forwarding performs zero filesystem reads, raster decodes, and SVG parses;
- image consumption and output instantiation perform zero filesystem reads, raster decodes, and SVG parses;
- rendering and device recovery perform zero filesystem reads, raster decodes, and SVG parses.

CPU and Vello reuse the existing neutral raster and simple SVG source paths. GPU preparation remains lazy and source-keyed. Device reset drops backend state but retains neutral sources and immutable values. Removing one output releases its local values and usages without invalidating another output.

Unused surface declarations are still validated eagerly, but create no resource-reference value, component instance, consumer usage, scene resource, GPU preparation, surface, frame, timer, native-service demand, or Wayland object.

## Limits

| Unit | Limit |
| --- | ---: |
| Resource-reference inputs per component | Included in 64 total inputs |
| Supplied inputs per invocation | 64 |
| Accepted kinds per resource-reference input | 2 |
| Resources per surface | 32 |
| Forwarding depth | 32 |
| Concrete resource-reference values per prepared root | 16,384 |
| Consumer usages per prepared root | Included in 50,000 expanded nodes |

Surface resource associations count toward the package limit of 4,096 resource associations. Their source files count toward the limit of 256 unique package resource sources and the existing candidate-read and neutral decoded-resource budgets. See [component resources](Resource.md) for the shared source, path, raster, and SVG limits.

Resource-reference inputs are static. Optional values, defaults, runtime mutation, state or action references, dynamic forwarding, CSS resources, fonts, advanced SVG, service-provided images, animation, and physics are not supported.

See [component inputs](Input.md), [component resources](Resource.md), [`HTMShell.Component`](README.md), [`ShellManifest`](../HTMShell/ShellManifest.md), and [components](../../guide/components.md).
