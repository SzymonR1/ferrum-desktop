# Compose

The Compose page provides read-only project discovery, structural validation of
a local base file with ordered overrides, a list of every service declared in
the selected files (including profile-gated services regardless of active
profiles), and read-only status for existing containers. The declared-service
list is local Compose CLI/file-derived and does not query an Engine; it is
separate from running-container status. A validated selection can be imported
under a local display label for the current application session only; it is
not persisted or treated as an Engine-discovered project.
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
docker compose --project-directory <base-parent> --file <base-file> --file <override-2> ... config --no-interpolate --no-env-resolution --services
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
is a point-in-time check of the explicitly selected files, not an atomic
security boundary: a same-UID process can change or replace a file after it is
checked. After the Compose version preflight, Compose reopens the original
selected paths for validation, so the files read need not be the same contents
that were measured. The size check excludes local files Compose reads through
`include` or service `extends.file`. It does not contact or require a Docker
Engine socket, and Ferrum does not write, modify, delete, or extract project
files. A successful result means structural validation only:
`--no-interpolate` skips interpolation and `--no-env-resolution` skips service
`env_file` resolution. The selected paths are held only for the current
validation session; Ferrum displays bounded, escaped basenames, a static
validation result, and the bounded escaped service list, never full paths or
raw CLI diagnostics.

## Import a validated configuration for this session

After validation succeeds for the current ordered files and profile selection,
enter a unique local display name and choose **Import validated selection**.
The name is a label only: it is not passed to Compose as a project name,
canonical identity, ownership claim, or authorization input. Imported entries
are shown separately from Engine-discovered Compose projects. The in-memory
list is limited to 32 entries and each name to 256 UTF-8 bytes; duplicate or
empty names are rejected.

Selecting an imported entry restores its exact base/override paths and profile
selection, then clears validation, declared services, container status, and
logs. Explicitly choose **Validate selected files** before requesting status or
logs. Importing or selecting an entry does not invoke the CLI, contact the
Engine, or run Compose up/start/stop/restart/build/pull; it does not copy,
write, or persist project files. Entries are lost when the application session
ends. The selected paths remain subject to the trusted-file warning below:
Compose can read local files referenced by `include` or service `extends.file`,
including related environment files, under the logged-in user's permissions.

On successful validation, Ferrum retains and displays the bounded service names
reported by the same `config --services` invocation. Names are escaped before
display in a virtualized list. Compose returns every service declared in the
selected files, including profile-gated services regardless of selected
profiles. The list is local CLI/file-derived, not running-container status; it
does not contact the Engine or trigger status/log operations. A later status
snapshot separately reports existing containers and may be empty even when
services are declared.
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
explicit `include.env_file` under the logged-in user's permissions. Each CLI
command has an eight-second deadline and a 1 MiB stdout cap; raw stderr and
diagnostics are discarded.

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
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> --file <override-2> ... ps --all --format json
```

This is a read-only status snapshot. `--all` includes stopped containers; the
Compose CLI's default orphan inclusion is preserved. Ferrum displays bounded,
escaped container name, service, optional per-container `Project` metadata,
state, and reported health. When Compose omits `Project`, the row shows
`Not reported`; rows may report different values. This engine-reported label is
untrusted metadata: it does not establish file ownership, canonical project
identity, authentication, or authorization, and it is not used by lifecycle
operations (none are available). Ferrum discards container IDs, command text,
published ports, exit-code data, and other response fields. No output means no
existing containers for that selection; it does not mean the Compose file
declares no services. The page does not poll automatically; refresh status when
needed.
Status also clears inherited environment values and disables the selected
project's default `.env`. Values normally supplied by the shell or that file may
therefore resolve differently; status can fail or match a different project
configuration than a normal shell invocation. It is a current read-only
snapshot, not proof that another environment or context has no containers.

See Docker's [Compose `ps` reference](https://docs.docker.com/reference/cli/docker/compose/ps/)
for the JSON Lines format and status command options.

Status is implemented only for Linux local Docker Unix sockets. It does not
persist a project, start/stop/restart containers, build/pull images, or mutate
project or Engine resources. The command uses Ferrum's private Docker config/HOME
and cleared environment; the selected project's default `.env` is disabled, and
remote Git/OCI loaders are disabled. If Compose cannot resolve the selected
project under this isolated environment, Ferrum shows a static status error
rather than raw CLI output. Compose may still read local files referenced by
`include` or `extends.file` and included-project environment sources under the
logged-in user's permissions; validate only trusted files.
The command uses the same subprocess timeout, output cap, and per-process
resource limits described above.

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

## Select Compose profiles

Enter a comma-separated list of Compose profile names. Ferrum trims outer
whitespace from each token; blank input means no explicit profiles. The input
is limited to 1024 UTF-8 bytes, at most 16 profiles, and 64 UTF-8 bytes per
name. Empty entries (including leading, trailing, or repeated commas) and
duplicate names are rejected. A name must match
`[A-Za-z0-9][A-Za-z0-9_.-]+`. The special value `*` is accepted only by itself
and enables every profile in the Compose model. It does not start services:
status still lists existing containers, and each log snapshot still targets one
selected service. Ferrum passes each selected profile as its own `--profile <name>`
argument before the Compose subcommand:

```text
docker compose --file <base-file> ... --profile <name> config --no-interpolate --no-env-resolution --services
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> ... --profile <name> ps --all --format json
docker --host unix://<selected-socket> compose --project-directory <base-parent> --file <base-file> ... --profile <name> logs --tail 200 --no-color <service>
```

Selected profiles are passed to structural validation, existing-container
status, and one-shot service logs. The `config --services` inventory includes
every service declared in the selected files regardless of the active profile
set. The status recheck immediately before logs uses the same profiles as the
earlier status query. Profile flags are separate argument-array entries, never
environment variables. CLI invocations retain Ferrum's private Docker config/HOME and cleared
allowlisted environment, including disabling the selected project's default
`.env` and remote Git/OCI loaders. Invalid profile input, or changing the
selection, clears validation, declared services, status, and logs and cancels
active Compose work; validate again before refreshing status or requesting
logs. Profiles and transient imports do not start, stop, restart, build, pull,
or otherwise manage project lifecycle.

See Docker's [Compose profiles guide](https://docs.docker.com/compose/how-tos/profiles/)
and [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose/).

Project persistence, lifecycle management (start/stop/restart/build/pull), live
log follow, and project mutations remain unsupported. Transient import is
session-only and does not persist files or project state. Structural validation
skips interpolation and service `env_file` resolution; status uses the isolated
environment above and may differ from a normal shell invocation. Profile
selection applies to structural validation, status, and one-shot service logs.
The declared-service inventory lists all services in the selected files,
including services whose profiles are not selected.
Status reports only existing containers; the separately shown declared-service
inventory comes from local Compose configuration and does not query the Engine.
Issue [#18](https://github.com/SzymonR1/ferrum-desktop/issues/18)
remains open/partial; [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10)
remains blocked. This is not a full-issue, release, or broader platform claim.

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
