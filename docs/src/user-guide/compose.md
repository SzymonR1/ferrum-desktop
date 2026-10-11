# Compose

The Compose page discovers the installed Compose CLI version and lists Compose
projects through Docker's Compose CLI:

- `docker compose version --short`
- `docker compose ls --all --format json`

The version probe detects the installed Docker Compose CLI version without
targeting an engine. The project listing is pinned to the socket selected by
Ferrum's current `EngineClient::local_socket_path()` using Docker's global
`--host unix://...` option. This page supports only Linux local Docker Unix
sockets; it is not a claim of support for Podman/rootless, remote Docker,
Windows, or macOS.

CLI processes use a private isolated Docker config and `HOME` with a cleared
allowlisted environment. If Compose is installed in the standard per-user CLI
plugin directory, only the `docker-compose` executable is linked into the
private config for discovery; custom `cliPluginsExtraDirs` from saved Docker
configuration are not loaded. The user's saved `config.json`, credentials, and
proxy settings are not copied or passed to the CLI. Each command has an
eight-second deadline and a 1 MiB stdout cap. Ferrum retains and displays only
each project's `Name` and `Status`. It ignores `ConfigFiles` and all paths,
along with raw command diagnostics. Display output is escaped; raw stderr and
daemon errors are not displayed.

The installed Docker CLI and Compose plugin execute under the logged-in user
with normal filesystem and network permissions. Isolated config and environment
are not a sandbox; treat both binaries as trusted local code.

This is a read-only discovery page. Compose-file validation or import, project
start/stop/restart, build/pull, logs, and any other project mutation are not
implemented.

For command details, see the official Docker documentation for [Compose
`ls`](https://docs.docker.com/reference/cli/docker/compose/ls.md), [Compose
`version`](https://docs.docker.com/reference/cli/docker/compose/version), and
the [Docker CLI](https://docs.docker.com/reference/cli/docker/).

The direct CLI probe and optional ignored Ferrum listing smoke both passed with
Compose v5.6.0 and Docker Engine 29.9.0/API 1.56; the app test only lists
existing projects and validates safe output invariants, without mutation or
printing project names.
Linux local Docker was validated; the native Compose page visual check is
unavailable. Issue [#18](https://github.com/SzymonR1/ferrum-desktop/issues/18)
remains open; issue [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10)
remains blocked. This page does not claim full issue completion or a release.
