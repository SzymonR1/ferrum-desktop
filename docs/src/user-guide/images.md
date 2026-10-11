# Images

**Status:** implemented for the Linux local Docker Unix socket: image
listing/inspection, anonymous public pulls, local image tagging/removal, and
confirmed local Buildx builds. No live pull, tag, removal, or build was
performed during validation; native Images-page visual validation remains
unverified.

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

Image listing, inspection, anonymous pulls, tagging, removal, and builds are
limited to the local Docker Unix socket on Linux. Builds require Docker CLI and
the Buildx plugin in `PATH`. Podman/rootless behavior and broader endpoint or
platform compatibility remain unvalidated under open, blocked [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10).
This partial slice does not complete [issue #16](https://github.com/SzymonR1/ferrum-desktop/issues/16),
which remains open and ready; [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19)
also remains open and ready. No release or broader support claim is made.
No live pull, tag, removal, or build was performed during validation. Native
Images-page visual validation remains unverified. See the
[platform support matrix](../platform-support.md).

## Building a local image

Choose a local directory containing a regular root-level `Dockerfile` and
enter a bounded `repository:tag` target. Ferrum rejects uppercase repository
characters and validates Docker tag syntax and target length, but leaves the
full repository grammar to Docker CLI. It invokes Docker CLI Buildx using
argument arrays with `--host unix://<selected EngineClient socket>` pinned to
the selected local Docker Engine Unix socket on Linux. Docker CLI applies
`.dockerignore` and its normal tar-context semantics. Remote contexts, custom
builders, platform selection, and push are unsupported. Private base-image
authentication is unsupported and may cause a build to fail.

Ferrum requires Docker CLI and the Buildx plugin in `PATH`. Before every build,
it requests explicit confirmation because Dockerfiles can run arbitrary builder
commands and may reassign an existing tag. The confirmation names the selected
Docker context or a safe endpoint-source label when no named context is
selected; it never displays the socket path. Context switching is disabled
while the confirmation is open and until the build reaches a terminal result.
Build execution uses a private temporary empty Docker config and isolated
`HOME`; Ferrum does not read user registry credentials or forward proxy
settings, build arguments, secrets, or SSH access.

Output is streamed as sanitized plain text with bounded in-memory retention and
is not persisted to logs or audit traces. Cancellation is best effort: Ferrum
terminates the Docker CLI process group, but cannot guarantee builder work
stopped or determine whether the image/tag changed. Inventory refreshes after
every terminal result, including cancellation, because the outcome may be
uncertain.

