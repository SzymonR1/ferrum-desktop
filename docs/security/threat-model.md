# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. The implementation provides a read-only local container inventory and on-demand container inspection.

## Trust boundaries

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local Linux Unix socket. Access to a Docker socket is effectively host-level control; a read-only GUI screen does not make the endpoint unprivileged. Windows/macOS engine APIs are not implemented here. Engine responses, container names, image references, state strings, status text, and mount paths are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Keep remote TCP/HTTP/TLS/SSH transports disabled in the first slice. Reject explicitly configured remote schemes rather than downgrade to plaintext.
- Do not log or display raw `DOCKER_HOST` values, which may contain sensitive endpoint data.
- Keep Docker CLI context selection session-only; do not log or display raw context endpoints.
- Render engine-provided strings and inspection details as plain text; do not interpret metadata as markup or commands. The inspection view omits environment variables, labels, and command arguments.
- Use bounded API timeouts, keep engine I/O off the UI thread, and distinguish connection errors from an empty inventory.
- Do not store registry credentials, TLS keys, SSH keys, or other secrets.
- Persist only the schema version and startup auto-connect boolean; reject unknown fields, bound input size, and keep config files/private temporary files owner-only.
- Do not include configuration paths or contents in settings errors or logs. Corrupt settings fall back to defaults and are not overwritten until explicit user save.
- Do not start, stop, remove, prune, or otherwise mutate containers in this initial slice.

## Deferred attack surfaces

- Remote TCP/HTTP/TLS/SSH transport trust, credentials, and identity validation.
- Podman API compatibility and rootless endpoint discovery.
- Container lifecycle confirmations and synchronization.
- Engine log output and terminal escape sequences.
- Compose files, external process invocation, archive extraction, and file paths.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary.

## Validation plan

The initial live-engine test is read-only. Later security tests must cover malformed responses, permission failures, credential redaction, TLS/SSH trust failures, path traversal, terminal escape handling, and destructive-operation confirmations using disposable resources only. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).
