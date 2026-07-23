# `HTMShell.Elements`

The `HTMShell.Elements` module contains six immutable built-in behavior declarations.

## Elements

- [`state-text`](state-text.md): Projects typed state into text content.
- [`state-token`](state-token.md): Projects finite state into `data-htm-state`.
- [`action-button`](action-button.md): Dispatches one approved pointer action.
- [`clock-text`](clock-text.md): Formats one wall-clock instant in a semantic `time` element.
- [`state-value`](state-value.md): Projects a numeric value into text and a `value` attribute.
- [`repeat`](repeat.md): Expands a keyed service collection from an inert template.

Declarations use ordinary HTML. Registered descendants inside a repeat use `data-htm-local-id` instead of `id`. Unknown HTMShell behavior attributes are rejected.

User-defined kinds, dynamic loading, nested repetition, scripting, and a general event model are unavailable.

See [state and actions](../../guide/state-and-actions.md).
