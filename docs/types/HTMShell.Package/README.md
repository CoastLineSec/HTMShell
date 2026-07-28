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

The graph is resolved in deterministic dependency-first order. Cycles, ID conflicts, location conflicts, version conflicts, and package-kind violations reject the complete candidate.

Validated package data is published as one immutable generation. Headless and live documents can share that generation while retaining independent document and surface identities.

## Limits

The graph permits at most 64 packages, 32 direct dependencies per package, and dependency depth 16. Each manifest is at most 256 KiB, and one candidate may read at most 256 MiB.

There is no network resolution, package registry, version solver, optional dependency, global search path, component export, component syntax, scoped component CSS, or hot reload.

See [local packages](../../guide/packages.md) and [`ShellManifest`](../HTMShell/ShellManifest.md).
