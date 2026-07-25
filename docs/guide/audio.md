# PipeWire audio

HTMShell presents PipeWire nodes, default audio relationships, volume, mute
state, channels, and the read-only link graph. One process connection serves
every output.

## Service state

`pipewire.availability` is `unavailable`, `synchronizing`, or `ready`. `pipewire.ready` becomes true after initial graph synchronization. `pipewire.node_count` is the number of published nodes.

PipeWire absence does not stop the shell. Reconnection clears the old generation before publishing fresh node identities.

## Audio nodes

Use the `pipewire.nodes` repeat source:

```html
<template id="audio-nodes"
          data-htm-element="repeat"
          data-htm-source="pipewire.nodes">
  <article>
    <span data-htm-element="state-text"
          data-htm-local-id="description"
          data-htm-bind="item.description"></span>
    <data data-htm-element="state-value"
          data-htm-local-id="volume"
          data-htm-bind="item.volume"
          data-htm-format="percent"></data>
    <span data-htm-element="state-token"
          data-htm-local-id="mute"
          data-htm-bind="item.mute_state"></span>
  </article>
</template>
```

`item.audio_status` is `unsupported`, `unavailable`, or `ready`. An unsupported node is not audio capable. An unavailable audio node has not supplied complete authoritative audio parameters.

`item.volume` is a perceptual average where `1.0` is ordinary 100 percent volume. Values above `1.0` are preserved. `item.mute_state` is `muted`, `unmuted`, or `unavailable`.

`item.can_set_volume` and `item.can_set_mute` describe write capability. Readable state does not imply permission to control the node.

Application streams, sinks, sources, and virtual audio nodes use the same bindings.

## Item-local controls

Controls inside `pipewire.nodes` target the current keyed item:

```html
<input type="range"
       data-htm-element="range-control"
       data-htm-local-id="volume-control"
       data-htm-bind="item.volume"
       data-htm-action="pipewire.audio.set_volume"
       data-htm-enabled-bind="item.can_set_volume"
       min="0"
       max="1"
       step="0.01">

<button data-htm-element="action-button"
        data-htm-local-id="mute-control"
        data-htm-action="pipewire.audio.toggle_mute"
        data-htm-enabled-bind="item.can_set_mute">
  Toggle mute
</button>
```

The mute actions are `pipewire.audio.mute`, `pipewire.audio.unmute`, and `pipewire.audio.toggle_mute`. They do not accept a target inside the node repeat. The keyed item identity is captured for dispatch. Raw PipeWire IDs are never targets.

## Default controls

Actual default sink and source controls are allowed outside a repeat:

```html
<input id="output-volume"
       type="range"
       data-htm-element="range-control"
       data-htm-bind="pipewire.default_sink.volume"
       data-htm-action="pipewire.audio.set_volume"
       data-htm-target="pipewire.default_sink"
       data-htm-enabled-bind="pipewire.default_sink.can_set_volume"
       min="0"
       max="1"
       step="0.01">

<button id="output-mute"
        data-htm-element="action-button"
        data-htm-action="pipewire.audio.toggle_mute"
        data-htm-target="pipewire.default_sink"
        data-htm-enabled-bind="pipewire.default_sink.can_set_mute">
  Toggle default output
</button>
```

`pipewire.default_source` is the other writable target. Configured defaults expose read-only audio state but cannot be targeted.

## Volume behavior

The range defaults are `min="0"`, `max="1"`, and `step="0.01"`. The runtime maximum is `2.0`. A value above `1.0` is allowed only when the author explicitly sets a larger `max`.

Amplification can clip or distort. The runtime never enables it through an omitted bound and never rewrites an externally amplified value merely because a control has a lower visual maximum.

Average-volume writes preserve the current channel balance.

## Ordered channels

`item.channel_status` is `unsupported`, `unavailable`, or `ready`.
`item.channel_count` is zero until an authoritative channel vector is ready.

One contextual repeat may appear inside `pipewire.nodes`:

```html
<template data-htm-element="repeat"
          data-htm-source="item.channels">
  <div class="channel">
    <span data-htm-element="state-text"
          data-htm-local-id="position"
          data-htm-bind="item.position_name"></span>
    <data data-htm-element="state-value"
          data-htm-local-id="index"
          data-htm-bind="item.index"></data>
    <data data-htm-element="state-value"
          data-htm-local-id="volume"
          data-htm-bind="item.volume"
          data-htm-format="percent"></data>
    <input type="range"
           data-htm-element="range-control"
           data-htm-local-id="control"
           data-htm-bind="item.volume"
           data-htm-action="pipewire.audio.set_channel_volume"
           data-htm-enabled-bind="item.can_set_volume"
           min="0"
           max="1"
           step="0.01">
  </div>
</template>
```

