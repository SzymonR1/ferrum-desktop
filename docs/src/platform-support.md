# Platform support

Status distinguishes compilation from runtime validation. No platform is fully
supported until its required UI, engine, and installation behavior has been
exercised.

| Environment | Status | Evidence and limitations |
|---|---|---|
| Linux Wayland session (Fedora 44, KDE Plasma) | Engine integration tested; surface not visually verified | With `WAYLAND_DISPLAY` present, Winit selects Wayland. The app process loaded 15 containers, matching `docker ps -aq`; a missing local socket produced a connection error. The local desktop inspection provider does not support screenshots or accessibility, so the Wayland surface was not visually captured. |
| Linux X11 through XWayland | Smoke-tested | On 2026-10-08, the X11 backend opened a Ferrum window. A window-only capture displayed the real Docker Engine version/API and 15 live container rows (names, images, states, and statuses), matching `docker ps -aq` (15). A second capture showed the disconnected state with a safe socket diagnostic. This is XWayland, not a separate pure X11 session. |
| Linux Docker CLI contexts with local Unix sockets | Implemented; deterministic coverage added and live coverage is opt-in | Context enumeration/switching requires the Docker CLI. Without it, direct `DOCKER_HOST` and automatic local socket discovery remain available. Only local Unix-socket endpoints are selectable; the ignored live test reads version and inventory without modifying resources. |
| Linux local Docker Unix-socket image listing, inspection, read-only history, anonymous public pull/push, tagging, removal, and local Buildx builds | Scoped engine API/UI and local Buildx flows; fake-socket/headless tests; opt-in read-only history smoke passed 2026-10-11 on Docker Engine 29.9.0/API 1.56; no live image mutation, registry contact for push, or live build; native Images-page visuals unverified | History is read-only and uses the selected local Docker Unix socket and exact current inventory image ID; only validated full SHA-256 layer IDs, nonnegative creation times, and nonnegative sizes are retained; CreatedBy, tags, comments, and other daemon fields are discarded. It is bounded to eight seconds, 16 MiB, and 65,536 rows. Push accepts one explicit tag already reported for the selected image, reinspects its ID before submission, and requires confirmation warning that layers/metadata may disclose secrets. It sends an empty JSON body without stored credentials or X-Registry-Auth; registries requiring authentication reject the request. Push coverage uses fake sockets and headless UI only and neither mutates images nor contacts a registry. Buildx coverage uses controlled helpers, not a live build; the CLI flow is pinned to the selected local socket with a private config and bounded transient output. No live image mutation/build is claimed. Podman/rootless and remote support remain unavailable or unvalidated; issues #16 and #19 remain partial/open and #10 remains blocked/open. |

| Linux local Docker Unix-socket volume list/inspect/usage/name-only creation and guarded removal | Scoped API/UI slice; fake-socket and headless tests, opt-in owned-volume live test; native page not visually validated | List/inspect retains allowlisted metadata; usage reports `Size` and point-in-time `RefCount` only for exact inventory matches (`-1`, negative, or missing values are unavailable), with a 30-second/16 MiB usage bound. List, inspect, create, and removal use eight-second request bounds. Creation sends only `Name` and uses Docker's default local driver. Removal requires exact listed/inspected identity, `RefCount == 0`, and explicit confirmation; DELETE omits `force`, and a changing dependency may make Docker reject it. Optional driver/scope/time/mountpoint fields escape selected unsafe text; mountpoints are never accessed. Context switching is blocked and inventory refreshes after every mutation result. No pruning, modification, connection, disconnection, or filesystem browsing is offered. Podman/rootless is unvalidated; remote, Windows, and macOS support is not implemented. Issues #17 and #10 remain open. |
| Linux local Docker Unix-socket network list/inspect/create/remove/connect/disconnect | Scoped Linux API/UI slice; fake-socket/headless coverage; opt-in live attachment smoke passed 2026-10-10 on Docker Engine 29.9.0/API 1.56; native page not visually validated | Creation sends only `Name`; removal requires a fresh zero-reference inspection and exact-ID confirmation without force/prune. Attachment controls require complete validated membership IDs and an exact current-inventory container ID in `Running` state, plus confirmation. Requests send only `Container`, omit endpoint configuration/Force, use an eight-second deadline, and are not retried. Verbose inspection adds aggregate IP allocation counts; details and unavailable-value semantics are below. The live smoke uses preinstalled `FERRUM_TEST_IMAGE`, `--pull never`, and exact-ID cleanup without force/prune. No usage, Podman/rootless, or remote support is claimed. |
| Linux local Docker Compose discovery, ordered validation, all-declared-service inventory, status, and one-shot service-log snapshots | Read-only CLI/UI with fake-CLI/headless profile and inventory coverage; ordered validation/status smokes passed with Compose v5.6.0; profile validation smoke confirms all service names, including an inactive-profile service, without Engine access; native page visual check unavailable; no real service log was read | Discovery/status/log queries use the selected local Unix socket. Structural validation and its `config --services` inventory use selected files and the local CLI only, with no Engine contact. The inventory lists every declared service, including profile-gated services regardless of profile selection; it is not running-container status. One-shot logs recheck current service membership before querying; Compose tails 200 lines per container, while Ferrum shows at most the first 200 combined response lines and 256 KiB without a global chronological merge. Output is escaped/transient; there is no live follow. Profile selection is bounded and passed as repeated `--profile` arguments to validation/status/logs; the inventory still lists all declarations. The isolated CLI environment disables the selected project's `.env`; environment-dependent resolution may differ from a normal shell. Include/extends reads remain under user permissions; validate only trusted files. Project import/persistence, lifecycle, build/pull, live follow, and project mutations remain unsupported; #18 partial/open and #10 blocked/open. |
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

