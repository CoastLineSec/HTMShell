# HTMShell compositor integration

## Experimental status

This document defines the compositor responsibilities for the experimental
`htm-shell-v1` contract. The contract is not stable, standardized, or part of
`wayland-protocols`. Breaking revisions remain possible while implementations
are evaluated.

HTMShell defines client-visible semantics. A supporting compositor implements
those semantics through its own protocol, scene, input, damage, and policy
architecture. The client does not identify the compositor or select a
compositor-specific path.

```text
HTMShell runtime
        │
        │ core Wayland + existing extensions + htm-shell-v1
        ▼
Compositor-owned role, scene, input, damage, and policy integration
```

Shell content remains a normal `wl_surface`. The contract adds no renderer,
display list, custom pixel transport, or compositor object identifier.

## The missing semantic

Existing Wayland protocols carry surface content, buffers, commits, frame
callbacks, input, scaling, synchronization, desktop state, and secure locking.
They do not, as one contract, express this missing semantic:

> An authorized desktop-shell client can assign a semantic,
> compositor-managed shell-root role to a normal Wayland surface, with defined
> placement, scene participation, standard input focus, lifecycle, and secure
> ordering.

Layer shell overlaps with desktop placement and exclusive-zone use cases. It
does not establish HTMShell's connection-scoped controller authority,
capability handshake, permanent semantic root, or future shell-root contract.
`htm-shell-v1` is not a general layer-shell replacement for ordinary clients,
and version 1 defines no exclusive zone.

The missing concept is not a workspace, toplevel-enumeration, scaling,
synchronization, capture, or input-event protocol. HTMShell reuses those
protocols where applicable.

## Architecture boundary

The HTMShell process owns document parsing, CSS, layout, text, component state,
ordinary rendering, detailed UI hit testing, and shell behavior. The compositor
owns:

- selection of the authorized shell controller;
- the permanent Wayland surface role;
- output association and logical configuration;
- secure scene ordering;
- mapping, unmapping, clipping, transforms, and damage integration;
- top-level hit testing and standard seat focus;
- deterministic cleanup and malformed-client containment.

HTML, CSS, DOM state, component state, and application pixels do not cross this
contract.

## Mandatory compatibility baseline

A compositor may claim baseline compatibility only when it implements all of
the following:

1. Authorize one controller connection through compositor or session policy.
2. Make `htm_shell_manager_v1` available to that connection.
3. Advertise the complete version 1 capability set before `ready`.
4. Accept an unassigned, unbuffered `wl_surface` from the controller.
5. Assign that surface the permanent HTMShell root role.
6. Associate the root with one live `wl_output`.
7. Configure a positive output-local logical extent.
8. Require acknowledgement before the first non-null buffer commit.
9. Apply ordinary committed Wayland surface state.
10. Map the root at the semantic `overlay` position.
11. Place it above ordinary application content.
12. Keep it below session-lock and security-critical compositor content.
13. Include it in normal scene traversal and output clipping.
14. Include it in normal top-level hit testing.
15. Respect its committed `wl_surface` input region.
16. Assign pointer focus through the normal Wayland seat path.
17. Deliver standard `wl_pointer` events in surface-local coordinates.
18. Bound damage to the affected root and normal renderer expansion.
19. Remove presentation, hit-test, focus, and authority state on every teardown
    path.
20. Contain malformed requests to the offending client without exposing
    compositor pointers or internal identities.

The version 1 capability set contains `root_overlay` and
`standard_pointer_focus`. Both are mandatory even though they use the
capability event mechanism. Missing either is baseline failure, not an optional
downgrade.

The baseline does not require workspaces, application enumeration, keyboard
focus, session-lock implementation, fractional scaling, GPU buffers, explicit
synchronization, presentation timestamps, materials, capture, or previews.

## Semantic overlay root

Version 1 defines only `overlay`:

- it is associated with exactly one output for its lifetime;
- it is above ordinary application content on that output;
- it is below session-lock and security-critical compositor content;
- it has no workspace association or exclusive layout reservation;
- it is pointer-eligible only inside its committed input region;
- transparent pixels remain input-eligible unless excluded by that region;
- it cannot request a numeric z index, render stage, or global input grab.

Multiple overlay roots are allowed. On one output, later root-creation requests
are ordered above earlier HTMShell overlay roots without crossing secure
compositor boundaries. Version 1 has no reorder or role-switching request.

Background, workspace, and panel roles are deferred. Their names do not imply
current protocol support.

## Surface lifecycle

