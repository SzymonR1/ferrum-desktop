# Images

**Status:** Linux local Docker Unix-socket image listing/inspection, anonymous
public pulls, and local image tagging. No live tag or pull operation is claimed.

Open **Images** in the sidebar to view images returned by the connected local
Docker engine. Search is case-insensitive across repository tags and image IDs.
Select a row to load its details, including tags, creation time, size,
architecture, operating system, and author when reported by the engine. Missing
engine fields are shown as **Not reported**.

The list and inspection requests run asynchronously. Loading, empty, connection,
and engine-error states are shown in the page. Search filters the most recently
loaded inventory; use **Refresh** to request a new image list.

## Pulling public images

The pull form accepts an anonymous public image reference only. Ferrum does not
read credentials or send registry authentication. While a pull is active,
**Stop monitoring** stops Ferrum's client-side progress stream; the image
transfer may continue in the engine. A completion event already received by
Ferrum may still update the page and trigger an inventory refresh.

## Tagging a local image

Select an image, enter a repository and Docker tag, and choose **Tag image**.
Ferrum validates a nonempty target with no ASCII whitespace/control characters,
a Docker tag matching `[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}`, and a combined
`repository:tag` length of at most 512 bytes. This is bounded client-side
validation, not full repository-name validation; the Docker engine validates
the repository.

Before every request Ferrum shows an explicit confirmation naming the selected
image and target. Docker Engine v1.56 may reassign an existing
`repository:tag`; the image data remains. **Cancel** sends no request. Tagging
updates the local image store only: it does not push to a registry, read
credentials, or send registry-authentication headers. Requests go only to the
connected local Docker Unix socket on Linux.

Only one tag request is allowed at a time. The request uses the engine's bounded
8-second API request timeout. Leaving Images or changing context invalidates
the displayed result but does not claim to cancel an in-flight request. A
successful response refreshes the current image inventory when Images is still
active.

## Scope and limitations

Image listing, inspection, anonymous pulls, and tagging are limited to the local
Docker Unix socket on Linux. Remove, build, push, and private-registry
authentication are not offered. Issue
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) remains open; this
sub-slice is not a release. Podman/rootless behavior, remote endpoints, and
broader engine compatibility remain unsupported or unvalidated under the
still-open [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10).
There are no live tag or pull tests and no native image-page visual validation.
See the [platform support matrix](../platform-support.md).

