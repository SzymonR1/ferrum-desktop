# Security policy

Ferrum treats access to a Linux Docker-compatible Unix socket as privileged. Windows/macOS engine integration is outside the current scope. The application must not elevate privileges, relax socket permissions, store plaintext credentials, or send remote engine traffic without explicit secure transport and identity validation. The experimental embedded terminal is limited to the selected local socket; terminal output is untrusted, OSC 52 clipboard and title requests are ignored, user clipboard actions are explicit, and terminal contents must not be logged.

Container log text is untrusted. Ferrum escapes C0/C1 controls and selected
bidirectional, zero-width, and invisible formatting characters before rendering,
clipboard copy, and file export. This reduces display spoofing but does not
detect or redact arbitrary data printed by a container.
Engine-provided container names, image identifiers/tags/inspection fields,
network names/metadata, attached-container names, and volume names and optional
volume detail values are untrusted. Ferrum escapes selected control,
bidirectional, zero-width, invisible-format, Unicode-tag, and Hangul-filler
characters in resource rows, details, and membership/confirmation labels. Each
engine-sourced display field is limited to 4 KiB of raw or escaped text; values
beyond that are shown as `[display text omitted]`. Volume names receive this
protection in the virtualized list button/accessibility label, details heading,
removal confirmation, in-progress label, and success label. Optional volume
details (driver, scope, creation time, and host mountpoint) are omitted by the
engine adapter when raw or escaped text exceeds 4 KiB; the page shows these as
**Not reported**. The UI independently enforces the 4 KiB display bound and
shows `[display text omitted]` for oversized values that reach rendering.
Escaping is presentation-only; raw names remain unchanged for identity
comparisons and requests. The mountpoint is never accessed. This does not detect
or redact secrets or arbitrary log content.

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
The request body is limited to 16 KiB, the response to 64 KiB, and the request
to eight seconds. A create
response must identify the exact requested name. Transport errors or timeouts
may follow a successful create, so Ferrum refreshes inventory after every
result and blocks context switching while the request is pending. Page
invalidation does not abort a submitted request.

Volume create and removal results emit local structured traces with only the
static action (`create` or `remove`) and `success`, `not_submitted`, or
`request_error` outcome. Volume names, mountpoints, endpoints, error text, and
daemon bodies are excluded; the process trace is not persistent or tamper-proof.

Ferrum lists and inspects volumes, then makes a separate filtered read-only
usage request; only `Size` in bytes and the point-in-time `RefCount` for an
exact inventory-matched volume are displayed when reported. Missing, negative,
or unknown metric values remain unavailable. A usage failure is shown
separately while successful inspect metadata stays visible. Only volume name,
driver, scope, creation time, and mountpoint are retained; selected control,
bidirectional, invisible-format, Unicode-tag, and Hangul-filler characters in
volume names are escaped at UI display boundaries, with raw-or-escaped text
capped at 4 KiB and longer text shown as `[display text omitted]`. Raw names
remain unchanged for identity comparisons and API request values. Selected
control, bidirectional, and invisible formatting characters in optional
display fields are escaped as visible `\u{...}` text. The mountpoint is never
accessed. Labels, options, status, raw response bodies, and filesystem browsing
are not exposed.

Removal is offered only for a listed volume with a matching inspection and an
exact-name usage snapshot reporting `RefCount == 0`; positive, missing, or
unknown counts disable it. Confirmation names the volume and warns that the
snapshot can change before deletion. Ferrum sends the negotiated
`DELETE /v{api}/volumes/{name}` without `force`, with an eight-second bound.
Docker may reject removal if the volume becomes in use. A timeout or transport
error can follow a successful deletion. Ferrum marks inventory stale after every
result and refreshes immediately if Volumes remains open, or when that page is
opened again; a fresh inspection/usage snapshot is required before retrying.
Only one volume mutation may be active and context switching is blocked while it is
pending. Pruning, modification, connection, disconnection, and filesystem
browsing are not offered. Podman/rootless behavior remains unvalidated; remote
endpoints remain unsupported.

The Networks page lists and inspects networks, creates networks by name, and
allows constrained removal and attachment operations through the selected local
Docker Unix socket on Linux. Creation sends only the required `Name` field to
the negotiated endpoint and relies on Docker's default bridge driver; no
driver, IPAM, or options controls are exposed. Names must be non-whitespace,
at most 255 UTF-8 bytes, and contain no control characters. Request/response
bodies are bounded, and daemon bodies are not displayed or logged.

The network metadata allowlist is ID, name, driver, scope, creation time,
internal/attachable, EnableIPv4, and EnableIPv6. Inspect may report an optional
point-in-time count from the `Containers` map and separately retain its keys
only when every key is a full 64-character hexadecimal container ID. Endpoint
values are discarded. Container names are shown only from the current
container inventory; endpoint names, endpoint IDs/configuration, MAC/IP
addresses, labels, options, IPAM, peers, services, status, and raw bodies are
not retained from network responses.

Removal is offered only for selected custom networks, never Docker's predefined
`bridge`, `host`, or `none` networks. A fresh inspection of the selected exact
ID must report `attached_container_count == Some(0)`, and the user must confirm
the network name and full ID. The bounded eight-second
`DELETE /v{negotiated}/networks/{id}` has no force or prune parameters. The
count is not atomic with deletion, so Docker may reject removal if dependencies
change before the request.

Connect/disconnect controls are limited to selected local custom networks with
a fresh, complete membership map and a target container whose full ID matches
the current inventory and whose reported state is `Running`. Stopped, paused,
or unknown states are disabled. Confirmation names the network and container
and shows both full IDs; disconnect warns that it may interrupt connectivity.

Container-change events invalidate the cached state. A Networks-page refresh
loads both inventories, and attachment controls remain disabled until the
container inventory is current; read failures require a fresh refresh.
Requests use the validated full network/container IDs and send only
`Container`, with no endpoint configuration. Disconnect omits `Force` so
Docker's default `false` applies. Requests have an eight-second bound and are
not retried. A timeout or transport failure may follow an engine-side change;
after either result Ferrum refreshes network/container inventories and
re-inspects the selected network. If refresh fails, the network list is marked
stale. Safe typed errors expose only endpoint kind and HTTP status where
applicable; raw daemon bodies and IDs are excluded from errors.

Network mutation traces record only the static create/remove/connect/disconnect
action, `success` or `request_error`, and validated full IDs when available.
Connect/disconnect events include both network and container IDs; names,
endpoints, error text, and daemon bodies are excluded. These process traces are
not persistent or tamper-proof. Only one network mutation may be active, and
context switching is blocked until it returns. Network usage statistics and
credential access are not provided. Podman/rootless and remote endpoints
remain unsupported or unvalidated. Issues #17, #19, and #10 remain open; no
release is claimed.

