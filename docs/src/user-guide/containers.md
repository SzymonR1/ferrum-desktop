# Containers

**Status:** Implemented for the Linux local-engine slice.

Ferrum displays the live inventory returned by the selected Docker-compatible
engine, including stopped containers. The inspection dialog also offers
state-gated lifecycle actions (Start, Stop, Restart, Pause, Unpause, Remove)
only when the container is in a known state where that action is safe. Remove
requires explicit confirmation. Engine response bodies are not exposed.

External changes are synchronized from Docker container events. An inventory
snapshot is fetched on initial subscription and after reconnect; any open
inspection is reloaded so state-gated actions do not remain stale. If bounded
reconnection is exhausted, Ferrum reports event synchronization unavailable;
use Refresh to request a newer inventory snapshot.

## Container logs

The inspection dialog provides a Logs view for historical output and optional
live following. Logs are available only through the selected local Unix-socket
Docker-compatible engine; remote engines, Podman/rootless, Windows, and macOS
are not covered.

Docker Engine exposes its logs endpoint only for the `json-file` and `journald`
logging drivers. Unsupported-driver errors are reported safely without exposing
the daemon response body.

Choose a historical tail of 100, 500, 1000, or 5000 lines, toggle timestamps,
and filter stdout/stderr. Search is case-insensitive and applies to lines
currently retained by the view. Follow is enabled by default only when the
inspected container is Running. For a TTY container, Docker combines output
into a raw stream, so stdout/stderr cannot be separated; Ferrum displays the
combined data as plain text and never as a terminal.

The view is bounded to 8 MiB or 10,000 lines and discards oldest lines when
those limits are reached, with a visible discarded-line count. Individual
Docker frames are limited to 1 MiB and lines to 64 KiB. Output is not written
to disk or logged. Refresh reloads the selected tail and reapplies the current
Follow setting. A clean EOF, such as a container stopping, ends the stream
without an automatic retry. For request or stream failures, Follow retries at
most three times per outage (250, 500, and 1000 ms); the retry budget resets
only after a reconnect delivers at least one log line, not merely when the
stream opens. After reconnect Ferrum reloads the recent tail and clears buffered
rows to avoid duplicate replay. Closing the Logs view or inspection, switching
engine context, or app teardown cancels the stream.

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
not. Remote TCP/HTTP/TLS/SSH contexts are rejected. Podman/rootless behavior is
not validated, and Windows/macOS remain deferred to issue
[#21](https://github.com/SzymonR1/ferrum-desktop/issues/21) / M8. See the
[platform support matrix](../platform-support.md) for current evidence.
