# `HTMShell.Elements`

The `HTMShell.Elements` module contains four immutable built-in behavior declarations.

## Elements

- [`state-text`](state-text.md): Projects typed state into text content.
- [`state-token`](state-token.md): Projects finite state into `data-htm-state`.
- [`action-button`](action-button.md): Dispatches one approved pointer action.
- [`clock-text`](clock-text.md): Formats one wall-clock instant in a semantic `time` element.

All declarations use ordinary HTML elements and require a unique `id`. Unknown HTMShell behavior attributes are rejected.

User-defined kinds, dynamic loading, templates, scripting, and a general event model are unavailable.

See [state and actions](../../guide/state-and-actions.md).
