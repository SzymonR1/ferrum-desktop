# Changelog

All notable project changes are documented here. The project has not published a release.

## Unreleased

- Added a native Rust workspace with an eframe desktop entry point.
- Added an asynchronous, read-only local Docker-compatible container inventory path using Bollard.
- Added application-owned container summaries, connection and empty states, architecture decisions, developer documentation, and CI configuration.
- Added Linux settings persistence for the startup auto-connect preference, with safe migration, private permissions, atomic replacement, and a settings window.
- Podman compatibility, remote endpoints, container lifecycle actions, logs, embedded terminal, and installers remain unvalidated or unimplemented.
- Current support scope is Linux only; Windows/macOS are deferred to the cross-platform milestone.
