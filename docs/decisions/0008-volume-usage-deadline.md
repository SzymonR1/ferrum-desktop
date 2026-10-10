# ADR 0008: Volume usage request deadline

- **Status:** Accepted for the Linux local Docker volume-usage slice
- **Date:** 2026-10-09

## Context

Ferrum retrieves selected-volume usage with the volume-filtered Docker Engine
API request `GET /v{api}/system/df?verbose=true&type=volume` over the selected
local Docker Unix socket on Linux. The daemon can take longer to calculate this
disk-usage response than the common eight-second JSON request deadline. The
usage response still needs a bounded body size, and waiting for it must not
block the UI thread.

## Decision

Give only the volume-filtered `system/df` request a 30-second total deadline.
Keep its response-body cap at 16 MiB. Run the request asynchronously, do not
retry it, and keep the eight-second deadline for every other JSON request.

This is a read-only request in the existing Linux local Docker Unix-socket
scope. It does not add remote, Podman, rootless, Windows, or macOS support.
Issues [#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) and
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remain open; this
decision does not claim full issue acceptance or a release.

## Alternatives

### Keep the eight-second deadline for volume usage

Rejected because a slow daemon could time out while calculating system disk
usage, even when the selected volume is available.

### Increase the deadline for every JSON request

Rejected because it would make unrelated version, inventory, and inspection
requests wait longer. The larger bound applies only to the volume-usage call.

### Retry a timed-out usage request

Rejected because another request could add work to a slow daemon without
making the first calculation finish sooner.

## Rationale

The per-request 30-second bound gives the daemon more time for this calculation
without changing latency limits for other JSON operations. The 16 MiB cap
limits response buffering, asynchronous execution keeps the UI responsive, and
no retries avoid overlapping repeated calculations.

## Consequences

- Volume usage can take up to 30 seconds before Ferrum reports a timeout.
- Other JSON requests continue to use the eight-second deadline.
- The volume-usage response remains capped at 16 MiB and Ferrum makes no retries.
- The request remains asynchronous and read-only on Linux with a local Docker
  Unix socket.
- A timeout or failed response appears as a usage error; the inspected volume
  details remain visible.
- No support is implied for other engines or platforms, and issues #17 and #10
  remain open.
