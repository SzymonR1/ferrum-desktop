# Images

**Status:** Linux local Docker Unix-socket image listing/inspection and bounded read-only layer history, anonymous pulls by default with opt-in per-pull Secret Service/pass credential-helper authentication, local tagging/removal, confirmed local Buildx builds, and anonymous push of one existing local tag. An opt-in read-only history smoke passed on Docker Engine 29.9.0/API 1.56; no live pull/auth, tag, removal, build, or push integration was performed or validated. No image mutation is claimed. Native Images-page visual validation remains unverified.

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

## Pulling images

Pulls are anonymous by default. To use credentials for one pull, first choose
**Secret Service** or **pass** in Settings, install/configure that Docker
credential helper for your Linux user session, then enable **Use the configured
credential helper for this pull only** before selecting **Pull image**. The
choice is consumed and reset when that pull begins; changing the reference or
configured helper also clears it. With no selected helper or no per-pull opt-in,
the request omits `X-Registry-Auth` and remains anonymous.

Ferrum's helper choice is global; it does not inspect Docker's `credsStore` or
`credHelpers` mappings. To populate a helper store, configure Docker CLI to use
the same supported helper and sign in with `docker login`; see Docker's
[credential store and helper guide](https://docs.docker.com/reference/cli/docker/login/#credential-stores).
If registries use different helper types, change Ferrum's setting before
opting in to a pull. Docker `auths` entries, including base64-encoded
credentials, are not read.

After opt-in, Ferrum invokes only the selected `docker-credential-secretservice`
or `docker-credential-pass` binary with argv `get`. It sends the normalized
registry key on stdin: Docker Hub references use
`https://index.docker.io/v1/`; other registries use the hostname (and optional
port) without a scheme. Ferrum does not read Docker `config.json` or other
credential entries. Helper execution has a 30-second deadline and 64 KiB
stdout bound. Stderr and helper error text are discarded. A failed helper,
malformed response, or exceeded bound fails the pull without falling back to
anonymous access.

Ferrum sends the helper result to the currently selected local Docker Engine
over its Unix socket using `X-Registry-Auth`. Ferrum does not persist secrets in
settings, audit records, or logs. Treat both the selected host helper and local
Engine as trusted: the Engine receives the credential and its registry-mirror
configuration may forward it according to daemon behavior. Ferrum does not
control or validate that downstream routing. Use authentication only when you
trust the Engine and its mirror configuration. Push and Buildx remain
credential-free and do not use this helper setting.

While a pull is active, **Hide progress** suppresses progress updates in the UI
only: Ferrum continues draining the Engine response and keeps other image
mutations disabled until the terminal response. Leaving Images or changing
context hides stale pull results without aborting the reader; closing Ferrum
does not guarantee the daemon-side transfer stops.

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

If the request or progress stream ends without a terminal response, the push
outcome is unknown. Ferrum keeps image mutations blocked for the current
application session and shows the unresolved target. Verify registry state
before restarting Ferrum or using another image client; restarting does not
guarantee that Docker stopped the transfer. Ferrum does not automatically retry
or unlock an ambiguous push.


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

Image listing, inspection, read-only history, pulls, tagging, removal, builds,
and anonymous push of one existing local tag are limited to the local Docker
Unix socket on Linux. Pulls are anonymous unless a configured Secret Service or
`pass` helper is explicitly enabled for a single operation. Push and Buildx stay
credential-free, and registries requiring authentication reject them. Builds
require Docker CLI and Buildx; private base-image authentication remains
unsupported. The helper-assisted pull path and its integration with a live
engine/registry have not been validated. Podman/rootless behavior and broader
endpoint or platform compatibility remain unvalidated under open, blocked
[issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10). Issue
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) remains partial/open.
No release or broader support claim is made. No live pull/auth, tag, removal,
build, or push was performed during validation. Native Images-page visual
validation remains unverified. Fake-socket and headless tests do not contact a
real engine or mutate image resources. See the
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
is not persisted to logs or audit traces. Build outcome tracing records only a
static action and result; when persistence succeeds, the redacted outcome is
also included in the local durable audit. If Docker cannot start the Buildx
process, Ferrum reports that the build was not submitted. A failed or cancelled
build after process start is treated as potentially state-changing. Build
output remains transient, bounded, and is never persisted. Cancellation is best
effort: Ferrum terminates the Docker CLI process group, but cannot guarantee
builder work stopped or determine whether the image/tag changed. Inventory
refreshes after every terminal result, including cancellation.

