# Compose

The Compose page provides read-only project discovery, structural validation of
a local base file with ordered overrides, a list of every service declared in
the selected files (including profile-gated services regardless of active
profiles), and read-only status for existing containers. It also supports
explicit imported-configuration start/stop, single-service
restart/pause/unpause, and bounded one-service scaling for the current imported,
revalidated selection. The declared-service list is local Compose CLI/file-
derived and does not query an Engine; it is separate from running-container
status. Import labels are local display labels, never Engine-discovered project
identities; imports are session-only unless explicitly saved below.
Project discovery uses Docker's Compose CLI:

- `docker compose version --short`
- `docker compose ls --all --format json`

The version probe detects the installed Docker Compose CLI version without
targeting an engine. The project listing is pinned to the socket selected by
Ferrum's current `EngineClient::local_socket_path()` using Docker's global
`--host unix://...` option. Discovery supports only Linux local Docker Unix
sockets; it is not a claim of support for Podman/rootless, remote Docker,
Windows, or macOS.

## Validate local Compose files

Click **Choose base file** and select one local `.yaml` or `.yml` file, then
use **Add override file** to append up to 15 local overrides in your chosen
order. The base is order 1; overrides are numbered from 2, and later files
override earlier definitions. At most 16 files total are accepted.

Use **Remove override N** to remove an override. **Change base file** replaces
the current base and clears its overrides; add any needed overrides again in
the intended order.
Ferrum validates after each file selection. After removing an override, use
**Validate selected files** to revalidate the remaining ordered set.

Ferrum first preflights the installed Compose CLI with
`docker compose version --short` through its private isolated Docker config.
A preflight failure (for example, unavailable CLI/plugin) is distinct from a
Compose config-validation failure. On successful preflight Ferrum passes each
selected path as a separate argument, with the base first and each override
following in the chosen order:

