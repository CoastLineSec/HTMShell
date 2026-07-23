# `state-value`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element

`state-value` presents typed numeric state in a semantic `data` element.

## Usage

```html
<data id="battery-energy"
      data-htm-element="state-value"
      data-htm-bind="battery.energy"
      data-htm-format="energy"></data>
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tag | `data` |
| `id` | Required outside repeats |
| `data-htm-local-id` | Required inside repeats |
| `data-htm-bind` | Required typed numeric key |
| `data-htm-format` | Optional, defaults to `raw` |
| `value` | Runtime-owned |

Authors cannot provide `value`. The runtime updates text and `value` incrementally. Unknown values display the unknown marker and have no `value`.

Formats are:

| Format | Display |
| --- | --- |
| `raw` | Canonical number without a unit |
| `percent` | Rounded whole number plus `%` |
| `energy` | One decimal place plus `Wh` |
| `power` | One decimal place plus `W` |
| `duration` | Compact seconds, minutes, hours, or days |

The binding determines which formats are valid. Negative durations, nonfinite numbers, and out-of-domain percentages are rejected.

## See also

- [`repeat`](repeat.md)
- [Display device](../HTMShell.Services.UPower/DisplayDevice.md)
