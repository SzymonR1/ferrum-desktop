# Platform support

Status distinguishes compilation from runtime validation. No platform is fully
supported until its required UI, engine, and installation behavior has been
exercised.

| Environment | Status | Evidence and limitations |
|---|---|---|
| Linux Wayland session (Fedora 44, KDE Plasma) | Engine integration tested; surface not visually verified | With `WAYLAND_DISPLAY` present, Winit selects Wayland. The app process loaded 15 containers, matching `docker ps -aq`; a missing local socket produced a connection error. The local desktop inspection provider does not support screenshots or accessibility, so the Wayland surface was not visually captured. |
| Linux X11 through XWayland | Smoke-tested | On 2026-10-08, the X11 backend opened a Ferrum window. A window-only capture displayed the real Docker Engine version/API and 15 live container rows (names, images, states, and statuses), matching `docker ps -aq` (15). A second capture showed the disconnected state with a safe socket diagnostic. This is XWayland, not a separate pure X11 session. |
| Linux Docker CLI contexts with local Unix sockets | Implemented; deterministic coverage added and live coverage is opt-in | Context enumeration/switching requires the Docker CLI. Without it, direct `DOCKER_HOST` and automatic local socket discovery remain available. Only local Unix-socket endpoints are selectable; the ignored live test reads version and inventory without modifying resources. |
| Linux local Docker Unix-socket image listing, inspection, anonymous public pull, tagging, and removal | Engine API/UI scope implemented; headless tests only; no live image mutation or native image-page visual validation | Listing, inspection, pull, tag, and removal use the validated local Docker Unix socket. Pull, tag, and removal send no registry authentication or stored credentials. Tagging requires confirmation on every request because Docker Engine v1.56 may reassign an existing `repository:tag`; the image data remains. Removal snapshots the selected ID and known tags, warns about last-reference data deletion and possible non-force rejection, and uses `noprune=true` without `force`. Fake-socket and headless UI tests do not contact a real engine or mutate images; no live pull/tag/removal operation is claimed; issues #16 and #10 remain open. |

