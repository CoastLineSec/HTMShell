# Built-in elements

HTMShell's built-in element model is experimental. It attaches a small amount
of validated shell behavior to ordinary HTML elements; it does not replace
HTML with another component language and does not load executable code.

Declarations use `data-htm-element` and require a normal, nonempty HTML `id`.
The ID is stable within one parsed document. A live element identity combines
that ID with the document generation, so the same author-provided ID on two
outputs still names two independent runtime elements.

The initial registry contains exactly `state-text` and `action-button`. It is
compiled into the runtime, immutable, and independent of the compositor and
output.

## State text

A state-text declaration binds a `span`, `p`, or `output` element to one
approved host-provided display string:

```html
<span
  id="overlay-status"
  data-htm-element="state-text"
  data-htm-bind="overlay.status">
</span>
```

The host updates the element's text through the existing incremental document
mutation path. The element itself retains its identity, and the document is
not reparsed. State-text content must not contain child elements, and there is
no formatting or expression language.

Supported binding keys are:

| Key | Value supplied by the host |
| --- | --- |
| `output.label` | Session-local output diagnostic label |
| `output.scale` | Effective compositor-provided presentation scale |
| `surface.template_id` | Manifest surface-template ID |
| `overlay.status` | Output-local open or closed state |
| `overlay.activation_count` | Output-local activation count |
| `shell.last_action` | Output-local description of the last shell action |

Output labels are diagnostics, not persistent output identities. All values
are final strings supplied by the host; dotted keys are fixed enum values, not
arbitrary property paths.

## Action button

An action-button declaration must use a `button` element:

```html
<button
  id="overlay-toggle"
  type="button"
  data-htm-element="action-button"
  data-htm-action="overlay.toggle">
  Toggle overlay
</button>
```

Supported actions and their permitted sources are:

| Action | Panel | Overlay |
| --- | ---: | ---: |
| `overlay.toggle` | Yes | No |
| `overlay.close` | No | Yes |
| `overlay.activate` | No | Yes |

Actions are typed during initialization and rechecked against the live surface
when dispatched. They cannot name host methods, commands, files, or network
resources. State and actions are scoped to the output that owns the surface.

Activation uses standard Wayland pointer events and logical runtime hit
testing. A press must begin on an enabled action button and its release must
resolve to the same live element. Pointer leave, surface unmap, output removal,
or pointer-capability loss cancels a pending activation. Descendant content
inside a button remains part of the owning button's hit area. The standard
HTML `disabled` attribute prevents dispatch.

## Validation

Declarations are discovered once after the HTML document is parsed. Document
initialization rejects:

- unknown built-in element names;
- missing or duplicate registered IDs;
- a declaration on an unsupported HTML tag;
- missing or unknown binding keys and actions;
- actions used from an unauthorized surface kind;
- conflicting or unknown `data-htm-*` behavior attributes;
- state-text elements containing child elements.

Other `data-*` attributes and documents with no built-in declarations remain
valid. Configure, scale, pointer, state, and overlay map changes use the saved
document-owned index and never rescan source text or the DOM.

## Styling and current limits

Built-in behavior injects no colors, layout, or materials. Authors use normal
tag, ID, class, attribute, `:hover`, `:active`, and `[disabled]` CSS selectors.
Layout and hit geometry remain logical at scale 1 and fractional scales.

The current model has no JavaScript, expressions, templates, dynamic
components, event propagation framework, timers, services, component packages,
or user-defined registry entries. It is a narrow static shell-composition
experiment, not a general widget system.
