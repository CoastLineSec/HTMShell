# `HTMShell.Component.Resource`

**Kind:** Definition-owned static image | **Status:** Experimental

`HTMShell.Component.Resource` gives a schema version 2 component definition or surface template a bounded catalog of package-owned raster images and simple SVG geometry. Component resources are referenced by logical name from component-owned HTML image elements. Surface resources are available only for typed component-input assignment.

## Declaration

A component export may contain an ordered `resources` array:

```json
{
  "name": "media-card",
  "source": "components/media-card.html",
  "resources": [
    {
      "name": "speaker-photo",
      "type": "raster",
      "source": "assets/speaker.png"
    },
    {
      "name": "speaker-symbol",
      "type": "svg",
      "source": "assets/speaker.svg"
    }
  ]
}
```

Every entry contains exactly `name`, `type`, and `source`. Supported types are `raster` and `svg`. A component may declare at most 32 resources. A schema version 2 panel or overlay may also declare at most 32 entries with the same shape and validation. Names are local to one component definition or one surface template, and one source file may be associated with several names, definitions, or surfaces.

Names contain 1 through 64 lowercase ASCII bytes. They start with a letter, end with a letter or digit, and otherwise use lowercase letters, digits, and single hyphens. Consecutive hyphens are invalid. The names `resource`, `component`, `input`, `slot`, `style`, `state`, `action`, `service`, `surface`, `host`, and `repeat` are reserved. Names beginning with `htm-`, `xml-`, or `xlink-` are also reserved.

## Markup reference

Component-owned HTML consumes either resource kind through one exact form:

```html
<img src="resource:speaker-symbol" alt="">
```

The `resource:` prefix is lowercase. The reference contains only one declared logical name, with no slash, query, fragment, percent encoding, or dependency alias. This profile applies only to `<img src>`. It does not add component resource support to SVG `<image>`, `srcset`, CSS `url()`, fonts, media elements, or generic data consumers.

Ordinary relative, absolute, network, and data URL image sources remain invalid in component definitions. Root documents do not receive a general `resource:` catalog and retain their existing resource behavior. A strict surface catalog is consulted only when its root assigns `resource:name` to a declared `resource-reference` component input. An ordinary root `<img src="resource:name">` remains invalid.

## Ownership

A definition-owned image resolves through that definition's catalog. Fallback content uses the callee definition catalog. A nested child uses its own catalog.

Assigned slot content retains caller ownership:

- root-owned projected content keeps the root resource pipeline;
- parent-component-owned projected content keeps the parent definition catalog;
- projection into a child does not grant access to the child catalog.

A component cannot address another component's resource name unless it declares its own association. Dependency package aliases are not resource lookup paths.

A typed resource-reference input can transport one already resolved association to a callee. The source remains owned by the original surface or component association, while the receiving component image owns a fresh usage. Static forwarding retains the same source and origin association. The callee receives no logical-name lookup, catalog, path, URL provider, decoder, or parser authority. See [resource-reference inputs](ResourceReferenceInput.md).

## Paths and filesystem boundary

Resource source paths resolve from the package root that owns the component export. Paths are normalized, package-relative UTF-8 paths with at most 512 bytes and 32 components. Absolute paths, empty or current-directory components, parent traversal, backslashes, NUL, queries, fragments, percent encoding, URL or foreign-scheme syntax, and environment expansion are invalid.

Every path component is opened without following symbolic links. The final object must be a regular file. Directories, FIFOs, sockets, devices, and other special files are invalid. The bounded read verifies the opened object before and after reading and rejects an observed mutation instead of publishing uncertain content. No file descriptor remains in the package snapshot.

Hard-linked paths remain separate logical source identities.

## Raster profile

The accepted encoded raster formats are:

- PNG;
- JPEG;
- static WebP.

GIF, animated WebP, animated PNG, BMP, TIFF, ICO, AVIF, and other raster formats are invalid. HTMShell detects the format from encoded bytes rather than the file extension. Animated content is rejected instead of displaying one frame.

The immutable snapshot stores width, height, encoded format metadata, and straight-alpha RGBA8 pixels in top-to-bottom row order. PNG and WebP alpha are preserved. JPEG pixels receive alpha 255. Transparent-pixel RGB values follow the locked decoder result. The runtime does not promise ICC color management, EXIF orientation application, or gamma-perfect correction beyond the current decoder pipeline.

## Simple SVG profile

A component SVG is static, self-contained, geometry-only, font-free, CSS-free, reference-free, and subresource-free.

The only allowed elements are:

```text
svg
g
path
rect
circle
ellipse
line
polyline
polygon
```

