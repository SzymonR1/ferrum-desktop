# Threat model

## Compose discovery scope

The Compose page is read-only and supports only Linux local Docker Unix
sockets. The CLI version probe uses `docker compose version --short` and does
not target an engine. Project listing uses
`docker compose ls --all --format json`, pinned with Docker global
`--host unix://...` to the selected `EngineClient::local_socket_path()`. Each
CLI process uses a private isolated Docker config/HOME and a cleared allowlist.
If the Compose plugin is installed in the standard per-user directory, link
only its `docker-compose` executable into the private config; do not copy or
load user `config.json`, credentials, or proxy settings. Custom
`cliPluginsExtraDirs` from saved config are not loaded. Each command has an
eight-second deadline and a 1 MiB stdout cap. Retain and display only project
Name and Status; reject malformed or duplicate Name/Status fields and duplicate
project identities. Ignore ConfigFiles, paths, raw stderr, and daemon errors.
No Compose-file validation/import, project start/stop/restart, build/pull, logs,
or other project mutation is implemented. Podman/rootless, remote Docker,
Windows, and macOS are not covered. Only Linux local Docker was validated; the
native page visual check is unavailable, issue #18 remains open, and issue #10
remains blocked.

The installed Docker CLI and Compose plugin execute as Ferrum under the logged-in
user's normal filesystem and network permissions. Private config and environment
isolation are not a sandbox; treat both binaries as trusted local code.

{{#include ../../security/threat-model.md}}
