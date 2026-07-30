# `HTMShell.Package`

**Kind:** Local package graph | **Status:** Experimental

`HTMShell.Package` describes the validated ownership graph used by headless and live shell loading.

## Package metadata

Schema version 2 package metadata has this exact shape:

```json
{
  "package": {
    "id": "org.example.controls",
    "kind": "library",
    "version": "1.2.3"
  }
}
```

| Field | Requirement |
| --- | --- |
| `id` | Lowercase reverse-DNS package ID, maximum 255 bytes. |
| `kind` | `shell` for the graph root or `library` for an imported package. |
| `version` | Optional SemVer 2.0.0 metadata, maximum 255 bytes. |

The `local.` ID prefix is reserved for schema version 1 and manifestless headless compatibility identities.

## Dependency

A dependency declaration has exactly:

```json
{
  "alias": "controls",
  "id": "org.example.controls",
  "path": "packages/controls"
}
```

| Field | Requirement |
| --- | --- |
| `alias` | Unique lowercase name, maximum 64 bytes. |
| `id` | Exact expected logical package ID. |
| `path` | Normalized local directory relative to the declaring package. |

Aliases cannot be `self`, `root`, `input`, `state`, `action`, `service`, `surface`, `slot`, or `htm`.

Dependency paths cannot use parent traversal, absolute paths, backslashes, URLs, symbolic links, or special files. Every dependency remains inside the root shell directory.

## Ownership

One graph contains exactly one `shell`, at its root. Only that package may declare the supported panel and overlay topology. A `library` may depend on libraries but cannot create surfaces or load presentation content as an import side effect.

Schema version 2 shell and library packages may declare an ordered `components` export table with optional literal typed inputs, content slots, package-owned scoped stylesheet associations, and named static raster resources. Definitions remain inert until an explicit root or component [`htm-use`](../HTMShell.Component/README.md#component-use) references them. Declared component sheets and raster sources are validated with the package. Unused definitions create no live style scope, resource usage, renderer object, surface, or service demand.

The graph is resolved in deterministic dependency-first order. Cycles, ID conflicts, location conflicts, version conflicts, and package-kind violations reject the complete candidate.

Validated package data is published as one immutable generation. Headless and live documents can share that generation while retaining independent document and surface identities.

## Limits

The graph permits at most 64 packages, 32 direct dependencies per package, dependency depth 16, 256 component exports per package, and 4,096 component exports per graph. Each component declares at most 64 literal inputs, 32 slots, 16 stylesheet paths, and 32 raster resources. One package may use 64 unique component stylesheet files and 256 unique raster source files. Each manifest is at most 256 KiB, each component source document is at most 2 MiB, each component stylesheet is at most 1 MiB, each encoded raster is at most 8 MiB, and one candidate may read at most 256 MiB.

There is no network resolution, package registry, version solver, optional dependency, global search path, dynamic component binding, package-global library style, component external SVG, CSS URL asset, font loading, or hot reload. Schema version 2 components support caller-owned projection, selector-isolated component styles, and declared PNG, JPEG, and static WebP images without `:host`, `::slotted()`, or Shadow DOM.

See [local packages](../../guide/packages.md), [components](../../guide/components.md), [component inputs](../HTMShell.Component/Input.md), [component styles](../HTMShell.Component/Style.md), [component raster resources](../HTMShell.Component/Resource.md), and [`ShellManifest`](../HTMShell/ShellManifest.md).
