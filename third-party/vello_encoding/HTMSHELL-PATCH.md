# Vello encoding 0.9.0 atlas-residency correction

This directory vendors the Rust source, manifests, README, and licenses from the
published vello_encoding 0.9.0 crate. The upstream version is unchanged.

- Registry archive SHA-256: 2346f5f0d7dccb3582fcd397b4a57b43165209f1424e0d76e85dd814db164af7
- Upstream repository: https://github.com/linebender/vello
- Published VCS revision: 875f324f21da93019cae9e8e61d4abfd69893206
- Published VCS subdirectory: vello_encoding
- License: Apache-2.0 OR MIT; both license files are retained.

The registry-generated Cargo.lock and Cargo download bookkeeping are not needed
for this workspace dependency and are omitted. The workspace Cargo.lock remains
authoritative.

## Local correction

Resolver::resolve begins an image-cache resolve pass before its solid-only fast
path and returns the resident atlas metadata for that path. The former
begin_resolve call inside resolve_patches is removed, so the generation advances
exactly once per resolve.

Without this correction, a raster -> solid-only -> raster sequence reports atlas
dimensions 1024x1024 -> 0x0 -> 1024x1024. The renderer replaces the physical atlas,
but the resolver still reports the cached raster as clean and supplies no upload.
The raster becomes transparent. This was reproduced with actual pixels on an
Intel Arc A770 using Vulkan before applying this patch.

Clearing per-pass uploads is required as well as retaining dimensions: returning
the previous pass's upload list could replay stale texture overrides. Dirty flags
for images not referenced by a solid-only pass are preserved. Atlas growth,
repacking, eviction, and renderer reset remain under the existing implementation.

A fresh solid-only renderer now reports the default 1024x1024 atlas, like the
existing gradient/glyph-only path, instead of a 1x1 placeholder. This bounded
allocation is intentional; lazy empty-atlas allocation is a separate optimization.

## Validation

The HTMShell physical regression is
render::gpu::atlas_regression::hardware_raster_atlas_survives_resource_free_passes.
It checks actual pixels across two targets, solid and empty passes, repeated
replay, atlas growth, and renderer reset. Run it serially with gpu-renderer.

Resolver unit tests cover per-pass metadata and dirty-image preservation.
The patch adds no new cache, renderer workaround, public syntax, or dependency
version upgrade. Remove this local patch only when a reviewed upstream release
passes the same regressions.
