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
- Added bounded historical and followed container logs in the inspection dialog for local Unix-socket engines, with non-TTY stdout/stderr labels and combined labels for TTY rows, TTY-aware decoding, and in-memory limits.
- Added Copy, plain-text export to a user-selected path (the save dialog defaults to `container-logs.txt` and offers a text-file filter; the extension is not enforced), preserving source labels for retained visible rows, plus Pause/Resume and auto-scroll controls.
- Copy and Export escape terminal control characters in visible container-log rows as literal `\xNN`.
- Hardened container-log export by rejecting final-component symlinks and non-regular targets before writing, creating new regular files with owner-only permissions, and preserving ordinary rwx permission bits on existing files (the OS may clear setuid/setgid bits).
- Added an experimental embedded interactive exec terminal for the Linux local Docker-compatible Unix-socket slice. Terminal output is untrusted; OSC 52 clipboard and title requests are ignored, clipboard operations are user initiated, terminal contents are not logged, and closing the session does not stop or remove its container. Podman/rootless and remote compatibility remain open under issue #10.