Image inventory, inspection, read-only layer history, anonymous public-image
pulls and push, local image tagging, and image removal are limited to Linux
local Docker Unix sockets.

History is a read-only `GET /v{api}/images/{id}/history` through the negotiated
API on the selected local socket, using the exact selected ID from the current
image inventory. Only validated full SHA-256 layer IDs, nonnegative `Created`
timestamps, and nonnegative `Size` values are retained; `CreatedBy` command
text, tags, comments, and other daemon fields are discarded. History is bounded
to an eight-second request timeout, 16 MiB response, and 65,536 rows. Fake-
socket response tests cover the row boundary and rejection of excess rows. The
opt-in ignored read-only smoke passed on 2026-10-11 against Docker Engine
29.9.0/API 1.56 using a preinstalled image and selected local Unix context; it
listed and inspected the image, read history, and did not mutate it.

Push is anonymous-only: it reads no saved registry credentials and does not
send `X-Registry-Auth`; registries requiring credentials reject the request.
It accepts one explicit tag already reported for the selected image and
reinspects that image ID before submission, but this check is not atomic with
external Docker clients. Confirmation names the image and target and warns
that layers/metadata may contain secrets. Push tests use fake sockets and
headless UI state only; they do not mutate images or contact a registry. No
live image push is claimed. Closing Ferrum does not guarantee daemon-side
transfer work stops.

Each tag request requires confirmation because Docker Engine v1.56 may reassign
an existing `repository:tag`; the image data remains. Image-removal confirmation
snapshots the selected ID and known tags and warns that removing the last
reference can delete image data. Removal omits `force` and sets `noprune=true`
to preserve untagged parent images; the engine may refuse images used by
containers or with other tags. Only one image mutation (pull/push/tag/remove) is
in flight at a time. A timeout or request error does not prove removal had no
effect; stale UI results are ignored without aborting or claiming to undo a
submitted request. Inventory refresh after success occurs only when Images is
current. No live image mutation is claimed; tests do not mutate images.

