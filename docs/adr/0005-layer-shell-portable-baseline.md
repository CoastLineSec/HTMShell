# ADR 0005: Layer shell as the portable presentation baseline

## Status

Accepted for the experimental scale-1 and fractional-scale live-presentation
profiles. Reversible while the shell host and runtime APIs remain
experimental.

## Context

The headless runtime can parse, style, lay out, mutate, and paint a local
HTML/CSS document without a browser shell. The experimental `htm-shell-v1`
contract then proved a compositor-neutral semantic root, but its standard
pointer path requires first-class compositor integration.

Ordinary shell components should not require a compositor patch or plugin.
wlr layer shell already provides a broadly implemented Wayland role for
panels, overlays, backgrounds, launchers, notifications, and similar desktop
surfaces.

## Decision

HTMShell uses layer shell as its portable baseline presentation mechanism.
Existing Wayland protocols are preferred before new HTMShell wire behavior is
defined.

The runtime discovers advertised protocol globals and never detects compositor
identity. Its HTML/CSS, layout, text, interaction, and paint behavior remains
compositor-neutral.

The first live path proved one full-output overlay surface:

- one parse-once local HTML/CSS document;
- compositor-configured logical dimensions;
- CPU rendering to premultiplied RGBA;
- conversion to native `WL_SHM_FORMAT_ARGB8888`;
- a bounded shared-memory buffer pool;
- frame-callback scheduling and buffer-release ownership;
- standard pointer hover, active, and click-driven host mutation;
- a surface input region derived from resolved shell geometry.

The portable host now also proves one process managing a persistent panel and
a transient overlay. Each surface owns independent configure state, frame
callback state, shared-memory buffers, input region, pointer state, and
parse-once document. The documents may receive deterministic mutations from a
small shared host-state model without sharing presentation ownership.

The panel uses the top layer, top/left/right anchors, and an exclusive zone
equal to its configured height. The overlay uses the overlay layer, all four
anchors, and no exclusive zone. Closing the overlay unmaps its normal Wayland
surface while retaining its parsed document for a later remap. Manifest-driven
overlays release their transient Wayland role after the null-buffer unmap and
create a fresh role when reopened; document identity remains unchanged.

Portable surfaces are defined by a validated local JSON manifest. Version 1
contains one top-panel template and one transient-overlay template. Each
template expands independently for every eligible output.

Output identity is runtime-scoped. A registry global and local generation own
each output instance; output names and descriptions are diagnostics, not
persistent configuration identifiers. Each output owns independent parsed
documents, Wayland objects, buffers, callbacks, pointer state, input regions,
and host-controlled overlay state. Output addition creates only the missing
instance group. Output removal destroys only that generation, reclaims its
resource budget, and leaves other outputs active. A process with no eligible
outputs remains connected and idle until an output appears.

Backgrounds, notifications, persistent monitor selection, and cross-output
state policy remain deferred.

## Enhanced compositor integration

The experimental `htm-shell-v1` contract remains an optional path for
capabilities that existing protocols cannot adequately express, such as
workspace-native roots, application-surface interleaving, compositor-owned
materials, and advanced synchronized transitions. It is not required for
portable presentation and its current wire behavior is unchanged.

## Presentation constraints

Logical layout dimensions and physical presentation dimensions are distinct.
Each live `wl_surface` generation owns its preferred scale and, when the
compositor advertises both fractional-scale and viewporter, its own
fractional-scale and viewport objects. The renderer paints logical scene
geometry into a checked, ceiling-rounded physical buffer at the preferred
numerator over 120. The viewport destination remains the configured logical
surface size and `wl_surface` buffer scale remains 1. Scale changes affect only
the owning surface and safely retire old physical buffer pools.

Scale 1 remains the fallback when the complete optional protocol pair is not
available or no usable preference has been received. Manifest dimensions stay
logical and do not override compositor scale. Partial damage remains deferred;
the live path uses full logical-surface damage.

The initial transport is CPU-rendered `wl_shm`. It is not zero-copy. DMA-BUF,
GPU rendering, explicit synchronization, presentation feedback, color
management, and HDR are deferred until the portable lifecycle is established.

The headless scene damage estimate is not compositor-ready. The live path may
damage the full surface when renderer expansion cannot be bounded reliably.

## Consequences

Benefits:

- basic shell operation needs no compositor plugin, patch, or private IPC;
- the same executable follows advertised Wayland protocols on every supporting
  compositor;
- input, focus, frame pacing, and buffer ownership use standard Wayland
  lifecycle;
- enhanced integration can evolve independently without blocking a usable
  portable shell.

Costs and limitations:

- layer shell cannot express every originally envisioned scene relationship;
- `wl_shm` adds CPU conversion and memory-copy cost;
- a full-output overlay requires a carefully bounded input region;
- each eligible output receives its own panel and transient-overlay instances;
- every layer surface has an independent buffer pool and frame scheduler;
- every layer surface has independent scale and viewport state;
- aggregate shared-memory use is bounded across the process as well as within
  each pool;
- fractional scale increases CPU paint and shared-memory requirements;
- installed font and CPU-renderer behavior still affect pixels;
- scale 1 remains available when optional fractional protocols are absent.

## Acceptance criteria

- The live client contains no compositor detection or compositor-specific IPC.
- It creates one overlay layer surface through the advertised global.
- It performs the initial bufferless commit and configure acknowledgement.
- One parse-once document survives input and viewport changes.
- Two shared-memory buffers obey release ownership.
- Frame callbacks prevent continuous idle rendering.
- Standard pointer events drive hover, active, and one document mutation.
- A core input region excludes unused transparent surface area.
- One process independently schedules and owns a top-layer panel and an
  overlay-layer transient surface.
- The panel exclusive zone and overlay click-through region follow layer-shell
  semantics without compositor-specific policy.
- Opening and closing the overlay retains both parse-once documents.
- A validated manifest expands stable surface template IDs per eligible output.
- Output generations prevent stale callbacks, releases, and pointer state from
  aliasing recreated instances.
- Output addition and removal do not reconstruct unrelated documents.
- Output names remain diagnostic and no persistent monitor selector is implied.
- Preferred scale is per surface; logical layout, pointer coordinates, and
  input regions do not become physical-pixel geometry.
- Fractional presentation paints at physical density, uses a logical viewport
  destination, and keeps buffer scale 1.
- Scale and configure changes coalesce into one latest presentation revision,
  with bounded retirement of older shared-memory pools.
- Mixed-scale outputs remain independent and the scale-1 fallback remains
  functional when optional protocol globals are unavailable.
- The experimental compositor contract and prior tests remain intact.
- Public documentation describes the contract as optional enhanced integration.

## Stop conditions

Reconsider the baseline if correct presentation requires compositor detection,
private compositor IPC, a plugin, a custom pixel socket, a custom pointer
protocol, continuous polling, unsafe buffer reuse, reparsing HTML per frame, or
separate runtime behavior for individual compositors.

## Final decision

```text
CONTINUE WITH FRACTIONAL-SCALE PORTABLE PROFILE
```

Manifest-driven panel and overlay instances now retain logical CSS geometry
while rendering independent physical buffers at compositor-preferred scales.
Scale 1 remains the portable fallback. GPU presentation, partial damage,
persistent output-selection policy, and higher-level shell components remain
deferred.
