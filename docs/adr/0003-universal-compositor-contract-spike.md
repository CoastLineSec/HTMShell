# ADR 0003: Validate a universal compositor contract before runtime integration

- Status: experimental and reversible
- Date: 2026-07-16
- Decision: UNIVERSAL CONTRACT VALID; COMPOSITOR CORE SUPPORT REQUIRED

## Context

The headless gates established that HTMShell can maintain and paint a local HTML/CSS document. They did not establish how a shell scene should acquire compositor-owned placement, lifecycle, and input semantics. Testing that boundary independently prevents HTML, CSS, Blitz, and renderer choices from shaping the compositor contract.

HTMShell defines the contract. A supporting Wayland compositor implements it. Hyprland is the first feasibility host because it is available in the development environment; it does not define the protocol, identifiers, root roles, capability names, client behavior, or future runtime architecture.

## Experimental decision

Reuse standard Wayland for the content path: an ordinary `wl_surface`, `wl_shm` buffers, damage, commits, frame callbacks, buffer release, outputs, seats, and pointer events. Reuse suitable extension protocols for workspaces, foreign toplevels, fractional scale, presentation, synchronization, session locking, capture, and other capabilities instead of copying them into an HTMShell namespace.

The experimental `htm-shell-v1` protocol adds only the missing shell-specific semantics: provisional controller authorization, compositor-neutral capability advertisement, one output-associated semantic `overlay` root, and a configure/acknowledge lifecycle. `overlay` means above ordinary application content and below secure session-lock content. It is not a numeric z index or compositor render stage.

The generic Rust probe binds only standard Wayland, existing general extensions, and `htm-shell-v1`. It does not identify the compositor or use compositor-specific IPC. The temporary host under `prototypes/compositor-host/` is deliberately not a backend framework and shares no code or types with the HTML/CSS runtime.

Mandatory baseline behavior demonstrated by the probe is connection-scoped shell ownership, capability advertisement, one semantic root on a normal surface, output association, configure/acknowledge, standard buffer and frame lifecycle, and disconnect cleanup. Workspace association, application interleaving, native materials, previews, advanced presentation, and similar features remain optional future capabilities with compositor-neutral semantics.

## Existing protocol boundary

Core Wayland owns surfaces, shared-memory buffers, buffer release, frame callbacks, outputs, and seat input. Existing extension protocols are preferred for workspaces and foreign toplevels, fractional scaling, presentation timing, explicit synchronization, background effects, session locking, data control, image capture, and color management. Gate B.0 does not duplicate those interfaces. No broadly adopted protocol currently supplies the combined trusted-controller and semantic shell-root role required by this experiment.

## Hyprland feasibility result

The exact Hyprland 0.55.4 test build can register the private global, assign an experimental permanent surface role, render the imported surface texture at a semantic post-window position, damage its bounded region, and clean up on surface, client, or plugin teardown. The host uses the compositor's existing Wayland import and surface-pass path: no pixel side channel, CPU readback, raw OpenGL, second renderer, or function hook is involved.

These operations depend on private Hyprland objects because the plugin API does not expose stable facilities for protocol-global registration, extensible surface roles, semantic scene-node insertion, output association, or bounded scene damage. A plugin-owned surface role also requires the controller connection to be closed before the plugin shared object unloads so no role vtable survives `dlclose`.

Standard `wl_pointer` remains the correct client protocol, but Hyprland's normal hit-test graph cannot register an arbitrary plugin-owned root. Broad interception of compositor input was rejected. A narrow compositor-core API for semantic scene/view registration and ordinary hit testing is therefore required for clean pointer delivery. The host does not advertise pointer-focus support.

## Portability

A wlroots compositor could implement the same contract with a custom global, native surface role, scene node, and standard seat focus. Niri could implement it through Smithay dispatch, role, render-element, and input-target integration. Wayfire provides a third, plugin-oriented example of custom shell-protocol and scene/input integration. These implementations differ materially while the protocol vocabulary remains unchanged.

## Acceptance criteria

- Keep the protocol, bindings, and probe compositor-neutral.
- Reuse standard Wayland surface, buffer, frame, output, and seat behavior.
- Authorize one controller before granting shell-root authority.
- Configure and display one semantic root from a `wl_shm`-backed `wl_surface`.
- Commit two frames and observe both frame callbacks and buffer releases.
- Remove roots and focus on destruction, disconnect, and plugin unload.
- Contain invalid lifecycle requests without destabilizing the nested compositor.
- Identify the exact standard-input integration gap rather than inventing custom pointer events.
- Demonstrate that other compositor architectures can implement the same semantics.
- Keep HTML/CSS, renderer, package, and compositor-specific behavior outside the contract.

## Stop conditions

Stop or redesign if the protocol exposes compositor internals, the client must detect a compositor or call its private IPC, pixels require a custom transport, the host needs broad function interception or a second renderer, input requires a custom event protocol, cleanup cannot be guaranteed, or another compositor would need different contract semantics.

## Consequences and limitations

Version 1 is a private feasibility protocol, not a standard or stable public API. Its inherited session capability is provisional and does not select a production authorization policy. Only one output and the `overlay` role were exercised. Fractional-scale protocol discovery worked, but the nested backend remained at integer scale, so fractional presentation is unproven. DMA-BUF, explicit synchronization, presentation timestamps, color management, HDR, keyboard focus, touch, and materials remain outside this gate.

The temporary host is tightly coupled to one exact Hyprland commit and is not distributable as a stable plugin. A durable Hyprland implementation should add first-class core support for extensible shell roles, semantic scene placement, hit testing, and safe plugin lifecycle rather than preserve the prototype's private access pattern.

## Result

The generic client displayed and updated a standard shared-memory surface, observed frame callbacks and buffer releases, consumed standard workspace state, and remained unaware of its compositor. Repeated nested lifecycle and invalid-request runs completed after fixing plugin-unload ordering. The remaining pointer failure is an identified host API gap, not a reason to introduce compositor-specific client behavior.

The decision is **UNIVERSAL CONTRACT VALID; COMPOSITOR CORE SUPPORT REQUIRED**. This validates the portable contract direction while requiring a narrow compositor integration proposal before HTMShell runtime content is connected to it.
