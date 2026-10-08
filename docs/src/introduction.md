# Ferrum

Ferrum is a native Rust desktop application for managing existing Docker-compatible container engines. It does not bundle an engine, install privileged services, open an external terminal, or run a JavaScript desktop runtime.

## Current status

Ferrum provides a Linux local-engine container inventory with search, sorting,
inspection, state-gated lifecycle actions, and bounded synchronization of
external container events. Remove requires confirmation. The app supports local
Unix-socket connections, including session-only Docker CLI contexts that select
local Unix sockets. Remote engines and Podman/rootless behavior are unsupported
or unvalidated; Windows and macOS remain deferred. Supported behavior and
validation evidence are tracked in the [platform matrix](platform-support.md).
The native UI surface and lifecycle action clicks have not been visually
validated.
## Documentation map

- [Build and run](developer-guide/building.md)
- [Container user guide](user-guide/containers.md)
- [Test the workspace](developer-guide/testing.md)
- [Architecture](architecture/overview.md)
- [Security and trust](security/threat-model.md)
- [Platform support](platform-support.md)

The project is developed in public issues and pull requests; planned work is not described as implemented functionality.
