# HTMShell compositor integration

## Status

This document describes the experimental HTMShell compositor contract. It is
an implementation proposal, not a stable standard or an accepted
`wayland-protocols` extension. Breaking changes remain possible while the
contract is being validated.

HTMShell defines client-visible semantics. A supporting compositor implements
those semantics using its own scene, input, and policy architecture. HTMShell
does not identify the compositor or select a compositor-specific runtime path.

```text
HTMShell runtime
        │
        │ standard Wayland + htm-shell-v1
        ▼
Compositor-owned protocol, scene, input, and policy integration
```

The initial contract carries shell content in an ordinary `wl_surface`. It does
not introduce a display list, custom pixel transport, renderer, or compositor
object pointer.

## Why compositor cooperation is required

An ordinary application surface cannot safely choose privileged desktop
placement. The compositor alone has authoritative knowledge of:

- secure session-lock ordering;
- application and compositor scene ordering;
- output and workspace lifecycle;
- top-level pointer hit testing and seat focus;
- damage and presentation scheduling;
- which client is permitted to act as the desktop shell.

HTMShell therefore requests a semantic shell role. The compositor translates
that role into its own scene representation. The client never requests a render
callback, internal scene path, or numeric z position.

## Mandatory baseline

A compositor may claim baseline HTMShell compatibility only when every item in
this table is implemented.

| Requirement | Why it is mandatory |
| --- | --- |
| Authorized controller | Ordinary clients must not acquire shell placement. |
| `htm_shell_manager_v1` discovery | The generic client needs one compositor-neutral entry point. |
| Complete capability advertisement | The client must know whether baseline semantics are actually available. |
| `root_overlay` | Version 1 needs one usable semantic shell root. |
| `standard_pointer_focus` | A visible shell that cannot receive normal pointer input is not usable. |
| Normal `wl_surface` role assignment | Content and buffer lifetime remain standard Wayland behavior. |
| One `wl_output` association | Placement and logical coordinates need an authoritative output. |
| Configure and acknowledgement | The compositor chooses the logical root extent before content maps. |
| Standard buffer commit | The client uses `wl_surface.attach`, damage, and commit. |
| Standard frame callback | The client must pace updates without continuously redrawing. |
| Standard buffer release | The client must know when a buffer may be reused. |
| Standard pointer hit testing and delivery | Input uses `wl_seat` and `wl_pointer`, not a private event stream. |
| Root, surface, and disconnect cleanup | A failed shell must leave no scene node, focus, or authority behind. |
| Failure containment | A bad request disconnects the offending client, not the compositor. |

`root_overlay` and `standard_pointer_focus` are capability events because the
experimental protocol already has capability discovery. They are nevertheless
mandatory for baseline conformance. Omitting either is a baseline failure, not
an optional downgrade.

Baseline conformance does not require workspaces, toplevel enumeration,
keyboard focus, session locking, fractional scaling, DMA-BUF, explicit
synchronization, presentation timestamps, materials, or previews.

## Compatibility and optional capabilities

HTMShell uses a hybrid model:

1. Baseline conformance is one binary semantic contract.
2. Existing Wayland globals advertise independent standardized or staged
   facilities.
3. Future HTMShell-only features may be grouped into a small number of
   semantic profiles when a real feature needs them.

This avoids hierarchical labels that imply every compositor must implement an
identical feature ladder. It also avoids dozens of tiny flags that would merely
reconstruct compositor-specific branching in the client.

Potential future profiles include desktop-state integration and advanced
composition. They are not part of version 1. A compositor that omits an optional
profile must preserve all baseline behavior unchanged.

## Version 1 semantic root

Version 1 defines only `overlay`.

### Overlay

- The root is associated with exactly one `wl_output` for its lifetime.
- It is placed above ordinary application content on that output.
- It is placed below session-lock surfaces and security-critical compositor UI.
- It may receive pointer focus only where its committed `wl_surface` input
  region permits.
- Alpha does not control hit testing; the input region does.
- It reserves no exclusive layout area.
- It is not associated with a workspace.
- It cannot select an internal compositor layer or render stage.
- It cannot acquire global input exclusivity.

The authorized controller may create multiple overlay roots. For roots on the
same output, later role-creation requests are above earlier requests. This
ordering applies only among HTMShell overlay roots; it cannot cross secure
compositor ordering boundaries. Version 1 has no reorder request.

### Deferred roles

The following role ideas are deliberately deferred:

| Role | Intended direction | Missing work |
| --- | --- | --- |
| `background` | Behind ordinary application content, never above lock content | Wallpaper ownership, input defaults, and output transition semantics |
| `workspace` | Associated with compositor-authoritative workspace state | Portable workspace-object association and transition rules |
| `panel` | Persistent shell chrome, potentially reserving layout space | Relationship to layer-shell and exclusive-zone policy |

