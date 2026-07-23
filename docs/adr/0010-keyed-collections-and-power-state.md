# ADR 0010: Keyed collections and power state

## Status

Accepted for the experimental portable shell host.

## Context

Complete UPower presentation needs a changing device collection. Power Profiles also exposes active holds. Fixed numbered keys would lose item identity and would not represent insertion or removal safely.

## Decision

Quickshell UPower 0.3.0 is the author-capability benchmark. HTMShell expresses that capability through ordinary HTML, typed state, and typed actions.

A standard inert HTML `template` with `data-htm-element="repeat"` declares a collection. The supported sources are `upower.devices` and `power_profile.holds`. Source keys and service generations identify items. Registered descendants use template-local IDs. Runtime reconciliation preserves unchanged subtrees and performs bounded insertion, removal, update, and move operations.

Repetition is read-only and nonnested. It supports `state-text`, `state-token`, and `state-value` descendants. Numeric state uses semantic `data` elements with finite formats and a runtime-owned `value` attribute.

UPower's aggregate display device remains distinct from enumerated devices. UPower and Power Profiles share one direct pollable system bus connection. Each service has independent owner and source generations. Service absence is valid.

Profile selection uses three typed actions. HTMShell waits for confirmed service state and does not create or release profile holds.

## Consequences

- Dynamic service lists do not require HTML reparsing or full document rebuilds.
- Item identity survives property changes and deterministic moves.
- Identical duplicate profile holds have only occurrence-level identity because the service provides no stable cookie.
- UPower object paths remain internal and never become DOM IDs.
- Direct sysfs access remains excluded.
- General collection, service, expression, action-target, and component frameworks remain deferred.
- No polling, worker thread, or async runtime is introduced.
