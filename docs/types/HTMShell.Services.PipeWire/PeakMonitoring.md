# Peak monitoring

Explicit, process-shared PipeWire scalar peak monitoring.

Peak monitoring begins only for an enabled `peak-monitor` on a mapped surface.
Ordinary PipeWire state never creates a monitor stream. Disabled declarations
and enabled declarations on closed retained surfaces create no stream demand.

HTMShell creates at most one input monitor stream for each generation-safe
target node. Item-local and actual-default declarations, documents, and
outputs resolving to that node share it. The final mapped enabled consumer
removes the stream within the event-loop cycle.

The stream uses the existing PipeWire connection, F32 samples, automatic
connection and mapped buffers. Sink targets request sink capture. The transport
prefers the node's bounded `object.serial`; it falls back to the current global
ID only at the transport boundary. Neither value is an author target.

The process callback calculates the absolute maximum sample for every
negotiated channel, applies the cube-root perceptual mapping, stages only the
latest vector, and returns the buffer. It performs no document, Wayland, D-Bus,
filesystem, or network work. The raw samples are never retained or exposed.

Publications are bounded to 60 per second per active node. A newer callback
replaces an unpublished vector, duplicates are suppressed, and no backlog is
built. Stream and layout generations reject stale callbacks. Transient failure
retries after 250 milliseconds, 1 second, 5 seconds, and then at most every 30
seconds while active demand remains.

Source monitoring is capture-adjacent. PipeWire policy may deny it or display
a capture indicator. HTMShell does not bypass that policy and does not record,
persist, transmit, smooth, average, or retain peak history.

Limits include 256 active shared streams process-wide, 64 peak channels per
stream, 256 interested declarations per target node, and 4,096 mapped monitor
declaration identities process-wide.