Inside `item.channels`, `item.*` means the current channel. It shadows the
outer node item. The parent node is the implicit control target; there is no
`parent.*`, selector, index target, or traversal syntax. Put node fields outside
the contextual repeat.

Channel order is the authoritative PipeWire vector order. `item.index` is a
zero-based, layout-local label and is not an identity or action target.
`item.position` supplies a stable CSS token and `item.position_name` supplies
human-readable text. `item.status`, `item.can_set_volume`,
`item.is_auxiliary`, and `item.is_custom` are finite state bindings.

Named SPA positions remain distinct. Auxiliary values use `aux-1` through
`aux-4096`. Custom values use `custom-1` through `custom-4294901760`.
Unrecognized values use `unknown`.

If a channel map is absent, HTMShell uses mono, stereo, three-channel, quad,
5.1, 7-channel, and 7.1 layouts for vectors of one through eight channels.
Larger vectors use `unknown` positions. A short position vector is extended
with `unknown`; a long one is ignored past the volume-vector length. Duplicate
positions remain separate ordered items.

A volume-only update preserves channel identity. Count, order, position, or
fallback-layout changes replace that node's channel-layout generation. A
layout replacement cancels stale controls.

## Average and channel writes

`pipewire.audio.set_channel_volume` is valid only on `range-control` inside
`item.channels`. It has no explicit `data-htm-target`. The write starts from the
latest coordinated full vector, replaces one channel, and sends the entire
vector. Other channels keep their values.

Average and channel controls share one node coordinator. An average intent
scales the latest desired vector. A later channel intent replaces only that
channel. A later average intent scales the vector including earlier channel
changes. Only the latest complete vector is retained, so pointer motion does
not create an unbounded queue.

Public channel volume remains authoritative PipeWire state. Confirmation,
failure, timeout, removal, layout replacement, and reconnect use the same
`idle`, `pending`, `failed`, and `unavailable` control states as node-average
controls. The default range remains `0` to `1` with step `0.01`; an explicit
larger `max`, up to the runtime maximum `2.0`, is required for amplification.

## Confirmation and failures

PipeWire state is authoritative. A range thumb can follow the pointer locally, but bound volume changes only after PipeWire reports the new value.

Each control owns `data-htm-state`:

- `idle`: controllable with no outstanding request
- `pending`: waiting for authoritative confirmation
- `failed`: the latest request failed or timed out
- `unavailable`: the target is missing, stale, unsupported, or not writable

Pointer motion retains only the latest desired volume. Writes are bounded to one active volume operation and one active mute operation per node. Denial, timeout, node removal, document replacement, and reconnect cannot confirm stale controls.

Queued volume writes are spaced by at least 16 milliseconds.

External volume and mute changes update idle controls. A failed range returns to the latest authoritative value.

## Defaults and exact properties

Actual defaults describe current session policy. Configured defaults describe stored preferences. Each relationship can be `unavailable`, `unresolved`, or `available`.

Inside `pipewire.nodes`, `item.property` reads one static key:

```html
<span data-htm-element="state-text"
      data-htm-local-id="application"
      data-htm-bind="item.property"
      data-htm-property-key="application.name"></span>
```

Common keys include `application.name`, `application.icon-name`, `media.name`, `media.title`, and `media.artist`. Presence is not guaranteed.

## Links and grouped connections

`pipewire.link_count` and `pipewire.link_group_count` report the authoritative
collection sizes after synchronization. Repeat over individual port links with
`pipewire.links`:

```html
<template id="links"
          data-htm-element="repeat"
          data-htm-source="pipewire.links">
  <div>
    <span data-htm-element="state-text"
          data-htm-local-id="source"
          data-htm-bind="item.source.description"></span>
    <span data-htm-element="state-token"
          data-htm-local-id="state"
          data-htm-bind="item.state"></span>
    <span data-htm-element="state-text"
          data-htm-local-id="target"
          data-htm-bind="item.target.description"></span>
  </div>
</template>
```

Each link represents one source port to one target port. Stereo routing
usually has two links. Link readiness is `unavailable`, `partial`, or `ready`.
The seven PipeWire states are `error`, `unlinked`, `init`, `negotiating`,
`allocating`, `paused`, and `active`; unrecognized future states are
`unknown`.

