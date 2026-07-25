# `action-button`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element | **Declaration:** `data-htm-element="action-button"`

`action-button` dispatches one approved action from an ordinary HTML button.

## Usage

```html
<button id="overlay-toggle"
        type="button"
        data-htm-element="action-button"
        data-htm-action="overlay.toggle">
  <img src="assets/overlay.svg" alt="">
  <span>Overlay</span>
</button>
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tag | `button` |
| `id` | Required, nonempty, and unique in the document |
| `data-htm-element` | Must be `action-button` |
| `data-htm-action` | Required approved action |
| `data-htm-target` | Required for clock and external PipeWire audio actions, forbidden where the action defines no target |
| `data-htm-enabled-bind` | Optional typed Boolean availability binding |
| `disabled` | Optional HTML boolean attribute |

The action must be permitted from the document's surface kind. Other HTMShell behavior attributes are rejected.

## Interaction

A left-button press captures the deepest eligible action button under the logical pointer position. A release dispatches exactly once only when it resolves to the same live button.

Images, labels, and token elements nested inside the button resolve to their owning button. Releasing elsewhere does not dispatch.

Pointer leave, surface unmap, output removal, pointer capability loss, or stale document identity cancels the pending action. The presence of `disabled` prevents activation, including `disabled="false"` under HTML boolean-attribute rules.

Actions cannot execute arbitrary methods, shell commands, files, or network requests.

Clock actions resolve `data-htm-target` as an exact `id` in the same document. The target must be a live [`clock-text`](clock-text.md) declaration. Target identity is checked again at dispatch.

`data-htm-enabled-bind` controls the runtime portion of the effective disabled state. Unknown, unavailable, or false values disable the button. An author-provided `disabled` attribute is permanent and is never removed by a binding.

Inside `pipewire.nodes`, the mute actions `pipewire.audio.mute`, `pipewire.audio.unmute`, and `pipewire.audio.toggle_mute` target the current item and require `item.can_set_mute`. No target attribute is used. Other actions and repeats remain read-only.

The same node repeat permits
`pipewire.defaults.set_preferred_sink` with
`item.can_set_preferred_sink` and
`pipewire.defaults.set_preferred_source` with
`item.can_set_preferred_source`. These actions also use the implicit current
item and reject `data-htm-target`.

Top-level buttons may use `pipewire.defaults.clear_preferred_sink` with
`pipewire.configured_sink.can_clear` or
`pipewire.defaults.clear_preferred_source` with
`pipewire.configured_source.can_clear`. Clear actions also reject a target.

Outside a repeat, PipeWire mute actions require `pipewire.default_sink` or `pipewire.default_source` as `data-htm-target`. Raw IDs and configured defaults are rejected. `pipewire.audio.set_volume` is valid only on [`range-control`](range-control.md).

## See also

- [Overlay actions](../HTMShell.Actions/Overlay.md)
- [Clock actions](../HTMShell.Actions/Clock.md)
- [Power profile actions](../HTMShell.Actions/PowerProfile.md)
- [PipeWire audio actions](../HTMShell.Actions/PipeWireAudio.md)
- [PipeWire default actions](../HTMShell.Actions/PipeWireDefaults.md)
- [State and actions guide](../../guide/state-and-actions.md)