```text
docker compose --project-directory <base-parent> --file <base-file> --file <override-2> ... --profile <selected-profile> ... config --no-interpolate --no-env-resolution --services
docker compose --project-directory <base-parent> --file <base-file> --file <override-2> ... --profile '*' config --no-interpolate --no-env-resolution --services
```
Docker Compose merges files in the order supplied with `-f`/`--file`, with
later files overriding or extending earlier configuration. Compose resolves
relative paths against the first/base configuration file; Ferrum explicitly
sets `--project-directory` to that base file's parent, which determines path
resolution for the merged project. See Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for ordered file merging and path resolution, and the [`config` reference](https://docs.docker.com/reference/cli/docker/compose/config)
for the validation flags.

Validation is Linux local Docker Compose CLI only. Ferrum rejects a combined
selected-file size greater than 16 MiB across the base and all overrides
before launching any Docker/Compose process; exactly 16 MiB is accepted. This
size check is a point-in-time check: during standalone validation, a same-UID
process can change or replace a selected file after it is checked, and Compose
reopens the original paths after the version preflight. Included
`include`/`extends.file` files are outside the selected-file size check and are
read under the logged-in user's permissions. This does not contact or require
a Docker Engine socket, and Ferrum does not write, modify, delete, or extract
project files. A successful result means structural validation only:
`--no-interpolate` skips interpolation and `--no-env-resolution` skips service
`env_file` resolution. A prior successful validation is not a content snapshot
for a later operation; each confirmed mutation takes and revalidates its own
bounded snapshot as described below. Ferrum displays bounded, escaped
basenames, a static validation result, and the bounded escaped service list,
never full paths or raw CLI diagnostics.

## Import a validated configuration for this session

After validation succeeds for the current ordered files and profile selection,
enter a unique local display name and choose **Import validated selection**.
The name is a label only: it is not passed to Compose as a project name,
canonical identity, ownership claim, or authorization input. Imported entries
are shown separately from Engine-discovered Compose projects. The in-memory
list is limited to 32 entries and each name to 256 UTF-8 bytes; duplicate or
empty names are rejected.

Choose **Save for future sessions** on a session entry to explicitly register
its label, ordered absolute UTF-8 file paths, and validated profiles in Ferrum's
private settings file. No project content, environment variable, credential,
Engine identity, or command is stored. A saved entry is restored at startup as
an unselected, unvalidated registration. Selecting it restores its ordered
files and profiles, cancels any in-flight validation/status/log task, and
clears validation, declared services, container status, and logs. Explicitly
validate again before requesting status, logs, or lifecycle operations.
Loading, selecting, saving, and forgetting registrations do not run
Docker/Compose commands or contact an Engine.

Use **Forget saved registration** to remove only the persistent registration.
The entry remains available for the current session as session-only; this does
not delete files or Compose resources. Save and forget use the existing
atomic SettingsStore save, and a persistence failure leaves both the prior
saved status and persisted record unchanged with a static UI message.
Versionless and v1 settings migrate with an empty saved list. Invalid persisted
records are skipped with a static warning. The settings file is capped at
256 KiB, and registrations are capped at 32.

Neither selection nor persistence reads project contents or confines paths.
The selected paths remain subject to the trusted-file warning below: Compose
can read local files referenced by `include` or service `extends.file`,
including related environment files, under the logged-in user's permissions.

On successful validation, Ferrum retains two bounded service lists from
read-only `config --services` queries: the selected-profile list used to gate
existing service lifecycle actions, and an all-declared list queried with
`--profile '*'` for inventory and scaling. Names are escaped before display in
a virtualized list. Both are local CLI/file-derived, not Engine status; neither
query starts services or triggers logs. A status snapshot separately reports
existing containers and may be empty even when services are declared.
On Linux, each Compose subprocess (version, discovery, validation, and status)
runs with a 4 GiB `RLIMIT_AS` address-space ceiling and a 512 MiB `RLIMIT_DATA`
ceiling, preserving lower inherited limits. These per-process limits apply to
Compose reading both the selected and included files, and child processes
inherit them; they bound each process, not aggregate process-tree RSS. The
private child environment sets `GOMEMLIMIT=402653184` (384 MiB) for Go runtimes.
`GOMEMLIMIT` is a soft runtime target, not a hard memory cap, and Go may exceed
it to avoid excessive garbage-collection thrashing. Resource exhaustion can
cause a generic validation failure.

Compose can recursively read local Compose files referenced by `include` or
service `extends.file`, plus each included project's default `.env` and explicit
`include.env_file` paths. These may be outside the base file's directory and
are opened with the logged-in user's permissions. The point-in-time 16 MiB
aggregate size check covers only the explicitly selected base and override
files; included files and related environment files are not included in that
check, but are read by the same per-process resource-limited CLI. `COMPOSE_DISABLE_ENV_FILE=1`
disables only the selected project's default `.env`; the validation flags do
not confine included-project reads. Remote Git/OCI loaders are disabled, so
remote `include`/`extends.file` references fail without a fetch. The selected
files are not a path-containment boundary: users should validate only trusted
files. Private config/HOME and environment isolation are not a sandbox, and the
per-process limits do not cap aggregate process-tree RSS. The CLI/plugin is
trusted local code.

Ferrum clears inherited environment values, sets `COMPOSE_DISABLE_ENV_FILE=1`,
and sets `COMPOSE_EXPERIMENTAL_GIT_REMOTE=false` and
`COMPOSE_EXPERIMENTAL_OCI_REMOTE=false`. No user environment files are passed
by Ferrum itself; included projects may still read their default `.env` and
explicit `include.env_file` under the logged-in user's permissions. Discovery,
validation, status, and one-shot log subprocesses have an eight-second deadline
per command; imported start/stop/restart/pause/unpause/up/scale mutations use
the 30-second lifecycle deadline (restart also has a separate eight-second
status check).
Each call has a 1 MiB stdout cap; raw stderr and diagnostics are discarded.

CLI processes use a private isolated Docker config and `HOME` with a cleared
allowlisted environment. If Compose is installed in the standard per-user CLI
plugin directory, only the `docker-compose` executable is linked into the
private config; custom `cliPluginsExtraDirs` from saved Docker configuration
are not loaded. Saved Docker `config.json` credentials and proxy settings are
not read or passed to the CLI. This does not prevent Compose from reading local
environment files declared by included projects.

## Inspect existing project containers

After the selected base and overrides pass structural validation, click
**Refresh existing containers** to query the same selected local Docker Unix
socket. Ferrum invokes Compose with the files in their selected order:

```text
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> --file <override-2> ... --profile <selected-profile> ... ps --all --format json
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> --file <override-2> ... --profile '*' ps --all --format json
```

The selected-profile query remains the status source for regular service
actions. The additional all-profile query is read-only and provides accurate
scale counts for profile-gated services, including stopped containers. Both
queries use `--all`, which includes stopped containers; Compose's default
orphan inclusion is preserved. Ferrum displays bounded, escaped container
name, service, optional per-container `Project` metadata, state, and reported
health. When Compose omits `Project`, the row shows `Not reported`; rows may
report different values. This engine-reported label is untrusted metadata: it
does not establish file ownership, canonical project identity, authentication,
or authorization, and it does not determine lifecycle scope. Ferrum discards
container IDs, command text, published ports, exit-code data, and other response
fields. No output means no existing containers for that query; it does not mean
the Compose file declares no services. The page does not poll automatically;
refresh status when needed.
Status also clears inherited environment values and disables the selected
project's default `.env`. Values normally supplied by the shell or that file may
therefore resolve differently; status can fail or match a different project
configuration than a normal shell invocation. It is a current read-only
snapshot, not proof that another environment or context has no containers.

See Docker's [Compose `ps` reference](https://docs.docker.com/reference/cli/docker/compose/ps/)
for the JSON Lines format and status command options.

Status is implemented only for Linux local Docker Unix sockets. The status query
itself is read-only; lifecycle actions are separately documented below. It uses
Ferrum's private Docker config/HOME and cleared environment; the selected
project's default `.env` is disabled, and remote Git/OCI loaders are disabled.
If Compose cannot resolve the selected project under this isolated environment,
Ferrum shows a static status error rather than raw CLI output. Compose may still
read local files referenced by `include` or `extends.file` and included-project
environment sources under the logged-in user's permissions; validate only
trusted files. The command uses the same subprocess timeout, output cap, and
per-process resource limits described above.

## Start/stop an imported configuration or restart, pause, or unpause a service

The Linux local Docker-socket page exposes **Start imported project** and
**Stop imported project** only when the current selection was imported for this
session and then explicitly validated successfully. **Restart service SERVICE**
is shown beside each service present in the current successful container-status
snapshot. **Pause service SERVICE** is enabled only for a declared service with
a current status row exactly `running`; **Unpause service SERVICE** is enabled
only for a declared service with a current status row exactly `paused`.
Stopped, unknown, or missing statuses are ineligible for pause/unpause. All
actions require the current imported, explicitly revalidated selection.
Selecting an import clears old validation/status/log data; revalidate before
requesting an operation and refresh status before choosing a service action.
The local import label is display-only and is never passed as
`--project-name`. Untrusted per-container `Project` metadata is not used to
choose or authorize the target. Scope comes from the selected ordered
files/profiles, current local socket, and the validated service in current
status; pause/unpause additionally require that service to be declared and in
the eligible state.

Before each confirmed mutation, Ferrum opens each selected file without
following a final symlink and copies the ordered bytes into write-sealed Linux
memory-backed file descriptors (memfds). The aggregate copy is capped at
16 MiB; failure to capture a valid snapshot fails closed. Preflight validates
those snapshots, then submits the mutation with the same descriptors. Each
service action's fresh read-only status query also uses the same snapshots
before mutation.
Compose receives `/proc/self/fd/N` file references while `--project-directory`
remains the original base file's parent, preserving normal relative-path
resolution. No named on-disk copies of the selected Compose files are created;
the host may still retain memory pages in swap or core dumps, so memfd does
not provide secret erasure. Changes to a source file after its snapshot cannot
change that operation; a change before snapshotting is the content that is
validated and submitted.

This protects only the explicitly selected base and ordered overrides. Local
files reached through `include`, service `extends.file`, included-project
`.env`/`include.env_file`, or service `env_file`, as well as Dockerfiles and
build contexts, are not snapshotted and remain trusted, live local inputs.
They may be read or change under the logged-in user's permissions. This is not
a sandbox or a path-containment guarantee; use only trusted project trees.

The confirmation names the imported local label, ordered safe file basenames,
selected profiles, action, and target service where applicable; it explains
that pause suspends a running service's processes and unpause resumes them,
and that neither action creates or removes project resources (including
containers, networks, or volumes). Cancelling it sends no command.
Confirmation is rejected if the imported label, ordered files, profiles,
successful validation, or target service's current status/eligibility no longer
match. Before a service action is submitted, Ferrum rechecks fresh read-only
status after validating sealed snapshots on the same selected Linux local
Docker Unix socket; if the service is no longer eligible, no mutation is sent.
Validation or status failure submits no mutation. Ferrum invokes the installed
Docker Compose CLI through argument arrays:

```text
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... start
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... stop
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... restart --no-deps <SERVICE>
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... pause <SERVICE>
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... unpause <SERVICE>
```

`start` starts existing containers; it does not create, build, or pull.
`stop` stops running containers without removing them. Restart targets only the
selected service; `--no-deps` prevents restarting dependent services. Docker
Compose restart does **not** apply changes made to the Compose configuration:
it restarts containers using their existing configuration. Pause suspends
processes in the selected running service; unpause resumes processes in that
service. They target only the validated service (never a project-wide
all-service command) and do not create or remove containers, networks, or
volumes. See Docker's [`pause`](https://docs.docker.com/reference/cli/docker/compose/pause/)
and [`unpause`](https://docs.docker.com/reference/cli/docker/compose/unpause/)
references.

These mutations are Linux local Docker Unix-socket only and preserve selected
file ordering, active profile arguments, and the base-parent project directory;
Ferrum does not add an import-label project name. Commands use Ferrum's private
Docker CLI config/HOME, cleared environment, disabled selected project `.env`,
disabled remote Git/OCI loaders, an eight-second per-command validation/status
bound, and a 30-second lifecycle-command deadline with child-process cleanup,
1 MiB output cap, and Linux process resource limits.

Lifecycle tasks remain owned by the app when navigating away; leaving Compose
does not cancel a command that may already have been accepted by the Engine.
Context switching is blocked while a lifecycle command is pending. After
completion Ferrum refreshes read-only status only if the same imported files,
profiles, validation, and selected context remain current; otherwise refresh
manually after returning to that configuration. For `start`, `stop`, `restart`,
`pause`, and `unpause`, Ferrum records only the static action and `started`,
`success`, `not_submitted`, or `request_error` outcomes in its best-effort local
audit history.
A `started` record is written before preflight; with no later
terminal outcome, Ferrum exited or final audit persistence failed, and the
Engine result is unknown. `not_submitted` means preflight/validation,
service-membership query, or process-spawn failure before the mutation starts;
a returned failure or timeout after process start is `request_error`. For
pause/unpause, a post-spawn failure or timeout is uncertain; successful status
refresh is required before retry. `up` instead records post-spawn failures as
`uncertain`, as described below. Audit persistence failure does not change the
operation result.
During shutdown Ferrum makes a best-effort,
up-to-one-second flush of the initial audit marker before aborting the lifecycle
worker. A submitted child may still be terminated, leaving Engine state
uncertain; refresh after restart. Service names, paths, labels, profiles,
endpoint values, raw diagnostics, and project metadata are never audited.
`down`, remove, and project-file mutation remain outside this partial Issue #18
slice. Compose pull is separately constrained as documented above. Explicitly
saved local registrations are the only persistent Compose-project metadata and
are described above.

## Scale one declared service

For the current imported selection, after explicit successful validation and a
successful current status refresh, the Linux page shows one Scale control per
declared service—not one per replica row, including services whose profiles
are inactive. Each control shows the count from the read-only all-profile
`compose --profile '*' ps --all --format json` query, including stopped
containers. The target starts empty: enter an integer from 1 through 32 to
enable the Scale controls; no target is chosen automatically. Scaling to zero
and project-wide scaling are not offered. Every declared service is eligible,
including one with zero observed containers.

The explicit confirmation identifies the imported local label (not project
identity), service, observed count, and target. It warns that scaling down can
stop/remove excess containers, scaling up can create containers, and Compose
`up` may recreate changed containers. It can also start stopped containers and
run the configured service code on the selected Engine. Scaling down/removing
containers or configuration-driven recreation discards container writable-
layer changes; mounted volumes follow Compose semantics. Dependencies do not
auto-start (`--no-deps`). If a target service is profile-gated, explicitly
naming it activates that service's profile for this command; sibling services
in that profile are not targeted or started. Ferrum requests no build, pull,
volume removal, orphan removal, or force recreation. Cancelling submits no
command and records only static `scale/not_submitted` audit data.

Before submission Ferrum rejects changed imports, files, profiles, validation,
status, busy work, invalid targets, undeclared services, and a stale observed
count. The worker snapshots selected files into write-sealed descriptors,
revalidates those exact snapshots using selected profiles, and obtains fresh
selected-profile and all-profile `ps --all` status on the same selected local
Docker Unix socket immediately before mutation. The command uses argument
arrays and deliberately has no global `--profile` option:

```text
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <snapshot-fd> ... up --detach --no-build --pull never --no-deps --scale SERVICE=N SERVICE
```

The operation uses Ferrum's private isolated CLI environment, 30-second
deadline, bounded output, and process cleanup. A failure before process spawn
is `not_submitted`; every submitted error, timeout, or cancellation is
`uncertain`. Ferrum requires a successful status refresh before another scale
attempt after an uncertain result. Best-effort audit stores only static
`scale` action and `started`, `success`, `not_submitted`, `request_error`, or
`uncertain` outcomes; it excludes project labels, service names, counts, paths,
profiles, socket values, and command output. This remains Linux local Docker
Compose CLI support only; it does not complete Issue #18 or claim Podman,
remote, Windows, or macOS support.

## Pull images for an imported configuration

After confirmation, **Pull images** is available for the current imported,
successfully revalidated file/profile selection. Ferrum invokes:

```text
docker --host unix://<selected-socket> compose --progress plain --parallel 4 --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... pull --ignore-buildable
```

The pull is limited to Linux local Docker Unix sockets. Ferrum uses a private
Docker config/HOME and cleared environment; it does not read saved Docker
credentials or use credential helpers for Compose pulls. Authentication is
anonymous only, so private registries requiring credentials may reject the
pull. The command uses the selected ordered files, profiles, and base-parent
project directory. `--ignore-buildable` prevents pulling services that require
a build; Ferrum does not build images, start or create containers, or bring
the project up. The pull changes only the selected Engine's image store; it
does not create, start, stop, or remove project containers, networks, or
volumes. Pulled images may consume network bandwidth and disk space.

At most four service pulls run concurrently through Compose `--parallel 4`.
Once the pull command starts, its two-hour deadline and combined 1 MiB stdout/
stderr cap bound the CLI. Exceeding either stops Ferrum's process and leaves
image-store state uncertain. The UI shows a bounded, rate-limited summary of
recognized progress phases/counts; raw CLI lines, image names, paths, and
diagnostics are never shown. Cancellation kills only Ferrum's local Compose CLI
process group; Engine-side downloads may continue if the pull was submitted.
Refresh the image inventory or Compose status rather than assuming cancellation
undid the pull.

Ferrum's best-effort audit records only the static `compose_pull` event, `pull`
action, and `started`, `success`, `not_submitted`, `request_error`, or `uncertain`
outcome; it excludes labels, file paths, services, image references, credentials,
and command output. A `started` marker is recorded before process spawn; without
a later terminal outcome, Ferrum exited or final audit persistence failed and
the final Engine result is unknown.

Pull uses the isolated CLI environment described above. Compose may read local
selected or included configuration/environment files with the logged-in user's
permissions; validate only trusted files. This slice does not add support for
Podman, remote engines, or non-Linux platforms.

## Build images for an imported configuration

After explicit confirmation, **Build imported images** is available only for
the selected imported project after successful revalidation of its same ordered
Compose files and profiles, using the selected Linux local Docker Unix socket.
The import label is display-only and does not select or authorize scope.
Ferrum invokes only this fixed argv shape:

```text
docker --host unix://<selected-socket> compose --progress plain --parallel 2 --project-directory <base-parent> --file <snapshot-fd-1> --file <snapshot-fd-2> ... --profile <profile> ... build
```

Ferrum adds no `--push`, command-line `--ssh`, user-selected builder options, or
extra command-line build arguments. Compose-file `build.args` remain active and
may expose secrets; do not put credentials there. Compose-file `build.ssh` may
grant SSH-agent access, and `build.secrets` may expose configured secret
contents to build instructions. Inspect both, and build only trusted projects.
Saved Docker credentials/helpers, proxy environment, and user Docker
config are not forwarded. Ferrum uses private
Docker config/HOME and a cleared allowlisted environment; selected-project
`.env` and remote Compose Git/OCI includes are disabled. Linux child resource
limits, a two-hour deadline, and a combined 1 MiB stdout/stderr cap apply.
Output is untrusted: only bounded, rate-limited phase/count summaries appear.
Raw lines, image references, paths, and diagnostics are never shown or
persisted.

Build runs project-controlled Dockerfile instructions with access to the
Compose-defined contexts; it may read files beyond the selected YAML, fetch
remote build contexts/base images, execute arbitrary builder/network code,
consume CPU/disk/network, change the selected Engine image store, and may
reassign existing local image tags. Review `.dockerignore` and build only
trusted project files. This is not sandboxed or path-contained. Only anonymous
registry access is available; private registries that need credentials may
reject requests.

Cancellation during preflight terminates the validation CLI process group and
submits no build. After build submission, cancellation kills only Ferrum's CLI
process group; the builder/daemon may continue and image state is uncertain.
Timeout or output overflow has the same uncertain result. There is no automatic
retry. Static best-effort audit records only `compose_build`/`build` and
`started`, `success`, `not_submitted`, `request_error`, or `uncertain` values.
No project/file/service/context/image/credential/output data is recorded.
The red-before/fixed-after fake-CLI regression replaces selected source files
during preflight and verifies that the mutation receives the original sealed
snapshot. An opt-in smoke uses the installed Compose CLI and owned temporary
base, override, and include files with `config --services` only; it does not
contact an Engine, registry, or user project.

## Create and start (local images)

After explicit confirmation, **Create and start (local images)** is available
only for the current imported selection after its ordered files and profiles
have been successfully revalidated and the current Compose status refresh has
succeeded. Refresh status before the first `up` and after any uncertain result;
if navigation skips automatic refresh, a successful manual refresh is needed.
Ferrum uses the selected Linux local Docker Unix socket and sealed snapshots
of those selected files. Its fixed Compose operation is:

```text
docker compose ... up --detach --no-build --pull never
```

Ferrum also pins the command to the selected socket and supplies the base
project directory, ordered snapshot file descriptors, and selected profiles.
Compose creates and starts project containers and networks and may create
missing project volumes, as described in Docker's
[Compose volume documentation](https://docs.docker.com/reference/compose-file/volumes/).
It may recreate existing containers when configuration or image changes require it.
With `--detach`, Compose returns while containers
continue running; use the existing status and service-log views to monitor them.
Project services can write to volumes and host mounts described by the trusted
project. It uses locally present images
only: this operation does not build or pull images, remove orphan containers,
remove volumes, or force recreation. If an image is missing,
use the separate **Build imported images** or **Pull images** action as
appropriate. Compose pulls are anonymous-only and may fail for private
registries requiring credentials.

The confirmation warns to use only trusted project files because containers
and missing project volumes may be created and existing containers may be
recreated; required images must already exist locally. Compose runs trusted
service code on the selected Engine; this is not a sandbox. Ferrum uses its
private Docker config/HOME and cleared environment, and does not pass saved
Docker credentials or invoke Build/Pull as part of `up`.

The command has a 30-second deadline and a 1 MiB stdout cap; raw stdout is
discarded and stderr is suppressed. Best-effort audit records only static `up`
action and `started`, `success`, `not_submitted`, `request_error`, or
`uncertain` outcomes, without paths or project metadata. A failure after the
process starts, including timeout, is uncertain.
A successful current status snapshot is required before every `up` confirmation.
The control stays disabled while refresh is pending or failed. Ferrum refreshes
status automatically after completion only while the same page and selection
remain current; if refresh fails or navigation skips it, refresh successfully
before trying again. Fake-CLI/headless coverage does not establish live Engine
behavior; no disposable Engine is designated for a mutating integration test.

This slice is limited to Linux local Docker Unix sockets. It does not claim
Podman, remote Engine, Windows, macOS, or full Issue #18/#19 support.


## One-shot service log snapshot

After structural validation and a successful status snapshot, use **Snapshot
logs for SERVICE** beside a listed service. Immediately before requesting logs,
Ferrum repeats a read-only status query with the same selected files and local
socket. If the service is no longer present, Ferrum reports that the status
should be refreshed.

```text
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> --file <override-2> ... logs --tail 200 --no-color <service>
```

This is a one-shot query, not live follow or polling. Compose's `--tail 200`
asks for up to 200 recent lines **per container**. Ferrum keeps at most the
first 200 lines of the combined CLI output and 256 KiB; a replicated service
may be truncated, and Ferrum does not create a globally time-sorted last-200
view. It accepts only bounded safe ASCII service selectors: 1–128 bytes,
starting with an ASCII letter or digit and continuing with ASCII letters,
digits, `_`, `-`, or `.`. Unsafe selectors are not passed to Compose. Output is
escaped and transient in memory only; it is never written to files, durable
audit, or tracing. Cancel aborts the subprocess; changing files, context, page,
or status invalidates the result.

The fresh status check and the log request are separate read-only CLI calls,
each bounded to eight seconds and 1 MiB of stdout (up to 16 seconds total).
Both use the selected local Docker Unix socket, selected ordered files, and
`--project-directory` pinned to the base parent. They use Ferrum's private
Docker config/HOME, cleared allowlisted environment, disabled selected-project
default `.env` and remote Git/OCI loaders, and the existing per-process resource
limits. As with validation and status, Compose remains trusted local code and
may recursively read local `include`, `extends.file`, and included-project
environment sources under the logged-in user's permissions. A file can still
change between the status check and log request; use only trusted selected
files.

See Docker's [Compose `logs` reference](https://docs.docker.com/reference/cli/docker/compose/logs/)
for CLI options and per-container tail semantics.

## Live service-log follow

Use **Follow logs for SERVICE** beside a service in the current successful
status snapshot. Follow requires the current session import and explicit
successful validation. Immediately before starting, Ferrum repeats a read-only
`compose ps --all --format json` query with the same ordered files, profiles,
and selected local Docker Unix socket; it refuses if the service is absent or
the query fails.

```text
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> --file <override-2> ... --profile <profile> ... logs --follow --tail 200 --no-color <SERVICE>
```

Compose's `--tail 200` applies per container. Ferrum retains a rolling maximum
of 200 combined lines/256 KiB and does not globally time-sort replicated output.
Raw lines are capped at 4096 bytes; when a line exceeds the cap, Ferrum discards
its remainder through newline and displays `[line omitted: exceeds 4096 bytes]`.
Output is escaped, transient, and excluded from files, audit, tracing, and
debug output. A dedicated 16-event queue applies backpressure; there is no
overall stream deadline or automatic retry. A clean EOF ends the session and a
static error is shown on failure.

**Stop log follow**, changing the imported files or profiles, refreshing status,
switching context/page, and closing Ferrum cancel the local Compose CLI
process group and discard queued/stale output. Follow is read-only and does not
stop or alter containers. It uses the same private Docker config/HOME, cleared
environment, and process limits described above. Selected and included Compose
files remain trusted local inputs under the logged-in user's permissions.
No real service logs or native visual workflow were validated.


## Select Compose profiles

Enter a comma-separated list of Compose profile names. Ferrum trims outer
whitespace from each token; blank input means no explicit profiles. The input
is limited to 1024 UTF-8 bytes, at most 16 profiles, and 64 UTF-8 bytes per
name. Empty entries (including leading, trailing, or repeated commas) and
duplicate names are rejected. A name must match
`[A-Za-z0-9][A-Za-z0-9_.-]+`. The special value `*` is accepted only by itself
and enables every profile in the Compose model. It does not start services:
status still lists existing containers, and each log query targets one selected
service. Ferrum passes each selected profile as its own `--profile <name>`
argument before the Compose subcommand:

```text
docker compose --file <base-file> ... --profile <name> config --no-interpolate --no-env-resolution --services
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> ... --profile <name> ps --all --format json
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> ... --profile <name> logs --tail 200 --no-color <service>
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> ... --profile <name> logs --follow --tail 200 --no-color <service>
```

Selected profiles are passed to structural validation, existing-container
status, one-shot snapshots, live service-log follow, and imported-configuration
start/stop/restart/up.
`config --services` inventory includes every service declared in the selected
files regardless of active profiles. Before either one-shot logs or follow,
the fresh status recheck uses the selected profiles. Profile flags
are separate argument-array entries, never environment variables. CLI
invocations retain Ferrum's private Docker config/HOME and cleared allowlisted
environment, including disabling the selected project's default `.env` and
remote Git/OCI loaders. Invalid profile input or changing the selection clears
validation, declared services, status, and snapshot/follow output and cancels
their in-flight tasks; validate again before refreshing status, requesting a
snapshot, or following logs. Lifecycle operations already submitted are not
cancelled by selection or navigation.

See Docker's [Compose profiles guide](https://docs.docker.com/compose/how-tos/profiles/)
and [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose/).

Saved registrations are the only persistent Compose-project metadata; ordinary
imports remain session-only until explicitly saved. Registration data excludes
project contents and does not confine selected paths. Structural validation
skips interpolation and service `env_file` resolution; status uses the isolated
environment above and may differ from a normal shell invocation. Start/stop/
restart use only the currently selected imported and revalidated configuration,
as described above. Restart requires the service in a successful status
snapshot and does not apply Compose configuration changes. The declared-service
inventory lists all services in the selected files, including services whose
profiles are not selected. Status reports only existing containers; the
separately shown declared-service inventory comes from local Compose
configuration and does not query the Engine. Issue
[#18](https://github.com/SzymonR1/ferrum-desktop/issues/18) remains
partial/open; [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10)
remains open/blocked. This is not a full-issue, release, or broader platform
claim.

The direct CLI probe and discovery smoke, single-file and ordered base/override
temporary-file validation smokes, and loopback remote-loader safety smoke
passed with Compose v5.6.0. The read-only temporary-project status smoke passed
with Docker Engine 29.9.0 and created no containers. The ignored ordered
profile-validation smoke asserts all declared service names, including one in
inactive profile `other`; it passed with Compose v5.6.0 using temporary files
and no Engine access. The opt-in profile-status smoke also passed using a unique
temporary project and the selected local socket on Docker Engine 29.9.0/default
context; `ps` returned no containers and the test created or changed no
resources. All validation smokes use test-owned files only. Discovery and status
query the selected local engine read-only; the remote-loader smoke verifies
Git/OCI references do not open a local connection or contact an external host.
The native Compose page visual check remains unavailable.
For security implications of remote and local references, see Docker's [Compose trust model](https://docs.docker.com/compose/trust-model).
This page does not claim full issue completion or a release.
