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

The Linux application provides container inventory, inspection, bounded
historical/live logs, state-gated lifecycle actions, image listing and
inspection, anonymous-by-default pulls and pushes with opt-in Linux
credential-helper authentication, local tagging, confirmed removal, and an
experimental embedded exec terminal over the selected local Docker-compatible
Unix socket. Container Remove requires confirmation and does not force-remove
containers or remove volumes. Every image-tag request requires confirmation
because Docker Engine v1.56 may reassign an existing `repository:tag`; the
image data remains. Removal confirmation snapshots the selected ID and known
tags, warns that removing the final reference may delete image data, and
explains that the non-force request may be rejected for images used by
containers or with other tags. Removal omits `force` and sets `noprune=true`.
Image pull/push/tag/removal use only the selected local Linux Docker Unix socket;
Buildx remains credential-free. Ferrum does not support remote engine
connections or read Docker `config.json` credential entries.

Pull and push authentication require both a configured `SecretService` or
`pass` helper and a separate explicit one-operation checkbox; anonymous pulls
and pushes remain the default. Ferrum invokes only the selected
`docker-credential-secretservice` or `docker-credential-pass` binary with argv
`get`, sends the normalized registry key on stdin, and uses bounded helper
output/time. Helper stderr and error text are discarded; malformed results,
helper failures, and exceeded bounds stop the selected operation before Engine
submission without anonymous fallback. Registry-auth settings store only the
helper enum; credential material is not persisted or logged.
An explicitly opted-in saved Compose registration stores a bounded local label,
ordered absolute UTF-8 paths, and validated profiles in the existing private
settings file. It stores no project contents, environment values, credentials,
Engine identity, or commands. The file cap is 256 KiB and the registry cap is
32. Saved paths are not confined and persistence does not inspect project files;
Compose's referenced-file reads remain under the user's permissions. Loading,
selection, save, and forget do not execute Compose/Docker commands or contact
an Engine. Invalid persisted records are skipped and never automatically
selected or validated.

After opt-in, the selected local Engine receives the helper credential over
its Unix socket for that pull or push. Treat the selected host helper and Engine
as trusted. The Engine's registry-mirror configuration may forward credentials
according to daemon behavior; Ferrum does not control or validate downstream
routing. Opt in only when the Engine and its mirror configuration are trusted.
The Engine AuthConfig and short-lived helper/serialization buffers are
zeroized where supported; HTTP-stack `HeaderValue` copies cannot be guaranteed
wiped. Helper-assisted pull/push integration has not been validated against a
live engine or registry; issue #16 remains partial/open.
Image layer history is read-only on the selected Linux local Docker socket and
bounded to a 16 MiB response and 65,536 rows. Only validated full layer IDs and
nonnegative creation times/sizes are retained; `CreatedBy` command text, tags,
comments, and other daemon fields are discarded.

Hiding image-pull or image-push progress or invalidating the Images
page/context does not abort an active response reader: Ferrum drains the
response and keeps the mutation gate closed until a terminal event. Closing
Ferrum does not guarantee that daemon-side pull or push work stops.

Image pull, push, tag, and removal requests emit local structured process traces
with only the static action and a `success`, `not_submitted`, or `request_error`
outcome. `not_submitted` identifies local validation rejection or credential
helper failure before Engine HTTP submission. Image IDs, references, repository
tags, endpoints, error text, and daemon response bodies are not included in
those traces.

Ferrum also maintains a Linux-local, bounded JSONL copy of mutation outcomes
for container lifecycle, confirmed imported Compose start/stop/service restart
and `up`, Compose pulls, image pull/push/tag/removal and Buildx image builds,
volume create/removal, and network create/removal/connect/disconnect requests.
`audit.jsonl` is created
lazily in Ferrum's descriptor-anchored config directory with directory mode
`0700` and file mode `0600`. Schema version 1 records a timestamp, static
event/action/outcome, and validated full resource IDs only where existing
traces permit.

