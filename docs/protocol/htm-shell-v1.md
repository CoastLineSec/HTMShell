# `htm-shell-v1` protocol semantics

## Experimental status

`htm-shell-v1` is an experimental, compositor-neutral Wayland protocol owned by
the HTMShell project. It is not stable, standardized, or part of
`wayland-protocols`. The XML is the wire definition; this document defines the
lifecycle and policy semantics that are difficult to express in XML alone.

The current interface names remain `htm_shell_manager_v1` and
`htm_shell_root_v1`. The project-specific `htm_` prefix avoids namespace
collision, while the documentation and XML clearly mark the protocol as
experimental. A rename would not improve its semantics and would create churn
for the development tools. Interface names do not promise future compatibility
while the protocol is explicitly experimental.

## Object model

```text
wl_registry
    └─ htm_shell_manager_v1             connection-scoped controller binding
           └─ htm_shell_root_v1         permanent role for one wl_surface
                  ├─ wl_surface         standard content and committed state
                  └─ wl_output          immutable output association
```

Wayland object identity supplies root lifetime. No scalar root, output,
workspace, or compositor object identifier is added.

## Manager lifecycle

1. The compositor decides whether a client connection may receive shell
   authority.
2. The client binds `htm_shell_manager_v1` version 1.
3. The development reference implementation uses `authenticate` as a
   provisional bootstrap.
4. After authority is granted, the compositor sends zero or more `capability`
   events followed by exactly one `ready` event.
5. No capability event follows `ready` on that manager object.
6. The client may issue `get_root` only after `ready`.

Production direction is to make access to the privileged manager imply that
compositor/session policy already authorized the connection. The
`authenticate` request is not a stable authentication design and may disappear
in a breaking experimental revision.

Destroying the manager binding does not destroy roots created through it and
does not revoke connection-scoped authority. Disconnecting the Wayland client
destroys every root and revokes authority.

Only one controller connection exists at a time. A second claim receives
`controller_exists`. Reconnecting after the controller disconnects begins from
an empty state; no root or capability state is restored automatically.

## Capabilities

Version 1 defines:

| Value | Name | Classification |
| ---: | --- | --- |
| 1 | `root_overlay` | Mandatory baseline |
| 2 | `standard_pointer_focus` | Mandatory baseline |

Unknown capability values are additive optional information and must be
ignored. The `ready` event terminates the initial capability set. A compositor
missing either known mandatory value is not baseline-conformant even if it can
render a root.

Future facilities already represented by separate Wayland globals are
discovered through those globals instead of duplicated as HTMShell capability
events.

Version 1 guarantees no unadvertised or future optional capability.

## Role rules

Version 1 defines only `overlay`.

An overlay root is above ordinary application content on its associated output
and below session-lock surfaces and security-critical compositor content. It
reserves no application layout area and has no workspace association. Pointer
eligibility follows its committed `wl_surface` input region.

The role is permanent for the `wl_surface` lifetime. `get_root` fails when:

- the client has not received controller authority;
- the surface belongs to another client;
- the output belongs to another client or is unavailable;
- the surface already has any role;
- the surface already has an attached buffer;
- the requested role value is unknown or unsupported;
- the same surface already has an HTMShell root.

The controller may create more than one overlay root. On a shared output,
later-created overlay roots are above earlier-created overlay roots. This order
is confined below secure compositor content. Version 1 has no reorder request.

## Root state machine

```text
unassigned wl_surface
        │ get_root
        ▼
waiting_for_configure
        │ configure(serial, width, height)
        ▼
waiting_for_ack
        │ ack_configure(serial)
        ▼
configured_unmapped
        │ attach(non-null) + commit
        ▼
mapped ◄──────────────┐
        │              │ later configure + ack + commit
        │ attach(NULL) + commit
        ▼
configured_unmapped
        │ root.destroy / surface.destroy / disconnect / output removal
        ▼
inert_or_destroyed
```

The compositor sends an initial configure immediately after accepting the
root. Width and height are positive output-local logical dimensions. Buffer
scale, buffer transform, viewports, and fractional scale use existing Wayland
protocols.

The client must acknowledge the initial configure before attaching a buffer.
A commit with a non-null buffer before acknowledgement raises
`buffer_before_ack`.

The compositor may send later configures. Each serial identifies pending root
state. Acknowledging the newest pending serial acknowledges that serial and all
earlier pending serials. An unknown, stale, or duplicate serial raises
`invalid_ack`. Acknowledged state becomes current with the related
`wl_surface.commit`.

The configured logical extent is compositor-owned. The compositor clips or
positions committed surface content within that extent according to standard
surface state. It may reject dimensions beyond documented resource limits with
`invalid_size`; limits must be bounded and usable for the baseline role.

## Standard surface behavior

After acknowledgement, content uses only standard Wayland operations:

- `wl_surface.attach`;
- `wl_surface.damage` or `damage_buffer`;
- `wl_surface.frame`;
- `wl_surface.set_input_region`;
- buffer scale and transform;
- `wl_surface.commit`;
- `wl_buffer.release`.

A non-null committed buffer maps the root. A null committed buffer unmaps it.
The compositor delivers a frame callback only for a requested callback and
releases a buffer when it is no longer used. No pixel data appears in HTMShell
protocol requests.

