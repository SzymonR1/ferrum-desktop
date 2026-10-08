# Ferrum

Ferrum is a native Rust desktop application for managing existing Docker-compatible container engines. It does not bundle an engine, install privileged services, open an external terminal, or run a JavaScript desktop runtime.

## Current status

The first implementation focuses on a real, read-only container inventory. Supported behavior and validation evidence are tracked in the [platform matrix](platform-support.md). Do not infer Podman, remote-engine, lifecycle, logs, or embedded-terminal support from the product plan.

## Documentation map

- [Build and run](developer-guide/building.md)
- [Test the workspace](developer-guide/testing.md)
- [Architecture](architecture/overview.md)
- [Security and trust](security/threat-model.md)
- [Platform support](platform-support.md)

The project is developed in public issues and pull requests; planned work is not described as implemented functionality.
