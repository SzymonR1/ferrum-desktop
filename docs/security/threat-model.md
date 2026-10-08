# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. The current implementation is a read-only local container inventory.

## Trust boundaries

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local socket or named pipe. Access to a Docker socket is effectively host-level control; a read-only GUI screen does not make the endpoint unprivileged. Engine responses, container names, image references, state strings, and status text are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Keep remote TCP/HTTP/TLS/SSH transports disabled in the first slice. Reject explicitly configured remote schemes rather than downgrade to plaintext.
- Do not log or display raw `DOCKER_HOST` values, which may contain sensitive endpoint data.
- Render engine-provided strings as plain text; do not interpret metadata as markup or commands.
- Use bounded API timeouts, keep engine I/O off the UI thread, and distinguish connection errors from an empty inventory.
- Do not store registry credentials, TLS keys, SSH keys, or other secrets.
- Do not start, stop, remove, prune, or otherwise mutate containers in this initial slice.

## Deferred attack surfaces

- Docker contexts and remote TLS/SSH trust, credentials, and identity validation.
- Podman API compatibility and rootless endpoint discovery.
- Container lifecycle confirmations and synchronization.
- Engine log output and terminal escape sequences.
- Compose files, external process invocation, archive extraction, and file paths.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary.

## Validation plan

The initial live-engine test is read-only. Later security tests must cover malformed responses, permission failures, credential redaction, TLS/SSH trust failures, path traversal, terminal escape handling, and destructive-operation confirmations using disposable resources only. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).
