# Security policy

Ferrum treats access to a Linux Docker-compatible Unix socket as privileged. Windows/macOS engine integration is outside the current scope. The application must not elevate privileges, relax socket permissions, store plaintext credentials, or send remote engine traffic without explicit secure transport and identity validation. The experimental embedded terminal is limited to the selected local socket; terminal output is untrusted, OSC 52 clipboard and title requests are ignored, user clipboard actions are explicit, and terminal contents must not be logged.

## Reporting a vulnerability

Please use the private vulnerability-reporting feature for this repository in GitHub Security Advisories when it is available. If that feature is unavailable, contact the repository owner through GitHub before sharing details publicly. Do not include credentials, private keys, or production endpoint secrets in public issues.

## Current scope

The Linux application provides container inventory, inspection, bounded historical/live logs, state-gated lifecycle actions, image listing, inspection, anonymous public pulls, tagging, confirmed image removal, and an experimental embedded exec terminal over the selected local Docker-compatible Unix socket. Container Remove requires explicit confirmation and does not force-remove containers or remove volumes. Every image-tag request requires explicit confirmation because Docker Engine v1.56 may reassign an existing `repository:tag`; the image data remains. Image removal confirmation snapshots the selected image ID and known tags, warns that removal of the final reference may delete image data, and explains that the non-force request may be rejected for images used by containers or with other tags. The request sets `noprune=true` to preserve untagged parent images, omits `force`, and does not send registry credentials or authentication. A bounded 30-second removal timeout does not prove that engine state remained unchanged; invalidating the page ignores stale results without aborting or claiming to undo a submitted request. Only one image mutation is allowed at a time. Image pull, tag, and removal operate only through the selected local Linux Docker Unix socket; remote engine connections, credentials, Compose, archive handling, managed engines, and release updates are not implemented. The terminal does not launch an external terminal; closing its exec session does not stop or remove the container. Issue #10 remains open for Podman/rootless and remote compatibility. Image removal has fake-socket and headless UI coverage only; tests do not delete a real image or mutate image resources. See [`docs/security/threat-model.md`](docs/security/threat-model.md) for current risks and controls.
Hiding image-pull progress or invalidating the Images page/context does not abort
an active pull reader: Ferrum drains the response and keeps the mutation gate
closed until a terminal event. Closing Ferrum does not guarantee that the
daemon-side pull stops.

Image pull, tag, and removal requests emit local structured process traces with
only the static action and a `success`, `not_submitted`, or `request_error`
outcome. `not_submitted` identifies local image ID, reference, or tag validation
that occurs before HTTP submission. Image IDs, references, repository tags,
endpoints, error text, and daemon response bodies are not logged. These events
are not persistent or tamper-proof; `request_error` does not establish that
engine state remained unchanged.

The Volumes page is available only for the selected local Docker Unix socket
on Linux. Creation sends only `Name` to the negotiated volume-create endpoint
and uses Docker's default local driver; no driver, labels, or options are
configurable. Names must be nonblank, at most 4096 UTF-8 bytes, contain no
control or selected invisible formatting characters, and not be `.` or `..`.
The request body is limited to 16 KiB, the response to 64 KiB, and the request to eight seconds. A create
response must identify the exact requested name. Transport errors or timeouts
may follow a successful create, so Ferrum refreshes inventory after every
result and blocks context switching while the request is pending. Page
invalidation does not abort a submitted request.

Volume-create results emit local structured traces with only the static
`create` action and `success`, `not_submitted`, or `request_error` outcome.
Volume names, mountpoints, endpoints, error text, and daemon bodies are
excluded; the process trace is not persistent or tamper-proof.

Ferrum lists and inspects volumes, then makes a separate filtered read-only
usage request; only `Size` in bytes and the point-in-time `RefCount` for an
exact inventory-matched volume are displayed when reported. Missing, negative,
or unknown metric values remain unavailable. A usage failure is shown
separately while successful inspect metadata stays visible. Only volume name,
driver, scope, creation time, and mountpoint are retained; selected control,
bidirectional, and invisible formatting characters in driver, scope, creation
time, and mountpoint are escaped as visible `\u{...}` text. The mountpoint is
never accessed. Labels, options, status, raw response bodies, and filesystem
browsing are not exposed. Volume removal, pruning, modification, connection,
and disconnection are not offered. Podman/rootless behavior remains
unvalidated; remote endpoints remain unsupported.

The Networks page lists and inspects networks and creates networks by name
through the selected local Docker Unix socket on Linux. Creation sends only the
required `Name` field to the negotiated network-create endpoint and relies on
Docker's default bridge driver; no driver, IPAM, or options controls are
exposed. Names must be non-whitespace, at most 255 UTF-8 bytes, and contain no
control characters. The create request body is bounded to 1 KiB and its
successful response body to 64 KiB; daemon bodies are not displayed or logged.

The inventory allowlist is ID, name, driver, scope, creation time,
internal/attachable, and EnableIPv4/EnableIPv6. Inspect may report only an
optional point-in-time count from the `Containers` map; list responses may omit
attached-container details. Ferrum does not retain or display container IDs,
names, endpoint records, MAC/IP addresses, labels, options, IPAM, peers,
services, status, or raw bodies.

Removal is offered only for selected custom networks, never Docker's predefined
`bridge`, `host`, or `none` networks. The UI requires a fresh inspection of the
selected exact ID with `attached_container_count == Some(0)` and explicit
confirmation naming the network and full ID; positive or unreported counts
cannot be submitted. The request is a bounded eight-second
`DELETE /v{negotiated}/networks/{validated-full-id}` with no force or prune
parameters. The count is a snapshot, not an atomic deletion guarantee, and
Docker may reject removal if dependencies change before the request. User-facing
errors contain only safe endpoint/status information where applicable; raw
daemon bodies and network IDs are never included in errors. The process trace
records the static create/remove action, request outcome, and a network ID only
when it is exactly 64 hexadecimal characters; it omits names, endpoints, raw
errors, and daemon bodies and is not persistent or tamper-proof. A transport
error or timeout may occur after removal has taken effect.
Only one network mutation is active at a time, and context switching is blocked
until its result arrives. Successful removal refreshes the current inventory.

Ferrum does not access credentials, provide network usage, or offer connect or
disconnect operations. Podman/rootless and remote endpoints remain unsupported
or unvalidated. Issues #17 and #10 remain open; no release is claimed.

