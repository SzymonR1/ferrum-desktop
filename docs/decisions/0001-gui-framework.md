# ADR 0001: Native GUI framework

- **Status:** Accepted for the M1 implementation; platform, accessibility, and performance validation remain open.
- **Date:** 2026-10-08

## Context

Ferrum needs a native Rust desktop UI for container inventories and operations. The first screen must work with a dense, engine-backed list while keeping I/O off the UI thread. The framework must have a viable path to Linux Wayland/X11, Windows, macOS, keyboard navigation, accessibility, HiDPI, and an embedded terminal. There are no repository benchmarks or existing GUI conventions to preserve.

## Decision

Use **egui/eframe 0.36.2** for the initial native application. Build the first interface with the `glow`, `default_fonts`, `accesskit`, `wayland`, and `x11` features, subject to dependency/license checks and platform CI. Use `egui::ScrollArea::show_rows` for fixed-height container rows rather than rendering every item. Keep engine work in async tasks and request UI repaints from completed tasks.

This is an implementation decision, not a claim that startup, idle resource use, accessibility, or cross-platform behavior has been proven. Platform support status is recorded separately and must be updated only with test evidence.

## Alternatives considered

| Framework | Advantages for Ferrum | Tradeoffs |
|---|---|---|
| Slint 1.18.1 | Declarative Rust integration; official Winit backend documents Linux Wayland/X11, Windows, and macOS; table/model and accessibility features are available. | Adds a UI DSL and build-tooling path. Framework licensing offers GPLv3, a desktop royalty-free option with disclosure conditions, or commercial licensing; the project would need to settle and audit applicable terms. A table prototype and embedded-terminal integration still need validation. |
| Iced 0.14.0 | Rust-native, message/update architecture, async tasks, and cross-platform intent. | Its published documentation describes the software as experimental. This review did not establish a first-party virtualized data-grid or complete screen-reader contract, so Ferrum would own more table/accessibility validation. |
| egui/eframe 0.36.2 | Rust-native, flexible custom operations UI; `ScrollArea::show_rows` renders only visible fixed-height rows; eframe supports native desktop windows and repaint requests from other threads; MIT OR Apache-2.0 licensing. | Ferrum owns more interaction and table semantics. AccessKit integration is not proof of complete native assistive-technology support. UI state and view code require deliberate organization. |

## Rationale

- A virtualized list is available in egui without adding a separate data-grid dependency. Sorting, column resizing, selection, and richer table semantics remain application work.
- eframe's native app contract supports a Rust application entry point and thread-safe repaint requests, allowing async engine results to wake the UI without blocking the event loop.
- The currently published eframe crate declares Rust 1.95 as its minimum. Ferrum therefore sets Rust 1.95 as the initial MSRV and must validate the full locked dependency graph against that toolchain.
- The selected crate license is compatible with the repository's Apache-2.0 license; transitive license compliance remains enforced in CI.
- No comparable framework startup, idle CPU, memory, accessibility, or rendering benchmarks were found or run. No performance superiority is claimed.

## Consequences

- Ferrum uses an immediate-mode Rust UI and implements its own compact container-table behavior.
- Accessibility roles, focus order, keyboard operation, text scaling, and Linux Wayland/X11 behavior require direct validation. See [issue #20](https://github.com/SzymonR1/ferrum-desktop/issues/20).
- Windows, macOS, additional Linux compositors, and packaging are not supported claims until compilation and runtime evidence is recorded in [issue #21](https://github.com/SzymonR1/ferrum-desktop/issues/21).
- Startup, idle memory/CPU, large-list, and stream behavior require reproducible measurements before a performance claim is made.
- Reconsider this choice only if the M1 prototype exposes a concrete accessibility, platform, or maintenance limitation.

## References

- [eframe 0.36.2 API and platform documentation](https://docs.rs/eframe/0.36.2/eframe/)
- [eframe native startup](https://docs.rs/eframe/0.36.2/eframe/fn.run_native.html)
- [egui 0.36.2 virtualized rows](https://docs.rs/egui/0.36.2/egui/containers/scroll_area/struct.ScrollArea.html#method.show_rows)
- [Slint Winit backend](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/)
- [Slint licensing](https://github.com/slint-ui/slint/blob/master/LICENSE.md)
- [Iced current documentation](https://docs.rs/iced/0.14.0/iced/)
