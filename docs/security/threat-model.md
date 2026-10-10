# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. Ferrum provides inventory, inspection, read-only volume usage and name-only volume creation, image tagging and removal, scoped container lifecycle actions, network list/inspect/create and constrained removal, logs, and an experimental embedded terminal over the Linux local-engine path.


## Trust boundaries

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local Linux Unix socket. Access to a Docker socket is effectively host-level control; UI action gating and confirmation do not make the endpoint unprivileged. Windows/macOS engine APIs are not implemented here. Engine responses, container names, image references, state strings, status text, and mount paths are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Keep direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH and remote Docker context transports disabled. Return typed unsupported errors without fallback to local discovery, and do not read remote TLS/SSH credentials.
- Do not log or display raw `DOCKER_HOST` values, which may contain sensitive endpoint data.
- Keep Docker CLI context selection session-only; do not log or display raw context endpoints.
- Render engine-provided strings and inspection details as plain text; do not interpret metadata as markup or commands. Inspection details omit environment variables, labels, and command arguments.
- Use private bounded request paths for version, API negotiation, inventory, inspection, lifecycle, volume usage, and network creation/removal. Version/inventory/inspection and event-subscription setup use an 8-second timeout; lifecycle requests use a bounded 30-second timeout for graceful Stop/Restart; only the filtered volume-usage request uses a 30-second timeout with a 16 MiB response limit. Network creation uses bounded JSON `POST /v{negotiated}/networks/create` with only `Name`, a 1 KiB request-body limit, a 64 KiB success-response limit, and a safe typed response/error path. Network removal uses a bounded eight-second `DELETE /v{negotiated}/networks/{id}` after validating a full 64-character hexadecimal ID; it has no force/prune parameters, does not consume daemon response bodies, and exposes only safe typed errors. Timed-out or transport-interrupted mutations may still change state; never log or expose raw response bodies. Container Remove requires confirmation and sends neither force nor volume-removal parameters.
- Restrict volume listing, inspection, and usage to the selected local Docker Unix socket on Linux. Retain/display only volume name, driver, scope, creation time, and mountpoint as untrusted plain text; never access the mountpoint. The separate filtered read-only `GET /v{api}/system/df?verbose=true&type=volume` provides usage only for an exact inventory-matched volume; retain/display only `Size` in bytes and point-in-time `RefCount` when reported. Missing, negative, or unknown metric values remain unavailable, and usage failures appear separately while successful inspect metadata remains visible. Discard unknown daemon fields, labels, options, status, and raw response bodies. A reference count is not a deletion-safety guarantee.
  Create only by sending `Name` to the negotiated `POST /v{api}/volumes/create` endpoint. Validate nonblank names of at most 4096 UTF-8 bytes, reject control or selected invisible formatting characters and `.`/`..`, and send no driver, labels, or options. Bound the request body to 16 KiB, response to 64 KiB, and total request to eight seconds. Require the response name to match the submitted name exactly; escape selected control, bidi, and invisible formatting characters in optional display fields as visible `\u{...}` text. A timeout or transport error may follow a successful mutation; block context switching while pending, do not abort on page invalidation, and refresh the inventory after every result. No volume removal, pruning, modification, connection, disconnection, or filesystem browsing is offered. Podman/rootless behavior remains unvalidated, remote endpoints remain unsupported, and issues #17 and #10 remain open.
- Emit local structured process traces for volume creation with only the static `create` action and `success`, `not_submitted`, or `request_error` outcome. Do not log volume names, mountpoints, endpoints, raw errors, or daemon bodies. A request error does not prove the engine state remained unchanged; these events are not a persistent or tamper-proof audit store.
- Keep network listing and inspection allowlisted on the selected Linux local Docker Unix socket, and offer name-only default bridge creation with bounded `POST /v{negotiated}/networks/create` containing only `Name`. Names are non-whitespace, at most 255 UTF-8 bytes, and contain no control characters; no driver, IPAM, or options controls are sent. Retain/display only ID, name, driver, scope, creation time, internal, attachable, EnableIPv4, and EnableIPv6. Inspect may retain only an optional point-in-time count of the `Containers` map; list responses may omit attached-container details. Discard container IDs, names, endpoint records, MAC/IP addresses, labels, options, IPAM, peers, services, status, and raw bodies. Enable removal only for a selected custom network whose fresh matching inspection reports `Some(0)`; positive or unreported counts and the predefined `bridge`, `host`, and `none` networks cannot be submitted. Require explicit confirmation naming the exact network and full ID. The attached-container count is not atomic with deletion; Docker may reject removal if dependencies change. A timeout or transport error can follow a successful deletion. Keep one network mutation in flight and block context switching until completion; refresh the current inventory after success. Do not access credentials or offer network usage, connect, or disconnect. Podman/rootless and remote support are not claimed.
- Anonymous image pulls are restricted to the selected local Unix-socket Docker
  engine and send no `X-Registry-Auth` header or stored credentials. Progress
  responses are bounded to 1 MiB per HTTP frame and 64 KiB per JSON record;
  displayed status is capped at 256 bytes; control characters and selected
  invisible formatting characters (bidi marks, zero-width marks/joiners, and
  soft hyphens) are replaced before rendering. Only status and optional byte
  counts are exposed. Daemon error details are discarded, never logged or displayed.
  At Hide progress, Ferrum stops forwarding UI progress but continues draining
  the response until a terminal event, keeping the image-mutation gate closed.
  Page/context invalidation hides stale progress without aborting the reader.
  Closing Ferrum still does not guarantee that daemon-side pull work stops.