Source and target relations support only the documented fields. Relation
status is `available`, `unresolved`, or `unavailable`. A missing endpoint does
not hide a link. Endpoint labels can update while the link clone retains its
identity. Relation paths are not expressions, cannot be chained, and cannot be
used as action targets.

Raw link, node, and port IDs are session-local diagnostics. They are not DOM
identities, persistent references, joins, or action targets.

`pipewire.link_groups` groups links with the same source and target node:

```html
<template id="groups"
          data-htm-element="repeat"
          data-htm-source="pipewire.link_groups">
  <article>
    <data data-htm-element="state-value"
          data-htm-local-id="members"
          data-htm-bind="item.member_count"></data>
    <template data-htm-element="repeat"
              data-htm-source="item.links">
      <div>
        <span data-htm-element="state-token"
              data-htm-local-id="member-state"
              data-htm-bind="item.state"></span>
      </div>
    </template>
  </article>
</template>
```

The group state is the state of one retained representative link, not a
best-state, worst-state, or all-channel aggregate. The representative remains
until that link disappears. Its replacement is the lowest remaining
session-local link ID. Group identity depends only on the connection
generation and source-target pair.

Top-level links and groups include monitor routes. `item.is_monitor` is true
when the target node has PipeWire media category `Monitor` or `Manager`.

## Node connection tracking

Inside `pipewire.nodes`, `item.link_group_count`,
`item.link_group_status`, and contextual `item.link_groups` provide the
HTMShell equivalent of a node link tracker:

```html
<template data-htm-element="repeat"
          data-htm-source="item.link_groups">
  <div>
    <span data-htm-element="state-token"
          data-htm-local-id="direction"
          data-htm-bind="item.connection_direction"></span>
    <span data-htm-element="state-text"
          data-htm-local-id="peer"
          data-htm-bind="item.peer.description"></span>
  </div>
</template>
```

For a sink node, tracking selects incoming groups. For every other node type,
including source and untracked nodes, it selects outgoing groups. A
bidirectional node is a sink for this selection rule. Self-links use `self`;
other direction tokens are `incoming`, `outgoing`, and `unknown`.

Node tracking excludes groups whose target is a PipeWire monitor or manager
node. The complete top-level collections still include those groups.
`item.peer.*` resolves the endpoint opposite the tracked node.

`item.channels`, group `item.links`, and node `item.link_groups` are sibling
forms of one contextual level. The inner `item.*` shadows the outer scope.
They cannot be nested inside one another and there is no `parent.*`, `../`,
selector, interpolation, or arbitrary relation syntax.

## Demand and limits

Documents activate only the PipeWire state they consume. Audio parameter subscriptions and write coordinators are shared process-wide. Removing the final audio consumer releases audio demand. Closed retained overlays may update without receiving a frame.

Channel projection and channel-write demand are tracked separately. Several
contextual repeats and outputs share one normalized channel vector per node.
Removing the last channel consumer releases public channel projection when no
average-volume consumer still needs the internal vector.

Link collection, link detail, group collection, member projection, node
tracking, and relation projection are separate demand classes. Documents and
outputs share the process graph. Removing the last graph consumer releases
link proxies and graph-detail demand without disturbing independent audio or
channel consumers.

Limits include:

- 16 `pipewire.nodes` repeats per document
- 16 `pipewire.links` repeats per document
- 16 `pipewire.link_groups` repeats per document
- 64 registered PipeWire bindings per repeated item
- 32 property lookups per item
- 128 PipeWire audio controls per document
- 16 PipeWire audio controls per repeated item
- 64 range controls per document
- 8 range controls per repeated item
- 8 `item.channels` repeats per outer node template
- 32 contextual repeats per document
- 8 `item.links` repeats per group template
- 8 `item.link_groups` repeats per node template
- 32 contextual graph repeats per document
- 64 relation bindings per item
- 16,384 public links and 4,096 public link groups process-wide
- 64 public channels per node
- 64 registered bindings per channel item
- 8 channel range controls per channel item
- 256 channel range controls per document
- exactly one contextual repeat level
- 4,096 node write coordinators, bounded by the graph limit
- one pending mute intent and one pending volume intent per node
- a two-second confirmation timeout

## Current limitations

Channel mute, channel-map writes, preferred-default writes, link mutation,
peak monitoring, stream movement, arbitrary graph queries, and spatial graph
rendering are not available.

See the tracked [audio inspector dashboard](../../examples/audio-inspector/shell.json).
