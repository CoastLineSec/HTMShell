# Built-in elements

HTMShell's built-in element model is experimental. It attaches a small amount
of validated shell behavior to ordinary HTML elements; it does not replace
HTML with another component language and does not load executable code.

Declarations use `data-htm-element` and require a normal, nonempty HTML `id`.
The ID is stable within one parsed document. A live element identity combines
that ID with the document generation, so the same author-provided ID on two
outputs still names two independent runtime elements.

The registry contains exactly `state-text`, `action-button`, and `state-token`.
It is compiled into the runtime, immutable, and independent of the compositor
and output.

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
| `clock.time` | Process-scoped local time in fixed zero-padded `HH:mm` form |
| `battery.percentage` | Process-scoped aggregate percentage, or `—` when unknown |
| `battery.status` | Process-scoped aggregate availability and charge state |
| `output.label` | Session-local output diagnostic label |
| `output.scale` | Effective compositor-provided presentation scale |
| `surface.template_id` | Manifest surface-template ID |
| `overlay.status` | Output-local open or closed state |
| `overlay.activation_count` | Output-local activation count |
| `shell.last_action` | Output-local description of the last shell action |

Output labels are diagnostics, not persistent output identities. All values
are final strings supplied by the host; dotted keys are fixed enum values, not
arbitrary property paths.

### Clock state

`clock.time` is the first service-backed state key. It uses the existing
`state-text` element:

```html
<span
  id="clock"
  data-htm-element="state-text"
  data-htm-bind="clock.time">
</span>
```

One process-level scheduler samples local civil time and publishes one
immutable display value to every document that binds this key. It updates at
the next visible minute change and remains blocked between deadlines; there is
no timer per output, surface, or element. The format is fixed to zero-padded
24-hour `HH:mm`. Seconds, custom format strings, and author-created timers are
not supported.

If the system time zone cannot be discovered, the host uses UTC and reports
that fallback diagnostically. Live time-zone reconfiguration remains
experimental and is not guaranteed.

### Battery state

Battery state is process-scoped, read-only, and sourced from UPower's aggregate
display device on the system bus. HTMShell does not enumerate batteries,
interpret sysfs power-supply data, or poll battery state. One event-driven
source serves every subscribed document.

```html
<div
  id="battery-state"
  data-htm-element="state-token"
  data-htm-bind="battery.status">
  <span
    id="battery-percentage"
    data-htm-element="state-text"
    data-htm-bind="battery.percentage"></span>
  <span
    id="battery-warning"
    data-htm-element="state-token"
    data-htm-bind="battery.warning"></span>
</div>
```

`battery.percentage` displays a rounded whole percentage such as `78%`.
Unknown, absent, and unavailable values display `—`. `battery.status` text is
one of `Battery unavailable`, `No battery`, `Battery`, `Charging`,
`Discharging`, `Empty`, `Fully charged`, `Pending charge`, or
`Pending discharge`.

No battery is a valid `absent` state and is distinct from an `unavailable`
UPower service. Service absence does not prevent the shell from starting, and
owner and property signals update state without a periodic polling loop.
Battery controls, per-device state, health, charge thresholds, and remaining
time are unsupported.

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

## State token

A state-token declaration projects one approved finite state into the
runtime-owned `data-htm-state` attribute:

```html
<span
  id="overlay-indicator"
  class="status-dot"
  data-htm-element="state-token"
  data-htm-bind="overlay.status">
</span>
```

State tokens require a stable ID and may use only `div`, `span`, or `section`.
The approved token bindings and their complete domains are:

| Key | Scope | Token domain |
| --- | --- | --- |
| `overlay.status` | Output | `open`, `closed` |
| `surface.scale_profile` | Surface | `scale-1`, `fractional` |
| `battery.status` | Process | `unavailable`, `absent`, `unknown`, `charging`, `discharging`, `empty`, `full`, `pending-charge`, `pending-discharge` |
| `battery.warning` | Process | `unknown`, `none`, `discharging`, `low`, `critical`, `action` |

`surface.scale_profile` describes the effective presentation profile, not the
exact fractional numerator. A change between fractional numerators therefore
does not change this token.

HTMShell alone writes `data-htm-state`; author-provided values are rejected.
Tokens are typed enum values, never arbitrary state strings, whitespace token
lists, expressions, class names, or CSS declarations. Author classes and
unrelated attributes remain unchanged. Token changes use the same incremental
document-mutation boundary as text bindings and preserve the element identity
and one-time declaration index.

Appearance remains ordinary CSS:

```css
#overlay-indicator[data-htm-state="open"] {
  opacity: 1;
}

#overlay-indicator[data-htm-state="closed"] {
  opacity: 0.5;
}
```

An action button may contain images, labels, and state-token descendants.
Those descendants remain within the owning button's normal hit area; no
separate icon-button kind or generalized event propagation system is added.

## Validation

Declarations are discovered once after the HTML document is parsed. Document
initialization rejects:

- unknown built-in element names;
- missing or duplicate registered IDs;
- a declaration on an unsupported HTML tag;
- missing or unknown binding keys and actions;
- text-only keys used as token bindings, or tokens on unsupported tags;
- author-provided `data-htm-state` attributes;
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

The current model has no JavaScript, expressions, arbitrary attribute/class/
style bindings, templates, dynamic components, event propagation framework,
author-defined timers, service plugins, component packages, or user-defined
registry entries. The clock and UPower battery source are narrow native state
sources, not a general service or widget framework.