Adding one of these requires a demonstrated semantic gap and a protocol version
change. Their names are not current capabilities.

## Surface and configure lifecycle

The client creates an unassigned `wl_surface` with no attached buffer, then asks
the manager to assign the semantic role.

```text
manager ready
    │
    ├─ create wl_surface (no role, no attached buffer)
    ├─ get_root(surface, output, overlay)
    │       │
    │       └─ configure(serial, logical size)
    ├─ ack_configure(serial)
    ├─ wl_surface.attach + damage + frame + commit
    │       │
    │       ├─ wl_callback.done
    │       └─ wl_buffer.release
    ├─ attach NULL + commit                 (unmap)
    ├─ root.destroy                         (remove role object)
    ├─ wl_surface.destroy
    └─ disconnect                           (revoke authority)
```

The role is permanent for the `wl_surface` lifetime. Destroying the root object
unmaps it but does not make the surface eligible for another role. Destroying
the surface first makes the compositor destroy its root resource. A controller
disconnect destroys every associated root and revokes authority.

The compositor may send a later configure when output-local logical geometry
changes. A client may have several pending configures. Acknowledging the newest
serial acknowledges earlier pending configures. Buffer state corresponding to a
configure takes effect on the subsequent `wl_surface.commit`.

The client must not attach a buffer before acknowledging the initial configure.
The compositor rejects a surface that already has a conflicting role or an
attached buffer when `get_root` is processed.

## Output association

The `wl_output` passed to `get_root` is immutable for the root lifetime. The
compositor validates that it belongs to the requesting client and represents a
currently available output.

If the `wl_output` global is removed, the compositor must immediately:

1. unmap the root;
2. remove it from hit testing;
3. clear pointer focus if it targets the root;
4. make the role object inert.

The client observes standard `wl_registry.global_remove` and destroys the root.
An inert root cannot remap from later surface commits. Moving content to another
output requires a new surface and root.

## Buffer and frame lifecycle

Baseline conformance uses core Wayland and `wl_shm` so that no GPU-sharing
mechanism is required to validate the contract. A compositor must:

- consume standard committed `wl_surface` state;
- honor surface damage within the root;
- schedule a frame only when needed;
- deliver requested `wl_surface.frame` callbacks;
- release each `wl_buffer` when it is no longer in use;
- never require pixels through a separate socket;
- never perform a CPU readback for this contract;
- unmap on a committed null buffer;
- avoid continuous redraw while the surface is idle.

Production buffer mechanisms may later include Linux DMA-BUF and explicit
synchronization through their existing protocols. They do not change the shell
role semantics.

## Pointer and hit-testing requirements

A conforming compositor integrates every mapped shell root into its normal
top-level hit-test graph or equivalent. For an eligible point it must use its
normal seat implementation to focus the root's `wl_surface` and deliver
`wl_pointer` events.

Required behavior:

1. Apply semantic scene ordering before choosing a hit target.
2. Transform the pointer into surface-local logical coordinates.
3. Test the committed `wl_surface` input region.
4. Assign focus through the standard Wayland seat path.
5. Deliver enter, motion, button, axis, frame, and leave events as applicable.
6. Return points outside the root to ordinary compositor hit testing.
7. clear focus immediately when the root unmaps, is destroyed, loses its
   output, or disconnects.

Alpha is not an input mask. A fully transparent pixel remains interactive when
it is inside the input region. An empty input region makes the root
click-through. The default input region follows core `wl_surface` rules.

Surface and buffer transforms, output transforms, integer scale, viewport
state, and fractional positions must be included in the coordinate conversion.
Wayland fixed-point coordinates preserve fractional surface-local positions.

When overlay roots overlap, the later-created root is considered first. Secure
session-lock content always has priority. Popup semantics are not defined by
version 1; assigning an XDG popup role to the shell-root surface is a role
conflict. A future popup design must retain normal seat grabs and cannot use a
private pointer stream.

The compositor must not solve this requirement by globally intercepting pointer
events in a plugin. Root registration, scene placement, hit testing, and focus
cleanup must share one compositor-owned lifecycle.

## Coordinate spaces

Implementations must keep these spaces explicit:

| Space | Meaning |
| --- | --- |
| Surface-local logical | Input region, configure extent, pointer coordinates |
| Output-local logical | Semantic root placement relative to its output |
| Global compositor logical | Output placement in the compositor scene |
| Buffer | Pixel coordinates after buffer scale and transform |
| Framebuffer | Renderer/output coordinates after output scale and transform |

The protocol exposes only the configured logical extent and standard Wayland
objects. Internal conversions remain compositor-owned.

## Minimal compositor integration capability

This is a semantic requirements list, not an API or storage prescription.

