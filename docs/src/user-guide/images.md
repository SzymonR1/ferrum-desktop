# Images

**Status:** Local Docker Unix-socket image listing/inspection and anonymous public pulls on Linux.

Open **Images** in the sidebar to view images returned by the connected local
Docker engine. Search is case-insensitive across repository tags and image IDs.
Select a row to load its details, including tags, creation time, size,
architecture, operating system, and author when reported by the engine. Missing
engine fields are shown as **Not reported**.

The list and inspection requests run asynchronously. Loading, empty, connection,
and engine-error states are shown in the page. Search filters the most recently
loaded inventory; use **Refresh** to request a new image list.

## Scope and limitations

This slice supports image listing and inspection plus anonymous pulls through
a local Docker Unix socket on Linux. The pull form accepts a public image
reference only; it does not read credentials or send registry authentication.
While a pull is active, **Stop monitoring** aborts Ferrum's client-side
progress stream and shows a stopped-monitoring notice; it does not cancel or
guarantee stopping the daemon's image transfer. A completion event already
received by Ferrum may still update the page and trigger an inventory refresh.

After a successful pull, Ferrum refreshes the list; the image remains in the
connected engine's image store.

Tag, remove, build, push, and private-registry authentication are not offered.
Issue [#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) remains open;
this sub-slice is not a release. Podman/rootless behavior, remote endpoints,
and broader engine compatibility remain unsupported or unvalidated under the
still-open [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10).
No live pull or native visual validation is claimed. See the
[platform support matrix](../platform-support.md).