| Linux local Docker Unix-socket volume list/inspect/usage/name-only creation and guarded removal | Scoped API/UI slice; fake-socket and headless tests, opt-in owned-volume live test; native page not visually validated | List/inspect retains allowlisted metadata; usage reports `Size` and point-in-time `RefCount` only for exact inventory matches (`-1`, negative, or missing values are unavailable), with a 30-second/16 MiB usage bound. List, inspect, create, and removal use eight-second request bounds. Creation sends only `Name` and uses Docker's default local driver. Removal requires exact listed/inspected identity, `RefCount == 0`, and explicit confirmation; DELETE omits `force`, and a changing dependency may make Docker reject it. Optional driver/scope/time/mountpoint fields escape selected unsafe text; mountpoints are never accessed. Context switching is blocked and inventory refreshes after every mutation result. No pruning, modification, connection, disconnection, or filesystem browsing is offered. Podman/rootless is unvalidated; remote, Windows, and macOS support is not implemented. Issues #17 and #10 remain open. |
| Linux local Docker Unix-socket network list/inspect/create/remove/connect/disconnect | Scoped Linux API/UI slice; fake-socket/headless coverage; opt-in live attachment smoke passed 2026-10-10 on Docker Engine 29.9.0/API 1.56; native page not visually validated | Creation sends only `Name`; removal requires a fresh zero-reference inspection and exact-ID confirmation without force/prune. Attachment controls require complete validated membership IDs and an exact current-inventory container ID in `Running` state, plus confirmation. Requests send only `Container`, omit endpoint configuration/Force, use an eight-second deadline, and are not retried. The live smoke uses preinstalled `FERRUM_TEST_IMAGE`, `--pull never`, and exact-ID cleanup without force/prune. No usage, Podman/rootless, or remote support is claimed. |
| Linux local Docker Unix-socket Compose discovery and single-file validation | Read-only CLI/UI; optional live discovery listing, temporary-file validation, and loopback remote-loader smokes passed with Compose v5.6.0 and Docker Engine 29.9.0/API 1.56; native page visual check unavailable | Discovery uses `docker compose version --short` and `docker compose ls --all --format json`, pinning only the listing to the selected `EngineClient::local_socket_path()`. Validation uses `rfd::AsyncFileDialog` to accept one local YAML file, then runs `docker compose --project-directory <parent> --file <selected-file> config --quiet --no-interpolate --no-env-resolution`; the parent is the selected file's directory. The private child environment sets `COMPOSE_DISABLE_ENV_FILE=1`, `COMPOSE_EXPERIMENTAL_GIT_REMOTE=false`, and `COMPOSE_EXPERIMENTAL_OCI_REMOTE=false`, so selected-project interpolation and service `env_file` resolution are skipped and remote Git/OCI `include`/`extends.file` fetches are disabled. The loopback-only safety smoke verified remote Git/OCI `include` references do not open a local connection; it contacts no external host or engine. Multiple files and profiles remain unsupported. Compose may recursively read local files referenced by `include` or service `extends.file`, and included projects may also read their default `.env` or explicit `include.env_file` paths, including outside the selected directory under the logged-in user's permissions. These reads are not confined by the selected path; validate only trusted files. Both commands use private Docker config/HOME, an allowlisted environment, an eight-second deadline, and a 1 MiB stdout cap. Validation contacts no engine or external host and Ferrum itself writes no project files; selected paths and raw diagnostics are not displayed. See Docker's [Compose config](https://docs.docker.com/reference/cli/docker/compose/config), [Compose CLI](https://docs.docker.com/reference/cli/docker/compose), [Compose include reference](https://docs.docker.com/reference/compose-file/include/), [Compose trust model](https://docs.docker.com/compose/trust-model), and [environment-variable guide](https://docs.docker.com/compose/how-tos/environment-variables/envvars/). Only Linux local Docker discovery was validated. Issue #18 remains open; issue #10 remains blocked. |
| Linux local lifecycle and container-event synchronization | Engine integration-tested; lifecycle audit events unit-tested; UI action clicks not visually verified | On 2026-10-08, the opt-in ignored lifecycle test passed against Docker Engine 29.8.2/API 1.56. It exercised Start/Stop/Restart/Pause/Unpause/Remove, observed an external CLI Start event, and verified state, inventory, and removal for one owned disposable container. Ferrum gates actions on known state; Remove is confirmed and does not force-remove or delete volumes. Lifecycle request results emit structured process-tracing events with action, `success` or `request_error`, and a validated full container ID when available; raw errors, names, endpoints, and daemon bodies are excluded. A request error does not prove the engine state remained unchanged. These events are not a persistent audit store. Event processing is bounded, reconnect retries are limited, and manual refresh remains available. |
| Linux local historical/live container logs | Engine integration-tested; headless Logs controls tested; native surface not visually/screen-reader validated | On 2026-10-08, the opt-in ignored live test passed against the selected local Unix-socket Docker engine using the preinstalled `ghcr.io/catthehacker/ubuntu:rust-latest` image. It decoded timestamped historical and followed stdout/stderr markers, verified TTY mode, dropped the stream, and removed the owned container. A headless egui/accessibility test interacts with the real Logs dialog, exercising Pause/Resume, auto-scroll, Copy output, and Export availability. Native click/screenshot/screen-reader validation remains unavailable because the desktop provider cannot enumerate Ferrum or capture/accessibility state. |
| Linux local embedded terminal | Experimental; local Docker-compatible Unix sockets only; visual and screen-reader validation not established | On 2026-10-09, the opt-in live terminal test passed against Docker Engine 29.8.2/API 1.56 using the preinstalled `ghcr.io/catthehacker/ubuntu:rust-latest` image. It verified TTY input/output, ANSI and Unicode bytes, resize to 37x101, Ctrl+C, and that dropping the exec session leaves the container running; the test then stopped and removed its own container, and no `ferrum-live-exec-*` container remained. The engine path is live-tested; the native terminal surface remains untested visually and with a screen reader. Terminal output is untrusted; OSC 52 clipboard and title requests are ignored; clipboard controls require explicit user action. Podman/rootless and remote compatibility remain open in issue #10. |
| Linux eframe/AccessKit interaction | Headless interaction-tested; native screen-reader validation not established | On 2026-10-09, headless egui_kittest tests verified the Search text-field label, AccessKit row actions, Tab/Space focus activation, and a virtualized 1,000-row inventory. The desktop provider cannot enumerate Ferrum or capture screenshots, so native screen-reader output and visual focus treatment remain unvalidated. |
| Linux remote Docker endpoints and contexts (direct `DOCKER_HOST`; TCP/HTTP/HTTPS/SSH) | Unsupported; deterministic rejection coverage | Direct remote `DOCKER_HOST` values and remote contexts return typed unsupported errors before local socket discovery. Ferrum does not read remote TLS/SSH credentials or downgrade transports. |
| Other Linux compositors (GNOME, XFCE, Sway, Hyprland) | Not fully validated | Only the current KDE Plasma host was exercised; other compositors remain untested. |
| Windows | Not implemented; deferred by current Linux-only scope | Named-pipe support, runtime validation, and packaging are future work tracked by [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21). |
| macOS Apple Silicon and Intel | Not implemented; deferred by current Linux-only scope | Engine integration and packaging are future work tracked by [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21). |
| Podman on Linux | Not validated; no support claim | Docker CLI context presence is not evidence of Podman compatibility. Podman executable/daemon and rootless socket behavior remain unvalidated; see [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10) and [issue #11](https://github.com/SzymonR1/ferrum-desktop/issues/11). |

The #10 context-selection slice covers Linux local Unix-socket contexts only.
Direct remote DOCKER_HOST values and remote contexts are explicitly rejected
without local fallback; remote transport support remains disabled. Podman/rootless
validation and a real engine endpoint matrix remain open under issue #10. The
experimental terminal and lifecycle support do not expand that transport or
platform scope. Windows named pipes and macOS remain deferred to
[#21](https://github.com/SzymonR1/ferrum-desktop/issues/21) and M8.

Image inventory, inspection, anonymous public-image pulls, local image tagging,
and image removal are limited to Linux local Docker Unix sockets. Each tag
request requires confirmation because Docker Engine v1.56 may reassign an
existing `repository:tag`; the image data remains. Image-removal confirmation
snapshots the selected ID and known tags and warns that removing the last
reference can delete image data. Removal omits `force` and sets `noprune=true`
to preserve untagged parent images; the engine may refuse images used by
containers or with other tags. Only one image mutation (pull/tag/remove) is in
flight at a time. A timeout or request error does not prove removal had no
effect; stale UI results are ignored without aborting or claiming to undo a
submitted request. Inventory refresh after success occurs only when Images is
current. No live pull, tag, or removal operation is claimed, and tests do not
mutate images. **Hide progress** suppresses pull updates but keeps Ferrum draining
the response and the image-mutation gate closed until a terminal event.
Page/context invalidation hides stale updates without aborting the reader;
closing Ferrum does not guarantee that daemon-side pull work stops.
Private-registry authentication, Podman/rootless behavior, and remote endpoints
remain unsupported or unvalidated. Issues
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) and
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remain open.
Network listing, inspection, name-only default-bridge creation, guarded unused
custom-network removal, and confirmed connect/disconnect are limited to Linux
local Docker Unix sockets. The optional ignored read-only
`local_engine_lists_and_inspects_existing_network_read_only` test requires
`FERRUM_TEST_NETWORK` to name an existing network, lists networks, then
inspects the matching listed ID without mutation or cleanup. The optional
create-and-remove test preflights a generated name, verifies a full returned
ID and exact inventory name before cleanup. It removes only that ID through
`EngineClient::remove_network` and confirms absence.
The `Drop` fallback retains that `EngineClient`, pinned to the selected Unix
socket, and attempts exact-ID removal only if still armed; it uses no force or
prune. If ownership cannot be confirmed, no removal is sent and the network
may remain for manual inspection.

The UI permits network removal only for selected custom networks whose fresh
matching inspection reports `Some(0)` attached containers. Connect/disconnect
controls require a fresh complete `Containers` map containing only full
hexadecimal IDs and a target full ID in current inventory whose state is
`Running`; stopped, paused, and unknown states are disabled. Confirmation
names both exact targets. The bounded attachment POST sends only `Container`,
no endpoint configuration or `Force`, and is not retried; an uncertain result
may follow a daemon-side change. Ferrum refreshes network/container inventory
and inspects the selected network after either result. Mutations are
single-flight and block context switching. Container-change events invalidate
cached state; a Networks refresh reloads both inventories, and controls remain
disabled when the container read is stale or fails.

The opt-in live attachment test requires a current local Unix Docker context
and a preinstalled `FERRUM_TEST_IMAGE` containing `/bin/sleep`. On 2026-10-10
it passed against Docker Engine 29.9.0/API 1.56: the test created a uniquely
named network and container with `--network bridge --pull never`, verified
their exact generated names and full IDs, started the container, connected
and inspected membership, disconnected and re-inspected, then stopped and
removed only the owned resources without force or prune. It never pulled an
image or published ports/host mounts.

The network cleanup guard is armed only after a successful create response's
full ID matches the generated name in fresh inventory. A create error,
mismatched ID, or failed inventory read does not authorize cleanup; the network
may remain for manual inspection. The container guard is armed only after its
full ID matches the exact generated name. Once armed, both guards use the same
pinned `EngineClient` socket and exact IDs without force or prune. If identity
cannot be verified, no stop/remove request is sent and the resource may remain.
The native Networks page remains unverified visually. Podman/rootless and
remote endpoints remain unsupported or unvalidated. Issues #17 and #10 remain
open; this slice does not complete either issue or constitute a release claim.

Compose discovery and single-file validation are limited to Linux local Docker.
Validation preflights `docker compose version --short` through the private
isolated Docker config; a failed preflight is distinct from a config-validation
failure. Validation uses no engine. Success means structural validation only;
the selected project's interpolation and service `env_file` resolution are
skipped. Remote Git/OCI loaders are disabled and rejected without a fetch.
Compose may recursively read local Compose files referenced by `include` or
service `extends.file`, plus each included project's default `.env` and
explicit `include.env_file` paths. These may be outside the selected file's
directory under the logged-in user's permissions; the selected path is not a
containment boundary. Validate only trusted Compose files; the CLI/plugin is
trusted local code and is not sandboxed.
Multi-file merge, profiles, `.env`/service `env_file` resolution, remote
OCI/Git, import, lifecycle/build/pull/logs, and mutations remain unsupported.
Ferrum itself performs no project writes or engine operations during
validation; selected paths and raw diagnostics are not displayed. Issue #18
remains open and issue #10 remains blocked; no release/support expansion is
claimed.
