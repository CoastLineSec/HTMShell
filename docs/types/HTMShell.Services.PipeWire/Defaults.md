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

The complete keys are:

`pipewire.default_sink.status`, `pipewire.default_sink.name`, `pipewire.default_sink.nickname`, `pipewire.default_sink.description`, `pipewire.default_sink.media_class`, `pipewire.default_sink.raw_id`.

`pipewire.default_source.status`, `pipewire.default_source.name`, `pipewire.default_source.nickname`, `pipewire.default_source.description`, `pipewire.default_source.media_class`, `pipewire.default_source.raw_id`.

`pipewire.configured_sink.status`, `pipewire.configured_sink.name`, `pipewire.configured_sink.nickname`, `pipewire.configured_sink.description`, `pipewire.configured_sink.media_class`, `pipewire.configured_sink.raw_id`.

`pipewire.configured_source.status`, `pipewire.configured_source.name`, `pipewire.configured_source.nickname`, `pipewire.configured_source.description`, `pipewire.configured_source.media_class`, `pipewire.configured_source.raw_id`.

## Status

- `unavailable`: PipeWire is not ready or default metadata is absent.
- `unresolved`: metadata does not currently resolve to a live node.
- `available`: metadata resolves to a current node.

Missing default metadata does not make the node graph unavailable. Raw IDs are session-local diagnostics.

## See also

- [`Node`](Node.md)