Local Buildx invokes the Docker CLI through argument arrays pinned to the
selected local socket with an empty private config; build output is bounded and
transient. Buildx tests use controlled helper processes and do not run a live
build or mutate images. Private-registry authentication, Podman/rootless
behavior, and remote endpoints remain unsupported or unvalidated. Issues
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) and
[#19](https://github.com/SzymonR1/ferrum-desktop/issues/19) remain partial/open;
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remains blocked/open.
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

Verbose IPAM inspection reports only point-in-time aggregate used/reserved and
dynamically available IP allocation counts when the local Docker Engine reports
them; these are not traffic measurements, and per-container/interface counters
are not aggregated. Invalid CIDR keys or duplicate normalized subnet keys make
both metrics unavailable. Missing, malformed, negative, or overflowing count
values and checked-sum overflow make only the affected metric unavailable; the
other metric may remain available. Subnet/CIDR keys, addresses, endpoint data,
and raw status are not exposed. Podman/rootless behavior remains unvalidated
for this usage data.

The optional read-only `local_engine_reads_verbose_ipam_allocation_for_existing_network_read_only`
test passed on 2026-10-11 against Docker Engine 29.9.0/API 1.56 with
`FERRUM_TEST_NETWORK=bridge`. It confirmed that this local engine reports an
aggregate allocation count. This does not validate other engines or endpoints;
the native Networks page remains visually unverified.

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

Compose discovery, ordered multi-file structural validation, and read-only
container status are limited to Linux local Docker Compose CLI. Validation
selects one base `.yaml`/`.yml` file followed by up to 15 local overrides in
user-chosen order (base first; 16 files maximum). Docker merges files in `-f`
order; Ferrum sets `--project-directory` to the base file's parent. See Docker's
[Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for path and merge behavior, and [`config`](https://docs.docker.com/reference/cli/docker/compose/config)
for structural-validation flags.

After selected files pass structural validation, status runs
`docker --host unix://<selected-socket> compose --project-directory <base-parent> --file ... ps --all --format json`
against that same socket. This read-only JSON Lines query includes stopped
containers and the CLI's default orphan inclusion; it reports only existing
containers. The separate declared-service inventory comes from the
engine-independent validation CLI result; an empty status response does not
imply that no services are declared. Ferrum retains only
bounded escaped name/service/state/health; IDs, commands, ports, and raw
diagnostics are discarded. Empty output is distinct from a query error. Status
is a manually refreshed snapshot and does not mutate project or engine resources.

Validation preflights `docker compose version --short` through the private
isolated Docker config; preflight failure is distinct from config-validation
failure. It does not use an engine or mutate project files. Success means
structural validation only; interpolation and service `env_file` resolution
are skipped. Remote Git/OCI loaders are disabled and rejected without a fetch.
Compose may recursively read local files referenced by `include` or service
`extends.file`, plus each included project's default `.env` and explicit
`include.env_file` paths. These may be outside the base directory and are read
under the logged-in user's permissions. `COMPOSE_DISABLE_ENV_FILE=1` disables
only the selected project's default `.env`; the `config` flags do not confine
included-project reads. The selected files are not a containment boundary;
validate only trusted files. The CLI/plugin is trusted local code and is not
sandboxed. Ferrum clears inherited environment, does not pass user environment
files, uses private Docker config/HOME, and bounds each command to eight
seconds and 1 MiB stdout.

Status uses the cleared environment/private CLI config and disables the selected
project's default `.env`; environment-dependent projects may fail or resolve
differently from a normal shell invocation. Included local files and their
environment sources may still be read under the logged-in user's permissions.

The read-only temporary-project status smoke passed with Compose v5.6.0 on
Docker Engine 29.9.0 and created no containers. Discovery, single-file
validation, ordered base/override, and loopback remote-loader smokes also passed;
the validation project smokes use temporary files and do not contact the engine.
These checks do not validate arbitrary projects or lifecycle operations. Native
Compose-page visual validation is unavailable. One-shot service log snapshots
are covered by fake-CLI/headless tests only; no real project logs were read.
Compose profile selection is supported only for structural validation,
read-only status, and one-shot service logs, including the same-profile status
recheck immediately before logs. Input is comma-separated and trimmed per
token; blank means no explicit profiles. Input is limited to 1024 UTF-8 bytes,
16 profiles, and 64 bytes per name; empty and duplicate entries are rejected.
Names match `[A-Za-z0-9][A-Za-z0-9_.-]+`, with `*` accepted only alone. Each
profile is a separate `--profile` argv pair before `config`, `ps`, or `logs`,
not an environment variable. Invalid or changed selection clears results,
cancels in-flight work, and requires revalidation. Private Docker config/HOME,
the cleared environment, disabled selected-project `.env`, and disabled remote
loaders remain in effect. This adds no project import/persistence,
start/stop/restart, build/pull, live log follow, or project mutation. See
Docker's [Compose profiles guide](https://docs.docker.com/compose/how-tos/profiles/)
and [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose/).
Issue #18 remains partial/open; issue #10 remains blocked/open. No full issue
completion, release, or broader support claim is made.