A Compose lifecycle entry contains only the static `compose_mutation` event,
`start`/`stop`/`restart`/`up` action, and `started`, `success`,
`not_submitted`, `request_error`, or `uncertain` outcome; labels, files,
profiles, socket values, service names, credentials, and raw errors are
excluded. Where persisted, `started` records that Ferrum began a confirmed
attempt before preflight/CLI execution. If it has no later terminal outcome,
Ferrum exited or final audit persistence failed; it does not prove whether
Compose reached or changed the engine, so refresh status. `not_submitted`
covers local preflight/validation or any process-spawn failure before the
lifecycle process starts; a returned failure or timeout after start is
`request_error` for start/stop/restart and `uncertain` for `up`. During
shutdown Ferrum makes a best-effort, up-to-one-second flush of the initial
marker before aborting the lifecycle worker; a submitted child may still be
terminated with Engine state uncertain. A build entry contains only the static
`image_mutation` event, `build` action, and outcome. It excludes build output
and all build-specific metadata, as well as names, image references/tags,
endpoints, credentials, raw errors, daemon bodies, and terminal/log contents.
Each record is synced; the file has a 16 MiB hard cap and is not automatically
truncated or rotated. Interprocess lock acquisition uses nonblocking retries
with a 250 ms deadline; continued contention is a persistence failure. When
full or when persistence fails, Ferrum emits a static process warning and does
not claim the event was persisted; the engine request outcome is unchanged.
This is local Ferrum-request history only, mutable by the same-UID user, not
tamper-proof, and does not prove final engine state or record external engine
actions.

Issues #16 and #19 remain partial/open; issue #18 remains partial/open; issue
#10 remains open/blocked for Podman/rootless and broader endpoint validation.
No release or platform/endpoint support is claimed.

## Local image builds

The Linux Images page supports confirmed local builds only through Docker CLI
and its Buildx plugin in `PATH`, explicitly targeting the selected local Docker
Unix socket. Builds accept only a local directory containing a regular
root-level `Dockerfile` and a bounded `repository:tag` target; client validation
rejects uppercase repository characters and Docker CLI validates the full
repository grammar. Docker CLI handles `.dockerignore` and tar-context
semantics. Ferrum uses a private temporary empty Docker config and isolated
`HOME`; it does not read user registry credentials or pass proxy settings,
build arguments, secrets, SSH access, push requests, or remote contexts.
Private base-image authentication is unsupported and may cause a build to fail.
Podman/rootless and broader endpoint/platform compatibility remain unvalidated
under open, blocked issue #10.

Every build requires confirmation because Dockerfiles can run arbitrary builder
commands and an existing tag may be reassigned. The confirmation names the
selected Docker context or a safe endpoint-source label when no named context is
selected, without displaying the socket path. Context switching is disabled
while the confirmation or build remains active. Output is sanitized plain text,
bounded in memory, and not persisted to logs or audit storage. Cancellation is
best effort and terminates the Docker CLI process group; builder work may
continue and image/tag state may be uncertain. Ferrum refreshes image inventory
after every terminal result.
Build traces contain only static `image_mutation` / `build` action and
`success`, `not_submitted`, or `request_error` outcome; context path, tag, and
output are excluded. Failure to spawn Buildx is recorded as `not_submitted`;
unsuccessful completion after the process starts is `request_error` because
image/tag state may be uncertain. These static outcomes are included in the
existing best-effort durable redacted JSONL audit; build output remains
transient and unpersisted.

