# ADR 0006: Bounded container logs

- **Status:** Accepted for Linux local-Unix sockets only
- **Date:** 2026-10-08

## Context

Container inspection needs access to both historical output and an optional live stream without introducing terminal behavior or allowing an unbounded response, queue, or UI buffer. Docker's logs endpoint streams response bodies and has different wire formats for TTY and non-TTY containers. A reconnect must not duplicate the tail already displayed.

## Decision

Add a Logs view to the existing container inspection dialog. It is available only for the selected local Unix-socket Docker-compatible engine and uses the negotiated API version. The query supports stdout, stderr, timestamps, follow, and historical tail selection. The engine endpoint supports only json-file and journald logging drivers; unsupported/error responses become safe status errors without exposing raw bodies. TTY mode comes from allowlisted `Config.Tty`.

TTY output is raw combined data; stdout/stderr cannot be separated. Non-TTY data uses Docker's 8-byte multiplex frames to preserve the stream type. Logs are displayed as plain text, never interpreted as a terminal. Search is case-insensitive over currently retained lines; stdout/stderr filters, a timestamp toggle, tail choices of 100/500/1000/5000, and Follow are provided. Follow defaults on only when the inspected container is Running.

A frame is capped at 1 MiB and a line at 64 KiB. The app retains at most 8 MiB or 10,000 lines and discards oldest lines with a visible count. Delivery uses a dedicated bounded 16-message channel; the UI drains and batches messages. Output is neither persisted nor logged.

Follow retries request/stream failures at most three times per outage, waiting
250, 500, and 1000 ms. A clean EOF (for example, when a container stops) ends
the stream without retrying. The retry budget resets only after a reconnect
delivers at least one log line, not merely when an HTTP stream opens. Reconnect
reloads the recent tail and clears buffered rows to avoid duplicate replay.
Manual refresh reloads history with the current Follow setting. Closing the
Logs view or inspection, changing engine context, or application teardown
cancels the stream.

This decision applies only to local Unix-socket engines on Linux. It makes no support claim for remote engines, Podman/rootless, Windows, or macOS.

## Alternatives considered

### Unbounded collection or disk persistence

Rejected: output can be large or sensitive. Explicit in-memory limits and no persistence bound exposure and resource use.

### Decode every stream as multiplexed

Rejected: Docker emits raw combined bytes for TTY containers, where stream separation is unavailable. TTY is determined from the allowlisted inspected configuration; non-TTY framing preserves stdout/stderr.

### Render as a terminal

Rejected: terminal escape sequences and control data are untrusted output. The view remains plain text.

### Retry indefinitely or append a replayed tail

Rejected: indefinite retries obscure loss of service, and appending a fresh historical tail can duplicate displayed rows. Bounded retries plus a tail reload and buffer reset make recovery explicit.

## Consequences

- Historical and followed output is available in the inspection dialog, with bounded memory and delivery.
- TTY output is necessarily combined; stdout/stderr filters cannot split it.
- Reconnect and manual refresh re-read recent history; no output is stored on disk or written to logs.
- Local endpoint restrictions remain unchanged; no remote or cross-platform support is implied.
