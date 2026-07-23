# ADR 0006: Built-in element registry

## Status

Accepted for the experimental portable shell host. The registry and runtime
interfaces remain unstable.

## Context

The portable host can create manifest-driven panel and overlay surfaces on
each output, keep their documents alive across presentation changes, and
mutate those documents from host-controlled state. Static shell composition
also needs a narrow way for authored elements to display that state and invoke
known shell actions without introducing JavaScript or a second UI language.

## Decision

HTML and CSS remain HTMShell's shell authoring languages. Initial behavior is
attached to ordinary semantic HTML through validated `data-htm-*`
declarations.

The built-in registry is compile-time defined, immutable after initialization,
and independent of compositor and output identity. Its first profile contains
only:

- state text, which binds an ordinary text container to one typed host state
  key; and
- action buttons, which bind an ordinary `button` to one typed host action.

Every registered element has an explicit HTML ID. Its live identity combines
that ID with the parse-once document generation. Binding updates use the
existing DOM text-mutation path, preserve the element and document identities,
and resolve only targets already present in a document-owned binding index.

State is output-scoped. Actions are validated for their source surface during
document initialization and again at dispatch. Pointer activation uses normal
Wayland events and logical runtime geometry; no custom input protocol or
general browser event propagation model is introduced.

Layout, appearance, hover, active, and disabled styling remain ordinary CSS.
The registry injects no theme or layout.

## Consequences

The portable host can compose a static panel and overlay whose state labels and
buttons remain independent across outputs and presentation scales. Registry
discovery happens once per document generation. Configure changes, scale
changes, overlay role recreation, and state updates do not rebuild the index or
reparse HTML.

The registry deliberately cannot evaluate expressions, invoke arbitrary host
methods, load code, execute commands, or access services. JavaScript is not
required. Dynamic components, a general widget system, service-backed
elements, templates, event bubbling, and component packaging remain deferred.

## Acceptance criteria

- Only the declared state-text and action-button kinds are accepted.
- Element IDs and action/state names are validated and typed.
- Bound text uses incremental mutation without reparsing.
- A valid pointer click dispatches exactly one output-scoped action.
- Disabled, canceled, stale, unauthorized, or closed-surface actions do not
  dispatch.
- Element identity survives text, configure, scale, and overlay-role changes.
- Output replacement creates fresh element generations.
- Unchanged values and unrelated output state schedule no frame.
- Scale-1 and fractional presentation retain logical hit geometry.
- No script engine, dynamic loader, service polling, or compositor-specific
  behavior is introduced.

## Final decision

```text
CONTINUE WITH BUILT-IN ELEMENT MODEL
```
