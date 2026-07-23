# ADR 0008: Typed visual state

## Status

Accepted for the experimental portable shell host. The authoring and runtime
interfaces remain unstable.

## Context

HTMShell can already bind typed text and actions to ordinary HTML without
JavaScript. A useful panel also needs finite state to affect appearance without
forcing authors to encode visual meaning into display text or requiring the
host to overwrite classes and styles.

The projection must remain narrower than general data binding. HTML and CSS
continue to own document structure, layout, and appearance.

## Decision

HTMShell adds one compile-time built-in kind, `state-token`. A declaration
binds an ordinary `div`, `span`, or `section` with a stable HTML ID to one
approved token-producing state key.

The runtime projects the typed finite value into one reserved
`data-htm-state` attribute. Authors cannot provide or select the target
attribute. Token domains are compile-time defined:

- `overlay.status` produces `open` or `closed`;
- `surface.scale_profile` produces `scale-1` or `fractional`.

CSS remains solely responsible for interpreting these tokens visually through
ordinary attribute selectors. HTMShell does not inject layout, colors,
materials, or styles, and it preserves author classes and unrelated
attributes.

Token targets are discovered once with the existing built-in declarations and
stored in a typed document-owned binding index. Updates use incremental
attribute mutation, preserve element identities, and share the normal style,
layout, paint, and per-surface scheduling pipeline. Text and token projections
from one state change may be applied before one surface frame.

Static panel composition remains ordinary HTML and CSS. Local images and SVG
assets use the existing package-contained resource path. Icon-and-label
controls remain normal `action-button` elements with ordinary descendants.

## Consequences

- output-local overlay state can change panel CSS without affecting another
  output;
- surface-local scale-profile state can differ between mixed-scale surfaces;
- unchanged tokens produce no mutation or redraw;
- configure, scale, and overlay-role changes do not rebuild declaration or
  binding indexes;
- local icons require no native icon registry or service;
- the clock scheduler and existing text/action semantics remain unchanged.

Arbitrary attribute, class, style, expression, and template binding remain
unsupported. Dynamic components, a general reactive system, native service
expansion, a widget framework, themes, and compositor materials remain
deferred.

## Acceptance criteria

- the immutable registry contains only `state-text`, `action-button`, and
  `state-token`;
- state-token accepts only its approved tags, keys, and finite token domains;
- only the runtime writes `data-htm-state`;
- token changes preserve author markup, element identity, parse count, and
  declaration index;
- ordinary CSS observes token changes;
- text and token updates remain surface- and output-isolated;
- local icon-and-label buttons retain existing pointer action behavior;
- scale-1, fractional-scale, output lifecycle, clock, and idle behavior remain
  intact;
- no new native service, timer, dependency, script engine, or general binding
  framework is introduced.

## Final decision

```text
CONTINUE WITH TYPED VISUAL STATE
```
