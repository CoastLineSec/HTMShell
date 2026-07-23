# `Output`

**Module:** `HTMShell.State` | **Kind:** State group | **Scope:** Output

Output state describes the output associated with one surface instance.

## Keys

### `output.label`

**Presentation:** Text

Format:

```text
Output: <diagnostic label>
```

The label comes from the current Wayland output name when available. Otherwise HTMShell creates a session-local label such as `output-global-12`.

> **Warning:** Output labels are diagnostics. They are not persistent output identifiers or manifest selectors.

### `output.scale`

**Presentation:** Text

Format:

```text
Scale: 1.60×
```

The value uses the effective compositor-provided scale and two decimal places. It changes when the surface's effective preferred scale changes.

## Usage

```html
<span id="output"
      data-htm-element="state-text"
      data-htm-bind="output.label"></span>
```

## See also

- [`Surface`](Surface.md)
- [Surfaces guide](../../guide/surfaces.md)
