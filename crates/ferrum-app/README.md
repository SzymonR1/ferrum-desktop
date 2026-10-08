# ferrum-app

The native Ferrum desktop entry point. It owns presentation and application state; engine I/O runs in Tokio tasks and is kept outside the eframe UI callback.

Run from the workspace root with `cargo run -p ferrum-app`. The initial container overview reads from a local Docker-compatible Unix socket or named pipe. Remote TCP/TLS/SSH endpoints and Podman compatibility are not enabled or claimed in this first slice.
