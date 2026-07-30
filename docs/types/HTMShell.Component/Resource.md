# `HTMShell.Component.Resource`

**Kind:** Definition-owned static raster | **Status:** Experimental

`HTMShell.Component.Resource` gives a schema version 2 component definition a bounded catalog of package-owned raster images. Resources are explicit manifest declarations and are referenced only by logical name from component-owned HTML image elements.

## Declaration

A component export may contain an ordered `resources` array:

```json
{
  "name": "media-card",
  "source": "components/media-card.html",
  "resources": [
    {
      "name": "speaker-icon",
      "type": "raster",
      "source": "assets/speaker.png"
    }
  ]
}
```

Every entry contains exactly `name`, `type`, and `source`. The only supported type is `raster`. A component may declare at most 32 resources. Names are local to one component definition, and one source file may be associated with several names or definitions.

Names contain 1 through 64 lowercase ASCII bytes. They start with a letter, end with a letter or digit, and otherwise use lowercase letters, digits, and single hyphens. Consecutive hyphens are invalid. The names `resource`, `component`, `input`, `slot`, `style`, `state`, `action`, `service`, `surface`, `host`, and `repeat` are reserved. Names beginning with `htm-`, `xml-`, or `xlink-` are also reserved.

## Markup reference

Component-owned HTML consumes an associated raster through one exact form:

```html
<img src="resource:speaker-icon" alt="">
```

The `resource:` prefix is lowercase. The reference contains only one declared logical name, with no slash, query, fragment, percent encoding, or dependency alias. This profile applies only to `<img src>`. It does not add component resource support to SVG, `srcset`, CSS `url()`, fonts, media elements, or generic data consumers.

Ordinary relative, absolute, network, and data URL image sources remain invalid in component definitions. Root documents do not receive a component resource catalog and retain their existing resource behavior.

## Ownership

A definition-owned image resolves through that definition's catalog. Fallback content uses the callee definition catalog. A nested child uses its own catalog.

Assigned slot content retains caller ownership:

- root-owned projected content keeps the root resource pipeline;
- parent-component-owned projected content keeps the parent definition catalog;
- projection into a child does not grant access to the child catalog.

A component cannot address another component's resource name unless it declares its own association. Dependency package aliases are not resource lookup paths.

## Paths and filesystem boundary

Resource source paths resolve from the package root that owns the component export. Paths are normalized, package-relative UTF-8 paths with at most 512 bytes and 32 components. Absolute paths, empty or current-directory components, parent traversal, backslashes, NUL, queries, fragments, percent encoding, URL or foreign-scheme syntax, and environment expansion are invalid.

Every path component is opened without following symbolic links. The final object must be a regular file. Directories, FIFOs, sockets, devices, and other special files are invalid. The bounded read verifies the opened object before and after reading and rejects an observed mutation instead of publishing uncertain content. No file descriptor remains in the package snapshot.

Hard-linked paths remain separate logical source identities.

## Raster profile and limits

The accepted encoded formats are:

- PNG;
- JPEG;
- static WebP.

GIF, animated WebP, animated PNG, SVG, BMP, TIFF, ICO, AVIF, and other formats are invalid. HTMShell detects the format from encoded bytes rather than the file extension. Animated content is rejected instead of displaying one frame.

| Limit | Value |
| --- | ---: |
| Resource declarations per component | 32 |
| Resource associations per package | 4,096 |
| Unique raster source files per package | 256 |
| Logical resource name | 64 bytes |
| Logical source path | 512 UTF-8 bytes |
| Source path components | 32 |
| Encoded source file | 8 MiB |
| Raster width | 4,096 pixels |
| Raster height | 4,096 pixels |
| Raster pixels | 16,777,216 |
| One decoded raster | 64 MiB |
| Decoded component resources per snapshot | 256 MiB |
| Total package candidate reads | 256 MiB |

Dimensions must be nonzero. HTMShell does not downscale or crop oversized sources.

## Decode and sharing

All declarations are validated eagerly during candidate construction, including resources on unused component definitions. Every unique owning-package and logical-path source is read and decoded once per candidate. Invalid unused resources reject the candidate.

The immutable snapshot stores width, height, encoded format metadata, and straight-alpha RGBA8 pixels in top-to-bottom row order. PNG and WebP alpha are preserved. JPEG pixels receive alpha 255. Transparent-pixel RGB values follow the locked decoder result. The runtime does not promise ICC color management, EXIF orientation application, or gamma-perfect correction beyond the current decoder pipeline.

Definitions receive immutable ordered associations. Component instances, prepared roots, headless and live documents, and outputs share the snapshot-owned neutral decoded source. Each materialized image still has a distinct generation-safe usage identity.

## Rendering and lifecycle

Natural dimensions enter the existing HTML image layout path. Current CSS sizing, clipping, opacity, transforms, and foreground effects remain authoritative. Resource resolution adds no wrapper, component host box, slot box, or selector marker.

CPU rendering consumes the shared neutral pixels without filesystem access or another decode. Vello prepares backend-private image state lazily for the current device generation. A device reset discards GPU state while retaining the neutral snapshot source for preparation again. Successful native Vello presentation does not require a readback or SHM fallback.

An unused library resource may be read and decoded during package validation, but it creates no component instance, prepared usage, scene resource, GPU upload, surface, frame, service demand, or Wayland object. Closed and idle surfaces perform no resource work.

Any declaration, path, read, format, decode, association, or reference failure rejects the complete candidate. The last published snapshot remains current.

External SVG resources, CSS URL assets, component fonts, generic data, resource-reference inputs, dynamic resource bindings, animation, and dependency resource aliases are not supported.

See [components](../../guide/components.md), [`HTMShell.Component`](README.md), [slots](Slot.md), [component styles](Style.md), and [local packages](../../guide/packages.md).
