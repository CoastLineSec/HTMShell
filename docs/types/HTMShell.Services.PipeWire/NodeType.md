# `NodeType`

**Module:** `HTMShell.Services.PipeWire`
**Kind:** Finite node classification

`item.node_type` provides canonical text and a lowercase CSS token.

| Text | Token |
| --- | --- |
| Untracked | `untracked` |
| Audio | `audio` |
| Video | `video` |
| Stream | `stream` |
| Source | `source` |
| Sink | `sink` |
| Audio sink | `audio-sink` |
| Audio source | `audio-source` |
| Audio duplex | `audio-duplex` |
| Audio output stream | `audio-output-stream` |
| Audio input stream | `audio-input-stream` |
| Video source | `video-source` |
| Video sink | `video-sink` |
| Unknown | `unknown` |

Exact PipeWire media classes produce the documented composite values. Unsupported combinations remain `unknown`; distinct known combinations are not collapsed.

## Usage

```css
.node[data-htm-state="audio-sink"] {
  border-color: #78d6ad;
}
```

## See also

- [`Node`](Node.md)
