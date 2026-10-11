# Images

**Status:** Linux local Docker Unix-socket image listing, inspection, and
read-only layer history, plus anonymous public pulls, local image tagging, and
confirmed image removal. An opt-in read-only history smoke passed on Docker
Engine 29.9.0/API 1.56; no live image mutation is claimed.

Open **Images** in the sidebar to view images returned by the connected local
Docker engine. Search is case-insensitive across repository tags and image IDs.
Select a row to load its details, including tags, creation time, size,
architecture, operating system, and author when reported by the engine. Missing
engine fields are shown as **Not reported**.

The read-only layer-history view loads asynchronously for the exact selected
image ID in the current inventory. It retains only validated full SHA-256 layer
IDs, nonnegative creation timestamps, and nonnegative sizes; `CreatedBy` command
text, tags, comments, and all other daemon fields are discarded and never
displayed.
The request uses the negotiated API version over the selected local socket and
is bounded to an eight-second timeout, a 16 MiB response, and 65,536 rows. If a
bound is exceeded or the response is invalid, the page shows a safe unavailable
state.

Image tags, identifiers, and inspection fields supplied by the engine are
treated as untrusted text. Selected controls, bidi/zero-width and
invisible-format characters, Unicode tags, and Hangul fillers appear as visible
escapes in image rows, tooltips, details, and tag confirmations. Engine-sourced
fields whose raw or escaped text exceeds 4 KiB are shown as
`[display text omitted]`. Requests still use the original ID; ordinary tag and
author text is not redacted.

The list, inspection, and history requests run asynchronously. Loading, empty,
connection, and engine-error states are shown in the page. Search filters the
most recently loaded inventory; use **Refresh** to request a new image list.
History results are discarded if the selected image or current inventory changes.

## Pulling public images

The pull form accepts an anonymous public image reference only. Ferrum does not
read credentials or send registry authentication. While a pull is active,
**Hide progress** suppresses progress updates in the UI only: Ferrum continues
draining the Engine response and keeps other image mutations disabled until the
terminal response. Leaving Images or changing context hides stale pull results
without aborting the reader; closing Ferrum does not guarantee the daemon-side
transfer stops.

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

## Removing a local image

Select an image and choose **Remove image** to review a confirmation that
snapshots the selected image ID and all repository tags known to Ferrum. The
confirmation warns that image data may be deleted if its last reference is
removed. Removal is not forced: Docker may refuse an image used by a container
or one with other tags. The request uses `noprune=true` so untagged parent images
are preserved; it omits `force` and does not send registry authentication.
Choose **Cancel** to close the confirmation without sending a request.

The request is a Docker Engine v1.56 image DELETE through the selected local
Docker Unix socket, using the negotiated API version and a bounded 30-second
timeout. A timeout or other request error does not establish whether the engine
changed state; refresh the inventory before deciding what to do next. Leaving
Images or changing context invalidates the displayed result but does not abort
or claim to undo a possibly submitted removal. A successful response refreshes
inventory only when Images is still the current page. Pull, tag, and removal
share one in-flight image-mutation gate.

No response body is displayed. Errors expose only safe endpoint/status details;
the UI does not expose daemon response bodies.

## Scope and limitations

Image listing, inspection, read-only history, anonymous pulls, tagging, and
removal are limited to the local Docker Unix socket on Linux. Build, push, and
private-registry authentication are not offered. Issue
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) remains open; this
sub-slice is not a release. Podman/rootless behavior, remote endpoints, and
broader engine compatibility remain unsupported or unvalidated under the
still-open [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10).
No live image-mutation smoke is claimed. Fake-socket and headless tests do not
contact a real engine or mutate image resources, and native image-page visual
validation remains outstanding. See the
[platform support matrix](../platform-support.md).