```text
authorized manager ready
        │
        ├─ create empty, unassigned wl_surface
        ├─ get_root(surface, output, overlay)
        │       └─ configure(serial, logical width, logical height)
        ├─ ack_configure(serial)
        ├─ attach + damage + frame + commit
        │       ├─ wl_callback.done
        │       └─ wl_buffer.release
        ├─ attach NULL + commit              optional explicit unmap
        ├─ root.destroy
        ├─ wl_surface.destroy
        └─ disconnect                        revoke all authority
```

The role is permanent for the `wl_surface` lifetime. Destroying the role object
unmaps it but does not make the surface reusable for another role. Destroying
the surface first destroys the associated root resource and all compositor
state.

A later configure may replace pending root geometry. Acknowledging the newest
pending serial acknowledges earlier pending serials. A stale, unknown, or
duplicate serial is a protocol error. Acknowledged state becomes current with a
subsequent `wl_surface.commit`.

A non-null committed buffer maps the configured root. A null committed buffer
unmaps it. Standard `wl_surface.frame` callbacks pace the client, and standard
`wl_buffer.release` events govern buffer reuse.

## Minimal compositor responsibilities

The names below describe responsibilities, not required source APIs. The
compositor owns their state and executes them on its normal Wayland/event-loop
boundary or an equivalent synchronized boundary.

| Responsibility | Trigger and preconditions | Observable result | Failure, cleanup, and security |
| --- | --- | --- | --- |
| Authorize shell controller | Session policy selects one client connection | The selected client can obtain manager authority | Withhold authority from every other client; revoke it on disconnect |
| Create shell root | Authorized manager supplies an unassigned, unbuffered surface, live output, and supported role | A connection-owned root object and permanent surface role exist | Reject invalid ownership, construction state, output, or role without partial state |
| Associate root with output | Root creation references a client-bound live output | Configure and placement use that output's logical coordinate space | Output loss makes the root inert and starts cleanup |
| Configure shell root | A valid root is created or its output-local extent changes | Send a positive logical extent and serial | Keep the root unmapped until a valid acknowledgement |
| Map shell root | An acknowledged root commits a valid non-null buffer | Root enters presentation and hit testing at its semantic role | Invalid state cannot become partially visible or focusable |
| Place shell root by role | A mapped `overlay` root participates in composition | Above applications, below secure content | Ignore no secure boundary and accept no client-selected z value |
| Add root to hit testing | A mapped root has a non-empty eligible input region | Normal top-level selection can return its surface and local point | Do not advertise pointer conformance if this integration is absent |
| Update shell root state | A live root commits buffer, damage, transform, scale, viewport, or input state | Scene and input systems observe one coherent committed state | Reject invalid dimensions and bound resource use |
| Damage shell root | Committed content, mapping, position, or teardown changes visible coverage | Schedule damage for affected old/new root coverage | Do not unconditionally damage unrelated outputs or the whole scene |
| Remove root from hit testing | Root unmaps, loses output, is destroyed, or disconnects | Root is no longer selectable and any focus is cleared | Removal precedes release of referenced role/module code |
| Unmap or destroy shell root | Null commit or any teardown path | Presentation, listeners, scene state, and focus disappear | Internal teardown is idempotent and leaves no stale reference |
| Revoke controller authority | Controller disconnects or the implementation is removed | All roots disappear and another authorized connection may start cleanly | No authority, focus, or root survives the connection |

Protocol dispatch must not block waiting for the shell process.

## Scene registration and damage

Rendering-only insertion is insufficient. A mapped root must be represented in
the compositor's ordinary scene or an equivalent authoritative structure that
participates in:

- mapping and unmapping;
- semantic ordering;
- output clipping and transforms;
- committed surface state and subsurface traversal where supported;
- bounded damage and frame completion;
- top-level hit testing;
- focus and destruction cleanup.

The compositor may use any internal scene representation. It must not create a
second, disconnected hit-test graph or fake invisible input surface.

Committed surface damage should be translated through the root's transforms
and clipped to the affected output. Mapping, unmapping, movement, and teardown
damage the union of relevant old and new coverage. Renderer-specific
antialiasing or sampling expansion is permitted; unrelated outputs or scene
regions must not be damaged merely because an HTMShell root exists.

## Hit testing and standard seat focus

For every pointer update, a conforming compositor applies its ordinary secure
scene order, transforms the point to surface-local logical coordinates, and
tests the committed input region. When the root wins the normal top-level hit
test, the compositor assigns focus through its standard seat implementation and
delivers ordinary `wl_pointer` events.

Required behavior includes:

- enter, motion, button, axis, frame, and leave as applicable;
- surface-local fixed-point coordinates after output, surface, buffer, and
  viewport transforms;
