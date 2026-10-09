# Changelog

All notable project changes are documented here. The project has not published a release.

## Unreleased

- Added a native Rust workspace with an eframe desktop entry point.
- Added an asynchronous Linux local Docker-compatible inventory and typed inspection path using negotiated API requests.
- Added state-gated container lifecycle actions and bounded Docker container-event synchronization with manual refresh fallback.
- Added application-owned container summaries, connection and empty states, architecture decisions, developer documentation, and CI configuration.
- Added Linux settings persistence for the startup auto-connect preference, with safe migration, private permissions, atomic replacement, and a settings window.
- Added Linux Docker CLI context catalog and session-only selection for local Unix-socket contexts; remote contexts remain unsupported.
- Added live container filtering and sorting. Podman/rootless behavior remains unvalidated; remote transports are unsupported. Windows named pipes and macOS remain deferred to issue #21/M8.
- Documented and regression-tested explicit rejection of direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH endpoints without local fallback; remote transports remain unsupported under issue #10.
- Added bounded historical and followed container logs in the inspection dialog for local Unix-socket engines, with non-TTY stdout/stderr labels and combined labels for TTY rows, TTY-aware decoding, and in-memory limits.
- Added Copy, plain-text export to a user-selected path (the save dialog defaults to `container-logs.txt` and offers a text-file filter; the extension is not enforced), preserving source labels for retained visible rows, plus Pause/Resume and auto-scroll controls.
- Copy and Export escape terminal control characters in visible container-log rows as literal `\xNN`.
- Hardened container-log export by rejecting final-component symlinks and non-regular targets before writing, creating new regular files with owner-only permissions, and preserving ordinary rwx permission bits on existing files (the OS may clear setuid/setgid bits).
- Added an experimental embedded interactive exec terminal for the Linux local Docker-compatible Unix-socket slice. Terminal output is untrusted; OSC 52 clipboard and title requests are ignored, clipboard operations are user initiated, terminal contents are not logged, and closing the session does not stop or remove its container. Podman/rootless and remote compatibility remain open under issue #10.
- Added headless AccessKit coverage and opt-in release-mode measurements for large inventory filtering/sorting and bounded log-buffer throughput; native screen-reader validation remains unverified.
- Added a read-only Images page with case-insensitive search, virtualized listing, and asynchronous image inspection, plus anonymous public-reference pull progress through the validated Linux local Docker Unix socket. No credential reads or registry-auth headers are used; no live pull or native visual validation is claimed. **Stop monitoring** aborts Ferrum's client-side progress stream only; it does not cancel or claim to stop daemon-side image transfer. Tag/remove/build/push, private-registry auth, Podman/rootless, and remote endpoints remain out of scope; issues #16 and #10 remain open.
- Added structured local tracing events for completed container lifecycle requests, recording action, outcome, and validated full IDs only; raw errors, names, endpoints, and daemon bodies are excluded. Request errors do not prove the engine state remained unchanged. These process events are not a persistent audit store.
- Hardened settings-file opens and settings-directory permission updates against final-component symlinks; a pre-existing symlinked final directory is rejected before chmod or settings-file creation. Temporary-file creation and rename remain path-based under the configured parent.
