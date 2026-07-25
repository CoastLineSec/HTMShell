# `ChannelControls`

**Module:** `HTMShell.Services.PipeWire`

One typed range action sets one current contextual channel:

```text
pipewire.audio.set_channel_volume
```

## Usage

```html
<input type="range"
       data-htm-element="range-control"
       data-htm-local-id="volume"
       data-htm-bind="item.volume"
       data-htm-action="pipewire.audio.set_channel_volume"
       data-htm-enabled-bind="item.can_set_volume"
       min="0"
       max="1"
       step="0.01">
```

This form is valid only inside `pipewire.nodes` then `item.channels`. The
current channel and parent node are implicit. `data-htm-target`, raw node IDs,
channel indexes, node names, DOM IDs, and selectors are invalid. An
`action-button` cannot dispatch the numeric action.

## Writes

The runtime starts from the latest coordinated vector, changes one perceptual
channel value, converts every channel to PipeWire linear gain, and writes the
full vector. Other channels keep their values and ordering.

Node-average and channel controls share one coordinator. An average intent
scales the latest complete vector. A later channel intent replaces only that
channel. A later average intent includes earlier channel changes. Only one
complete desired vector and one active write are retained per node.

Writes use a minimum 16 millisecond cadence and a two-second timeout. Public
volume changes only after authoritative confirmation.

## Control state

The exact channel control owns runtime `data-htm-state`: `idle`, `pending`,
`failed`, or `unavailable`. Its identity includes the document, connection,
node, channel-layout generation, channel item, and operation.

Layout replacement, node removal, reconnect, denial, and timeout cannot confirm
a stale control. Failure restores the latest authoritative channel value.

The default range is `0` to `1` with step `0.01`. Amplification requires an
explicit larger `max`; the runtime maximum is `2.0` and amplification can clip
or distort.

## See also

- [`AudioChannel`](AudioChannel.md)
- [`AudioControls`](AudioControls.md)
- [`range-control`](../HTMShell.Elements/range-control.md)
