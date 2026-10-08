# ADR 0002: Docker-compatible engine client

- **Status:** Accepted for the M1 read-only local container-list slice; broader endpoint and Podman compatibility remain open.
- **Date:** 2026-10-08

## Context

Ferrum must communicate with existing container engines using their API, not launch a terminal or implement an OCI runtime. A first useful slice needs an asynchronous, typed container list and must not block the native UI. Docker socket access is privileged. Podman documents a Docker-compatible API layer, but this environment has no Podman executable or daemon for validation.

## Decision

Use **Bollard 0.21.1** for Docker Engine API transport, version negotiation, response decoding, and container listing. Disable default features and initially enable only `pipe` for the Linux Unix-socket transport. Ferrum does not expose or claim Windows named-pipe support in this slice. Remote HTTP is not enabled. The full dependency graph, including Bollard's currently unspecified crate MSRV, must be checked against Ferrum's declared Rust 1.95 MSRV before release.

Expose a small Ferrum-owned `EngineClient` that returns Ferrum-owned `ContainerSummary` and `EngineInfo` values. The UI does not depend on Bollard response models. Do not add a generic `ContainerEngine` trait yet: there is one implemented adapter, so a second interchangeable adapter is not currently present to justify that seam.

For an explicit `DOCKER_HOST`, use only a local Unix-socket scheme on Linux. Reject remote TCP/HTTP/SSH schemes rather than silently sending privileged engine traffic without TLS. When `DOCKER_HOST` is unset, try the user runtime Docker socket (`$XDG_RUNTIME_DIR/docker.sock`) and then `/var/run/docker.sock`; report a connection error rather than treating a missing socket as an empty inventory. Docker contexts and remote TLS/SSH are deferred. Windows/macOS engine connections are outside the current scope.

Negotiate the Docker API version before listing containers, set a bounded request timeout, and perform all engine I/O on a Tokio task outside the GUI event loop. Preserve missing engine fields as unavailable and unknown lifecycle states as unknown. Never elevate privileges, change socket permissions, expose raw endpoint credentials in logs, or auto-start an engine.

## Alternatives considered

### Direct Docker Engine HTTP client

However, Ferrum would also own API version negotiation, Unix socket transport, response/error compatibility, and safe TLS behavior for possible later remote transports. This is more protocol and transport code to maintain for the initial Linux slice and is not the smaller safe implementation.

### Bollard with HTTP/TLS/SSH transports enabled

Bollard exposes additional transports, but they are not needed for this local-only slice. Enabling a plaintext `tcp://` path without complete, explicit TLS policy would be unsafe. Remote endpoint configuration is deferred to M2 and must validate TLS/SSH identities.

## Compatibility and security limits

- Docker documents API version negotiation as selecting the highest mutually supported API version while describing compatibility as best effort. Negotiation does not prove endpoint-level feature parity.
- Podman's official system-service documentation describes a Docker v1.40 compatibility layer and separate Libpod API. This establishes a plausible compatibility target, not tested parity. No Podman support claim is made until the API contract is exercised against named Podman versions and rootless configurations; see [issue #11](https://github.com/SzymonR1/ferrum-desktop/issues/11).
- Docker and Podman engine sockets grant extensive host/user authority. Preserve operating-system socket permissions and show actionable permission errors without attempting escalation.
- Remote TCP endpoints, TLS certificates, SSH host identity, Docker contexts, and persistent credentials require explicit M2 design and security review.

## Consequences

- The first list call uses the read-only Docker-compatible container-list API and includes stopped containers.
- UI state distinguishes connecting, connected with zero containers, and connection/API failure.
- Linux is the only implemented engine target. Windows/macOS runtime behavior and packaging are deferred to the cross-platform milestone.
- A Podman endpoint discovered in a later milestone does not become supported until contract-tested; Bollard's Podman helper alone is not validation.

## References

- [Bollard 0.21.1 Docker client](https://docs.rs/bollard/0.21.1/bollard/struct.Docker.html)
- [Bollard 0.21.1 container summary model](https://docs.rs/bollard/0.21.1/bollard/models/struct.ContainerSummary.html)
- [Docker Engine API and version negotiation](https://docs.docker.com/reference/api/engine/)
- [Podman system service compatibility, sockets, and security](https://docs.podman.io/en/latest/markdown/podman-system-service.1.html)
