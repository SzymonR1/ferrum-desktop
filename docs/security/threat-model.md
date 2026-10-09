# Ferrum threat model

**Status:** Initial scope documented; this is not a security audit. Ferrum provides inventory, inspection, image tagging and removal, scoped container lifecycle actions, logs, and an experimental embedded terminal over the Linux local-engine path.


## Trust boundaries

Ferrum runs as the logged-in desktop user and communicates with a Docker-compatible engine through a local Linux Unix socket. Access to a Docker socket is effectively host-level control; UI action gating and confirmation do not make the endpoint unprivileged. Windows/macOS engine APIs are not implemented here. Engine responses, container names, image references, state strings, status text, and mount paths are untrusted input.

## Current mitigations

- Do not elevate privileges, install services, change socket permissions, or start an engine automatically.
- Keep direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH and remote Docker context transports disabled. Return typed unsupported errors without fallback to local discovery, and do not read remote TLS/SSH credentials.
- Do not log or display raw `DOCKER_HOST` values, which may contain sensitive endpoint data.
- Keep Docker CLI context selection session-only; do not log or display raw context endpoints.
- Render engine-provided strings and inspection details as plain text; do not interpret metadata as markup or commands. Inspection details omit environment variables, labels, and command arguments.
- Use a private bounded request path for version, inventory, inspect, and lifecycle operations. Version/inventory/inspection and event-subscription setup use an 8-second timeout; lifecycle requests use a bounded 30-second timeout for graceful Stop/Restart. Timed-out actions may still be changing state, so errors advise refresh before retry. Remove requires confirmation and sends neither force nor volume-removal parameters. Never log or expose raw response bodies.
- Anonymous image pulls are restricted to the selected local Unix-socket Docker
  engine and send no `X-Registry-Auth` header or stored credentials. Progress
  responses are bounded to 1 MiB per HTTP frame and 64 KiB per JSON record;
  displayed status is capped at 256 bytes; control characters and selected
  invisible formatting characters (bidi marks, zero-width marks/joiners, and
  soft hyphens) are replaced before rendering. Only status and optional byte
  counts are exposed. Daemon error details are discarded, never logged or displayed.
  At Hide progress, Ferrum stops forwarding UI progress but continues draining
  the response until a terminal event, keeping the image-mutation gate closed.
  Page/context invalidation hides stale progress without aborting the reader.
  Closing Ferrum still does not guarantee that daemon-side pull work stops.
- Send image-tag requests only to the selected local Docker Unix-socket engine
  using the negotiated API version and the bounded 8-second request timeout.
  Validate the image ID and bound the combined `repository:tag` target to 512
  bytes; validate Docker tag syntax while leaving full repository grammar to
  the engine. Do not send `force`, `X-Registry-Auth`, stored credentials, or
  read/display response bodies. Typed failures expose only a safe endpoint label
  and HTTP status where applicable. Tagging changes a local repository/tag
  association only; it does not contact a registry. Require explicit
  confirmation before every request because Engine v1.56 may reassign an
  existing `repository:tag`; the image data remains. Page/context invalidation
  ignores stale UI results but does not abort or claim to cancel an in-flight
  request.
- Send image-removal requests only to the selected local Docker Unix-socket
  engine using the negotiated API version and a bounded 30-second timeout.
  Validate a nonempty image ID of at most 4096 bytes. Require an explicit
  confirmation that snapshots the selected ID and known repository tags and
  warns that removing the final reference may delete image data. The request
  omits `force` (so the engine default applies) and sets `noprune=true` to
  preserve untagged parent images; the engine may refuse images used by
  containers or with other tags. Do not send registry authentication or
  platform parameters, and never read or expose response bodies. Typed errors
  expose only safe endpoint/status information; timeout errors must explain
  that state may have changed. Only one image mutation may be active across
  pull/tag/remove. Page/context invalidation ignores stale UI results but does
  not abort or claim to undo a possibly submitted removal; refresh inventory
  after success only while Images is current.

- Emit a structured local tracing event when a Ferrum container lifecycle
  request returns. Record the action, `success` or `request_error` outcome, and
  a container ID only when it is exactly 64 hexadecimal characters. A request
  error does not prove that the engine state remained unchanged. Do not log
  container names, endpoints, raw errors, or daemon response bodies. These
  events use the configured process logger; they are not a persistent or
  tamper-proof audit store.
- Keep historical/live container logs restricted to the selected local Unix-socket engine. Use the negotiated API version and streaming response body; accept only json-file/journald drivers. TTY is read from allowlisted `Config.Tty`: TTY output is raw and combined, while non-TTY output is decoded from Docker's 8-byte multiplex frames. Render logs as plain text, never as a terminal. Bound frames to 1 MiB, lines to 64 KiB, retention to 8 MiB or 10,000 lines, and delivery to a dedicated 16-message channel; discard oldest retained lines with a visible count. Pause suspends UI draining, applying bounded backpressure to producer reads without unbounded memory or dropped lines; Resume continues delivery. Copy and Export include only currently visible, retained rows and escape terminal control characters as `\xNN`.
- Keep the experimental embedded terminal restricted to the selected local Docker-compatible Unix socket on Linux and the negotiated API version. Terminal output is untrusted; ignore OSC 52 clipboard and title requests, expose clipboard actions only to explicit user controls, and never put terminal bytes or daemon bodies in logs or errors. Closing the exec session closes its connection only; it does not stop or remove the container.
- Persist only the schema version and startup auto-connect boolean; reject unknown fields, bound input size, and keep config files/private temporary files owner-only.
- Do not include configuration paths or contents in settings errors or logs. Corrupt settings fall back to defaults and are not overwritten until explicit user save.
- Open settings files with Unix `O_NOFOLLOW` and `O_NONBLOCK` flags, verify the
  opened descriptor is a regular file before changing permissions, and reject
  final-component symlinks or non-regular files. Parent path components still
  follow the configured XDG/HOME path.
- When saving, open the final settings directory without following a symlink
  and apply `0700` through that descriptor. Create the private temporary file
  with `openat`, atomically replace the settings file with `renameat`, and
  clean up failed temporary files with `unlinkat`, all relative to the same
  opened directory descriptor. Replacing the final directory path after open
  cannot redirect those operations. Parent components remain resolved through
  the configured XDG/HOME path and are not descriptor-contained.

## Deferred attack surfaces

- Remote TCP/HTTP/TLS/SSH transport trust, credentials, and identity validation.
- Podman API compatibility and rootless endpoint discovery; issue #10 remains open for these and the broader engine matrix.
- Terminal streams beyond this bounded local exec session, Compose files, external process invocation, archive extraction, and file paths.
- Credential stores, registry authentication, managed engines, and update delivery.

These surfaces must receive explicit design and tests before their implementation. Engine access should be treated as privileged throughout; API version negotiation is not a security boundary. UI state gating and confirmation reduce accidental destructive operations but cannot constrain a compromised or malicious engine endpoint.

## Validation plan

Read-only live-engine tests cover version, inventory, and inspection without
image mutation. Fake-Unix-socket image protocol tests and headless Images UI
tests cover tag and removal request/confirmation behavior without contacting a
real engine or mutating images. There is no live image deletion or other live
image-mutation test. Separate opt-in ignored lifecycle, logs, and terminal tests
mutate engine state using one uniquely named disposable container and only its
validated full ID as cleanup authority. Logs and terminal tests require locally
installed images and never pull, tag, or delete images. See [issue #19](https://github.com/SzymonR1/ferrum-desktop/issues/19).

