# `Defaults`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** Process

Default relationships resolve WirePlumber metadata to current PipeWire nodes.

## Relationships

| Relationship | Prefix |
| --- | --- |
| Actual output | `pipewire.default_sink` |
| Actual input | `pipewire.default_source` |
| Configured output | `pipewire.configured_sink` |
| Configured input | `pipewire.configured_source` |

Each prefix provides:

| Suffix | Presentation |
| --- | --- |
| `.status` | text, token |
| `.name` | text |
| `.nickname` | text |
| `.description` | text |
| `.media_class` | text |
| `.raw_id` | numeric |
| `.audio_status` | text, token |
| `.volume` | numeric |
| `.mute_state` | text, token |
| `.can_set_volume` | text, token, Boolean enable binding |
| `.can_set_mute` | text, token, Boolean enable binding |

Configured relationships additionally provide:

| Key | Presentation |
| --- | --- |
| `pipewire.configured_sink.can_clear` | text, token, Boolean enable binding |
| `pipewire.configured_source.can_clear` | text, token, Boolean enable binding |

The complete keys are:

`pipewire.default_sink.status`, `pipewire.default_sink.name`, `pipewire.default_sink.nickname`, `pipewire.default_sink.description`, `pipewire.default_sink.media_class`, `pipewire.default_sink.raw_id`.

`pipewire.default_source.status`, `pipewire.default_source.name`, `pipewire.default_source.nickname`, `pipewire.default_source.description`, `pipewire.default_source.media_class`, `pipewire.default_source.raw_id`.

`pipewire.configured_sink.status`, `pipewire.configured_sink.name`, `pipewire.configured_sink.nickname`, `pipewire.configured_sink.description`, `pipewire.configured_sink.media_class`, `pipewire.configured_sink.raw_id`.

`pipewire.configured_source.status`, `pipewire.configured_source.name`, `pipewire.configured_source.nickname`, `pipewire.configured_source.description`, `pipewire.configured_source.media_class`, `pipewire.configured_source.raw_id`.

Audio keys follow the same prefixes:

`pipewire.default_sink.audio_status`, `pipewire.default_sink.volume`, `pipewire.default_sink.mute_state`, `pipewire.default_sink.can_set_volume`, `pipewire.default_sink.can_set_mute`.

`pipewire.default_source.audio_status`, `pipewire.default_source.volume`, `pipewire.default_source.mute_state`, `pipewire.default_source.can_set_volume`, `pipewire.default_source.can_set_mute`.

`pipewire.configured_sink.audio_status`, `pipewire.configured_sink.volume`, `pipewire.configured_sink.mute_state`, `pipewire.configured_sink.can_set_volume`, `pipewire.configured_sink.can_set_mute`.

`pipewire.configured_source.audio_status`, `pipewire.configured_source.volume`, `pipewire.configured_source.mute_state`, `pipewire.configured_source.can_set_volume`, `pipewire.configured_source.can_set_mute`.

`pipewire.configured_sink.can_clear` and
`pipewire.configured_source.can_clear` report whether the corresponding
configured preference can be removed.

## Status

- `unavailable`: PipeWire is not ready or default metadata is absent.
- `unresolved`: metadata does not currently resolve to a live node.
- `available`: metadata resolves to a current node.

Missing default metadata does not make the node graph unavailable. Raw IDs are session-local diagnostics.

Audio status is `unsupported`, `unavailable`, or `ready`. Mute state is `muted`, `unmuted`, or `unavailable`.

Only actual `pipewire.default_sink` and `pipewire.default_source` relationships
are audio-control targets. Configured relationship audio values are read-only.
Preferred-default actions update configured metadata separately; they never
target these relationship objects or optimistically change actual defaults.

An actual default is the current session-policy result. A configured default
is a stored preference. The two may differ, and changing the configured value
does not guarantee that existing streams move.

## See also

- [`Node`](Node.md)
- [`AudioNode`](AudioNode.md)
- [`AudioControls`](AudioControls.md)
- [`DefaultControls`](DefaultControls.md)