Issues #16 and #19 remain open and ready. This partial slice is not a release
or a support claim beyond local Linux Docker.

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
daemon bodies are excluded. A durable redacted copy is described above.

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
error can follow a successful deletion. Volume deletion is name-addressed rather
than bound to an immutable ID; a concurrent delete/recreate under the same name
cannot be detected by the preflight/confirmation snapshot. These checks are
not an atomic ownership guarantee. Ferrum marks inventory stale after every
result and refreshes immediately if Volumes remains open, or when that page is
opened again; a fresh inspection/usage snapshot is required before retrying.
Only one volume mutation may be active and context switching is blocked while
it is pending. Pruning, modification, connection, disconnection, and filesystem
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
internal/attachable, `EnableIPv4`, `EnableIPv6`, and an optional point-in-time
attached-container count. The `Containers` map keys are retained only as full
64-character hexadecimal IDs when every key is valid; endpoint values are
discarded. Container names are shown only from the current inventory. When
verbose inspect reports IPAM status, Ferrum retains only aggregate
`IPsInUse` and `DynamicIPsAvailable` allocation counts across subnets. Invalid
CIDRs or duplicate normalized subnet keys make both metrics unavailable.
Missing, malformed, negative, or overflowing count values and checked-sum
overflow make only the corresponding metric unavailable; the other may remain
available. Subnet/CIDR keys, addresses, endpoint names/IDs/configuration, labels,
options, peers, services, raw status, and raw response bodies are not retained
or displayed. These point-in-time values describe IP allocation, not network
traffic; per-container/interface counters are not aggregated.

Network inspection retains at most 16,384 `Containers` map entries,
independently of the 4 MiB response-body bound. A larger map makes the entire
inspection unavailable; Ferrum does not expose partial membership IDs or a
partial attached-container count.

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
not persistent or tamper-proof; a separate durable redacted copy is described
above. Only one network mutation may be active, and context switching is blocked
until it returns. IPAM parsing and metric availability are as described above;
credential access is not provided. Podman/rootless and remote endpoints remain
unsupported or unvalidated. Issue #10 remains open/blocked; no release is
claimed.

The Compose page keeps read-only project discovery for the selected Linux local
Docker Unix socket. It detects the installed CLI version with
`docker compose version --short` and lists projects with
`docker compose ls --all --format json`, pinning the listing to
`EngineClient::local_socket_path()` through Docker global `--host unix://...`.

An explicit validation action accepts one user-selected local `.yaml` or
`.yml` base file followed by local override files in the user's chosen order.
Order is one-based, with the base at order 1; no more than 16 files total are
accepted. Ferrum preflights `docker compose version --short` through the private
isolated Docker config, distinguishing CLI preflight failure from config
validation failure. It invokes Compose with the base and then each override as
separate `--file` arguments:

```text
docker compose --project-directory <base-parent> --file <base-file> --file <override-2> ... config --no-interpolate --no-env-resolution --services
```

Docker merges `-f`/`--file` inputs in the order supplied, and resolves relative
paths relative to the first/base config or `--project-directory`. Ferrum sets
the project directory to the base file's parent. See Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for ordered merging and path resolution and [`config`](https://docs.docker.com/reference/cli/docker/compose/config)
for the command flags.

Before launching Docker/Compose processes, Ferrum rejects an aggregate
selected base-and-override size greater than 16 MiB; exactly 16 MiB is
accepted. Standalone structural validation uses the original selected paths
after a point-in-time size check, so a same-UID process can still change or
replace their contents during that read. Local `include`/`extends.file` files
remain outside the selected-input size bound and are read under the logged-in
user's permissions.

