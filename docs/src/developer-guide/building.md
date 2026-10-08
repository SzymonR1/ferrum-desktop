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

The Linux adapter uses `DOCKER_HOST` when it selects a local Unix socket. Otherwise it tries `$XDG_RUNTIME_DIR/docker.sock` and `/var/run/docker.sock`. Windows/macOS, named pipes, remote TCP/HTTP/TLS/SSH, and Docker context selection are deferred.

If no engine is reachable, Ferrum opens and displays a connection diagnostic. An empty inventory is shown only after a successful engine response.