- Send image-tag requests only to the selected local Docker Unix-socket engine
  using the negotiated API version and the bounded 8-second request timeout.
  Validate the image ID and bound the combined `repository:tag` target to 512
  bytes; validate Docker tag syntax while leaving full repository grammar to
  the engine. Do not send `force`, `X-Registry-Auth`, stored credentials, or
  read/display response bodies. Typed failures expose only a safe endpoint label
  and HTTP status where applicable. Tagging changes a local repository/tag
  association only; it does not contact a registry. Require explicit
  confirmation before every request because Engine v1.56 may reassign an
  existing `repository:tag`; the image data remains. Page/context invalidation
  ignores stale UI results but does not abort or claim to cancel an in-flight
  request.
- Send image-removal requests only to the selected local Docker Unix-socket
  engine using the negotiated API version and a bounded 30-second timeout.
  Validate a nonempty image ID of at most 4096 bytes. Require an explicit
  confirmation that snapshots the selected ID and known repository tags and
  warns that removing the final reference may delete image data. The request
  omits `force` (so the engine default applies) and sets `noprune=true` to
  preserve untagged parent images; the engine may refuse images used by
  containers or with other tags. Do not send registry authentication or
  platform parameters, and never read or expose response bodies. Typed errors
  expose only safe endpoint/status information; timeout errors must explain
  that state may have changed. Only one image mutation may be active across
  pull/tag/remove. Page/context invalidation ignores stale UI results but does
  not abort or claim to undo a possibly submitted removal; refresh inventory
  after success only while Images is current.

- Emit a structured local tracing event when a Ferrum container lifecycle
  request returns. Record the action, `success` or `request_error` outcome, and
  a container ID only when it is exactly 64 hexadecimal characters. A request
  error does not prove that the engine state remained unchanged. Do not log
  container names, endpoints, raw errors, or daemon response bodies. These
  events use the configured process logger; they are not a persistent or
  tamper-proof audit store.
- Emit a structured local tracing event when a network create/remove request
  returns. Record the static action, `success` or `request_error` outcome, and
  a network ID only when it is exactly 64 hexadecimal characters. Do not log
  network names, endpoints, raw errors, or daemon response bodies. A request
  error does not prove the engine state remained unchanged. These events use
  the configured process logger; they are not a persistent or tamper-proof
  audit store.
- Emit local structured tracing events when image pull, tag, or removal requests
  return, containing only the static action and `success`, `not_submitted`, or
  `request_error` outcome. `not_submitted` identifies local validation rejection
  before HTTP submission. Do not log image IDs, references, repository tags,
  endpoints, raw errors, or daemon bodies. A `request_error` does not prove that
  engine state remained unchanged. These events are not persistent or
  tamper-proof.
- Keep historical/live container logs restricted to the selected local Unix-socket engine. Use the negotiated API version and streaming response body; accept only json-file/journald drivers. TTY is read from allowlisted `Config.Tty`: TTY output is raw and combined, while non-TTY output is decoded from Docker's 8-byte multiplex frames. Render logs as plain text, never as a terminal. Bound frames to 1 MiB, lines to 64 KiB, retention to 8 MiB or 10,000 lines, and delivery to a dedicated 16-message channel; discard oldest retained lines with a visible count. Pause suspends UI draining, applying bounded backpressure to producer reads without unbounded memory or dropped lines; Resume continues delivery. Copy and Export include only currently visible, retained rows and escape terminal control characters as `\xNN`.
- Keep the experimental embedded terminal restricted to the selected local Docker-compatible Unix socket on Linux and the negotiated API version. Terminal output is untrusted; ignore OSC 52 clipboard and title requests, expose clipboard actions only to explicit user controls, and never put terminal bytes or daemon bodies in logs or errors. Closing the exec session closes its connection only; it does not stop or remove the container.
- Persist only the schema version and startup auto-connect boolean; reject unknown fields, bound input size, and keep config files/private temporary files owner-only.
- Do not include configuration paths or contents in settings errors or logs. Corrupt settings fall back to defaults and are not overwritten until explicit user save.
- The agreed Linux settings-path contract uses an absolute
  `XDG_CONFIG_HOME` path or falls back to absolute `HOME`. At initialization,
  `SettingsStore` pins a descriptor for the longest existing directory before
  the final app settings directory. Existing symlink aliases in the
  user-selected XDG/HOME path are honored at that time. The configured
  location remains user-trusted and is not confined to `HOME`.