Before a confirmed Compose mutation, Ferrum opens each explicitly selected
file without following a final-component symlink, copies the ordered bytes to
write-sealed Linux memfds under the same 16 MiB aggregate limit, and validates
and submits using the same descriptors. A fresh status query preceding service
restart also uses those descriptors. Compose receives `/proc/self/fd/N` file
references while retaining the original base-parent `--project-directory` for
relative path semantics. Ferrum creates no named on-disk copy; host swap or
core dumps may retain memory pages, so these descriptors do not guarantee
secret erasure. If snapshot creation or descriptor passing is unavailable, the
operation fails closed. This binds only the explicitly selected files and is
not a path-containment boundary. Compose may still read live local
`include`/`extends.file`, `.env`, `include.env_file`, and service `env_file`
inputs; builds also consume live Dockerfiles and build contexts. Only trusted
project trees should be used. Same-UID and privileged processes remain inside
Ferrum's trust boundary; Compose and builder execution are not sandboxed.
On Linux, each Compose subprocess for version, discovery, validation, status,
one-shot and live service-log queries, and start/stop/restart/up has
`RLIMIT_AS` capped at 4 GiB and `RLIMIT_DATA` capped at 512 MiB, preserving
lower inherited limits. The
per-process limits apply while selected and included files; child processes
inherit them, so they bound each process, not aggregate process-tree RSS. The
private child environment sets `GOMEMLIMIT=402653184` (384 MiB) for Go runtimes.
This is a soft runtime target, not a hard cap: Go may exceed it to avoid
excessive GC thrashing. Resource exhaustion can produce generic validation,
status, log-query, or lifecycle failures. These limits and the point-in-time
selected-file check do not make the trusted CLI a sandbox or confine local
included-file reads.

Validation is read-only, does not contact or require an engine socket, and
Ferrum does not write, modify, delete, or extract project files. Success means
structural validation only: interpolation and service `env_file` resolution
are skipped. Ferrum displays bounded escaped basenames, static outcomes, and
the bounded escaped declared-service rows returned on successful validation;
explicit profile selection affects validation, read-only status, one-shot and
live service logs, and is passed to imported-project start/stop/restart/up. The
declared-service inventory includes every declaration in the selected files
regardless of active profiles. Session imports retain validated ordered paths,
profiles, and a bounded local label in memory. An explicit opt-in can persist a
registration containing only that label, absolute ordered UTF-8 paths, and
validated profiles in the existing private settings file. The settings file
has a 256 KiB cap and a maximum of 32 saved registrations. Loading, selecting,
saving, and forgetting do not invoke Compose/Docker or contact an Engine;
restored entries remain unselected and unvalidated. Selecting one clears stale
data and requires explicit revalidation. Forget removes only the registry
entry and leaves the current session entry available. Neither selection nor
persistence reads project contents or confines paths.
Project lifecycle commands use selected files/profiles and the selected local
socket; the local label does not establish project identity or authorization.
Build/pull are separate confirmed image operations described below; `up` is a
separately confirmed operation, while `down`, remove, and project-file mutations
remain unavailable.

Start/stop are available only for the currently selected imported entry
(session-only or explicitly saved) after explicit successful revalidation.
The local import label and Engine-reported `Project` metadata do not establish
identity or authorize scope.

Before each confirmed mutation, Ferrum opens each selected file without
following a final-component symlink, captures the ordered bytes in write-sealed
Linux memfds under a 16 MiB aggregate limit, and validates those snapshots. The
submitted start/stop/restart command uses the same `/proc/self/fd/N` descriptor
paths and original base-parent project directory. Restart's fresh read-only
`compose ps --all --format json` query also uses those descriptors and the
selected local Docker Unix socket; it submits no restart if the service is
absent or the query fails. Snapshot or validation failure submits no mutation.
No on-disk copy of the selected YAML is created.

Docker Compose `start` starts existing containers only; `stop` does not remove
them. Ferrum invokes only `restart --no-deps <SERVICE>` for restart; it does
not apply Compose configuration changes, and dependent services are not
restarted. Confirmation names service, imported label, ordered safe basenames,
profiles, and action; cancel submits nothing.

The separately confirmed **Create and start (local images)** operation is
limited to the current imported, successfully revalidated ordered files and
profiles, their sealed snapshots, and the selected Linux local Docker Unix
socket. It invokes fixed `up --detach --no-build --pull never` arguments,
creating and starting project containers and networks. Compose may recreate
existing containers when configuration or image changes require it. Only
locally present images are used; it does not build, pull, remove orphans,
remove volumes, or force recreation. Missing images require separate Build or
Pull actions; Compose pulls are anonymous-only.

