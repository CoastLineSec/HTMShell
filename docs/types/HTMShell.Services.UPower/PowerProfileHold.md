# `PowerProfileHold`

**Module:** `HTMShell.Services.UPower` | **Kind:** Keyed collection item | **Scope:** Process

`power_profile.holds` contains active Power Profiles holds.

## Item bindings

| Binding | Presentation |
| --- | --- |
| `item.profile` | Text and profile token |
| `item.application_id` | Text |
| `item.reason` | Text |

## Usage

```html
<template id="hold"
          data-htm-element="repeat"
          data-htm-source="power_profile.holds">
  <p class="hold">
    <span data-htm-element="state-text"
          data-htm-local-id="application"
          data-htm-bind="item.application_id"></span>
    <span data-htm-element="state-text"
          data-htm-local-id="reason"
          data-htm-bind="item.reason"></span>
  </p>
</template>
```

Items are ordered by profile, application ID, reason, then duplicate occurrence. The key also includes the service generation. Identical duplicate holds remain distinct, but their occurrence identity can shift when an indistinguishable duplicate disappears because the service exposes no stable cookie.

The source limit is 128 holds. Service loss empties the collection. HTMShell does not create or release holds.

## See also

- [`PowerProfile`](PowerProfile.md)
- [`repeat`](../HTMShell.Elements/repeat.md)
