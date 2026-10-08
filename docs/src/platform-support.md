# Platform support

Status distinguishes compilation from runtime validation. No platform is fully
supported until its required UI, engine, and installation behavior has been
exercised.

| Environment | Status | Evidence and limitations |
|---|---|---|
| Linux Wayland session (Fedora 44, KDE Plasma) | Engine integration tested; surface not visually verified | With `WAYLAND_DISPLAY` present, Winit selects Wayland. The app process loaded 15 containers, matching `docker ps -aq`; a missing local socket produced a connection error. The local desktop inspection provider does not support screenshots or accessibility, so the Wayland surface was not visually captured. |
| Linux X11 through XWayland | Smoke-tested | On 2026-10-08, the X11 backend opened a Ferrum window. A window-only capture displayed the real Docker Engine version/API and 15 live container rows (names, images, states, and statuses), matching `docker ps -aq` (15). A second capture showed the disconnected state with a safe socket diagnostic. This is XWayland, not a separate pure X11 session. |
| Other Linux compositors (GNOME, XFCE, Sway, Hyprland) | Not fully validated | Only the current KDE Plasma host was exercised; other compositors remain untested. |
| Windows | Not implemented; deferred by current Linux-only scope | Named-pipe support, runtime validation, and packaging are future work tracked by [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21). |
| macOS Apple Silicon and Intel | Not implemented; deferred by current Linux-only scope | Engine integration and packaging are future work tracked by [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21). |
| Podman on Linux | Not validated | No Podman executable or daemon was available; Docker-compatible API behavior and rootless socket discovery have not been tested. |

The initial engine integration is read-only and uses local Docker-compatible
sockets only. Remote endpoints, context switching, lifecycle operations,
installers, and embedded terminal behavior are not supported claims. Update
this matrix with tested OS/compositor or engine versions, date, available
features, and known limitations when new evidence is produced. See
[issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21).
