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

| Linux local Docker Unix-socket volume listing, inspection, and usage | Read-only API/UI slice; headless tests and optional live read-only coverage; native page not visually validated | Lists and inspects existing volumes and requests per-volume usage with `GET /v{api}/system/df?verbose=true&type=volume`. `Size` is reported in bytes; `RefCount` is the engine's point-in-time count of containers referencing the volume. Zero is a reported zero; `-1`, another negative value, or a missing value is unavailable. The volume-usage request has a scoped 30-second total request deadline and a 16 MiB response limit; only volume-usage calls receive this longer bound, while list and inspect retain their existing eight-second timeout. Ferrum accepts usage only for exact volume-name matches in the loaded inventory, retains no labels or options, and does not access mountpoints. The page displays the inspect mountpoint only as untrusted plain text. Volume creation, removal, pruning, modification, connection, and disconnection are not available; a zero reference count is not a deletion guarantee. Podman/rootless behavior is unvalidated, remote endpoints are unsupported, and Windows/macOS support is not implemented. Issues #17 and #10 remain open. |
| Linux local Docker Unix-socket network listing, inspection, default bridge creation, and unused custom-network removal | Scoped API/UI implementation; fake-socket contracts, headless state/UI coverage, and optional owned-resource live test; native UI not visually validated | Lists and inspects allowlisted metadata; creation sends only `Name`. Removal requires a fresh matching inspect with zero attached containers and an explicit full-ID confirmation; it uses negotiated DELETE without force/prune. Dependency snapshots are not atomic with deletion. The live test verifies its own full ID and generated name, removes via EngineClient, and confirms absence; CLI cleanup is only the Drop fallback. No usage, connect, disconnect, Podman/rootless, or remote support is claimed. |
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
Network listing, inspection, name-only default bridge creation, and constrained
removal are limited to Linux local Docker Unix sockets. The optional ignored
read-only test `local_engine_lists_and_inspects_existing_network_read_only`
requires `FERRUM_TEST_NETWORK` to name an existing network, lists networks,
then inspects the matching listed ID without mutation or cleanup. The optional
ignored create-and-remove test preflights a generated name, then requires the
returned full 64-character hexadecimal ID and a subsequent inventory entry to
match the generated name before arming cleanup. It removes only that verified
ID through `EngineClient::remove_network` and confirms the ID is absent. The
Drop guard's argument-array Docker CLI cleanup is an exact-ID fallback only;
neither path removes by name or runs prune. If ownership cannot be confirmed,
no removal or cleanup request is sent and the network may remain.

The UI rejects the predefined `bridge`, `host`, and `none` networks and enables
removal only after a fresh inspection of the selected matching ID reports
`Some(0)` attached containers. Positive or unavailable counts disable removal.
Confirmation names the exact network and full ID. This count is only a
point-in-time snapshot: Docker can reject a request if the network becomes
attached after inspection, and a transport error or timeout does not prove the network remains.
The bounded DELETE omits force and prune parameters; typed errors do not expose
daemon bodies. Only one network mutation runs at a time, context switching is
blocked while it is pending, and successful removal refreshes current inventory.
Default tests and fake-socket/headless UI tests do not contact or mutate a live
engine. Native UI click/visual validation remains unverified. Podman/rootless and
remote endpoints remain unsupported or unvalidated. Issues
[#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) and
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remain open; this
slice does not complete either issue or constitute a release claim.
