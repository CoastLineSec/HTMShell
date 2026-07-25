# `AudioChannel`

**Module:** `HTMShell.Services.PipeWire` | **Scope:** `item.channels` item

An audio channel is one ordered entry in its parent node's authoritative volume
vector.

## Bindings

| Key | Presentation | Value |
| --- | --- | --- |
| `item.position_name` | text | Stable English position name |
| `item.position` | text, token | Canonical SPA position |
| `item.index` | numeric | Zero-based vector index |
| `item.volume` | numeric | Authoritative perceptual volume |
| `item.status` | text, token | `unavailable` or `ready` |
| `item.can_set_volume` | text, token, enable binding | `true` or `false` |
| `item.is_auxiliary` | text, token | `true` or `false` |
| `item.is_custom` | text, token | `true` or `false` |

Missing or malformed volume is unavailable, not zero. Readable state and write
permission are separate.

## Named positions

| Name | Token |
| --- | --- |
| Unknown | `unknown` |
| N/A | `na` |
| Mono | `mono` |
| Front left | `front-left` |
| Front right | `front-right` |
| Front center | `front-center` |
| Low frequency effects | `lfe` |
| Side left | `side-left` |
| Side right | `side-right` |
| Front left center | `front-left-center` |
| Front right center | `front-right-center` |
| Rear center | `rear-center` |
| Rear left | `rear-left` |
| Rear right | `rear-right` |
| Top center | `top-center` |
| Top front left | `top-front-left` |
| Top front center | `top-front-center` |
| Top front right | `top-front-right` |
| Top rear left | `top-rear-left` |
| Top rear center | `top-rear-center` |
| Top rear right | `top-rear-right` |
| Rear left center | `rear-left-center` |
| Rear right center | `rear-right-center` |
| Front left wide | `front-left-wide` |
| Front right wide | `front-right-wide` |
| Low frequency effects 2 | `lfe-2` |
| Front left high | `front-left-high` |
| Front center high | `front-center-high` |
| Front right high | `front-right-high` |
| Top front left center | `top-front-left-center` |
| Top front right center | `top-front-right-center` |
| Top side left | `top-side-left` |
| Top side right | `top-side-right` |
| Low frequency effects left | `lfe-left` |
| Low frequency effects right | `lfe-right` |
| Bottom center | `bottom-center` |
| Bottom left center | `bottom-left-center` |
| Bottom right center | `bottom-right-center` |

SPA auxiliary positions use `aux-1` through `aux-4096`. Custom positions use
`custom-1` through `custom-4294901760`. A future code outside those domains
uses `unknown`.

## Identity

Identity includes the PipeWire connection generation, node identity,
channel-layout generation, ordered index, and normalized position. Repeated
positions remain distinct by index.

Volume and mute updates preserve identity. A count, order, position, fallback,
node, or parameter-generation change replaces the channel layout. `item.index`
is diagnostic and cannot target a control.

## See also

- [`Channels`](Channels.md)
- [`ChannelControls`](ChannelControls.md)
- [`Volume`](Volume.md)