The confirmation warns to use only trusted project files, that containers may
be created or recreated, and that the required images must already be local
because the operation does not build or pull. Compose project configuration
and service code run on the selected Engine without a sandbox. Ferrum's private
Docker config/HOME and cleared environment do not pass saved credentials.
The command deadline is 30 seconds. Any failure after process spawn, including
timeout, is uncertain and requires refreshing status before retrying. The audit
records only static `up` and outcome values, never paths, labels, or project
metadata. No live mutating integration test has run because no disposable
Engine is designated; Podman, remote, Windows/macOS, and full Issue #18/#19
support are not claimed.

These commands are Linux local Docker Unix-socket only. Outcome errors are
static and exclude CLI diagnostics. Navigation does not cancel an in-flight
lifecycle command because a submitted daemon-side mutation may already be
accepted; Ferrum keeps a redacted session outcome. Context switching remains
blocked while the task runs and status refresh occurs only for the same current
selection/context after terminal completion. On shutdown Ferrum flushes the
started audit marker before issuing the command; a submitted child may still be
terminated, leaving Engine state uncertain. Explicitly selected source files
are captured at operation start; changes after capture do not affect that
invocation. Recursive includes, `extends.file`, environment sources,
Dockerfiles, and build contexts remain live trusted local inputs under the
logged-in user's permissions. There is no path containment or sandbox.

Confirmed imported-configuration image pulls use the current imported and
successfully revalidated ordered files/profiles and the selected Linux local
Docker Unix socket:

```text
docker --host unix://<selected-socket> compose --progress plain --parallel 4 --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... pull --ignore-buildable
```

Compose pulls are anonymous only. Ferrum uses private Docker config/HOME and a
cleared environment; it does not read saved Docker credentials or invoke a
credential helper for Compose pulls. Private registries requiring credentials
may reject them. Compose `--parallel 4` limits the operation to four
concurrent service pulls. Once pulling starts, the CLI has a two-hour deadline
and a combined 1 MiB stdout/stderr cap; exceeding either stops the local process
and leaves image-store state uncertain. The UI shows only a bounded, rate-limited
summary of recognized progress phases/counts, never raw output.
`--ignore-buildable` excludes services that require a build. The command does
not build images, start/create services, or run `up`. It changes only the
selected Engine's image store; it does not create, start, stop, or remove project
containers, networks, or volumes. Pulled images may consume network bandwidth
and disk space.

Cancellation kills only Ferrum's local Compose CLI process group; it does not
guarantee submitted daemon-side work stops. A pull may continue after Ferrum
stops waiting, leaving image state uncertain; inspect current Engine state
rather than assuming the pull was undone. The best-effort audit event contains
only static `compose_pull`/`pull` and `started`, `success`, `not_submitted`,
`request_error`, or `uncertain` values. It excludes labels, paths/files,
services, image references, credentials, output, diagnostics, and socket values.
A `started` marker is recorded before process spawn; without a later terminal
outcome, Ferrum exited or final audit persistence failed and the final Engine
result is unknown.
This slice applies only to Linux local Docker Unix sockets; it does not claim
Podman, remote Docker, Windows, or macOS support.

Confirmed Compose builds run only for the same selected, imported,
successfully revalidated ordered files/profiles and selected Linux local Docker
Unix socket. The fixed argv is `docker --host unix://<selected-socket> compose
--progress plain --parallel 2 --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ...
--profile ... build`; Ferrum adds no `--push`, command-line `--ssh`, builder
options, or extra command-line build arguments. Compose-file `build.args` remain
active and may expose secrets; do not store credentials there. Compose-file
`build.ssh` may grant SSH-agent access, and `build.secrets` may expose
configured secret contents to build instructions. Inspect both, and only build
trusted projects. Saved Docker credentials/helpers, proxy environment, and user
config are not passed. Ferrum uses private
Docker config/HOME, a cleared allowlisted environment, disabled selected `.env`
and remote Compose Git/OCI includes, Linux limits, a two-hour deadline, and a
combined 1 MiB stdout/stderr cap. The UI exposes only bounded/rate-limited
allowlisted phase/count summaries; raw output, paths, references, and
diagnostics are neither shown nor persisted.

