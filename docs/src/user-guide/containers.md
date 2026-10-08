# Containers

**Status:** Implemented for the Linux local-engine slice.

Ferrum displays the live inventory returned by the selected Docker-compatible
engine, including stopped containers. The view is read-only: it does not start,
stop, rename, or remove containers.

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

## Connection limits

The current implementation is Linux-only. Local Unix-socket Docker CLI contexts
can be selected for the current session. Listing and switching contexts requires
the Docker CLI; direct local `DOCKER_HOST` and automatic socket discovery do
not. Remote TCP/HTTP/TLS/SSH contexts are rejected. Podman/rootless behavior is
not validated, and Windows/macOS remain deferred to issue
[#21](https://github.com/SzymonR1/ferrum-desktop/issues/21) / M8. See the
[platform support matrix](../platform-support.md) for current evidence.