- Traverse every remaining settings-directory component relative to the
  retained descriptor with `openat` and `O_NOFOLLOW`. Save may create missing
  components with `mkdirat`; load never creates directories. Reject symlinks at
  the final app settings directory on load and save, and reject symlinked or
  non-regular `settings.toml` entries on both paths. Before opening
  `settings.toml`, load sets the already-open final `ferrum` directory to
  exactly `0700`; it rejects non-regular files and files with `nlink != 1`
  before changing file permissions or reading contents. A single-link regular
  file is set to `0600` before it is read. Before saving, inspect the directory
  entry with `fstatat(AT_SYMLINK_NOFOLLOW)`; `renameat` never follows a
  symlink target if the entry changes concurrently.
  The link-count check and following mode change are not atomic. The final
  directory's `0700` mode limits access by different unprivileged UIDs, but
  same-UID processes and privileged actors remain inside the local filesystem
  trust boundary.
- Save requests `0700` for each missing remaining settings-path component and,
  only when `mkdirat` creates it, sets its mode to exactly `0700` through the
  opened directory descriptor. Existing ancestors are not normalized; the
  final app `ferrum` directory remains subject to the separate `0700`-on-save
  rule. Settings files and save-time temporary files use `0600`. Temporary
  files are created with `openat`, replaced with `renameat`, and removed with
  `unlinkat` on failed saves, all relative to the retained descriptor. A umask
  that removes owner-read needed to open a new directory makes save fail before
  temporary-file creation or settings writes; the new directory may remain.
- `mkdirat` and the following `openat` are separate operations. An actor with
  write/search access to the parent may replace a just-created name with a real
  directory before it is opened, subject to sticky-directory rules.
  `O_NOFOLLOW` rejects symlink replacements, not real-directory replacements.
  If a replacement cannot be opened, save fails. If it can be opened,
  normalization applies to that opened inode; a process without ownership or
  `CAP_FOWNER` fails with `EPERM` when normalizing a foreign-owned replacement,
  before settings writes. The replacement owner or a process with `CAP_FOWNER`
  can change its mode. No interprocess lock closes this race.
- Every filesystem operation after initialization uses the retained directory
  descriptor. Replacing the selected absolute parent path after initialization
  cannot redirect access away from it. This settings-path contract applies
  only to Linux.

## Deferred attack surfaces

- Remote TCP/HTTP/TLS/SSH transport trust, credentials, and identity validation.
- Podman API compatibility and rootless endpoint discovery; issue #10 remains open for these and the broader engine matrix.
- Terminal streams beyond this bounded local exec session, Compose files, external process invocation, archive extraction, and file paths.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary. UI state gating and confirmation reduce accidental destructive operations but cannot constrain a compromised or malicious engine endpoint.

## Validation plan

Read-only live-engine tests cover version, inventory, and inspection without
resource mutation. The optional ignored network list/inspect test requires
`FERRUM_TEST_NETWORK` to name an existing local network; it lists networks and
inspects the matching listed ID without mutation or cleanup. The separate
ignored create-and-remove test preflights a generated name, then requires a
full 64-character hexadecimal ID and an inventory entry matching that exact ID
and generated name before arming cleanup. It removes only the verified ID via
`EngineClient::remove_network` and confirms that ID is absent afterward. If
ownership cannot be confirmed, it sends no removal or cleanup request and the
created network may remain; the `Drop` guard's Docker CLI cleanup is an
exact-ID fallback only and never removes by name or prunes. Default tests and
fake-socket/headless network tests do not contact or mutate a live engine.
Headless Networks tests cover fresh zero-count gating, positive and unavailable
counts, predefined networks, named confirmation/cancel, stale ID/generation
results, daemon rejection, and single-mutation/context-switch gates. Fake-socket
network-removal tests cover the versioned exact-ID `DELETE`, absent force/prune
parameters, invalid IDs before connection, and typed redacted rejections.
Fake-Unix-socket image protocol tests and headless Images UI tests cover tag
and removal request/confirmation behavior without contacting a real engine or
mutating images. There is no live image deletion or other live image-mutation
test.
Separate opt-in ignored lifecycle, logs, and terminal tests mutate engine state
using one uniquely named disposable container and only its validated full ID as
cleanup authority. Logs and terminal tests require locally installed images and
never pull, tag, or delete images. See
[issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).

The optional ignored volume-create live test uses the current local Docker
context's local Unix socket path for both EngineClient and Docker CLI cleanup.
It preflights a generated name and arms cleanup only after the create response
and subsequent inventory both confirm that exact name. Cleanup removes only
that generated name with an argument-array `--host unix://...` request and no
force. If ownership cannot be confirmed, no cleanup is sent. After a cleanup
command returns, the guard is disarmed and an unsuccessful command is not
retried by name, so the volume may remain. The separate `FERRUM_TEST_VOLUME`
live test is read-only. Default fake-socket and headless UI tests do not contact
a live engine.
