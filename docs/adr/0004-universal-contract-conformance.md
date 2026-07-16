# ADR 0004: Universal contract conformance

## Status

Accepted for the experimental Gate B.1 spike. Reversible before protocol
stabilization.

Gate decision:

```text
UNIVERSAL CONTRACT VALID; REFERENCE HOST REQUIRES CORE SUPPORT
```

## Context

Gate B.0 demonstrated that a compositor-neutral Wayland client can discover the
HTMShell manager, acquire provisional authority, create a normal `wl_surface`,
receive a semantic `overlay` role, commit two `wl_shm` buffers, receive frame
callbacks and buffer releases, and clean up without knowing which compositor
implements the contract.

The temporary Hyprland host implemented presentation without function hooks.
It could not register plugin-owned roots in Hyprland's normal top-level pointer
hit-test graph. Rendering alone therefore did not provide standard pointer
focus. Gate B.0 concluded that the universal contract was valid but first-class
compositor integration was required.

## Decision

HTMShell continues to own one compositor-neutral contract. Supporting
compositors implement that contract through native code, a plugin, a module, or
shared compositor-toolkit support without changing client-visible semantics.

Gate B.1 adds:

- a readable protocol specification;
- implementation-neutral compositor integration requirements;
- a deterministic black-box conformance report model;
- a generic Wayland conformance tool;
- unit-tested lifecycle, capability, aggregation, and redaction behavior.

The existing protocol names remain unchanged. Their project prefix is unique,
and both XML and documentation explicitly mark the contract experimental. No
separate major/minor handshake is added; Wayland interface versions govern
additive evolution.

## Mandatory baseline

Baseline conformance requires:

- a compositor-authorized, connection-scoped controller;
- manager discovery and complete capability advertisement;
- `root_overlay` and `standard_pointer_focus`;
- one output-associated semantic `overlay` root on a normal `wl_surface`;
- configure and acknowledgement before the first buffer;
- standard buffer commits, damage, frame callbacks, and releases;
- normal `wl_pointer` hit testing and seat focus;
- deterministic cleanup on root, surface, output, client, and host teardown;
- failure containment for invalid clients.

Pointer support is mandatory because a visible shell with no normal input path
is not a usable shell. The missing reference-host pointer path is reported as a
conformance failure; it does not redefine the baseline.

## Optional capability strategy

Baseline conformance is binary. Independent existing Wayland protocols remain
independent globals. Future HTMShell-only facilities may form a small number of
semantic profiles after a real requirement is proven. The project will not add
many fine-grained flags that force the runtime to reconstruct compositor
personalities.

Background, workspace, and panel roles are deferred. Version 1 retains only
`overlay`. Workspaces, toplevels, scaling, presentation, synchronization,
locking, capture, clipboard, effects, and color management reuse existing
protocols where suitable.

## Authorization direction

Authorization belongs primarily to compositor and session policy. The preferred
production direction is a compositor-selected client connection with a
restricted global or inherited authority. Binding that global should imply that
policy has already authorized the controller.

The current authentication request is retained only as a provisional nested
reference-host bootstrap. It is not a production trust mechanism and may be
removed by a breaking experimental revision. A second compositor integration
should inform the stable launch and authorization model.

## Versioning direction

Normal Wayland interface versions are sufficient. Clients bind the supported
minimum of advertised and known versions. Additive messages increment the
manager ancestry and use `since`. Incompatible changes during experimentation
may rename the family; after stabilization they require a new interface family.

Unknown optional enum values are contained and ignored. Missing mandatory
capabilities fail baseline conformance.

## Conformance approach

The conformance tool is a generic Wayland client. It does not identify the
compositor, use compositor-specific IPC, link compositor headers, inspect scene
objects, or parse compositor logs as its primary assertion.

Machine-readable JSON has a stable schema and deterministic ordering. Volatile
timings are printed separately. Result categories are `PASS`, `FAIL`, `SKIP`,
`UNSUPPORTED`, `INCONCLUSIVE`, and `TIMEOUT`. Optional unsupported behavior does
not fail baseline; missing mandatory behavior does.

Pointer testing is operator-assisted because Gate B.1 does not select a private
input-injection mechanism. A compositor advertising standard pointer support
must deliver enter, motion, button, and leave through `wl_pointer`. A reference
host that does not advertise the mandatory capability fails immediately and
honestly.

## Existing protocol reuse

Core Wayland carries surfaces, buffers, damage, callbacks, outputs, seats, and
pointer input. The contract does not duplicate viewporter, fractional scale,
presentation time, Linux DMA-BUF, explicit synchronization, workspace,
foreign-toplevel, session-lock, cursor-shape, data-control, capture, background
effect, or color-management protocols.

## Reference host

The Hyprland prototype remains evidence, not a backend architecture. Gate B.1
corrects its acceptance of a pre-buffered surface because that contradicted the
configure-before-buffer lifecycle. It does not add broad pointer interception or
claim full conformance.

The reference host is expected to fail the mandatory pointer requirement until
the compositor exposes first-class semantic root and hit-test integration. That
core capability is preferable to function hooks or a Hyprland-shaped protocol.

## Consequences

Positive consequences:

- compositor maintainers can evaluate one explicit integration contract;
- missing behavior is visible as a black-box result rather than inferred from
  screenshots;
- protocol semantics stay independent of HTML/CSS and renderer choices;
- compositor differences remain implementation details;
- a second implementation can challenge the same contract without client
  changes.

Costs and risks:

- the current reference host cannot pass baseline conformance;
- authorization remains provisional;
- some cleanup and input assertions require operator or compositor-side
  diagnostics;
- the protocol may still change before a second compositor implementation;
- first-class compositor work is required before HTMShell can become usable.

## Acceptance criteria

- Every current protocol item has a semantic classification.
- Mandatory and optional behavior are separated.
- Root placement and input use implementation-neutral semantics.
- Authorization and versioning direction are explicit.
- Existing protocols are reused rather than wrapped.
- A generic black-box report and conformance tool exist.
- Lifecycle and result aggregation are unit tested.
- The reference host's pointer gap remains visible.
- wlroots-style, Smithay-style, plugin-oriented, and monolithic implementations
  can implement the same wire semantics.
- No compositor backend, core patch, HTML integration, or material API is added.

## Stop conditions

Reconsider the contract if conformance requires compositor detection,
compositor-specific IPC, custom pointer events, raw scene identifiers, duplicated
standard protocols, incompatible wlroots and Smithay semantics, or an
authorization mechanism tied to one compositor's configuration model.
