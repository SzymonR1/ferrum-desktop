# ferrum-app

The native Ferrum desktop entry point. It owns presentation and application state; engine I/O runs in Tokio tasks and is kept outside the eframe UI callback.

Run from the repository root with `cargo run -p ferrum-app`. The initial container overview reads from local Linux Docker-compatible Unix sockets only. Windows/macOS, named pipes, remote TCP/TLS/SSH, and Podman compatibility are out of scope for this slice.
