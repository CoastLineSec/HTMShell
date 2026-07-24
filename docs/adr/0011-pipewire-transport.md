# ADR 0011: PipeWire transport

## Status

Accepted for the experimental portable shell host. The graph snapshot is not yet a public authoring API.

## Context

Later audio support needs a current PipeWire graph, stable session identities, and default-node metadata. The source must coexist with Wayland, the clock timer, and system D-Bus without polling or adding a worker thread.

## Decision

HTMShell uses `pipewire` 0.10.0 with default features disabled. It requires the native PipeWire 0.3 client and SPA libraries. The dependency is contained behind a host-owned transport boundary.

The 0.10.0 sys crates have local build-script compatibility patches. Bindgen fallback files use crate-specific Cargo output directories. Three SPA sentinel macros use their fixed upstream header values because current Clang does not emit them reliably. These patches do not change the native ABI and should be removed when an upstream release includes equivalent fixes.

One process connection owns a thread-affine PipeWire loop. Its file descriptor is polled by the existing blocking event loop. Ready work is dispatched through a bounded number of nonblocking loop iterations. Callbacks stage owned graph deltas. Reconciliation and snapshot publication occur only after callback dispatch returns. The read-only context disables PipeWire's realtime helper because this source dispatches no realtime data and must not create a worker thread.

Initial discovery uses the core registry and two synchronization barriers. A graph becomes ready only after node, link, and metadata bindings are included in the second barrier. Nodes and links use a connection generation plus the PipeWire global ID. Reconnection clears the old graph and creates a fresh generation.

The source tracks read-only nodes, links, derived source-target link groups, and the `default` metadata object. Actual defaults and configured defaults remain distinct. Graph readiness does not depend on WirePlumber metadata.

Properties and graph sizes have explicit bounds. Snapshots have deterministic ordering and suppress duplicate normalized publications. PipeWire types, proxies, callbacks, and raw pointers remain inside the transport.

## Consequences

- PipeWire adds no worker thread, executor, async runtime, polling loop, or subprocess.
- PipeWire absence is nonfatal and uses bounded reconnect deadlines.
- Raw global IDs are session-local diagnostics, not persistent identities or DOM IDs.
- The transport performs no volume, mute, default, link, stream, or metadata writes.
- Audio parameters, channels, controls, peak monitoring, public bindings, repeat sources, and guide pages remain for later PipeWire work.
- If the safe crate stops exposing a pollable loop or deterministic proxy lifetimes, direct integration must be reassessed before adding broad FFI.
