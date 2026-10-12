# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. Ferrum provides container inventory and inspection, volume list/inspect/usage with name-only creation and dependency-gated removal, image listing/inspection and bounded read-only history plus anonymous pulls/tagging/removal and confirmed local Buildx builds, scoped container lifecycle actions, network list/inspect/create/removal/connect/disconnect on the Linux local-engine path with aggregate IPAM allocation counts when reported, logs, and an experimental embedded terminal.

Local builds use Docker CLI Buildx on Linux only. Invoke it with argument
arrays, explicitly pinned to `--host unix://<EngineClient::local_socket_path()>`;
delegate `.dockerignore` and tar-context behavior to Docker CLI. Require the
Buildx plugin in `PATH`, a local directory containing a regular root-level
`Dockerfile`, and a bounded `repository:tag` check that rejects uppercase
repository characters and leaves complete repository grammar to Docker CLI.
Use a private temporary empty Docker config and isolated `HOME`; strip inherited
Docker endpoint/context/TLS/custom-header/API/platform and proxy variables. Do
not read user credentials or provide registry auth, build args, secrets, SSH,
push, remote contexts, custom builders, or privileged entitlements. Private
base images may fail without authentication.
Confirm each build because Dockerfiles can execute arbitrary commands and
existing tags may be reassigned; the confirmation names the selected context or
a safe endpoint-source label when no named context is selected, without
displaying the socket path. Context switching is disabled while confirmation
or build is active. Stream only sanitized bounded plain text in memory; do not
persist output. Best-effort cancellation terminates the CLI process group;
refresh inventory after every terminal/cancel result because state may be
uncertain. Build traces contain only static action and outcome, excluding
context path, tag, and output. Their redacted outcome is also written to the
existing best-effort durable JSONL audit; only static event/action/outcome are
persisted for builds. Build output remains bounded transient UI data and is
never persisted.

Compose support includes read-only Docker CLI version/project discovery and
explicit structural validation of a user-selected local base file followed by
user-ordered local overrides on Linux. Files are numbered from 1 (the base)
and limited to 16 total. Docker merges `-f` files in the order supplied;
relative paths resolve from the first/base config or `--project-directory`,
which Ferrum sets to the base file's parent. See Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for ordered merge and path semantics and [`config`](https://docs.docker.com/reference/cli/docker/compose/config)
for validation flags.

Validation skips interpolation and service `env_file` resolution, uses no
engine, and does not mutate project files. Compose can still recursively read
local `include`/`extends.file` Compose files and included projects' default
`.env` and explicit `include.env_file` paths under the logged-in user's
permissions, potentially outside the base directory. Remote Git/OCI loaders
are disabled. The selected files and private CLI environment do not sandbox
those reads; validate only trusted files. Profiles, `.env`/service
`env_file` resolution, import, lifecycle/build/pull/logs, and project mutations
remain unsupported. Issue #18 remains partial/open; issue #10 remains
blocked/open. No release or broader support claim is made.


## Trust boundaries

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local Linux Unix socket. Access to a Docker socket is effectively host-level control; UI action gating and confirmation do not make the endpoint unprivileged. Windows/macOS engine APIs are not implemented here. Engine responses, container names, image references, state strings, status text, and mount paths are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Keep direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH and remote Docker context transports disabled. Return typed unsupported errors without fallback to local discovery, and do not read remote TLS/SSH credentials.
- Do not log or display raw `DOCKER_HOST` values, which may contain sensitive endpoint data.
- Keep Docker CLI context selection session-only; do not log or display raw context endpoints. Linux `EngineClient::local_socket_path` exposes the already-connected local socket only so trusted local tooling can target the same daemon; it does not change socket permissions and must not be logged or displayed.
- Keep Compose discovery read-only. `docker compose version --short` detects the
installed CLI version without targeting an engine; the project listing alone
uses `docker compose ls --all --format json` with Docker global
`--host unix://...` pinned to `EngineClient::local_socket_path()`. Run each CLI
process with a private isolated Docker config/HOME and a cleared allowlisted
environment. If the Compose plugin is in the standard per-user directory, link
only its `docker-compose` executable into the private config. Custom
`cliPluginsExtraDirs` from the saved config are not loaded; do not copy or load
user `config.json`, credentials, or proxy settings. Bound each command to eight
seconds and 1 MiB of stdout. Retain/display only escaped project Name and
Status; reject malformed or duplicate Name/Status fields and duplicate project
identities. Ignore ConfigFiles, paths, raw stderr, and daemon errors.

