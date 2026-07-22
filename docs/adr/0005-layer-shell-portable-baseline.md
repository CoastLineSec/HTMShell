# ADR 0005: Layer shell as the portable presentation baseline

## Status

Accepted for the experimental scale-1 live-presentation profile. Reversible
while the shell host and runtime APIs remain experimental.

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
surface while retaining its parsed document for a later remap.

Backgrounds, notifications, and multi-output policy remain deferred.

## Enhanced compositor integration

The experimental `htm-shell-v1` contract remains an optional path for
capabilities that existing protocols cannot adequately express, such as
workspace-native roots, application-surface interleaving, compositor-owned
materials, and advanced synchronized transitions. It is not required for
portable presentation and its current wire behavior is unchanged.

## Presentation constraints

Scale 1 is the supported live-presentation profile in this spike. Integer and
fractional-scale globals are observed conceptually, but fractional-scale
rendering and viewporter integration are deferred.

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
- this profile implements one output with a panel and one transient overlay;
- every layer surface has an independent buffer pool and frame scheduler;
- installed font and CPU-renderer behavior still affect pixels;
- scale 1 is the only completed presentation profile.

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
- The experimental compositor contract and prior tests remain intact.
- Public documentation describes the contract as optional enhanced integration.

## Stop conditions

Reconsider the baseline if correct presentation requires compositor detection,
private compositor IPC, a plugin, a custom pixel socket, a custom pointer
protocol, continuous polling, unsafe buffer reuse, reparsing HTML per frame, or
separate runtime behavior for individual compositors.

## Final decision

```text
CONTINUE WITH NARROWER LAYER-SHELL PROFILE
```

Scale-1 layer-shell presentation, including independent panel and transient
overlay surfaces in one process, satisfies the portable baseline requirements.
Fractional-scale presentation and multi-output policy remain deferred and must
be validated before the profile is broadened.
