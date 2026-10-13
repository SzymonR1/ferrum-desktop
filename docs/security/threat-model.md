# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. Ferrum provides container inventory and inspection, volume list/inspect/usage with name-only creation and dependency-gated removal, image listing/inspection and bounded read-only history, anonymous-by-default pulls and pushes with opt-in Linux helper authentication, tagging/removal and confirmed local Buildx builds, scoped container lifecycle actions, network list/inspect/create/removal/connect/disconnect on the Linux local-engine path with aggregate IPAM allocation counts when reported, logs, and an experimental embedded terminal.

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
context path, tag, and output. A Buildx spawn failure is `not_submitted`;
an unsuccessful outcome after a child starts is `request_error` because
builder-side state may be uncertain. The redacted outcome is also written to
the existing best-effort durable JSONL audit; only static event/action/outcome
are persisted for builds. Build output remains bounded transient UI data and is
never persisted.

Compose support includes read-only Docker CLI version/project discovery,
explicit structural validation of a user-selected local base file followed by
ordered local overrides, and two bounded read-only declared-service inventories
from successful validation on Linux. The selected-profile `config --services`
list preserves existing lifecycle eligibility; a separate
`config --profile '*' --services` query supplies all declared services for
inventory/scale, including inactive-profile services. Status reports existing
containers for selected files; it does not establish that the configuration has
no other services. Files are limited to 16 total. Docker merges `-f` files in
supplied order; relative paths resolve from the first/base config or
`--project-directory`, which Ferrum sets to the base file's parent. See Docker's
[Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for ordered merge/path semantics and [`config`](https://docs.docker.com/reference/cli/docker/compose/config)
for `config --no-interpolate --no-env-resolution --services`.
On Linux, explicitly selected base and override files have a combined 16 MiB
byte limit (exactly 16 MiB is accepted); larger selections are rejected before
any Docker/Compose process is launched. This is a point-in-time check, not an
atomic security boundary: a same-UID process may change or replace a selected
file after it is checked. After the Compose version preflight, Compose reopens
the original selected paths for validation, so the contents read may differ
from those measured by the check. The check does not include local files read
through `include` or service `extends.file`.
Linux Compose subprocesses for version, discovery, validation, status, one-shot
and live service-log queries, start/stop/restart/scale/up have per-process
`RLIMIT_AS` of 4 GiB and `RLIMIT_DATA` of 512 MiB, preserving lower inherited
limits.
These limits apply
while Compose reads selected and included files. They are inherited by child
processes and bound each process, not aggregate process-tree RSS; they are not a
sandbox. The private child environment sets `GOMEMLIMIT=402653184` (384 MiB), a
Go runtime soft target that may be exceeded to avoid excessive GC thrashing.
Resource exhaustion may produce generic validation, status, log, or lifecycle
failures.

Compose remains trusted local code, not a sandbox. The point-in-time selected-
file size check does not eliminate the race with same-UID changes, and neither
it nor the process limits confine included local-file reads or eliminate the
need to validate only trusted files.

Validation skips interpolation and service `env_file` resolution, uses no
engine, and does not mutate project files. Compose can still recursively read
local `include`/`extends.file` Compose files and included projects' default
`.env` and explicit `include.env_file` paths under the logged-in user's
permissions, potentially outside the base directory. Remote Git/OCI loaders
are disabled. The selected files and private CLI environment do not sandbox
those reads; validate only trusted files. Explicit profiles affect validation,
read-only status, one-shot service logs and live service-log follow, and imported
configuration start/stop/restart/scale/pull/up. Lifecycle is available only for
the currently selected, session-imported, explicitly revalidated files/profiles
and the selected local Docker Unix socket. Existing service actions retain
selected-profile service eligibility. Status and scale counts use read-only
selected-profile and `--profile '*'` `ps --all --format json` queries so
inactive-profile and stopped containers are counted without changing service
action eligibility. Before service restart Ferrum requires current status
membership, revalidates the files/profiles, and makes a fresh selected-profile
status query on the same socket before mutation. Failure or absent membership
submits no restart. The import label and engine-reported `Project` metadata do
not establish identity or authorization.
Compose `start` starts existing containers; `stop` does not remove them.
Service restart invokes only `restart --no-deps <SERVICE>`; Docker Compose
restart does not apply configuration changes. These lifecycle operations do
not build, pull, remove resources, or persist project storage. Confirmed scale,
pull, and `up` are separate operations described below. Navigation
does not cancel submitted lifecycle work; its redacted result is retained for
the session. A static `started` audit entry precedes preflight; without a later
terminal outcome, the final state is unknown and must be reconciled by
refreshing the current project status. Issues #18 and #19 remain partial/open;
issue #10 remains blocked/open.

Saved Compose registrations are an explicit local opt-in in the existing
descriptor-pinned settings store, not a second persistence layer. Each stores a
unique bounded label, 1–16 ordered absolute UTF-8 file paths, and validated
profiles; project contents, environment values, credentials, Engine identity,
and commands are excluded. Schema v2 migrates v1/versionless settings with an
empty list. Invalid persisted entries are skipped and warned about without
becoming selected or validated. The settings file has a 256 KiB cap, and saved
entries are limited to 32. Loading, selecting, saving, and forgetting execute
no Docker/Compose CLI and make no Engine request; selection only clears stale
state and requires explicit revalidation. Forget only removes the registration,
not files or resources. Persisting paths neither reads project contents nor
confines paths. Compose may read referenced files under the user's permissions,
so validate only trusted projects.

The service inventory uses separate selected-profile and all-profile Compose
CLI queries. The latter uses `--profile '*' config --services` to include every
declared service, including inactive profiles. Each response is capped at 128
KiB, 1024 rows, and 128 bytes per name; malformed or duplicate output fails
with a static error. Escaped service names are shown in a bounded virtualized
list. This is local CLI/file-derived data, not container status; these queries
make no Engine requests.
No release or broader support claim is made.

Status is available only after selected files pass structural validation and a
local Docker Unix socket is connected. Ferrum merges two read-only
`compose ps --all --format json` queries on that socket: one with the selected
profiles and a second with `--profile '*'`. The selected-profile result remains
the source for regular service-action eligibility; the all-profile result
provides accurate counts for scaling inactive-profile services. `--all`
includes stopped containers and Compose's default orphan inclusion is
preserved. Ferrum retains bounded, escaped container name, service, optional
per-container `Project` metadata, state, and optional health; when `Project` is
omitted, the UI shows `Not reported`. Values may vary row by row. The Project
value is untrusted engine-reported display metadata only; it does not establish
file ownership, canonical identity, authentication, or authorization and is
never used for lifecycle operations. Ferrum discards IDs, commands, published
ports, exit codes, and unknown fields. Empty output means no existing
containers matched the isolated project selection, not that the file declares
no services. Status is a manually refreshed point-in-time snapshot.

Status uses a private CLI configuration, clears inherited shell variables, and
sets `COMPOSE_DISABLE_ENV_FILE=1`. Environment-dependent projects may fail or
resolve differently than a normal shell invocation. Compose may still read
local includes and included-project environment sources under the logged-in
user's permissions; validate only trusted files.

One-shot Compose service logs are available only after structural validation
and a successful status snapshot. Immediately before `logs`, Ferrum repeats a
read-only `compose ps --all --format json` query with the same selected files
and socket, and refuses if the service is no longer present. The `--tail 200`
option is per container; Ferrum displays at most the first 200 combined CLI
output lines and 256 KiB, so replicas may be truncated and output is not a
global chronological tail. Only a bounded safe ASCII selector (1–128 bytes,
letter/digit first, then ASCII letters/digits/`_`/`-`/`.`) is accepted.

Logs may contain secrets and are displayed only after explicit user action.
They are treated as untrusted text, escaped before rendering, and kept only in
transient UI state; no log content enters tracing, audit, error strings, or
disk. Each of the fresh status and logs subprocesses is bounded to eight
seconds and 1 MiB stdout and inherits the private CLI environment and process
resource limits above. Selected files can change between status and snapshot;
local includes remain in the trusted-Compose caveat. This is a one-shot
snapshot, not a follow stream, and it does not mutate project/engine state.
Tests use fake CLI output; no real user project logs were read.

Live Compose service follow is available only for an imported, explicitly
validated selection and a service in current status. Before `logs --follow
--tail 200 --no-color SERVICE`, Ferrum repeats `compose ps --all --format json`
with the same selected files, profiles, and local socket and refuses absent or
failed membership checks. The CLI stream is read-only, has no total deadline,
and does not retry; a separate 16-event channel provides backpressure. Raw
lines are capped at 4096 bytes, overlong tails are discarded through newline,
and the UI retains at most 200 combined lines/256 KiB. Output is escaped and
transient, never stored, logged, or audited. Stop, config/status/page/context
invalidation, and shutdown terminate Ferrum's CLI process group. This does not
mutate containers; no real service logs were read or visually validated.
Selected and included Compose files remain trusted local inputs as described
above.

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local Linux Unix socket. Access to a Docker socket is effectively host-level control; UI action gating and confirmation do not make the endpoint unprivileged. Windows/macOS engine APIs are not implemented here. Engine responses, container names, image references, state strings, status text, and mount paths are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Default Linux discovery checks `$XDG_RUNTIME_DIR/docker.sock`,
  `$XDG_RUNTIME_DIR/podman/podman.sock`, then `/var/run/docker.sock`. The
  Podman path is a rootless user-socket discovery candidate only; no Podman API
  compatibility is claimed. Ferrum does not add a dedicated rootful Podman
  socket candidate or change socket permissions. Connecting to an existing
  socket may trigger its user-configured systemd socket activation; Ferrum does
  not invoke Podman or systemd, or enable a unit.
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
  plus `--project-directory <base-parent> config --no-interpolate
  --no-env-resolution --services`. The same invocation returns declared
  service names; it does not use `--quiet` or start a second service-listing
  process. Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
  specifies ordered `-f` merge and path resolution relative to the first config
  or `--project-directory`; the [`config` reference](https://docs.docker.com/reference/cli/docker/compose/config)
  documents these options.

- Keep validation read-only, engine-independent, and bounded to eight seconds
  and 1 MiB stdout per command. Before launching any Docker/Compose process,
  reject a combined selected base-and-override file size above 16 MiB; exactly
  16 MiB is accepted. This is a point-in-time size check, not atomic against
  same-UID changes; Compose reopens the original paths after preflight. This
  applies only to explicitly selected files, not local `include`/`extends.file`
  files. Success is structural validation only; interpolation and service
  `env_file` resolution are skipped. Do not mutate, extract, or write project
  files.

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
- Accept optional comma-separated Compose profiles for validation, read-only
  status, one-shot service logs and live follow, and imported-configuration
  start/stop/restart/pull/up/scale. Validation retains selected-profile
  services for existing lifecycle eligibility and separately queries all
  declared services with `--profile '*' config --services`. Status/counts use
  read-only selected-profile and all-profile `ps --all` queries. Limit input to
  1024 UTF-8 bytes, 16 profiles, and 64 UTF-8 bytes per name. Trim outer
  whitespace per token; blank means no explicit profiles. Reject empty entries
  and duplicate names. Accept `[A-Za-z0-9][A-Za-z0-9_.-]+` names and standalone
  `*`; pass each selected profile as its own `--profile <name>` argv before
  validation/status/log/lifecycle commands, never through an environment
  variable. Scale `up` is the exception: omit all global `--profile` flags so
  explicitly targeting one profile-gated service activates that service alone,
  not its profile siblings. Use selected profiles for regular service-action
  status rechecks. Invalid or changed selection clears validation/status/log
  output, cancels read-only/follow tasks, and requires revalidation; already-
  submitted mutations are not cancelled.
- Keep private Docker config/HOME, cleared allowlisted environment, disabled
  selected-project `.env`, and disabled remote Git/OCI loaders. An explicitly
  opted-in persistent registration stores only a unique bounded label, ordered
  absolute UTF-8 paths, and validated profiles in SettingsStore. It stores no
  project content, environment values, credentials, Engine identity, or
  commands; entries cap at 32, and the settings file caps at 256 KiB.
  Session imports remain transient unless saved. Loading, selecting, saving,
  and forgetting execute no CLI or Engine request; restore is unselected and
  unvalidated. Selection clears old results and requires explicit
  revalidation. Forget removes only the registration and leaves the current
  entry session-only. Persisted paths are not confined and project contents
  are not read during save.
Only the currently selected imported registration after successful explicit
revalidation can start/stop/restart/scale/pull/up. Revalidate before each
operation; failure must prevent the requested operation. Restart additionally
requires current successful status membership and a fresh selected-profile
status query with the same local Docker Unix socket before mutation. Scale
requires all-declared-service membership and an observed count matching a fresh
all-profile `ps --all` query, in addition to selected-profile snapshot
validation/status; the mutating `up` has no `--profile` flags. The current
socket, ordered files, profiles, and base-parent directory scope operations,
not the local label or engine-reported `Project` metadata. Docker `start` starts
existing containers; `stop` does not remove them. Restart uses only
`restart --no-deps <SERVICE>`; Compose restart does not apply configuration
changes. Start/stop/restart do not build, pull, remove resources, or persist
project storage. Confirmed pull, scale, and `up` are separate operations.
Pull is limited to selected imported, explicitly revalidated files and profiles
on the current local Docker Unix socket. It uses a private config/HOME and
cleared environment; saved Docker credentials/helpers are not read, and private
registries may reject anonymous pulls. Pull changes the selected Engine's image
store but does not build or change project containers, networks, or volumes.
  Progress retains only allowlisted phases/counts; combined stdout/stderr is
  capped at 1 MiB, and raw CLI output is not shown or persisted. Preflight
  cancellation submits no pull. After submission, cancellation, timeout, or
  output-limit termination kills only the local CLI process group; Engine-side
  image downloads may continue and the result is uncertain.
  Compose build is separately confirmed and limited to the same selected,
  imported, successfully revalidated ordered files/profiles and local socket.
  It uses fixed `compose --progress plain --parallel 2 ... build` argv. Ferrum
  adds no `--push`, command-line `--ssh`, builder options, or extra command-line
  build args. Compose-file `build.args` remain active and may expose secrets; never
  store credentials there. Compose-file `build.ssh` may grant SSH-agent access,
  and `build.secrets` may expose configured secret contents to build instructions.
  Inspect both and build only trusted projects. Saved credentials/helpers,
  proxy environment, and user Docker config are not passed. The private CLI
  config/HOME,
  disabled selected `.env`/remote Git/OCI includes, child limits, two-hour
  deadline, and 1 MiB output cap constrain Ferrum's CLI, not project behavior.
  Only bounded, rate-limited allowlisted phase/count summaries are shown; no raw
  output is persisted.
  Build runs project-controlled instructions with access to Compose-defined contexts and
  may read beyond selected YAML, fetch remote contexts/base images, execute
  arbitrary builder/network code, consume resources, and change the selected Engine
  image store; existing local image tags may be reassigned. Review `.dockerignore`;
  build only trusted project files. This is not sandboxed or path-contained.
  Anonymous auth only; private credentialed registries may fail. Preflight
  process group before submission. Once submitted, cancellation kills only the
  local CLI group; the builder/daemon may continue. Timeout/output overflow
  leave image state uncertain; no retry occurs. Build audit retains only static
  `compose_build`/`build` start and outcome values. Fake-CLI/temp-fixture tests
  do not mutate Engine resources or contact registries/remote contexts.
- Allow confirmed Compose `up` only for the current imported selection after
  its ordered files/profiles have been revalidated and a current status query
  has succeeded. Use sealed snapshot descriptors and the selected Linux local
  Docker Unix socket with fixed `up --detach --no-build --pull never` arguments.
  It creates/starts project containers and networks and may create missing
  project volumes. It may recreate existing containers when configuration or
  image changes and uses only local images. It does not build or pull images,
  remove orphan containers, remove volumes, or force recreation. Missing images
  require separate Build/Pull actions; Compose pulls are anonymous only.
  Running services may write to configured volumes and host mounts.
  Confirmation warns to use only trusted project files and discloses
  container/network/volume creation, container recreation, and local-image
  requirements. Project configuration and service code run on the selected
  Engine; this is not a sandbox and no saved credentials are passed.
  Use a 30-second deadline and 1 MiB stdout cap; raw stdout is discarded and
  stderr is suppressed. Audit only static `up` action and the `started`,
  `success`, `not_submitted`, `request_error`, or `uncertain` outcomes; exclude
  paths, labels, and project metadata. Any post-spawn failure, including
  timeout, is uncertain. Require a successful current status snapshot before
  each confirmation; keep the action disabled while refresh is pending or
  failed. Automatic refresh runs only after completion while the same page and
  selection remain current; refresh successfully after navigation or failure.
  Controlled fake-CLI/headless tests cover command semantics and the status
  retry gate; no live mutation ran because no disposable Engine is designated.
  This slice does not claim Podman, remote, Windows/macOS, full issue completion,
  support expansion. See Docker's [official Compose `up` reference](https://docs.docker.com/reference/cli/docker/compose/up/) and
  [volume documentation](https://docs.docker.com/reference/compose-file/volumes/).
  Remote references remain disabled and are not fetched. Existing temporary-
  file/status smokes do not validate arbitrary projects. Native page visual
  check is unavailable.
- Allow confirmed service-scoped Compose scaling only for the current imported
  selection after ordered files/profiles are explicitly revalidated and
  successful selected-profile and all-profile `ps --all` snapshots are
  available. Provide one control per all-declared service, including inactive
  profile services, use the all-profile observed count including stopped
  containers, and leave the target blank until the user enters an integer
  1–32; disable every Scale button while the target is empty or invalid.
  Confirm the imported local label (not project identity), service, current
  count, and target. Warn about excess-container removal, creation,
  configuration recreation, writable-layer data loss on removal/recreation,
  mounted-volume Compose semantics, and that `up` can start stopped containers
  and run configured service code on the selected Engine. State that
  dependencies are not started and a profile-gated target is activated without
  enabling its sibling services.
  Snapshot selected files into write-sealed descriptors, revalidate them with
  the selected profiles, and recheck membership/all-profile `ps --all` count
  immediately before fixed `up --detach --no-build --pull never --no-deps
  --scale SERVICE=N SERVICE` argv. The mutating `up` must not receive global
  `--profile` flags; explicit service targeting activates only its required
  profile. Do not request build, pull, volume/orphan removal, or force
  recreation. The operation is Linux local Docker Compose CLI only, bounded to
  30 seconds, using the isolated CLI environment and child limits. Any submitted
  error, timeout, or cancellation is uncertain; require successful status
  refresh before retry. A cancelled confirmation records only static
  `scale/not_submitted`; audit excludes target/count, service, project label,
  file/profile/socket data, and output.
  The ignored live smoke uses a unique generated project with a profile-gated
  target and same-profile sibling, preflights project/network absence, and
  verifies only the target starts/scales. It uses a preinstalled image, no
  mounts/ports/pull/build, then attempts generated-project-scoped `down` without
  force, prune, or volume removal. It has not been run; live Engine validation
  remains pending with the integration owner. Issue #18 remains partial/open.
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
  Remove only a selected listed volume whose matching inspection reports the exact name and whose exact-name usage snapshot reports `Some(0)` references. Require explicit confirmation naming the volume; positive or unavailable counts cannot be submitted. Send only the negotiated `DELETE /v{api}/volumes/{name}` without `force`, bound it to eight seconds, and do not consume or expose daemon response bodies. The usage count is not atomic with deletion; Docker may reject the request if dependencies change. Timeouts or transport errors may follow a successful removal. Permit one volume mutation at a time, block context switching while pending, and refresh inventory after every result. Do not offer pruning, modification, connection, disconnection, or filesystem browsing. Podman/rootless remains unvalidated and remote endpoints remain unsupported; issue #10 remains open and blocked for the broader endpoint matrix.
- Treat duplicate/conflicting volume-usage `VolumeUsage`, `Items`, `Name`, or
  `UsageData` keys and repeated exact-name items as ambiguous; their usage is
  unavailable and cannot satisfy the `RefCount == 0` removal gate.
- Emit local structured process traces for volume create/removal with only static action and `success`, `not_submitted`, or `request_error` outcome. Do not log volume names, mountpoints, endpoints, raw errors, or daemon bodies. A request error does not prove engine state remained unchanged; these are not a persistent or tamper-proof audit store, and the separate durable redacted copy is described below.
- Keep network listing and inspection allowlisted on the selected Linux local Docker Unix socket; create networks with bounded `POST /v{negotiated}/networks/create` containing only non-whitespace, at-most-255-byte `Name`, with no driver, IPAM, or options controls. Retain/display only allowlisted network metadata and the aggregate IPAM counts described in the next bullet; discard raw IPAM and `Status` fields. From inspect, retain the point-in-time `Containers` count and its keys only when every key is a full 64-character hexadecimal ID; discard endpoint values. The UI permits connect/disconnect only for a selected local custom network with a fresh complete membership set and a target full ID in current inventory whose state is `Running`; stopped, paused, and unknown states are disabled. Container-change events invalidate cached state; Networks refreshes both inventories and disables attachment actions until container inventory is current. Require explicit confirmation naming the exact network and container; send only `Container`, no endpoint configuration or `Force`, with an eight-second deadline and no retry. Disconnect uses Docker's default `Force=false`. Refresh network/container inventory and re-inspect after every result; treat transport errors/timeouts as potentially state-changing and stale refresh as requiring new inspection. Removal excludes `bridge`, `host`, and `none`, requires fresh matching inspection with `Some(0)` attached containers and explicit full-ID confirmation, and sends an eight-second DELETE without force/prune. Counts are not atomic with mutations. Keep one mutation in flight and block context switching; Podman/rootless and remote support remain unvalidated.
- Bound each network inspect `Containers` map to 16,384 entries independently
  of the 4 MiB response-body limit. Reject the complete inspection on overflow
  rather than returning a partial count or membership set, bounding retained
  attached-container IDs from an engine-controlled map.
- Request verbose network inspection only on the selected Linux local Docker Unix socket. Validate each subnet key as an IPv4/IPv6 CIDR and normalize its network address/prefix before duplicate detection; invalid CIDRs or duplicate normalized subnet keys make both aggregate metrics unavailable. Retain only aggregate point-in-time `IPsInUse` and `DynamicIPsAvailable` allocation counts across reported subnets, aggregate with checked arithmetic, and leave only the corresponding metric unavailable when its count is missing, malformed, ambiguous, negative, or overflowing, or its checked sum overflows; the other metric may remain available. Never retain subnet/CIDR keys, addresses, endpoint data, or raw status. These are allocation counts, not traffic totals; per-container/interface counters are not aggregated into per-network traffic.
- Image pulls and pushes use the selected local Docker Unix socket. Both are
  anonymous by default and omit `X-Registry-Auth`; each operation requires its
  own explicit UI opt-in before using helper credentials. Persist only the
  `None`/`SecretService`/`pass` helper enum in settings, never credential values.
  Invoke only the corresponding Docker credential-helper binary with argv
  `get`, a validated normalized Docker registry key on stdin, a 30-second
  deadline, and a 64 KiB stdout bound. Clear the child environment except for
  `PATH`, `HOME`, optional `PASSWORD_STORE_DIR`, and the minimum session,
  keyring, and GPG variables needed by the selected helper. Discard stderr and
  output, parsed secret strings, serialized AuthConfig, and encoded auth
  buffers. Ferrum does not directly read Docker `config.json` or its saved
  credential entries. On opt-in, Ferrum transiently consumes the selected
  helper's credential response. Helper failure,
  malformed protocol, invalid registry authority, timeout, or excess output
  returns a static safe error before Engine submission; never fall back
  anonymously after explicit helper use fails.

- Construct Docker Engine AuthConfig JSON only from helper username/secret or
  `<token>` identity token plus the canonical registry server key, then send
  Moby-compatible padded base64url `X-Registry-Auth` only on an opted-in pull
  or push. The Engine receives this credential; Ferrum does not persist or log
  it. Treat both the selected host helper and local Engine as trusted
  components. Docker Engine registry mirrors may forward credentials according
  to daemon configuration; Ferrum does not control or validate downstream
  routing. Users should opt in only when they trust the Engine and its mirror
  configuration. HTTP-stack `HeaderValue` copies cannot be guaranteed wiped.

- Buildx remains credential-free; image pulls and pushes are anonymous unless
  their own per-operation opt-in is selected. Pull/push progress responses
  remain bounded to 1 MiB per HTTP frame and 64 KiB per JSON record; displayed
  status is capped at 256 bytes and selected invisible formatting characters
  are replaced before rendering. Daemon error details are discarded. Hiding
  progress continues draining until terminal response; page/context
  invalidation hides stale progress without aborting the reader. Closing Ferrum
  does not guarantee daemon-side transfer work stops.
- Image push is limited to one existing `repository:tag` on the selected image
  and the selected Linux local Docker Unix socket. Ferrum reinspects the
  selected ID and verifies that tag before submission; this point-in-time check
  is not atomic with another Docker client. Require confirmation naming the
  image ID and target, and warn that layers and metadata may contain secrets.
  Anonymous pushes omit `X-Registry-Auth` and send the empty JSON fallback
  body. When a helper is configured, expose a separate unchecked per-push
  opt-in in the existing confirmation and disclose that the local Engine
  receives the credential and may forward it to registry mirrors. Never read
  Docker config or saved credential entries. Helper failure must stop before
  Engine submission and must not downgrade to anonymous. Always pass one
  explicit tag, never the daemon's all-tags behavior. Use bounded, sanitized
  progress and discard daemon error details and response bodies. Hiding progress
  or invalidating the page suppresses UI updates but continues draining until a
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
  validation rejection or credential-helper failure before Engine HTTP
  submission. Do not log image IDs, references, repository tags, endpoints, raw
  errors, or daemon bodies. A `request_error` does not prove that engine state
  remained unchanged; the durable copy is described below.

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
- Persist schema v2 settings with backward-compatible defaults, including the
  startup auto-connect preference, registry-helper enum, and an empty-default
  saved Compose registration list. Reject unknown fields, cap serialization and
  input at 256 KiB, cap registrations at 32, and keep config and temporary files
  owner-only. Save only a validated label, absolute ordered paths, and profiles;
  never persist project contents or secrets.
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
- Confirmed Compose mutations copy the explicitly selected base and ordered
  overrides into write-sealed Linux memfds before validation. Validation,
  submitted action, and service-action/scale status share those descriptors and
  the original project directory. No named on-disk copy is created, but host
  swap or core dumps may retain memory pages. This binds only explicitly
  selected files; transitive local inputs remain live, uncontained, and
  unsandboxed.
- Compose `.env`/service `env_file` and recursive local `include`/`extends.file`
  sources remain trust risks. Remote OCI/Git loaders are disabled for every
  Compose CLI invocation. Session imports retain path/profile metadata and a
  local label in memory only; explicit saved
  registrations persist only validated labels, ordered absolute paths, and
  profiles as described above. Compose `down`, resource removal, and project-file
  mutation remain deferred. Pulls are limited to the confirmed anonymous
  local-Docker operation described above; authenticated Compose pulls and remote
  engines remain unsupported. Confirmed Compose build is supported only for
  selected imported, revalidated files/profiles on the selected local Docker
  socket as described above; arbitrary credentials and remote engines remain
  unsupported. Scoped start/stop, service restart, and one-service scale are
  available only for a selected imported and revalidated configuration. Restart
  and scale also require current and freshly rechecked status membership/count.
  The bounded one-shot service log snapshot is implemented as described above.
- The explicit local Docker CLI Buildx invocation is an added external build process. Other external process integrations, archive extraction, and file-path surfaces remain deferred.
- Ferrum-managed credential persistence and helper integrations beyond the
  Linux Secret Service/`pass` choices; authenticated Podman/remote-engine
  operations; managed engines; and update delivery remain deferred. Linux
  local-Docker pulls and pushes have separate per-operation opt-ins through
  those helpers; live helper/registry authentication is not validated.

These remaining surfaces require explicit design and tests before implementation or broader support claims. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary. UI state gating and confirmation reduce accidental destructive operations but cannot constrain a compromised or malicious engine endpoint.

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
Podman/rootless and remote behavior remain unsupported or unvalidated under open, blocked issue #10. No broader endpoint or platform support is claimed.

Fake-Unix-socket image protocol tests and headless Images UI tests cover tag/removal and anonymous plus explicitly authenticated push request/confirmation behavior without contacting a real engine or registry, and without mutating image resources. Push tests verify exact image/tag preflight, one explicit target tag, the empty anonymous JSON body without an auth header, the opt-in `X-Registry-Auth` header without a plaintext credential, bounded progress, safe HTTP rejection, and daemon error redaction. Controlled helper-process tests use fake binaries; UI/audit regressions verify one-operation opt-in and a redacted `not_submitted` failure outcome. Controlled helper-process Buildx tests also do not contact a real engine or mutate images. No live image pull, push, tag, removal, build, or other image-mutation test is claimed. Separate opt-in ignored lifecycle, logs, and terminal tests mutate engine state using one uniquely named disposable container and only its validated full ID as cleanup authority. Logs and terminal tests require locally installed images and never pull, tag, push, or delete images. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).

The optional ignored volume create/remove test uses the current local Docker
context's parsed Unix socket path for EngineClient operations and must run only
against a disposable local Docker Engine. It preflights a generated name,
checks the create response and inventory, and requires an exact-name usage
snapshot reporting `RefCount == 0`. Docker volume operations are name-addressed
here and expose no immutable ID; a concurrent delete/recreate under the same
name cannot be distinguished by those checks. The generated-name verification
is best-effort, not a stable ownership proof, and cleanup by name may target a
same-name replacement if another actor races the test. The Drop fallback uses
the same pinned socket without force only if the test unwinds before removal
submission. There is no name-based retry after submission, and a failed request
may leave the volume. If initial preflight/response checks fail, no cleanup is
sent. The separate `FERRUM_TEST_VOLUME` live test is read-only. Default
fake-socket and headless UI tests do not contact a live engine.
