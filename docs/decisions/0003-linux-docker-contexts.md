# ADR 0003: Linux Docker context selection

- **Status:** Accepted for the Linux local-Unix context-selection slice; issue #10 remains open. Its Bollard response-transport, version, and list details are superseded by [ADR 0004: Bounded engine HTTP](0004-bounded-engine-http.md).
- **Date:** 2026-10-08

## Context

Ferrum already connects to Linux local Unix sockets through `DOCKER_HOST` and automatic XDG/system socket discovery. Docker CLI contexts provide named endpoints, but may also target network transports and include endpoint metadata that must not leak into Ferrum's UI or logs. This work is a scoped part of [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10), not completion of its remaining Podman/rootless and remote-policy criteria.

## Decision

- Discover contexts by invoking `docker context ls --format '{{json .}}'` with argument arrays, never a shell. Bound command duration and output, discard stderr, and use static user-facing errors.
- Parse newline-delimited JSON records into Docker-CLI-valid names, a current flag, and a transport classification. Bound displayed names to 64 ASCII bytes, keep Unix socket paths private, and omit raw endpoints from `Debug`, logs, and UI.
- Support only local Unix-socket endpoints. Disable TCP/HTTP/TLS/SSH contexts and reject them without downgrade. Do not read, store, or display remote TLS material or credentials.
- Preserve `EngineClient::connect_default()` behavior for local `DOCKER_HOST` and XDG/system sockets. Startup selection follows Docker CLI precedence: non-empty `DOCKER_CONTEXT`, then existing `DOCKER_HOST`, then a non-default current CLI context, then default local socket discovery. If Docker CLI is missing and there is no explicit `DOCKER_CONTEXT`, ordinary local socket discovery remains available; an explicit missing CLI context or unknown name returns a safe actionable error.
- Connect selected local contexts over Bollard's Unix transport using existing API negotiation and the eight-second request timeout. Context choice is session-only; Ferrum never modifies Docker CLI context/configuration.
- When a session switch fails, retain the prior client, engine information, and inventory, and report the switch error separately.

## Alternatives considered

### Shell out with `docker context inspect`

A shell would add quoting/injection hazards, and inspecting a context individually unnecessarily expands the output and processing path. The context list JSON format supplies the required name, current flag, endpoint, and error fields in one bounded invocation.

### Pass the context to Docker CLI for engine operations

Ferrum's engine operations use Bollard directly. Delegating connection behavior to Docker CLI would create a second API execution path and complicate typed inventory/error handling. Instead, extract only the local socket endpoint privately and use Bollard's existing transport.

### Enable remote transports or fall back to local discovery

Remote TCP, TLS, and SSH require an explicit credential, certificate, identity, and downgrade policy. They remain unsupported. Falling back when an explicitly selected context is missing or remote could silently connect to a different engine, so such requests fail visibly and safely.

## Consequences and validation limits

- Deterministic tests cover record parsing, safe context classification, name bounds, unknown names, and startup precedence without mutating process environment.
- An ignored live test reads version and inventory from the current local Docker CLI context. It does not create, stop, or remove engine resources.
- Docker CLI context availability is not Podman validation. Podman/rootless behavior and remote TLS/SSH policy remain open under issue #10; remote transports are not enabled by this ADR.
- Windows named pipes and macOS are outside this Linux-only slice and remain deferred to [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21) and M8.

## References

- [Docker CLI context documentation](https://docs.docker.com/engine/manage-resources/contexts/)
- [Docker CLI environment-variable precedence](https://docs.docker.com/reference/cli/docker/)
- [ADR 0002: Docker-compatible engine client](0002-engine-client.md)