The root is the only `svg` element and must declare a positive finite `viewBox`. `width` and `height` may both be omitted, in which case natural dimensions come from the viewBox. If present, both must use positive finite unitless or `px` values. Percentages and context-dependent units reject.

Allowed attributes are limited by element. They cover `width`, `height`, `viewBox`, `preserveAspectRatio`, finite transforms, opacity, solid hexadecimal fill and stroke, fill and stroke opacity, fill rule, stroke geometry, path data, and the geometry attributes needed by the allowed shapes. Paint is limited to `none` or hexadecimal colors in `#rgb`, `#rgba`, `#rrggbb`, or `#rrggbbaa` form. Named colors and `currentColor` are not accepted.

The SVG profile rejects:

- nested `svg`, `defs`, unknown elements, foreign namespaces, and unknown attributes;
- CSS, `style` elements, `style` attributes, classes, CSS variables, and stylesheets;
- text, font lookup, glyph conversion, and font declarations;
- image elements, embedded raster data, data URLs, external files, and network references;
- IDs, links, fragments, `href`, `xlink:href`, `symbol`, and `use`;
- gradients, patterns, paint-server URLs, clip paths, masks, filters, and markers;
- scripts, event-handler attributes, processing instructions, document types, entity declarations, and animation.

Component stylesheets and caller styles do not enter the external SVG tree. Presentation attributes are the only styling mechanism. The component parser uses explicit no-op image resolvers, an empty font database, no font resolver, no injected stylesheet, no resource directory, and XML parsing with DTD and external entity resolution disabled. Parsing the declared SVG performs no secondary filesystem read, raster decode, font query, or network attempt.

Root-owned external SVGs continue using the existing root document resource pipeline. That pipeline is separate and less restrictive.

## Limits

| Limit | Value |
| --- | ---: |
| Resource declarations per component | 32 |
| Resource declarations per surface | 32 |
| Resource associations per package | 4,096 |
| Unique resource source files per package | 256 |
| Logical resource name | 64 bytes |
| Logical source path | 512 UTF-8 bytes |
| Source path components | 32 |
| Encoded raster source file | 8 MiB |
| Raster width | 4,096 pixels |
| Raster height | 4,096 pixels |
| Raster pixels | 16,777,216 |
| One decoded raster | 64 MiB |
| Decoded component resources per snapshot | 256 MiB |
| Encoded SVG source file | 2 MiB |
| SVG natural width | 4,096 pixels |
| SVG natural height | 4,096 pixels |
| SVG natural area | 16,777,216 pixels |
| SVG allowed nodes | 4,096 |
| SVG element depth | 64 |
| SVG normalized path segments | 65,536 |
| Total package candidate reads | 256 MiB |

Dimensions must be positive. HTMShell does not downscale, crop, simplify, or flatten oversized sources to evade a limit.

## Validation and sharing

All declarations are validated eagerly during candidate construction, including resources on unused component definitions and surface templates. Every unique owning-package, resource kind, and logical-path source is read once per candidate. Raster sources are decoded once. SVG sources are structurally validated and normalized into one immutable tree once. Invalid unused resources reject the candidate.

Definitions receive immutable ordered associations. Component instances, prepared roots, headless and live documents, and outputs share the snapshot-owned neutral source. Each materialized image still has a distinct generation-safe usage identity.

Natural dimensions enter the existing HTML image layout path. Current CSS sizing, clipping, opacity, transforms, and foreground effects remain authoritative. Resource resolution adds no wrapper, component host box, slot box, or selector marker.

CPU and Vello consume the same immutable source without filesystem access or reparsing after publication. Raster GPU state is prepared lazily for the current device generation. Simple SVG remains neutral vector geometry and is recorded into the renderer scene from the shared tree, so there is no independent SVG image upload. Device and output replacement retain the package source and create fresh backend or usage generations as required.

An unused library or surface resource may be read and validated during package preparation, but it creates no component instance, resource-reference value, prepared usage, scene resource, rasterization, GPU preparation, surface, frame, service demand, or Wayland object. Closed and idle surfaces perform no resource work.

Any declaration, path, read, parse, decode, association, or reference failure rejects the complete candidate. The last published snapshot remains current.

SVG text, subresources, advanced reference graphs, CSS URL assets, component fonts, generic data, optional or dynamic resource-reference values, animation, and dependency resource aliases are not supported.

See [components](../../guide/components.md), [`HTMShell.Component`](README.md), [resource-reference inputs](ResourceReferenceInput.md), [slots](Slot.md), [component styles](Style.md), and [local packages](../../guide/packages.md).