- Allow an explicit structural validation action with one selected local
  `.yaml`/`.yml` base file and local overrides appended in user-chosen order.
  Order is one-based, with the base at 1; accept no more than 16 total files.
  Preflight Compose with `docker compose version --short` through the private
  isolated Docker config and distinguish preflight failure from config failure.
  Invoke Compose with the base `--file` first and each override afterward,
  plus `--project-directory <base-parent> config --quiet --no-interpolate
  --no-env-resolution`. Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
  specifies ordered `-f` merge and path resolution relative to the first config
  or `--project-directory`; the [`config` reference](https://docs.docker.com/reference/cli/docker/compose/config)
  documents the validation flags.

- Keep validation read-only, engine-independent, and bounded to eight seconds
  and 1 MiB stdout per command. Success is structural validation only;
  interpolation and service `env_file` resolution are skipped. Do not mutate,
  extract, or write project files.

- Treat Compose references as untrusted filesystem reads. Compose may
  recursively read local files referenced by `include` or service
  `extends.file`, plus each included project's default `.env` and explicit
  `include.env_file`. Those files may be outside the base directory and are
  opened under the logged-in user's permissions.
  `COMPOSE_DISABLE_ENV_FILE=1` disables only the selected project's default
  `.env`; `--no-interpolate` and `--no-env-resolution` do not confine included
  project reads. Remote Git and OCI loaders are disabled, so remote
  `include`/`extends.file` references fail without fetching. The selected files
  are not a path-containment boundary. The CLI/plugin is trusted local code,
  not a sandbox; validate only trusted files. Keep selected paths and raw
  diagnostics private; display only a bounded escaped basename and static result.

- Keep profiles, `.env`/service `env_file` resolution, file import, project
  start/stop/restart, build/pull/logs, and project mutations unsupported.
  Remote references remain disabled and are not fetched. Linux local discovery,
  single-file validation, ordered temporary base/override, and remote-loader
  safety smokes passed with Compose v5.6.0; they do not validate arbitrary
  projects or lifecycle behavior. The native page visual check is unavailable.
  Issue #18 remains partial/open and #10 remains blocked/open; no release or
  support expansion is claimed.
- Render engine-provided strings and inspection details as plain text; do not interpret metadata as markup or commands. Inspection details omit environment variables, labels, and command arguments.
- Escape selected control, bidi, zero-width, invisible-format, Hangul-filler,
  and Unicode-tag characters in engine-provided container inventory/inspection
  text, image identifiers/tags/inspection text, network metadata,
  membership/confirmation labels, and volume names and optional detail values.
  Limit each engine-sourced display field to 4 KiB of raw or escaped text;
  values over the limit are shown as `[display text omitted]`. Apply this same
  bounded UI display helper to volume names and optional driver, scope,
  creation-time, and host-mountpoint details. Preserve raw names for identity
  comparisons and requests; never access the mountpoint. This is display
  hardening, not secret detection or redaction.
- Use private bounded request paths for version, API negotiation, inventory, inspection, lifecycle, volume usage, and network creation/removal. Version/inventory/inspection and event-subscription setup use an 8-second timeout; lifecycle requests use a bounded 30-second timeout for graceful Stop/Restart; only the filtered volume-usage request uses a 30-second timeout with a 16 MiB response limit. Network creation uses bounded JSON `POST /v{negotiated}/networks/create` with only `Name`, a 1 KiB request-body limit, a 64 KiB success-response limit, and a safe typed response/error path. Network removal uses a bounded eight-second `DELETE /v{negotiated}/networks/{id}` after validating a full 64-character hexadecimal ID; it has no force/prune parameters, does not consume daemon response bodies, and exposes only safe typed errors. Timed-out or transport-interrupted mutations may still change state; never log or expose raw response bodies. Container Remove requires confirmation and sends neither force nor volume-removal parameters.
- Limit each typed inventory sequence to 65,536 entries, reserving at most the
  smaller of its size hint and cap. Before decoding a container, image,
  network, or volume list into typed DTOs, count array entries recursively
  across the response and reject totals above 262,144. This prevents per-row
  `Names` and `RepoTags` arrays from multiplying the memory limit; do not
  truncate. Keep the existing 16 MiB body and eight-second request limits.
  Errors remain static and exclude response contents.
- Restrict volume listing, inspection, and usage to the selected local Docker Unix socket on Linux. Retain/display only volume name, driver, scope, creation time, and mountpoint as untrusted plain text; never access the mountpoint. Apply the shared 4 KiB raw-or-escaped UI display bound to names and each optional driver, scope, creation-time, and host-mountpoint detail, showing `[display text omitted]` when over limit; preserve raw names for identity comparisons and API request values. The separate filtered read-only `GET /v{api}/system/df?verbose=true&type=volume` provides usage only for an exact inventory-matched volume; retain/display only `Size` in bytes and point-in-time `RefCount` when reported. Missing, negative, or unknown metric values remain unavailable, and usage failures appear separately while successful inspect metadata remains visible. Discard unknown daemon fields, labels, options, status, and raw response bodies. A reference count is not a deletion-safety guarantee.
  EngineClient also omits optional volume metadata above the same 4 KiB
  raw-or-escaped limit before retaining it, so the page shows those fields as
  unavailable. The UI applies the display cap again before rendering.
  Create only by sending `Name` to the negotiated `POST /v{api}/volumes/create` endpoint. Validate nonblank names of at most 4096 UTF-8 bytes, reject control or selected invisible formatting characters and `.`/`..`, and send no driver, labels, or options. Bound the request body to 16 KiB, response to 64 KiB, and total request to eight seconds. Require the response name to match the submitted name exactly; escape selected control, bidi, and invisible formatting characters in optional display fields as visible `\u{...}` text.
  Remove only a selected listed volume whose matching inspection reports the exact name and whose exact-name usage snapshot reports `Some(0)` references. Require explicit confirmation naming the volume; positive or unavailable counts cannot be submitted. Send only the negotiated `DELETE /v{api}/volumes/{name}` without `force`, bound it to eight seconds, and do not consume or expose daemon response bodies. The usage count is not atomic with deletion; Docker may reject the request if dependencies change. Timeouts or transport errors may follow a successful removal. Permit one volume mutation at a time, block context switching while pending, and refresh inventory after every result. Do not offer pruning, modification, connection, disconnection, or filesystem browsing. Podman/rootless remains unvalidated and remote endpoints remain unsupported; issues #17 and #10 stay open.
- Treat duplicate/conflicting volume-usage `VolumeUsage`, `Items`, `Name`, or
  `UsageData` keys and repeated exact-name items as ambiguous; their usage is
  unavailable and cannot satisfy the `RefCount == 0` removal gate.
- Emit local structured process traces for volume create/removal with only static action and `success`, `not_submitted`, or `request_error` outcome. Do not log volume names, mountpoints, endpoints, raw errors, or daemon bodies. A request error does not prove engine state remained unchanged; these are not a persistent or tamper-proof audit store, and the separate durable redacted copy is described below.
- Keep network listing and inspection allowlisted on the selected Linux local Docker Unix socket; create networks with bounded `POST /v{negotiated}/networks/create` containing only non-whitespace, at-most-255-byte `Name`, with no driver, IPAM, or options controls. Retain/display only allowlisted network metadata and the aggregate IPAM counts described in the next bullet; discard raw IPAM and `Status` fields. From inspect, retain the point-in-time `Containers` count and its keys only when every key is a full 64-character hexadecimal ID; discard endpoint values. The UI permits connect/disconnect only for a selected local custom network with a fresh complete membership set and a target full ID in current inventory whose state is `Running`; stopped, paused, and unknown states are disabled. Container-change events invalidate cached state; Networks refreshes both inventories and disables attachment actions until container inventory is current. Require explicit confirmation naming the exact network and container; send only `Container`, no endpoint configuration or `Force`, with an eight-second deadline and no retry. Disconnect uses Docker's default `Force=false`. Refresh network/container inventory and re-inspect after every result; treat transport errors/timeouts as potentially state-changing and stale refresh as requiring new inspection. Removal excludes `bridge`, `host`, and `none`, requires fresh matching inspection with `Some(0)` attached containers and explicit full-ID confirmation, and sends an eight-second DELETE without force/prune. Counts are not atomic with mutations. Keep one mutation in flight and block context switching; Podman/rootless and remote support remain unvalidated.
- Request verbose network inspection only on the selected Linux local Docker Unix socket. Validate each subnet key as an IPv4/IPv6 CIDR and normalize its network address/prefix before duplicate detection; invalid CIDRs or duplicate normalized subnet keys make both aggregate metrics unavailable. Retain only aggregate point-in-time `IPsInUse` and `DynamicIPsAvailable` allocation counts across reported subnets, aggregate with checked arithmetic, and leave only the corresponding metric unavailable when its count is missing, malformed, ambiguous, negative, or overflowing, or its checked sum overflows; the other metric may remain available. Never retain subnet/CIDR keys, addresses, endpoint data, or raw status. These are allocation counts, not traffic totals; per-container/interface counters are not aggregated into per-network traffic.
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

- Anonymous image push is limited to one existing `repository:tag` on the
  selected image and the selected Linux local Docker Unix socket. Ferrum
  reinspects the selected ID and verifies that tag before submitting the push;
  this point-in-time check is not atomic with another Docker client. Require
  confirmation naming the image ID and target, and warn that layers and
  metadata may contain secrets. Never read Docker config, credential helpers,
  or stored credentials. The generated API schema marks `X-Registry-Auth`
  required, but the Docker/Moby client omits it for anonymous access and sends
  an empty JSON object body; Ferrum follows that path without credentials. A
  registry requiring auth rejects the operation. Always pass one explicit tag,
  never the daemon's all-tags behavior. Use bounded, sanitized progress and
  discard daemon error details and response bodies. Hiding progress or
  invalidating the page suppresses UI updates but continues draining until a
  terminal event, keeping the image-mutation gate closed. If the request or
  stream fails ambiguously without a terminal response, latch the image-mutation
  gate for the rest of the application session. An interrupted push may have
  transferred layers; closing/restarting Ferrum does not guarantee daemon-side
  work stops.
- Read image layer history only from the selected Linux local Docker Unix
  socket. Bound the response body to 16 MiB and deserialize at most 65,536
  typed rows, then probe one additional value as `IgnoredAny` to reject excess
  without materializing a full response vector. Retain only validated full
  SHA-256 layer IDs, nonnegative timestamps, and sizes; discard `CreatedBy` text,
  tags, comments, and all other daemon fields. This keeps command text out of the
  UI model but does not detect secrets in other image metadata.
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
  pull/push/tag/remove. Page/context invalidation ignores stale UI results but does
  not abort or claim to undo a possibly submitted removal; refresh inventory
  after success only while Images is current.

- Emit a structured local tracing event when a Ferrum container lifecycle
  request returns. Record the action, `success` or `request_error` outcome, and
  a container ID only when it is exactly 64 hexadecimal characters. A request
  error does not prove that the engine state remained unchanged. Do not log
  container names, endpoints, raw errors, or daemon response bodies. These
  events use the configured process logger; the durable copy is described
  below.
- Emit a structured local tracing event when any network mutation returns.
  Record only the static create/remove/connect/disconnect action, `success` or
  `request_error`, and validated full network IDs when available;
  connect/disconnect events include a container ID only when it is also a full
  64-character hexadecimal ID. Do not log network/container names, endpoints,
  raw errors, or daemon response bodies. A request error does not prove engine
  state remained unchanged; the durable copy is described below.
- Emit local structured tracing events when image pull, push, tag, or removal
  requests return, containing only the static action and `success`,
  `not_submitted`, or `request_error` outcome. `not_submitted` identifies local
  validation rejection before HTTP submission. Do not log image IDs,
  references, repository tags, endpoints, raw errors, or daemon bodies. A
  `request_error` does not prove that engine state remained unchanged; the
  durable copy is described below.

- Maintain a Linux-local bounded JSONL copy of Ferrum-originated mutation
  outcomes for container lifecycle, image pull/push/tag/removal and Buildx
  image builds, volume create/removal, and network
  create/removal/connect/disconnect. Lazily create `audit.jsonl` in the
  descriptor-anchored Ferrum config directory, with directory mode `0700` and
  file mode `0600`. Schema v1 contains a timestamp, static event/action/outcome,
  and validated full resource IDs only where existing traces permit. A build
  record contains only static `image_mutation` / `build` and outcome fields;
  build output and build-specific metadata are never persisted. Exclude names,
  image references/tags, endpoints, credentials, raw errors, daemon bodies, and
  terminal/log contents. Sync each record; enforce a 16 MiB hard cap without
  automatic truncation or rotation. Use nonblocking interprocess-lock attempts
  with a 250 ms deadline; continued contention counts as a persistence failure.
  At capacity or on persistence failure, emit a static process warning and do
  not claim the event is durable; do not change the engine request outcome.
  This is local Ferrum-request history, mutable by the same-UID user, not
  tamper-proof, not proof of final engine state, and does not record external
  engine actions.
- Keep historical/live container logs restricted to the selected local Unix-socket engine. Use the negotiated API version and streaming response body; accept only json-file/journald drivers. TTY is read from allowlisted `Config.Tty`: TTY output is raw and combined, while non-TTY output is decoded from Docker's 8-byte multiplex frames. Render logs as plain text, never as a terminal. Bound frames to 1 MiB, lines to 64 KiB, retention to 8 MiB or 10,000 lines, and delivery to a dedicated 16-message channel; discard oldest retained lines with a visible count. Pause suspends UI draining, applying bounded backpressure to producer reads without unbounded memory or dropped lines; Resume continues delivery. Copy and Export include only currently visible, retained rows and escape terminal control characters as `\xNN`.
- Escape untrusted container-log text once when adding rows to `LogBuffer`:
  C0/C1 controls become visible `\xNN` text and selected bidi, zero-width, and
  invisible formatting characters become `\u{...}` text. The stored safe string
  is the one rendered in Logs rows and used by search, Copy, and Export. Count
  the escaped size against the existing 8 MiB retention bound and drop a line
  if its escaped form would exceed it. This is display sanitization, not secret
  detection or redaction.
- For visible log export, write the complete selected text to a unique sibling
  temporary file and atomically replace only the selected directory entry. New
  files are `0600`; existing writable regular targets retain ordinary rwx mode
  bits. Final-component symlinks and non-regular targets are rejected, and
  hard-link aliases keep the old contents. A failure before rename leaves
  the target unchanged; Ferrum attempts to unlink the temporary file.
  Replacing a target requires write and search access to its directory and
  creates a new inode owned by the current user; group ownership follows
  directory creation policy. Extended ACLs and other xattrs are not carried
  forward. Parent-path trust and crash durability are not expanded.
- Keep the experimental embedded terminal restricted to the selected local Docker-compatible Unix socket on Linux and the negotiated API version. Terminal output is untrusted; cap each raw OSC string at 1024 bytes before feeding vt100. On overflow, stop feeding the parser and discard the string tail until BEL, CAN, SUB, or ESC; pass the actual terminator to the parser to reset its state before resuming. This keeps vte's default-std OSC buffer bounded while the string remains open. Ignore OSC 52 clipboard and title requests, expose clipboard actions only to explicit user controls, and never put terminal bytes or daemon bodies in logs or errors. Closing the exec session closes its connection only; it does not stop or remove the container.
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
- Compose profiles, `.env`/service `env_file` resolution, remote OCI/Git inputs
  (remote loaders are disabled during validation), file import, project
  lifecycle actions, build/pull/logs, and project mutations.
- The explicit local Docker CLI Buildx invocation is an added external build process. Other external process integrations, archive extraction, and file-path surfaces remain deferred.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary. UI state gating and confirmation reduce accidental destructive operations but cannot constrain a compromised or malicious engine endpoint.

## Validation plan

Read-only live-engine tests cover version, inventory, and inspection without
resource mutation. The optional ignored network list/inspect test requires
`FERRUM_TEST_NETWORK` to name an existing local network and only inspects its
matching listed ID. The separate ignored network create/remove test preflights
a generated name, verifies the returned full ID against the exact inventory
name before arming cleanup, removes only that ID, and confirms its absence.
Ownership uncertainty never triggers name-based cleanup or prune.

The optional ignored Compose-validation smoke uses a controlled temporary
project and the installed CLI to exercise relative-path resolution without
contacting an engine or modifying user project files. The default Compose tests
use controlled subprocess fixtures and headless UI state coverage.

The optional ignored Compose remote-loader safety smoke uses Git/OCI-shaped
`include` references that target only an ephemeral `127.0.0.1` listener. It
passes only when the installed CLI rejects those references without a local
connection; it contacts no external host or engine.

The opt-in network connect/disconnect smoke test requires the current local
Unix Docker context and a preinstalled `FERRUM_TEST_IMAGE` containing
`/bin/sleep`. It preflights a unique network name, creates that network and a
container with `--network bridge --pull never`, no host mounts, and no published
ports; it never pulls an image. The network cleanup guard is armed only after a
fresh inventory matches the create response's full ID to the generated name.
A create error, mismatched ID, or failed inventory read does not authorize
cleanup; the network may remain for manual inspection. The container guard is
armed only after its full ID matches the exact generated name in inventory.
Once armed, both guards use the same pinned `EngineClient` socket and exact
IDs without force or prune. If ownership cannot be confirmed, no unverified
stop/remove request is sent and the resource may remain.
After exact ID/name verification, it starts the container, connects and inspects
membership, disconnects and re-inspects, then stops and removes only the owned
IDs without force or prune. Run only against a disposable local engine; the
default suite skips it.

Fake-socket attachment tests verify exact versioned POST paths, the minimal
`Container` body, omitted endpoint configuration/force, invalid IDs before
connection, and typed redacted failures. Headless Networks tests cover complete
membership and exact inventory-ID gates, the required `Running` target state,
connect/disconnect confirmation and cancel, stale selections/results, and
single-flight/context-switch gates. They do not contact a live engine; native
UI click/visual validation remains unverified. IPAM allocation counts are
implemented as described above; per-network traffic totals are not implemented.
Issues #17 and #10 remain open; Podman/rootless and remote behavior are not
claimed.
Fake-Unix-socket image protocol tests and headless Images UI tests cover tag/removal and anonymous push request/confirmation behavior without contacting a real engine or registry, and without mutating image resources. Push tests verify exact image/tag preflight, one explicit target tag, the empty anonymous JSON body with no auth header, bounded progress, safe HTTP rejection, and daemon error redaction. No live image push, deletion, or other image mutation test is claimed. Fake-socket image protocol tests, headless Images UI tests, and controlled helper-process Buildx tests do not contact a real engine or mutate images. There is no live pull, tag, removal, image build, or image-mutation test. Separate opt-in ignored lifecycle, logs, and terminal tests mutate engine state using one uniquely named disposable container and only its validated full ID as cleanup authority. Logs and terminal tests require locally installed images and never pull, tag, push, or delete images. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).

The optional ignored volume create/remove test uses the current local Docker
context's parsed Unix socket path for EngineClient operations. It preflights a
generated name, then arms cleanup only after the create response and inventory
confirm that exact name; the test also requires the exact-name usage snapshot
to report `RefCount == 0`. It disarms the guard before calling
`EngineClient::remove_volume` and confirms the name is absent. The Drop fallback
uses Docker CLI `--host unix://...` on the same pinned socket without force only
if the test unwinds before the removal request is submitted. There is no
name-based retry after submission; a failed request may leave the volume. If
ownership cannot be confirmed, no removal is sent. The separate
`FERRUM_TEST_VOLUME` live test is read-only. Default fake-socket and headless UI
tests do not contact a live engine.
