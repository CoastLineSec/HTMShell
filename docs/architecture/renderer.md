# Renderer architecture

HTMShell builds one immutable retained scene from document, style, layout, text,
SVG, image, and paint state. Scene nodes and render resources use
generation-scoped identities that do not depend on memory addresses, HTML IDs,
paint order, file paths, or renderer handles. Incremental document changes
produce a new scene revision, a bounded scene delta, and conservative logical
damage.

An immutable frame plan is the common renderer boundary. It contains the target
generation, scene revision, logical and physical geometry, scale, pixel
requirements, resources, damage, and finite diagnostic reasons. Renderers do
not parse HTML or CSS, mutate documents, schedule frames, own native services,
or select layer-shell policy.

## Rendering paths

The CPU renderer is the default reference renderer. Headless rendering and live
shared-memory presentation consume the same retained scene and frame-plan
contract. Exact CPU fixtures remain the source of truth for deterministic
pixels.

Color-only foreground filters isolate the complete element SourceGraphic in a bounded CPU image. The reference compositor applies normalized brightness, contrast, grayscale, hue rotation, invert, opacity, saturation, and sepia stages from left to right in encoded sRGB, clamps after every stage, and emits premultiplied RGBA8 once after the list. Spatial foreground filters and backdrop filters remain outside this execution path.

The optional `gpu-renderer` build feature provides an experimental Vello
backend. It supports offscreen rendering and deterministic readback for tests,
and it can present directly to an existing Wayland layer-shell surface. The
live path reuses the host's Wayland connection and keeps frame callbacks as the
scheduling authority. Successful GPU frames use neither CPU frame
rasterization, GPU readback, nor shared-memory presentation.

Vello renders solid and rounded geometry, text, SVG, opacity, clips, affine
transforms, background layers, and box shadows. Raster images are decoded by
the existing bounded resource path and composed by the GPU. Nonidentity foreground filters and backdrop filters request a complete CPU fallback so content is never silently
omitted.

## Damage and presentation

Each live GPU presenter owns a complete persistent backing image. Small logical
damage is converted conservatively to physical coordinates and replayed through
bounded 64 by 64 pixel tiles with a 2 pixel guard. Partial work is limited to
16 tile replays and 30 percent of the target area; larger or uncertain changes
use a complete GPU render. A partial transaction becomes current only after all
selected tiles have rendered and copied successfully.

Swapchain image contents are never assumed to persist and buffer age is not
used. Every acquired image is populated completely from the current backing by
a GPU conversion pass. That pass handles RGBA or BGRA channel order, sRGB or
non-sRGB targets, and compatible straight or premultiplied alpha modes.
Backing-to-surface conversion therefore remains full target even when Vello
raster work and backing updates are damage-limited.

The host queues bounded physical `damage_buffer` rectangles before presentation
when the Wayland surface version supports them. Older versions use a
conservative full logical damage fallback. Damage, conversion, presentation,
and frame callbacks remain distinct responsibilities.

## Resources, ownership, and recovery

The Vello backend cache is keyed by device generation, neutral resource
identity, resource version, and prepared representation. Entry, byte, and
single-resource limits bound the cache. Device reset discards backend handles,
while neutral scenes and resources remain authoritative.

Exactly one presenter owns a Wayland surface generation. GPU absence,
unsupported effects, allocation or preparation failure, surface failure, and
device loss are contained by typed errors and complete CPU fallback. A fallback
surface remains on CPU until a fresh surface generation, preventing presenter
ping-pong. Closed surfaces perform no presentation work, and idle surfaces
submit no frames.

The retained scene and frame plan are backend-neutral. A future backend can
replace Vello behind the same resource, rendering, recovery, and presentation
boundaries without changing the document authoring model.