| Operation | Preconditions and input | Required result | Failure behavior |
| --- | --- | --- | --- |
| Register controller | Compositor policy authorizes one client connection | Expose manager authority for that connection | Withhold authority or disconnect the claimant |
| Register shell root | Authorized manager, unassigned surface, valid output and role | Create connection-owned root state and assign permanent role | Protocol error on the manager |
| Configure shell root | Live root and output | Send positive output-local logical extent and serial | Keep root unmapped |
| Update root state | Valid acknowledged surface commit | Apply buffer, input region, transform, and damage atomically | Reject invalid lifecycle state |
| Map shell root | Acknowledged configure and non-null valid buffer | Add to semantic scene position and hit testing | Root remains unmapped |
| Unmap shell root | Null buffer, output loss, destroy, or disconnect | Remove from presentation and hit testing; clear focus | Must be idempotent internally |
| Associate output | Valid `wl_output` at creation | Retain output lifecycle link and coordinate conversion | Reject invalid or unavailable output |
| Place by role | Mapped root with `overlay` role | Above applications, below secure content | Do not substitute a client-selected z value |
| Include in hit testing | Mapped, input-eligible root | Normal seat targeting and surface-local coordinates | Do not advertise pointer capability if absent |
| Destroy shell root | Root/surface/client teardown | Release listeners, scene state, focus, and damage | No stale reference may remain |

All operations run on the compositor's normal Wayland/event-loop thread unless
the compositor provides an equivalent synchronization boundary. Protocol
dispatch must not block on the shell process.

## Authorization responsibility

Authorization is compositor policy. The preferred production direction is:

- the compositor or session manager selects and starts the shell;
- the privileged global is visible only to the selected client connection, or
  the connection receives equivalent inherited authority;
- access to the manager therefore implies controller authorization;
- only one controller exists per compositor session;
- disconnect immediately releases authority.

This resists another process owned by the same user better than a public global
with first-client-wins behavior. Executable-path matching alone is weak because
paths, namespaces, and same-user process control complicate identity. A reusable
string in the protocol is also not a production trust root.

The current `authenticate` request is a provisional reference-host bootstrap.
It is not the selected production mechanism and may be removed in a breaking
experimental revision. The contract leaves room to evaluate compositor launch,
session-manager brokering, and inherited Wayland connection state with a second
implementation.

## Security invariants

Protocol invariants:

- only the authorized controller receives shell authority;
- an ordinary client cannot assign an HTMShell role;
- a role is permanent for its surface lifetime;
- placement is semantic and compositor-controlled;
- a root never outranks session-lock content;
- a root receives only normal region-bounded pointer input;
- no compositor pointer, memory address, or internal index crosses the wire;
- invalid requests affect only the offending client;
- client disconnect removes every root and all authority;
- baseline requests expose no application pixels, window metadata, or workspace
  metadata.

Compositor policy chooses the controller and may impose stricter limits on root
count, memory, dimensions, and request rate as long as advertised baseline
semantics remain usable. Session-manager policy decides how the shell is
launched and recovered. Future sensitive capabilities need separate semantic
authorization and must not be implied by visual CSS.

## Existing protocol reuse

HTMShell does not duplicate these facilities:

| Protocol | Use | Baseline status | Availability direction | Behavior when absent |
| --- | --- | --- | --- | --- |
| Core Wayland | display, registry, surfaces, buffers, callbacks, outputs, seats, pointer | Required | Universal Wayland foundation | No baseline compatibility |
| `wl_shm` | portable conformance buffers | Required by the baseline harness | Core protocol; required compositor facility | Baseline harness cannot run |
| `wp_viewporter` | source/destination scaling | Optional | Stable extension with broad compositor support | Use integer buffer scale |
| Fractional scale | preferred non-integer scale | Optional | Staging extension; support is not universal | Baseline remains integer-scale |
| Presentation time | presentation feedback | Optional future | Stable extension; support varies | Frame callbacks remain available |
| Linux DMA-BUF | GPU buffer transport | Optional future | Stable Linux extension; unsuitable as a universal first transport | `wl_shm` remains valid |
| Linux DRM sync object | explicit synchronization | Optional future | Staging Linux extension; support is limited | No explicit-sync claim |
| `ext_workspace_v1` | workspace discovery/control | Optional independent protocol | Staging extension; compositor support varies | No workspace UI capability |
| `ext_foreign_toplevel_list_v1` | application toplevel discovery | Optional independent protocol | Staging extension; compositor support varies | No application-state capability |
| `ext_session_lock_v1` | secure session lock | Independent privileged protocol | Staging extension with multiple implementations | HTMShell overlay never substitutes for locking |
| Cursor shape | standard cursor selection | Optional | Staging extension; ordinary cursor paths remain available | Client uses ordinary surface/default cursor paths |
| Core data device / data control | clipboard and privileged data control | Future, separately authorized | Core clipboard is widespread; privileged data control is staging | No clipboard capability |
| Image capture/copy | previews and capture | Future, separately authorized | Staging extensions; support is not universal | No preview capability |
| Background effect | compositor background effects | Future | Staging extension; support is limited | Ordinary alpha/decorations only |
| Color management | color description and conversion | Future | Staging extension under active evolution | Baseline is SDR/sRGB-oriented |