Build executes project-controlled Dockerfile instructions with access to
Compose-defined contexts and may read files beyond the selected YAML, fetch
remote contexts/base images, run arbitrary builder/network code, consume
resources, and change the selected Engine image store; existing local image tags
may be reassigned. Review `.dockerignore`; build only trusted project files.
This is neither sandboxed nor path-contained. Anonymous auth only is available;
credential-required private registries may fail. Preflight cancellation kills
before build submission; post-submit cancellation kills only Ferrum's CLI
group, so builder/daemon work may continue and state is uncertain. Timeout and
output overflow also leave state uncertain; no retries are automatic. Audit
stores only static `compose_build`/`build` start and outcome values, excluding
project, file, service, context, image, credential, and output data.

On successful validation the same bounded CLI invocation returns every
declared service name via `config --services`, including services gated by
inactive profiles. Ferrum caps stdout at 1 MiB, then accepts at most 128 KiB
total service output, 1024 rows, and 128 bytes per name; malformed, duplicate,
or excess output fails with a static error. Names are escaped and rendered in
a virtualized list. This is a local CLI/file-derived inventory, not
container-status data; listing does not call the Engine or trigger status/log
operations. An empty result means no service names were returned for those
selected files.
The profile input is comma-separated, capped at 1024 UTF-8 bytes overall, 16
profiles, and 64 UTF-8 bytes per name. Ferrum trims outer whitespace per token;
blank input means no explicit profiles. Empty entries and duplicate names are
rejected. Names must match `[A-Za-z0-9][A-Za-z0-9_.-]+`; standalone `*`
enables every profile and is accepted only by itself. It does not start
services; the inventory remains all declarations in the selected files
regardless of active profiles or container state.
Ferrum passes each profile through an individual `--profile <name>` argv pair
before the Compose subcommand, never through an environment variable. The same
profiles are used for immediate status rechecks before one-shot logs, live
follow, and restart.
Invalid or changed selection clears prior validation, service inventory,
status, and log state, cancels in-flight Compose work, and requires
revalidation. Private Docker config/HOME and the cleared environment remain in
effect, including disabling the selected project's default `.env` and remote
Git/OCI loaders.

See Docker's [Compose profiles guide](https://docs.docker.com/compose/how-tos/profiles/)
and [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose/).

Read-only container status is available for selected files after structural
validation. It pins `docker compose ps --all --format json` to the selected
local Unix socket and does not create, start, stop, remove, or otherwise mutate
containers or project files. `--all` includes stopped containers; the CLI's
default orphan inclusion is preserved. Ferrum retains bounded escaped
container `Name`, `Service`, optional per-container `Project`, `State`, and
optional `Health`; missing project values render as `Not reported`, and rows may
report different values. The project label is untrusted engine-reported
metadata only: it does not establish file ownership, canonical identity,
authentication, or authorization and does not determine lifecycle scope. IDs, command text,
published ports, exit codes, and other response fields are discarded. Empty
output means no existing containers matched this isolated project selection,
not that no services are declared. Status is a manually refreshed point-in-time
snapshot.

