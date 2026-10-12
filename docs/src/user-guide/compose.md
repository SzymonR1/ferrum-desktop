# Compose

The Compose page keeps its read-only project discovery and provides structural
validation of a local base Compose file with ordered local overrides. Project
discovery uses Docker's Compose CLI:

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
docker compose --project-directory <base-parent> --file <base-file> --file <override-2> ... config --quiet --no-interpolate --no-env-resolution
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
validation session; Ferrum displays only bounded, escaped basenames and a static
result, never full paths or raw CLI diagnostics.
On Linux, each Compose subprocess (version, discovery, and validation) runs
with a 4 GiB `RLIMIT_AS` address-space ceiling and a 512 MiB `RLIMIT_DATA`
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

Profiles, `.env`/service `env_file` resolution, remote Git/OCI references
(explicitly disabled and rejected), import, project start/stop/restart,
build/pull, logs, and project mutations are unsupported. This multi-file
validation slice does not complete Issue [#18](https://github.com/SzymonR1/ferrum-desktop/issues/18), which remains open; [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remains blocked. It makes no full-issue, release, or broader platform-support claim.

For environment semantics, see Docker's [Compose environment variables guide](https://docs.docker.com/compose/how-tos/environment-variables/envvars/).
The direct CLI probe and discovery smoke, single-file and ordered base/override
temporary-file validation smokes, and loopback remote-loader safety smoke
passed with Compose v5.6.0. The ordered smoke uses owned temporary files without
an engine or user project; no project mutation is performed.
Discovery reads the selected local engine; validation parses local files
without contacting an engine. The remote safety smoke verifies Git/OCI include
references do not open a local connection or contact an external host. None of
the smokes mutates projects. Linux local Docker discovery was validated; the
native Compose page visual check is unavailable. For security implications of
remote and local references, see Docker's [Compose trust model](https://docs.docker.com/compose/trust-model).
This page does not claim full issue completion or a release.
