# Changelog

All notable project changes are documented here. The project has not published a release.

## Unreleased

- Added a native Rust workspace with an eframe desktop entry point.
- Added an asynchronous, read-only local Docker-compatible container inventory path using Bollard.
- Added application-owned container summaries, connection and empty states, architecture decisions, developer documentation, and CI configuration.
- Added Linux settings persistence for the startup auto-connect preference, with safe migration, private permissions, atomic replacement, and a settings window.
- Added a Linux Docker CLI context catalog and session-only selection for local Unix-socket contexts; remote contexts remain unsupported.
- Added live container filtering and sorting, a typed read-only inspection path, and an asynchronous details view that preserves unavailable fields.
- Remote TCP/HTTP/TLS/SSH transports remain unsupported; Podman/rootless behavior remains unvalidated and open in issue #10. Windows named pipes and macOS remain deferred to issue #21/M8.
