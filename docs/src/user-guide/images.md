# Images

**Status:** Read-only listing and inspection for a local Docker Unix socket.

Open **Images** in the sidebar to view images returned by the connected local
Docker engine. Search is case-insensitive across repository tags and image IDs.
Select a row to load its details, including tags, creation time, size,
architecture, operating system, and author when reported by the engine. Missing
engine fields are shown as **Not reported**.

The list and inspection requests run asynchronously. Loading, empty, connection,
and engine-error states are shown in the page. Search filters the most recently
loaded inventory; use **Refresh** to request a new image list.

## Scope and limitations

This slice supports only read-only image list and inspect requests through a
local Docker Unix socket on Linux. It does not offer image pull, tag, remove,
build, push, or registry authentication. Those image operations remain tracked
by [issue #16](https://github.com/SzymonR1/ferrum-desktop/issues/16), which is
still open.

Podman and rootless-engine behavior remain unvalidated. Remote endpoints are
unsupported and unvalidated by this slice. Broader engine and endpoint
validation remains tracked by [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10),
which is still open. Windows and macOS remain outside the current platform
scope; see the [platform support matrix](../platform-support.md).
