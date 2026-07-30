# `HTMShell.Component.Input`

**Kind:** Literal component value | **Status:** Experimental

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

Each declaration contains exactly `name`, `type`, and either `required: true` or `default`. `required: false` is valid only with a default. A required input cannot have a default. Unknown declaration fields, duplicate names, unsupported types, and invalid defaults reject the complete package candidate.

## Name

An input name contains 1 through 64 lowercase ASCII bytes. It starts with a lowercase letter; the remaining characters are lowercase letters, digits, or single interior hyphens. It cannot end with a hyphen or contain consecutive hyphens, dots, whitespace, or uppercase letters.

These names are reserved:

```text
component slot id class style input state action service resource repeat surface host
```

## Types

Exactly six literal types are available:

| Type | Manifest default | Invocation literal | Normalization |
| --- | --- | --- | --- |
| `string` | JSON string | HTML-decoded attribute text | Preserves Unicode, casing, and whitespace; maximum 4,096 UTF-8 bytes |
| `number` | JSON number | Complete finite decimal | Normalizes negative zero; no units or arithmetic |
| `boolean` | JSON boolean | Exactly `true` or `false` | Lowercase explicit values only |
| `token` | JSON string | One existing state token | Uses the existing canonical state-token vocabulary |
| `color` | JSON string | Context-free CSS color | Resolves to canonical encoded-sRGB RGBA; no `currentColor` or variables |
| `length` | JSON string | Finite `px` length or unitless zero | Normalizes to logical px; no percentages or context-dependent units |

State-reference, action-reference, and resource-reference inputs are not supported. Strings that resemble references remain literal strings and acquire no binding behavior.

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

`component` remains required. Every other attribute must be an `input-*` attribute matching one declared input. Unprefixed inputs, undeclared inputs, duplicate attributes, presence-only booleans, `id`, `class`, `style`, `slot`, and arbitrary host attributes are invalid. One use supplies at most 64 input attributes and 16 KiB of literal attribute bytes.

Renderable invocation children are accepted only when they route to a declared default or named slot. Inputs and projection remain separate contracts. See [slots](Slot.md).

## Required values and defaults

Required values must be present at every use. Defaults are parsed and normalized while the package candidate is built. An invalid default, missing required value, unknown input, or invalid supplied literal rejects the complete candidate before a surface or renderer observes it.

Resolved instance maps preserve declaration order and are immutable. A defaulted value and an explicitly supplied equivalent value produce the same semantic input version. Invocation attribute order and raw equivalent spellings do not affect that version. Input values do not define component instance, descendant DOM, or scene identity.

## Local visibility

The nearest component host exposes its map through:

```text
input.<name>
```

This namespace is instance-local and does not exist in root documents. A nested component receives only its own declared literals and defaults; it does not inherit or discover parent or sibling inputs.

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

Text nodes, ordinary attributes, and CSS are not scanned for placeholders. String substitution, interpolation, expressions, implicit input forwarding, component-local IDs, repeat integration, and hot reload are not supported. Component stylesheets are static and do not interpolate input values.

## Limits

| Unit | Limit |
| --- | ---: |
| Input declarations per component | 64 |
| Supplied inputs per invocation | 64 |
| Input name bytes | 64 |
| String input bytes | 4,096 |
| Supplied literal bytes per invocation | 16 KiB |
