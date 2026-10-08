# ADR 0007: Embedded container terminal

- **Status:** Experimental; accepted for Linux local-Unix sockets only
- **Date:** 2026-10-08

## Context

Container inspection sometimes needs an interactive shell, including bidirectional input, terminal sizing, and terminal rendering. Ferrum already uses a private Hyper/Hyperlocal HTTP transport over the selected Linux local Unix socket. Launching an external terminal would break the native, in-process interaction model and would require passing engine access elsewhere. Terminal output is untrusted and may contain control sequences or sensitive data.

## Decision

Provide an embedded terminal for an explicitly selected container through Docker Engine's exec API, only over the selected local Docker-compatible Unix socket on Linux. The exec command is passed as an exact argument array, with stdin/stdout/stderr attached and a TTY enabled. Create and start use the negotiated, API-versioned engine endpoint; start upgrades the HTTP/1.1 connection to a bidirectional byte stream. Terminal size updates use the corresponding versioned exec resize endpoint.

The native terminal view provides interactive input, TTY resize, ANSI and Unicode rendering, and cancellation on session/view/connection teardown. It does not invoke an external terminal. Closing the exec session closes only that connection; it does not stop or remove the container. Terminal bytes and daemon response bodies are never included in application logs or error text.

Terminal output is untrusted. OSC 52 clipboard writes and terminal title requests are ignored; clipboard access is available only through explicit user-initiated controls. The feature is experimental: do not infer visual or screen-reader validation from its implementation status.

Paste is requested through the explicit button. Delivery requires a fresh,
one-shot request while the terminal is focused; unrequested native paste
events are ignored, so no keyboard paste shortcut is exposed.

This slice is explicitly limited to Linux local Docker-compatible Unix sockets. It does not implement or claim Podman, rootless, remote, Windows, or macOS support. Issue #10 remains open for the broader engine compatibility matrix.

## Alternatives

### Launch an external terminal

Rejected: it introduces an external process and separate terminal workflow, contrary to the native embedded interaction goal.

### Render output as plain text

Rejected: plain text cannot provide interactive shell behavior or terminal applications' ANSI and cursor-based display semantics.

### Use a general remote-shell or host process

Rejected: it bypasses the Docker exec boundary and would expand host process, credential, and transport scope beyond this local-engine slice.

## Rationale

Docker Engine's exec create/start/resize operations provide a container-scoped interactive TTY. Hyper's HTTP/1.1 upgrade support provides the bidirectional upgraded stream over the existing local-socket transport, avoiding a second transport stack. A terminal emulator library provides screen-state interpretation without treating output as trusted application markup. Ignoring clipboard and title control requests limits output-driven interaction with the desktop.

## Consequences

- The feature is available only for Linux local Docker-compatible Unix-socket engines; API-versioning does not imply broader daemon compatibility.
- Exec sessions are separate from container lifecycle: closing a session does not stop or remove its container.
- Terminal content is sensitive and untrusted; it is not logged. Clipboard transfer is user initiated, and OSC 52/title requests are ignored.
- The terminal is an interactive view, not a guarantee of visual or assistive-technology validation.
- Live integration testing must use an explicitly owned disposable container and clean up only that test-owned container.
- Podman/rootless and remote-engine compatibility remain part of open issue #10; no support claim is made for them.

## References

- [Docker Engine API v1.52 exec create/start/resize schema](https://docs.docker.com/reference/api/engine/version/v1.52.yaml)
- [Moby exec and hijacked-stream client implementation](https://github.com/moby/moby/blob/master/client/container_exec.go)
- [Moby HTTP upgrade/hijack implementation](https://github.com/moby/moby/blob/master/client/hijack.go)
- [Hyper 1.12 HTTP upgrade API](https://docs.rs/hyper/1.12.0/hyper/upgrade/index.html)
- [vt100 0.16.2 documentation](https://docs.rs/vt100/0.16.2/vt100/)
