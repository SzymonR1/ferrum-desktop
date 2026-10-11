# Compose

The Compose page keeps its read-only project discovery and adds an explicit
action to validate one local Compose YAML file. Project discovery uses Docker's
Compose CLI:

- `docker compose version --short`
- `docker compose ls --all --format json`

The version probe detects the installed Docker Compose CLI version without
targeting an engine. The project listing is pinned to the socket selected by
Ferrum's current `EngineClient::local_socket_path()` using Docker's global
`--host unix://...` option. Discovery supports only Linux local Docker Unix
sockets; it is not a claim of support for Podman/rootless, remote Docker,
Windows, or macOS.

## Validate one local Compose file

Choose **Validate Compose file** and select one local `.yaml` or `.yml` file.
Ferrum first preflights the installed Compose CLI with
`docker compose version --short` through its private isolated Docker config.
A preflight failure (for example, unavailable CLI/plugin) is distinct from a
Compose config-validation failure. On successful preflight, Ferrum passes the
selected path as a separate argument:

```text
docker compose --project-directory <parent> --file <selected-file> config --quiet --no-interpolate --no-env-resolution
```

The project directory is the selected file's parent, so Compose resolves
relative paths from that directory. Validation does not contact or require a
Docker Engine socket, and Ferrum does not write, modify, delete, or extract
project files. A successful result means structural validation only:
interpolation and service `env_file` resolution are skipped. The selected path
is held only for the current validation session; Ferrum displays only a
bounded, escaped basename and a static result, never the full path or raw CLI
diagnostics.

Compose can recursively read local Compose files referenced by `include` or
service `extends.file`, including paths outside the selected Compose file's
directory, under the logged-in user's permissions. The selected file is not a
path-containment boundary. Private config/HOME and environment isolation are not
a sandbox; the Docker CLI and Compose plugin are trusted local code. Validate
only trusted Compose files.

Ferrum clears inherited environment values, sets `COMPOSE_DISABLE_ENV_FILE=1`,
passes no user environment files, and supplies `--no-interpolate` and
`--no-env-resolution`. Thus `.env` interpolation and service `env_file`
resolution are intentionally skipped. The eight-second command deadline and
1 MiB stdout cap apply to each CLI command, and raw stderr/diagnostics are
discarded.

Discovery and validation CLI processes use a private isolated Docker config
and `HOME` with a cleared allowlisted environment. If Compose is installed in
the standard per-user CLI plugin directory, only the `docker-compose`
executable is linked into the private config; custom `cliPluginsExtraDirs`
from saved Docker configuration are not loaded. The saved `config.json` is not
read; credentials and proxy settings are neither read nor passed to the CLI.

This remains a read-only page. Multi-file merge, profiles, `.env`/service
`env_file` resolution, remote OCI/Git, import, project start/stop/restart,
build/pull, logs, and project mutations are unsupported.

For official command and environment semantics, see Docker's
[Compose `config` reference](https://docs.docker.com/reference/cli/docker/compose/config),
[Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose),
and [Compose environment variables guide](https://docs.docker.com/compose/how-tos/environment-variables/envvars/).

The direct CLI probe, optional app-level discovery smoke, and controlled
temporary-file validation smoke passed with Compose v5.6.0 and Docker Engine
29.9.0/API 1.56. Discovery reads the selected local engine; validation parses
an owned temporary file without contacting an engine. Neither smoke mutates
projects. Linux local Docker discovery was validated; the native Compose page
visual check is unavailable. Issue [#18](https://github.com/SzymonR1/ferrum-desktop/issues/18) remains open; issue
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remains blocked.
This page does not claim full issue completion or a release.
