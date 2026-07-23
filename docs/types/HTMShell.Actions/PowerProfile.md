# `PowerProfile`

**Module:** `HTMShell.Actions` | **Kind:** Action group | **Scope:** Process

Power profile actions request one profile from Power Profiles.

## Actions

| Action | Effect |
| --- | --- |
| `power_profile.set_power_saver` | Request power saver |
| `power_profile.set_balanced` | Request balanced |
| `power_profile.set_performance` | Request performance |

Actions may originate from a panel or mapped overlay. They do not accept `data-htm-target`. An unavailable service rejects every request. Performance is also rejected when `power_profile.performance_available` is false.

Selecting the current profile makes no request. One request may be in flight, with at most one latest different request retained. Public state changes only after the service confirms it. Authorization failure, timeout, service loss, and stale replies leave the current state unchanged.

## Usage

```html
<button id="performance"
        data-htm-element="action-button"
        data-htm-action="power_profile.set_performance"
        data-htm-enabled-bind="power_profile.performance_available">
  Performance
</button>
```

HTMShell does not create or release profile holds and does not provide a Polkit agent.

## See also

- [`action-button`](../HTMShell.Elements/action-button.md)
- [`PowerProfiles`](../HTMShell.Services.UPower/PowerProfiles.md)
