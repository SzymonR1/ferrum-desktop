# Images

**Status:** Linux local Docker Unix-socket image listing/inspection, anonymous
public pulls, local tagging/removal, and anonymous push of an existing local
tag. No live pull, tag, removal, or push request is claimed.

Open **Images** in the sidebar to view images returned by the connected local
Docker engine. Search is case-insensitive across repository tags and image IDs.
Select a row to load its details, including tags, creation time, size,
architecture, operating system, and author when reported by the engine. Missing
engine fields are shown as **Not reported**.

Image tags, identifiers, and inspection fields supplied by the engine are
treated as untrusted text. Selected controls, bidi/zero-width and
invisible-format characters, Unicode tags, and Hangul fillers appear as visible
escapes in image rows, tooltips, details, and tag confirmations. Engine-sourced
fields whose raw or escaped text exceeds 4 KiB are shown as
`[display text omitted]`. Requests still use the original ID; ordinary tag and
author text is not redacted.

The list and inspection requests run asynchronously. Loading, empty, connection,
and engine-error states are shown in the page. Search filters the most recently
loaded inventory; use **Refresh** to request a new image list.

## Pulling public images

The pull form accepts an anonymous public image reference only. Ferrum does not
read credentials or send registry authentication. While a pull is active,
**Hide progress** suppresses progress updates in the UI only: Ferrum continues
draining the Engine response and keeps other image mutations disabled until the
terminal response. Leaving Images or changing context hides stale pull results
without aborting the reader; closing Ferrum does not guarantee the daemon-side
transfer stops.

## Pushing an existing local tag

Select an image and use the exact `repository:tag` reference already listed
for that image. Ferrum preselects a valid listed tag; you can choose another
listed tag or paste one of the image's current tags. A target not present on
the selected image is rejected. If the image needs a different target, tag it
first using the separate confirmed **Tag image** operation.

Ferrum accepts references up to 512 bytes with a bounded lowercase-ASCII
repository-name subset and Docker tag syntax; the engine validates full
repository semantics.

Before sending a push, Ferrum reinspects the selected image ID and verifies the
target tag is still reported for that image. This is a point-in-time check, not
an atomic lock against another Docker client changing tags concurrently.
Confirmation names the image ID and exact tag and warns that layers and metadata
are sent to the registry and may contain secrets. **Cancel** sends no push.

Push uses the negotiated Docker Engine API through the selected local Unix
socket, with an explicit tag so the daemon is not asked to push every tag. It
uses anonymous registry access only: Ferrum does not read Docker config,
credential helpers, or saved credentials. Docker registries requiring
authentication will reject the operation; response bodies are never displayed.
The API stream is bounded to 1 MiB per frame and 64 KiB per progress record;
displayed status is capped at 256 bytes. Opening the push request uses an
eight-second bound, and progress waits use a 60-second idle timeout. A timeout
or interrupted stream does not prove that no layers reached the registry.

**Hide push progress** stops forwarding updates but Ferrum continues draining
the response and keeps the image-mutation gate closed until a terminal event.
Leaving Images or changing context hides stale progress without aborting the
reader. Closing Ferrum does not guarantee that daemon-side push work stops.


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
inventory only when Images is still the current page. Pull, push, tag, and removal
share one in-flight image-mutation gate.

No response body is displayed. Errors expose only safe endpoint/status details;
the UI does not expose daemon response bodies.

## Scope and limitations

Image listing, inspection, anonymous pulls, tagging, removal, and anonymous
push are limited to the local Docker Unix socket on Linux. Build/Buildx and
private-registry authentication are unavailable. Issue
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) remains open; this
slice is not a release. Podman/rootless behavior, remote endpoints, and broader
engine compatibility remain unsupported or unvalidated under the still-open
[issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10).
There are no live pull, tag, removal, or push operations, tests do not mutate
images, and native image-page visual validation remains unavailable. See the
[platform support matrix](../platform-support.md).

