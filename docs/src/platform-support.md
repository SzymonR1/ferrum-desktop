# Platform support

Status distinguishes compilation from runtime validation. No platform is fully
supported until its required UI, engine, and installation behavior has been
exercised.

| Environment | Status | Evidence and limitations |
|---|---|---|
| Linux Wayland session (Fedora 44, KDE Plasma) | Engine integration tested; surface not visually verified | With `WAYLAND_DISPLAY` present, Winit selects Wayland. The app process loaded 15 containers, matching `docker ps -aq`; a missing local socket produced a connection error. The local desktop inspection provider does not support screenshots or accessibility, so the Wayland surface was not visually captured. |
| Linux X11 through XWayland | Smoke-tested | On 2026-10-08, the X11 backend opened a Ferrum window. A window-only capture displayed the real Docker Engine version/API and 15 live container rows (names, images, states, and statuses), matching `docker ps -aq` (15). A second capture showed the disconnected state with a safe socket diagnostic. This is XWayland, not a separate pure X11 session. |
| Linux Docker CLI contexts with local Unix sockets | Implemented; deterministic coverage added and live coverage is opt-in | Context enumeration/switching requires the Docker CLI. Without it, direct `DOCKER_HOST` and automatic local socket discovery remain available. Only local Unix-socket endpoints are selectable; the ignored live test reads version and inventory without modifying resources. |
| Linux remote Docker contexts (TCP/HTTP/TLS/SSH) | Unsupported | Remote transports are deliberately rejected; Ferrum does not read or configure remote TLS credentials, and does not downgrade to a local endpoint. |
| Other Linux compositors (GNOME, XFCE, Sway, Hyprland) | Not fully validated | Only the current KDE Plasma host was exercised; other compositors remain untested. |
| Windows | Not implemented; deferred by current Linux-only scope | Named-pipe support, runtime validation, and packaging are future work tracked by [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21). |
| macOS Apple Silicon and Intel | Not implemented; deferred by current Linux-only scope | Engine integration and packaging are future work tracked by [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21). |
| Podman on Linux | Not validated; no support claim | Docker CLI context presence is not evidence of Podman compatibility. Podman executable/daemon and rootless socket behavior remain unvalidated; see [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10) and [issue #11](https://github.com/SzymonR1/ferrum-desktop/issues/11). |

The #10 context-selection slice covers Linux local Unix-socket contexts only.
Remaining #10 work includes Podman/rootless validation and explicit remote
TLS/SSH policy; the issue remains open. Windows named pipes and macOS remain
deferred to [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21)
and M8.
