# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. Ferrum provides inventory, inspection, and scoped container lifecycle actions over the Linux local-engine path.

## Trust boundaries

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local Linux Unix socket. Access to a Docker socket is effectively host-level control; UI action gating and confirmation do not make the endpoint unprivileged. Windows/macOS engine APIs are not implemented here. Engine responses, container names, image references, state strings, status text, and mount paths are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Keep remote TCP/HTTP/TLS/SSH transports disabled in the first slice. Reject explicitly configured remote schemes rather than downgrade to plaintext.
- Do not log or display raw `DOCKER_HOST` values, which may contain sensitive endpoint data.
- Keep Docker CLI context selection session-only; do not log or display raw context endpoints.
- Render engine-provided strings and inspection details as plain text; do not interpret metadata as markup or commands. Inspection details omit environment variables, labels, and command arguments.
- Use a private bounded request path for version, inventory, inspect, and lifecycle operations. Version/inventory/inspection and event-subscription setup use an 8-second timeout; lifecycle requests use a bounded 30-second timeout for graceful Stop/Restart. Timed-out actions may still be changing state, so errors advise refresh before retry. Remove requires confirmation and sends neither force nor volume-removal parameters. Never log or expose raw response bodies.
- Process only Docker container-only JSONL events, cap each line at 64 KiB and each transport data frame at 1 MiB, and deliver through the existing bounded channel. Refresh inventory and any open inspection after initial subscription/reconnect and after a successful manual refresh so missed events cannot leave state-gated controls stale. Cancel event processing on context change or app teardown; allow at most three reconnect retries per outage (250 ms, 500 ms, 1 s), reset the budget after successful subscription, and surface synchronization loss after exhaustion. On exhaustion, retain manual refresh.
- Do not store registry credentials, TLS keys, SSH keys, or other secrets.
- Persist only the schema version and startup auto-connect boolean; reject unknown fields, bound input size, and keep config files/private temporary files owner-only.
- Do not include configuration paths or contents in settings errors or logs. Corrupt settings fall back to defaults and are not overwritten until explicit user save.

## Deferred attack surfaces

- Remote TCP/HTTP/TLS/SSH transport trust, credentials, and identity validation.
- Podman API compatibility and rootless endpoint discovery.
- Engine log output and terminal escape sequences.
- Compose files, external process invocation, archive extraction, and file paths.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary. UI state gating and confirmation reduce accidental destructive operations but cannot constrain a compromised or malicious engine endpoint.

## Validation plan

Read-only live-engine tests cover version, inventory, and inspection without mutation. The separate opt-in ignored lifecycle test mutates engine state using one uniquely named disposable container and only its validated full ID as cleanup authority. It never pulls an image or removes volumes; on failure, best-effort cleanup may force-remove only that exact test-owned ID. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).