- click-through behavior for an empty input region;
- ordinary application targeting outside the root;
- session-lock priority over every HTMShell root;
- immediate focus removal on leave, unmap, output loss, destruction,
  disconnect, or implementation unload.

Pixel alpha is not an input mask. Popup and grab semantics are outside version
1 and must not be approximated with a private pointer stream.

The implementation must not use broad interception of global pointer events,
poll pointer position, send custom HTMShell pointer events, or make the client
detect compositor-specific IPC.

The current Hyprland reference host proves protocol registration, role
assignment, standard-surface rendering, bounded root damage, and teardown. It
cannot register the root in Hyprland's ordinary top-level hit-test path through
a plugin-safe interface, so it does not advertise standard pointer focus. This
is a compositor integration gap, not a change to the universal client
contract.

## Output association

Output association is immutable. The compositor validates that the supplied
`wl_output` belongs to the controller connection and remains available.

When the output global is removed, the compositor immediately unmaps the root,
removes it from hit testing, clears focus, and makes the role object inert. The
client observes the normal registry removal and destroys the root. Later
commits cannot remap an inert root; movement to another output requires a new
surface and root.

Coordinate spaces remain distinct:

| Space | Contract use |
| --- | --- |
| Surface-local logical | Configure extent, input region, pointer coordinates |
| Output-local logical | Semantic root placement on its associated output |
| Global compositor logical | Internal output placement and scene traversal |
| Buffer | Standard buffer scale, transform, and viewport conversion |
| Framebuffer | Internal output scale and transform conversion |

Only the first two have HTMShell-specific semantics. Standard Wayland and
compositor internals own the remaining conversions.

## Authorization responsibility

Authorization is compositor or session policy, not presentation state. The
compositor determines which client receives shell authority. Production
directions include:

- launching the shell and associating authority with that connection;
- exposing the privileged global only to a selected client;
- inheriting authority through a protected connection or session mechanism;
- delegating selection to a trusted session manager.

Same-UID access, executable-path matching, and first-client-wins behavior are
not sufficient by themselves. Ordinary clients must not receive the role.

The current `authenticate` request and development capability value are
provisional bootstrap machinery, not production policy. They may be removed by
an incompatible experimental revision. Open questions are how authority is
handed to a restarted shell, how a session manager participates, and whether a
future stable protocol should assume that visibility of the global already
implies authorization.

## Cleanup and failure containment

The compositor must remove presentation, hit-test, focus, listeners, pending
configuration, output associations, and authority when any owning object
disappears. Cleanup applies to:

- explicit null-buffer unmap;
- root destruction before surface destruction;
- surface destruction before root destruction;
- output removal;
- abrupt client disconnect;
- compositor module or plugin unload;
- compositor shutdown or restart.

Malformed requests produce protocol errors for the offending client. They must
not leave partial state, stale object references, stale focus, repeated log
growth, or a compositor crash. If implementation code can be unloaded, every
callback, role object, and vtable owned by that code must be gone before its
module is unmapped.

## Existing Wayland protocols reused

HTMShell discovers and uses existing globals directly. The classifications
below describe contract dependency, not universal compositor availability.

| Concern | Existing protocol family | Classification |
| --- | --- | --- |
| Surface, commit, damage, callbacks, outputs | Core Wayland | Baseline |
| Shared-memory conformance buffers | `wl_shm` | Baseline harness |
| Pointer and future keyboard delivery | Core seat protocols | Pointer baseline; keyboard future |
| Buffer viewport and fractional scale | Viewporter and fractional-scale protocols | Optional and independent |
| Frame scheduling | Core frame callbacks | Baseline |
| Presentation feedback | Presentation-time protocol | Optional future |
| GPU buffers | Linux DMA-BUF | Optional future |
| Explicit synchronization | DRM syncobj explicit synchronization | Optional future |
| Workspaces | Existing workspace protocols | Optional and independent |
| Application enumeration | Existing foreign-toplevel protocols | Optional and independent |
| Secure locking | Session-lock protocol | Independent privileged facility |
| Clipboard and drag-and-drop | Core data-device and suitable data-control protocols | Future and independently authorized |
| Capture and previews | Existing image-capture protocols | Future and independently authorized |
| Background effects | Existing background-effect protocols where suitable | Future |
| Color management | Existing color-management protocols | Future |

Absence of an optional or future protocol removes that feature; it does not
change the meaning of baseline HTMShell behavior. The overlay role never
substitutes for session locking.

## Optional future capabilities

Version 1 has no optional HTMShell role or advanced composition profile.
Potential future work includes workspace association, other semantic roots,
application interleaving, native material regions, previews, and synchronized
animations. Each requires a demonstrated semantic gap and independent security
review before protocol expansion.