After structural validation and a successful status snapshot, a listed service
may request a one-shot `docker compose logs --tail 200 --no-color` query.
Immediately before logs, Ferrum reruns a read-only status query with the same
selected files and socket and refuses if the service is absent. `--tail 200`
applies per container; Ferrum keeps at most the first 200 lines of combined
CLI output and 256 KiB, so replicated output may omit some containers and is
not a global chronological tail. Only bounded safe ASCII service selectors
from current status rows are accepted. Output is escaped, transient, and never
persisted or sent to tracing/audit/errors. Cancellation and file/context/page/
status invalidation abort the owned query and reject stale results. Both CLI
calls use the selected local socket and ordered files with the base parent as
`--project-directory`, private Docker config/HOME, cleared allowlisted
environment, disabled selected-project `.env` and remote Git/OCI loaders,
per-process resource limits, an eight-second deadline, and a 1 MiB stdout cap
per call. A selected local file may still change between the status and logs
calls; the trusted CLI may read includes and included-project environment
sources under the logged-in user's permissions. This is not a sandbox or an
engine lifecycle operation.
Live Compose follow is a separate read-only `logs --follow --tail 200 --no-color
SERVICE` session after selected-file validation. Before starting, Ferrum repeats
the bounded status membership query with the same selected files, profiles, and
local Docker Unix socket. Compose's tail is per container; Ferrum retains a
rolling 200-line/256 KiB UI buffer behind a 16-event bounded channel and caps
raw lines at 4096 bytes, dropping overlong tails through newline. Escaped output
is transient and excluded from files, audit, tracing, Debug, and errors. Stop,
selection/status/page/context invalidation, and app shutdown abort the local CLI
process group; there is no overall stream deadline or automatic retry. This
read-only stream does not change container state; no live service logs were read.

Status uses the private cleared environment described below and disables the
selected project's default `.env`; environment-dependent projects may fail or
resolve differently than when Compose runs in the user's shell. Compose may
still read local includes and included-project environment sources under the
logged-in user's permissions. Validate only trusted files.

Compose can recursively read local files referenced by `include` or service
`extends.file`, plus each included project's default `.env` and explicit
`include.env_file` paths. These may be outside the base directory and are
opened with the logged-in user's permissions. `COMPOSE_DISABLE_ENV_FILE=1`
disables only the selected project's default `.env`; `--no-interpolate` and
`--no-env-resolution` do not confine included-project reads. Remote Git and OCI
loaders are disabled, so remote `include`/`extends.file` references fail
without fetching. The selected files are not a path-containment boundary.
Ferrum clears inherited environment values, disables remote loaders, and does
not pass user environment files, but these measures do not prevent the local
include/env reads described above. Private Docker config/HOME and environment
isolation are not a sandbox, and the CLI/plugin is trusted local code. Validate
only trusted Compose files.

CLI processes use a private isolated Docker config and `HOME` with a cleared
allowlisted environment. If Compose is installed in the standard per-user CLI
plugin directory, only the `docker-compose` executable is linked into the
private config; custom `cliPluginsExtraDirs` from saved config are not loaded.
Saved `config.json` credentials and proxy settings are not read or passed.
Discovery, validation, status, and one-shot log subprocesses have an eight-
second deadline per command. Imported start/stop/restart/up allows 30 seconds for
the lifecycle command after bounded validation commands; restart's fresh status
query is also bounded to eight seconds. Each command has a 1 MiB stdout cap.
Process resource exhaustion may cause generic validation, status, or lifecycle
failure.

Linux local Compose discovery and single-file validation, loopback-loader,
ordered multi-file temporary-file, and read-only temporary-project status
smokes have been exercised with Compose v5.6.0. Status used Docker Engine 29.9.0
and created no containers; validation smokes used only owned temporary files
without engine or user-project access. Controlled fake-CLI/headless regressions
cover service-restart argv, current/fresh status membership, UI confirmation and
cancel, and static redacted audit outcomes; no live lifecycle mutation or real
service logs were accessed. The native page visual check remains unavailable.
Issue #18 remains partial/open; issue #19 remains partial/open; issue #10
remains blocked/open. No full issue completion, release, or broader support
claim is made.

