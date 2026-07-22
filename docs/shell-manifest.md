# Experimental shell manifest

The portable HTMShell host can load a small local JSON manifest and expand its
surface templates into independent layer-shell instances on every eligible
Wayland output. This version 1 format is experimental. It is not a package,
settings, or long-term migration format.

## Example

```json
{
  "version": 1,
  "id": "portable-shell-demo",
  "surfaces": [
    {
      "id": "panel",
      "kind": "panel",
      "document": "panel.html",
      "outputs": "all",
      "edge": "top",
      "thickness": 52,
      "reserveSpace": true
    },
    {
      "id": "overlay",
      "kind": "overlay",
      "document": "overlay.html",
      "outputs": "all",
      "initiallyOpen": false
    }
  ]
}
```

Validate the tracked example without connecting to Wayland:

```sh
cargo run -p htmshell-live --locked -- \
  manifest examples/multi-output-shell/shell.json --validate-only
```

Run it on the current Wayland display:

```sh
cargo run -p htmshell-live --release --locked -- \
  manifest examples/multi-output-shell/shell.json
```

## Root fields

| Field | Version 1 rule |
| --- | --- |
| `version` | Must be the integer `1`. Unknown versions are rejected. |
| `id` | Nonempty stable manifest ID using lowercase ASCII letters, digits, and interior hyphens. |
| `surfaces` | Exactly one `panel` and one `overlay` in the current profile. IDs must be unique. |

Unknown fields are rejected so spelling mistakes do not silently change shell
behavior. IDs are independent of array position and are used to derive stable
layer-shell namespaces.

## Surface fields

Every surface defines:

| Field | Version 1 rule |
| --- | --- |
| `id` | Unique nonempty ID with the same character rules as the root ID. |
| `kind` | `panel` or `overlay`. |
| `document` | Local HTML path relative to the manifest directory. |
| `outputs` | Only `all` is supported. |

A `panel` also defines:

| Field | Version 1 rule |
| --- | --- |
| `edge` | Only `top` is supported. |
| `thickness` | Logical panel height from 1 through 512. |
| `reserveSpace` | When true, the exclusive zone equals `thickness`; otherwise it is zero. |

An `overlay` also defines:

| Field | Version 1 rule |
| --- | --- |
| `initiallyOpen` | Boolean initial mapping state. The tracked example uses `false`. |

The semantic presets derive layer, anchors, requested size, exclusive zone,
and keyboard-interactivity settings. The manifest does not expose arbitrary
Wayland layers, anchors, exclusive-zone values, or keyboard modes.

All manifest dimensions are logical pixels. The manifest does not select or
override output scale. When the compositor advertises both fractional-scale
and viewporter, each surface uses its compositor-provided preferred scale for
physical rendering while retaining logical layout and input geometry. If the
complete protocol pair is unavailable, the host uses its scale-1 presentation
path.

## Local path rules

Document paths are resolved relative to the manifest directory. Absolute
paths, parent-directory escapes, remote URLs, unsupported schemes, missing
files, and symbolic links that resolve outside that directory are rejected.
The manifest size, surface count, ID length, path length, and panel thickness
are bounded before live surface creation.

Validation completes before the host connects to Wayland. Referenced HTML and
CSS remain subject to the runtime's local-resource policy.

## Output expansion and identity

`outputs: "all"` creates one independent panel document and one independent
overlay document for every eligible output. Each instance owns its own Wayland
role lifecycle, parsed document, input state, frame callback, and shared-memory
pool. A closed overlay retains its document without retaining a mapped Wayland
role; reopening it creates a fresh role for the same document instance.
A panel action affects only the overlay associated with the same live output.

Output identity is session-local and combines the registry global with a local
generation. `wl_output.name` and description values are diagnostics only;
they are not persistent selectors or saved-state keys. If an output is removed
and later appears again, the host creates fresh output and surface generations
without rebuilding unrelated outputs.

The process stays connected and idle when no eligible outputs exist. Outputs
may use different compositor-provided scales, and every live surface owns its
own scale state and physical buffer pool. Persistent monitor selection,
manifest reload, scale overrides, additional panel edges, additional surface
kinds, and cross-output state synchronization are not supported by this
profile.
