# Building Ferrum

## Prerequisites

- Stable Rust with the workspace MSRV, Rust 1.95, installed through `rustup`.
- A C toolchain and `pkg-config` for native windowing dependencies.
- On Linux, the development packages for X11, Wayland, xkbcommon, and OpenGL used by eframe's `glow` backend.
- A Docker-compatible engine is optional for compilation and required only to verify live container discovery.

Ferrum does not install an engine, change socket permissions, or request elevated privileges. The local process must already have permission to access the chosen engine socket or named pipe.

## Build and launch

From the repository root:

```sh
cargo build --workspace
cargo run -p ferrum-app
```

The initial connection uses `DOCKER_HOST` when it selects a local Unix socket or Windows named pipe. On Unix, when `DOCKER_HOST` is unset, the initial implementation tries `$XDG_RUNTIME_DIR/docker.sock` and `/var/run/docker.sock`. Remote TCP, HTTP, TLS, SSH, and Docker context selection are not enabled in this slice.

If no engine is reachable, Ferrum opens and displays a connection diagnostic. An empty inventory is shown only after a successful engine response.
