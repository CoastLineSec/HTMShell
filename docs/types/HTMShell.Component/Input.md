# `HTMShell.Component.Input`

**Kind:** Typed component value | **Status:** Experimental

A schema version 2 component export may declare at most 64 ordered inputs:

```json
{
  "name": "status-card",
  "source": "components/status-card.html",
  "inputs": [
    {
      "name": "label",
      "type": "string",
      "required": true
    },
    {
      "name": "count",
      "type": "number",
      "default": 0
    },
    {
      "name": "enabled",
      "type": "boolean",
      "default": true
    }
  ]
}
```

Literal declarations contain exactly `name`, `type`, and either `required: true` or `default`. `required: false` is valid only with a default. A required literal input cannot have a default. Resource-reference declarations use the additional `resourceTypes` field and are always required. Unknown declaration fields, duplicate names, unsupported types, and invalid defaults reject the complete package candidate.

## Name

An input name contains 1 through 64 lowercase ASCII bytes. It starts with a lowercase letter; the remaining characters are lowercase letters, digits, or single interior hyphens. It cannot end with a hyphen or contain consecutive hyphens, dots, whitespace, or uppercase letters.

These names are reserved:

```text
component slot id class style input state action service resource repeat surface host
```

## Types

Six literal types are available:

| Type | Manifest default | Invocation literal | Normalization |
| --- | --- | --- | --- |
| `string` | JSON string | HTML-decoded attribute text | Preserves Unicode, casing, and whitespace; maximum 4,096 UTF-8 bytes |
| `number` | JSON number | Complete finite decimal | Normalizes negative zero; no units or arithmetic |
| `boolean` | JSON boolean | Exactly `true` or `false` | Lowercase explicit values only |
| `token` | JSON string | One existing state token | Uses the existing canonical state-token vocabulary |
| `color` | JSON string | Context-free CSS color | Resolves to canonical encoded-sRGB RGBA; no `currentColor` or variables |
| `length` | JSON string | Finite `px` length or unitless zero | Normalizes to logical px; no percentages or context-dependent units |

The seventh type is `resource-reference`. It carries one immutable caller-owned raster or simple SVG source. Its declaration requires a nonempty unique `resourceTypes` set containing `raster`, `svg`, or both, plus `required: true`:

```json
{
  "name": "icon",
  "type": "resource-reference",
  "resourceTypes": ["raster", "svg"],
  "required": true
}
```

Resource-reference inputs have no optional, null, or default form. See [`HTMShell.Component.ResourceReferenceInput`](ResourceReferenceInput.md) for assignment, forwarding, image consumption, ownership, and limits. State-reference and action-reference inputs are not supported. A literal string that resembles `resource:name` or `input:name` remains a string when the target declaration has a literal type.

## Invocation

Pass values with `input-<name>`:

```html
<htm-use
  component="controls.status-card"
  input-label="Connected"
  input-count="3"
  input-enabled="false">
</htm-use>
```

`component` remains required. Every other attribute must be an `input-*` attribute matching one declared input. A resource-reference target accepts a direct caller-owned `resource:name` assignment or static `input:name` forwarding. Unprefixed inputs, undeclared inputs, duplicate attributes, presence-only booleans, `id`, `class`, `style`, `slot`, and arbitrary host attributes are invalid. One use supplies at most 64 input attributes and 16 KiB of literal attribute bytes.

Renderable invocation children are accepted only when they route to a declared default or named slot. Inputs and projection remain separate contracts. See [slots](Slot.md).

## Required values and defaults

Required values must be present at every use. Literal defaults are parsed and normalized while the package candidate is built. Resource-reference defaults are forbidden. An invalid default, missing required value, unknown input, invalid supplied literal, unresolved resource, incompatible resource kind, or invalid forwarding relation rejects the complete candidate before a surface or renderer observes it.

Resolved instance maps preserve declaration order and are immutable. A defaulted literal and an explicitly supplied equivalent literal produce the same semantic input version. Invocation attribute order and raw equivalent spellings do not affect that version. Resource-reference assignments and forwarding hops have distinct generation-safe value identities while sharing the underlying neutral source. Input values do not define component instance, descendant DOM, or scene identity.

## Local visibility

The nearest component host exposes its map through:

```text
input.<name>
```

This namespace is instance-local and does not exist in root documents. A nested component receives only explicitly assigned values. It does not inherit or discover parent or sibling inputs. A component may statically forward a resource-reference value with `input-child="input:parent"` when the parent accepted-kind set is a subset of the child set.

Three existing display declarations can consume compatible local values:

| Consumer | Compatible input types |
| --- | --- |
| `state-text` | `string`, `number`, `boolean`, `token`, `color`, `length` |
| `state-token` | `token`, `boolean` |
| `state-value` | `number`, raw format only |

Example:

```html
<template data-htm-component="status-card">
  <article>
    <span data-htm-element="state-text" data-htm-bind="input.label"></span>
    <data data-htm-element="state-value" data-htm-bind="input.count"></data>
    <span data-htm-element="state-token" data-htm-bind="input.enabled"></span>
  </article>
</template>
```

These consumers resolve from immutable host-local data. They create no process-global state key, state subscription, action lookup, resource lookup, native-service demand, thread, timer, or renderer-specific state.

Resource-reference values have one separate consumer:

```html
<img src="input:icon" alt="">
```

Only component-owned or component-fallback HTML `<img src>` accepts that form. The binding is resolved before publication and does not enter the ordinary URL loader. Other elements, ordinary attributes, SVG image references, `srcset`, and CSS cannot consume it.

Text nodes, ordinary attributes, and CSS are not scanned for placeholders. String substitution, interpolation, expressions, implicit input forwarding, component-local IDs, repeat integration, and hot reload are not supported. Component stylesheets are static and do not interpolate input values.

## Limits

| Unit | Limit |
| --- | ---: |
| Input declarations per component | 64 |
| Supplied inputs per invocation | 64 |
| Input name bytes | 64 |
| String input bytes | 4,096 |
| Supplied literal bytes per invocation | 16 KiB |
| Resource-reference kinds per declaration | 2 |
| Concrete resource-reference values per prepared root | 16,384 |