## Pointer semantics

Mapped roots participate in the compositor's ordinary top-level hit test. The
compositor applies scene ordering, transforms the point into surface-local
logical coordinates, tests the committed input region, and assigns focus using
the standard seat implementation.

An empty input region is click-through. Pixel alpha has no effect on input. A
transparent pixel inside the input region remains a target. When roots overlap,
the later-created root is tested first. Session-lock content always wins.

Pointer focus is cleared on leave, unmap, output removal, root destruction,
surface destruction, client disconnect, or host/module teardown. The protocol
does not define custom pointer events.

## Output association

`get_root` takes one existing client-bound `wl_output`. The association is
immutable. The root uses output-local logical configuration and the compositor
owns conversion to global, buffer, and framebuffer coordinates.

When the corresponding registry global is removed, the compositor immediately
unmaps the root, removes it from hit testing, clears its focus, and makes it
inert. The client destroys the root after receiving
`wl_registry.global_remove`. Later commits cannot remap an inert root.

## Destruction ordering

### Root before surface

`htm_shell_root_v1.destroy` immediately unmaps and removes input eligibility.
The `wl_surface` stays alive with its permanent role and cannot receive another
role. The client then destroys the surface.

### Surface before root

Destroying `wl_surface` causes the compositor to destroy the associated root
resource and remove all presentation and input state. The client must not use
the stale root proxy.

### Client disconnect

The compositor revokes controller authority, removes every root, clears focus,
releases all listeners and scene state, and bounds resulting damage. No state is
carried into a later connection.

### Compositor restart

The Wayland connection is lost. A restarted shell discovers globals,
reacquires authority, and creates new surfaces and roots. No protocol object or
serial survives restart.

## Errors

Manager errors:

| Error | Meaning |
| --- | --- |
| `already_authenticated` | The provisional bootstrap was repeated on one manager. |
| `unauthorized` | Shell authority was not granted or was not completed. |
| `controller_exists` | Another client connection already owns shell authority. |
| `invalid_surface` | Surface ownership, construction state, or role is invalid. |
| `invalid_output` | Output ownership or availability is invalid. |
| `invalid_role` | The requested semantic role is not supported. |
| `duplicate_root` | The surface already has an HTMShell root. |

Root errors:

| Error | Meaning |
| --- | --- |
| `invalid_ack` | Configure serial is unknown, stale, or duplicate. |
| `buffer_before_ack` | A non-null buffer was committed before initial acknowledgement. |
| `invalid_size` | Surface or buffer dimensions exceed bounded implementation limits. |

Protocol errors terminate the offending client connection according to normal
Wayland behavior. They must not terminate the compositor.

## Versioning

Normal Wayland object versions are the only wire-version mechanism.

- The client binds no higher than the advertised manager version it supports.
- Child object versions are inherited from the manager ancestry.
- Additive requests and events increase the manager and affected child
  interface versions and use `since`.
- A client ignores unknown additive enum values when the specification permits
  expansion.
- A compositor never sends a request/event unavailable in the bound object
  version.
- No explicit protocol major/minor request duplicates `wl_registry.bind`.

While experimental, a breaking change may rename the protocol/interface family
and require source updates. After stabilization, an incompatible wire design
requires a new interface family such as `_v2`; it does not silently reinterpret
version 1 messages.

## Authorization assumptions

Shell authority is privileged. Preferred production behavior is to expose the
manager only to a client connection selected by compositor/session policy. A
compositor-launched client or inherited connection capability is stronger than
same-user access, executable-name matching, a public first-client-wins global,
or reusable authentication bytes.

The current bootstrap request exists only for the development reference
implementation. Its
bytes are never part of conformance output. A future stable protocol may assume
that binding the restricted global is sufficient and remove the request.

## Valid sequence

```text
bind manager v1
complete provisional development bootstrap
receive root_overlay
receive standard_pointer_focus
receive ready
create empty wl_surface
get_root(surface, output, overlay)
receive configure(17, 640, 160)
ack_configure(17)
attach(buffer A), damage, frame(callback A), commit
receive callback A.done
receive buffer A.release
attach(buffer B), damage, frame(callback B), commit
receive callback B.done
receive buffer B.release
attach(NULL), commit
destroy root
destroy surface
disconnect
```

## Invalid sequences

- `get_root` before authority or before `ready`.
- `get_root` with a surface owned by another client.
- `get_root` with an unavailable output.
- assigning an XDG, layer-shell, session-lock, subsurface, or second HTMShell
  role to the same surface.
- attaching a buffer before role assignment and then requesting a root.
- attaching a buffer before the initial configure acknowledgement.
- acknowledging an unknown or already acknowledged serial.
- requesting an unknown role value.
- using a root after its surface has been destroyed.
- committing to an inert root after its output disappeared.

## Security requirements

- The compositor, not the client, chooses secure scene ordering.
- Session-lock content always outranks version 1 roots.
- Pointer input remains constrained by normal hit testing and input regions.
- Client geometry and buffer dimensions are bounded.
- No application pixels or metadata are exposed by this baseline.
- No client-provided value becomes a compositor pointer or internal identifier.
- Optional sensitive features require separately defined authority.
- Visual HTML or CSS cannot grant protocol authority.
