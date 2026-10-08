# Building Ferrum

## Prerequisites

- Stable Rust with the workspace MSRV, Rust 1.95, installed through `rustup`.
- A C toolchain and `pkg-config` for Linux windowing dependencies.
- The development packages for X11, Wayland, xkbcommon, and OpenGL used by eframe's `glow` backend.
- A local Docker-compatible engine is optional for compilation and required only to verify live container discovery.

Ferrum is Linux-only in this implementation. It does not install an engine, change socket permissions, or request elevated privileges. The current process must already have permission to access a local Unix socket.

## Build and launch

From the repository root:

```sh
cargo build --workspace
cargo run -p ferrum-app
```

`packaging/linux/com.ferrum.desktop` is launcher metadata only; it assumes `ferrum` is on `PATH` and is not installed by the build commands.

The Linux adapter preserves `DOCKER_HOST` local-Unix behavior and automatic
socket discovery. Docker CLI contexts are also listed for session-only selection;
`DOCKER_CONTEXT` takes precedence over `DOCKER_HOST`, followed by a non-default
current local context and then automatic local socket discovery. Only context
endpoints using local Unix sockets are supported. Remote TCP/HTTP/TLS/SSH
contexts are rejected. Windows named pipes and macOS support remain deferred to
[issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21) and M8.
Context-selection work is a slice of [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10),
which remains open for Podman/rootless validation and remote TLS/SSH policy.

Context listing and switching require the Docker CLI. Ferrum's direct local
socket connection and automatic discovery do not require the CLI.

If no engine is reachable, Ferrum opens and displays a connection diagnostic. An empty inventory is shown only after a successful engine response.