Existing extension globals remain independently discoverable. HTMShell will
not create a large matrix of compositor-specific capability flags.

## Conformance testing

`htm-shell-conformance` is a compositor-neutral Wayland client. It does not
identify the compositor, call compositor-specific IPC, inspect internal scene
objects, or parse compositor logs as its primary assertion.

```sh
cargo run -p htm-shell-conformance --release --locked -- \
  --group baseline --timeout-ms 2000 --output result.json
```

The environment supplies development controller authority out of band. Reports
never contain that value. JSON ordering is deterministic; volatile timings are
written separately.

Result categories are `PASS`, `FAIL`, `SKIP`, `UNSUPPORTED`, `INCONCLUSIVE`,
and `TIMEOUT`. Missing mandatory behavior fails baseline conformance. Missing
optional behavior does not.

Pointer testing is operator-assisted until a compositor-neutral physical-input
injection method is selected. A compositor advertising pointer support must
deliver enter, motion, button, and leave through `wl_pointer`.

### Maintainer acceptance checklist

A compositor should not claim baseline compatibility unless all checks pass:

- [ ] The authorized client can bind the manager and receive the complete
      capability set.
- [ ] An empty ordinary surface can receive the overlay role and output
      association.
- [ ] Initial configure and acknowledgement ordering is enforced.
- [ ] Two ordinary buffer commits complete.
- [ ] Both frame callbacks and buffer releases arrive.
- [ ] The root is above applications and below session-lock content.
- [ ] The root participates in ordinary scene traversal and output clipping.
- [ ] The normal hit test returns the root inside its input region.
- [ ] Standard pointer enter, motion, button, and leave reach the surface.
- [ ] An empty input region prevents targeting.
- [ ] Unmapping or destroying a focused root removes pointer focus.
- [ ] Output removal makes the root inert.
- [ ] Abrupt client exit removes all roots and controller authority.
- [ ] Invalid lifecycle requests affect only the offending client.
- [ ] Repeated connect, render, disconnect, and implementation reload cycles
      leave no stale scene, listener, focus, or authority state.

Black-box results establish client-visible behavior. Compositor-side tests are
still needed for secure ordering, precise damage, listener/resource ownership,
and module-unload safety.

## Implementation guidance

### wlroots-based compositors

Protocol dispatch and surface-role plumbing may use wlroots facilities. The
compositor still owns policy and semantic ordering. A mapped root belongs in
the same scene traversal used for output composition and pointer surface
selection, followed by normal seat notification. Output, surface, and client
listeners own cleanup. Reusable wlroots support is optional; it does not define
the client contract.

### Smithay-based compositors

A compositor can store role state with the Wayland surface, represent the root
in its render-element/space model, include it in the ordinary surface-under-
pointer selection, and deliver focus through the normal Smithay seat path.
Policy, ordering, output association, and cleanup remain compositor-owned. A
shared Smithay handler is optional and does not change wire semantics.

### Plugin or module implementations

The host compositor needs first-class, unload-safe facilities for protocol
registration, permanent surface roles, semantic scene placement, normal hit
testing, focus cleanup, damage, and callback ownership. Rendering callbacks
alone are insufficient. Function interception and broad input interception are
not acceptable substitutes.

### Monolithic compositors

A compositor without a plugin system implements the same responsibilities
directly in its protocol, role, scene, seat, and lifecycle code. No adapter SDK
or plugin abstraction is required.

These architectures require different internal work but no different request,
event, role, capability, or client behavior.

## Non-requirements

Baseline support does not require:

- HTML, CSS, DOM, layout, text, scripting, or package code in the compositor;
- a compositor-side shell renderer or display-list interpreter;
- a custom pixel socket, CPU readback, or independently imported texture path;
- compositor detection or compositor-specific client IPC;
- numeric z positions, render-stage names, internal scene identifiers, or
  compositor object pointers;
- access to application buffers, pixels, window metadata, or workspace state;
- background, workspace, or panel roles;
- materials, blur, previews, capture, global shortcuts, or input grabs;
- keyboard, touch, gestures, clipboard, IME, or lock-screen behavior;
- a compositor fork when an appropriate native or plugin-safe integration
  point exists.

## Open questions

- Which launch/session mechanism should convey controller authority without a
  reusable credential on the wire?
- What shared resource limits should a future stable specification require?
- How should a later popup design participate in normal Wayland grabs?
- Which independent compositor implementation should challenge the contract
  before stabilization?
- Should a future protocol seek an ecosystem namespace after multiple
  implementations agree on the semantics?
