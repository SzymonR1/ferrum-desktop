# ADR 0006: Bounded container logs

- **Status:** Accepted for Linux local-Unix sockets only
- **Date:** 2026-10-08

Export target handling in this ADR was superseded by
[ADR 0012](0012-atomic-log-export.md); the bounded log-stream and UI
decisions remain active.

## Context

Container inspection needs access to both historical output and an optional live stream without introducing terminal behavior or allowing an unbounded response, queue, or UI buffer. Docker's logs endpoint streams response bodies and has different wire formats for TTY and non-TTY containers. A reconnect must not duplicate the tail already displayed.

## Decision

Add a Logs view to the existing container inspection dialog. It is available only for the selected local Unix-socket Docker-compatible engine and uses the negotiated API version. The query supports stdout, stderr, timestamps, follow, and historical tail selection. The engine endpoint supports only json-file and journald logging drivers; unsupported/error responses become safe status errors without exposing raw bodies. TTY mode comes from allowlisted `Config.Tty`.

TTY output is raw combined data; stdout/stderr cannot be separated, and rows
are labeled `combined`. Non-TTY data uses Docker's 8-byte multiplex frames to
preserve stream type; rows are labeled `stdout` or `stderr`. Logs are displayed
as plain text, never interpreted as a terminal. Search is case-insensitive over
currently retained lines; stdout/stderr filters, a timestamp toggle, tail
choices of 100/500/1000/5000, and Follow are provided. Follow defaults on only
when the inspected container is Running.

A frame is capped at 1 MiB and a line at 64 KiB. The app retains at most 8 MiB or 10,000 lines and discards oldest lines with a visible count. Delivery uses a dedicated bounded 16-message channel; the UI drains and batches messages. Logs are not persisted or logged automatically. Copy and Export operate only on rows currently visible after filtering and still retained in the buffer, preserving source labels in `<source>: <text>` format. Export writes plain text to the user-selected path; the native save dialog starts at `container-logs.txt` and offers a text-file filter, but the selected extension is not enforced. New regular files are created with owner-only Unix permissions (`0600`); writes to existing regular files preserve ordinary rwx permission bits, though the OS may clear setuid/setgid bits. Final-component symlinks and non-regular targets are rejected before truncation…

Follow retries request/stream failures at most three times during one Logs
view session, waiting 250, 500, and 1000 ms. Replayed historical lines do not
renew the budget, so a persistent failing tail cannot restart retries
indefinitely. A clean EOF (for example, when a container stops) ends the stream
without retrying. After exhaustion, manual Refresh starts a fresh bounded
session. Reconnect reloads the recent tail and clears buffered rows to avoid
duplicate replay. Closing the Logs view or inspection, changing engine context,
or application teardown cancels the stream.

This decision applies only to local Unix-socket engines on Linux. It makes no support claim for remote engines, Podman/rootless, Windows, or macOS.

## Alternatives considered

### Automatic persistence

Rejected: automatic log persistence can retain large or sensitive output beyond the bounded in-memory session. Explicit user-selected Export is a deliberate, bounded exception: it writes only currently visible retained rows to the destination chosen by the user.

### Decode every stream as multiplexed

Rejected: Docker emits raw combined bytes for TTY containers, where stream separation is unavailable. TTY is determined from the allowlisted inspected configuration; non-TTY framing preserves stdout/stderr.

### Render as a terminal

Rejected: terminal escape sequences and control data are untrusted output. The view remains plain text.

### Retry indefinitely or append a replayed tail

Rejected: indefinite retries obscure loss of service, and appending a fresh historical tail can duplicate displayed rows. Bounded retries plus a tail reload and buffer reset make recovery explicit.

## Consequences

- Historical and followed output is available in the inspection dialog, with bounded memory and delivery.
- TTY output is necessarily combined; stdout/stderr filters cannot split it.
- Reconnect and manual refresh re-read recent history; logs are not stored on disk or written to logs automatically. Explicit user-initiated export is limited to currently visible retained rows and writes plain text to a user-selected path.
- Local endpoint restrictions remain unchanged; no remote or cross-platform support is implied.
