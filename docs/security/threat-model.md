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
- Keep historical/live container logs restricted to the selected local Unix-socket engine. Use the negotiated API version and streaming response body; accept only json-file/journald drivers. TTY is read from allowlisted `Config.Tty`: TTY output is raw and combined, while non-TTY output is decoded from Docker's 8-byte multiplex frames. Render logs as plain text, never as a terminal. Bound frames to 1 MiB, lines to 64 KiB, retention to 8 MiB or 10,000 lines, and delivery to a dedicated 16-message channel; discard oldest retained lines with a visible count. Do not persist or log output. Cancel streams on close, context change, or app teardown; bound reconnect attempts and reload recent history after reconnect.
- Persist only the schema version and startup auto-connect boolean; reject unknown fields, bound input size, and keep config files/private temporary files owner-only.
- Do not include configuration paths or contents in settings errors or logs. Corrupt settings fall back to defaults and are not overwritten until explicit user save.

## Deferred attack surfaces

- Remote TCP/HTTP/TLS/SSH transport trust, credentials, and identity validation.
- Podman API compatibility and rootless endpoint discovery.
- General terminal streams, Compose files, external process invocation, archive extraction, and file paths.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary. UI state gating and confirmation reduce accidental destructive operations but cannot constrain a compromised or malicious engine endpoint.

## Validation plan

Read-only live-engine tests cover version, inventory, and inspection without mutation. Separate opt-in ignored lifecycle and logs tests mutate engine state using one uniquely named disposable container and only its validated full ID as cleanup authority. Logs test requires a locally installed image with `/bin/sh` and `/bin/sleep`, never pulls, and uses no mounts or ports. On failure, best-effort cleanup may force-remove only that exact test-owned ID; it never removes volumes. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).