Protocol availability differs by compositor and by `wayland-protocols` stage.
HTMShell discovers these globals directly. It does not mirror their requests or
events under an HTMShell namespace.

## Implementation guidance

### wlroots-style compositors

A wlroots compositor can dispatch the protocol itself, assign a dedicated
surface role, create a normal scene surface/subsurface tree under its chosen
overlay subtree, and include that subtree in the same scene query used for
pointer targeting. Normal seat notification performs focus delivery. Root and
output listeners own cleanup.

Reusable role plumbing in wlroots could reduce duplicate protocol code, but
semantic ordering and policy belong to each compositor. The client contract
does not depend on whether support is shared in wlroots.

### Smithay-style compositors

A Smithay compositor can generate protocol dispatch, retain role state in
surface data, add a mapped surface element/render element at the semantic
position, and return the same surface from its normal element-under-pointer
selection. Smithay seat focus traits then deliver standard input. Output and
surface destruction remove the element and focus state.

A reusable Smithay handler may be useful later, but the protocol does not
require one and does not expose Smithay types.

### Plugin-oriented compositors

A plugin system needs first-class APIs for:

- registering a connection-scoped protocol role;
- placing a standard surface by semantic role;
- registering that surface in normal hit testing;
- scheduling bounded damage and frame callbacks;
- removing the role, scene node, and focus before plugin code unloads.

Rendering callbacks alone are insufficient because they do not establish input
or role lifetime. Function interception is unsuitable: it couples the plugin to
private control flow and makes cleanup difficult to prove. If these APIs do not
exist, a small compositor-core extension is preferable to changing the
universal protocol or broadly intercepting input.

### Monolithic compositors

A monolithic compositor can implement the same semantic operations directly in
its native role, scene, and seat code. No plugin abstraction is required. The
wire lifecycle and conformance expectations remain identical.

## Black-box conformance

`htm-shell-conformance` observes only Wayland-visible behavior. It does not read
compositor IPC, internal objects, or compositor logs, and it does not identify
the compositor.

```sh
cargo run -p htm-shell-conformance --release --locked -- \
  --group baseline --timeout-ms 2000 --output result.json
```

The development environment supplies controller authority out of band. The
tool never writes that value into its report. Test groups are `discovery`,
`authorization`, `root`, `input`, `cleanup`, and `all`/`baseline`.

Results use these categories:

| Result | Meaning |
| --- | --- |
| `PASS` | Required observable behavior occurred. |
| `FAIL` | Advertised or mandatory behavior was violated or absent. |
| `SKIP` | The operator deliberately excluded the test. |
| `UNSUPPORTED` | An optional capability was not advertised. |
| `INCONCLUSIVE` | The environment could not establish the result. |
| `TIMEOUT` | A required event did not arrive within the selected limit. |

A missing mandatory capability is `FAIL`. A missing optional capability is
`UNSUPPORTED` and does not fail baseline aggregation. Required failures take
precedence over timeouts; timeouts take precedence over inconclusive results.

Input tests are operator-assisted until a compositor-neutral way to inject a
physical pointer path is selected. The test requires enter, motion, button, and
leave through `wl_pointer`. If the mandatory pointer capability is not
advertised, it fails immediately rather than pretending the test is optional.

The JSON schema and ordering are deterministic and omit volatile timings.
Feasibility timings are printed separately. Repetition creates a new Wayland
connection each cycle and can demonstrate that connection-scoped authority is
reacquired, but stale pixels and internal listener counts still require
compositor-side diagnostics during development.

## What HTMShell does not require

Baseline support does not require:

- HTML, CSS, DOM, text, or scripting code in the compositor;
- a compositor-side shell renderer;
- a custom pixel socket or display-list protocol;
- compositor detection in the client;
- compositor-specific identifiers or configuration schemas;
- layer-shell as the permanent shell role;
- access to application buffers or private window content;
- materials, blur, capture, previews, or global input grabs;
- a compositor fork when an appropriate plugin API already exists.

## Unresolved questions

- Which launch/session mechanism can grant controller authority portably without
  putting reusable credentials on the wire?
- Should a future stable protocol retain the current project namespace or seek
  broader standardization under an ecosystem namespace?
- Which role should follow `overlay`, and can an existing protocol cover its
  semantics first?
- How should shell-root popups participate in normal popup grabs?
- What resource limits belong in shared protocol language versus compositor
  policy?
- Which second compositor architecture should validate the specification before
  the protocol is stabilized?
