# Containers

**Status:** Implemented for the Linux local-engine slice.

Ferrum displays the live inventory returned by the selected Docker-compatible
engine, including stopped containers. The inspection dialog also offers
state-gated lifecycle actions (Start, Stop, Restart, Pause, Unpause, Remove)
only when the container is in a known state where that action is safe. Remove
requires explicit confirmation. Engine response bodies are not exposed.

Lifecycle request results emit a structured local tracing event containing the
action, outcome (`success` or `request_error`), and a full container ID only
when it is validated as 64 hexadecimal characters. A request error does not
prove that the engine state remained unchanged. Container names, endpoint
values, raw errors, and daemon response bodies are excluded. Events go to the
configured process logger; they are not a persistent or tamper-proof audit
store and do not record actions performed outside Ferrum.

External changes are synchronized from Docker container events. An inventory
snapshot is fetched on initial subscription and after reconnect; any open
inspection is reloaded so state-gated actions do not remain stale. If bounded
reconnection is exhausted, Ferrum reports event synchronization unavailable;
use Refresh to request a newer inventory snapshot.

## Container logs

The inspection dialog provides a Logs view for historical output and optional
live following. The Container logs action becomes available after inspection
details load; it is disabled while inspection is loading or unavailable. Logs
are available only through the selected local Unix-socket Docker-compatible
engine; remote engines, Podman/rootless, Windows, and macOS are not covered.

Docker Engine exposes its logs endpoint only for the `json-file` and `journald`
logging drivers. Unsupported-driver errors are reported safely without exposing
the daemon response body.

Choose a historical tail of 100, 500, 1000, or 5000 lines, toggle timestamps,
and filter stdout/stderr. Search is case-insensitive and applies to lines
currently retained by the view. Follow is enabled by default only when the
inspected container is Running. For a TTY container, Docker combines output
into a raw stream, so stdout/stderr cannot be separated; Ferrum labels each
such row `combined`. For non-TTY containers, Ferrum labels rows `stdout` or
`stderr` according to the stream type. Copy and Export preserve these labels,
formatting each row as `<source>: <text>` (for example, `stdout: ...`,
`stderr: ...`, or for TTY, `combined: ...`). Copy and Export escape terminal
control characters as literal `\xNN`; other text and row boundaries are
preserved.

The view is bounded to 8 MiB or 10,000 lines and discards oldest lines when
those limits are reached, with a visible discarded-line count. Individual
Docker frames are limited to 1 MiB and lines to 64 KiB. Copy and Export include
only rows currently visible after filtering and still retained in the buffer.
Export writes only the currently visible retained rows as plain text to the
path selected in the native save dialog. The dialog starts at
`container-logs.txt` and offers a text-file filter, but does not enforce the
extension. New regular files are created with owner-only Unix permissions
(`0600`). Existing regular targets must be writable; Ferrum writes the complete
export to a unique sibling temporary file and atomically replaces only the
selected entry. If a write or rename fails before commit, the previous
contents remain unchanged; Ferrum attempts to remove the temporary file.
Hard-link aliases retain the previous contents. Ordinary rwx permission bits
are preserved, though setuid/setgid bits are not. Replacing an existing target
requires write and search (execute) permission on its containing directory.
The replacement is a new inode owned by Ferrum's user; its group follows
directory creation policy. Extended ACLs and other xattrs are not preserved.
Final-component symlinks and non-regular targets are rejected before writing
or replacement.

Logs are not automatically exported or persisted. Refresh reloads the selected
tail and reapplies the current Follow setting.
Copy copies the visible rows to the clipboard. Clipboard contents may be
available to other applications or clipboard history. Treat log output as
untrusted and potentially sensitive; an export is a user-controlled file at
the destination you choose.
Pause suspends UI draining of the bounded 16-message stream channel. The
resulting backpressure holds producer reads without unbounded memory growth or
dropped lines; Resume continues delivery. Auto-scroll is independently
toggleable.

Change the Follow setting at any time. A clean EOF, such as a container
stopping, ends the stream without an automatic retry. For request or stream
failures, Follow retries at most three times during the current Logs view
session (250, 500, and 1000 ms). Replayed historical lines do not renew the
budget. After retry exhaustion, use Refresh to start a new bounded session.
Reconnect reloads the recent tail and clears buffered rows to avoid duplicate
replay. Closing the Logs view or inspection, switching engine context, or app
teardown cancels the stream.

## Find and sort containers

Use the filter field to search case-insensitively across container name, ID,
image, state, and status. Missing engine fields are not treated as text values.

Choose Name, Image, State, or Status to sort the current inventory. The order
control switches between ascending and descending. When an engine has not
reported an optional sort value, that container stays after entries with known
values. Ferrum filters and sorts the last successfully fetched inventory; use
Refresh to request a newer snapshot.

A successful empty inventory is shown separately from a filter with no matches.

## Inspect a container

Select a container's name to request its details from the connected engine.
Ferrum loads details asynchronously; closing the details window cancels the
pending request. The view can show the container ID, name, image, creation time,
state, restart and exit information, start/finish times, and mounts when the
engine reports them.

When an engine reports a non-empty state Ferrum does not recognize, the
reported text is shown as that container's state rather than treating
inspection as failed. An explicitly empty state is shown as `""`, distinct from
a missing state, which is **Not reported**.

A field omitted by the engine is shown as **Not reported**. Mount information
that is unavailable is distinct from a reported empty mount list. Ferrum does
not display environment variables, labels, or command arguments in this view.
Mount source paths can reveal host filesystem locations, so treat inspection
results as sensitive even though Ferrum does not persist them.

## Lifecycle and synchronization limits

Lifecycle actions use the negotiated engine API version and a bounded 30-second
wait. Stop and Restart may take longer because the engine applies its graceful
shutdown interval. Start/Stop treat HTTP 304 as idempotent success; other
unsuccessful statuses are safe typed errors. A timed-out action may still be
changing the container, so refresh before retrying. Remove does not force
removal or remove volumes. Event lines are capped at 64 KiB and each transport
data frame at 1 MiB. Delivery uses the bounded UI channel, and processing is
cancelled when the context changes or the app tears down.
For each outage, Ferrum retries up to three times with waits of 250 ms, 500 ms,
and 1 s; a successful subscription resets the retry budget. Exhaustion reports
event synchronization unavailable. Manual Refresh revalidates the inventory
and any open inspection.

The current implementation is Linux-only. Local Unix-socket Docker CLI contexts
can be selected for the current session. Listing and switching contexts requires
the Docker CLI; direct local `DOCKER_HOST` and automatic socket discovery do
not. Direct remote `DOCKER_HOST` values and remote TCP/HTTP/HTTPS/SSH contexts
return typed unsupported errors without local fallback. Podman/rootless behavior
is not validated, and Windows/macOS remain deferred to issue
[#21](https://github.com/SzymonR1/ferrum-desktop/issues/21) / M8. See the
[platform support matrix](../platform-support.md) for current evidence.
