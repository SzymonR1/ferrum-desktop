# ADR 0004: Bounded engine HTTP

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

Bollard provides a convenient Docker-compatible API client, but its version and list response path does not provide the response-body bounds and redaction guarantees required by Ferrum. A local engine can return unexpectedly large or sensitive JSON, so version negotiation, inventory, and inspection all need the same constrained transport contract. The selected endpoint policy remains Linux local Unix sockets only.

## Decision

All Linux local API JSON operations—version, inventory, and inspect—use a private Hyper/Hyperlocal request helper over the already selected Unix socket. Each operation has a bounded total timeout of 8 seconds and a response-body limit: version 64 KiB, inventory 16 MiB, and inspect 4 MiB. Only allowlisted Ferrum-owned DTO fields are deserialized. Raw engine response bodies are never logged or included in errors. Unknown inspect states are preserved as unknown rather than rejected or reinterpreted.

The server's reported API maximum remains the value exposed through `EngineInfo.api_version`. For requests, negotiate using the lower of that server maximum and Bollard's canonical `API_DEFAULT_VERSION` client maximum. Bollard remains a source of the canonical client maximum, not the transport or decoder for these JSON operations.

This ADR supersedes the response-transport decisions in ADRs [0002](0002-engine-client.md) and [0003](0003-linux-docker-contexts.md). It preserves their local endpoint selection and context policy. Remote transports remain unsupported; this decision adds neither Podman support nor rootless endpoint support.

## Alternatives

### Keep Bollard for version and inventory

Rejected because those response paths do not provide the required overall bounded-body and safe-decoding contract.

### Replace all engine access with a general-purpose transport abstraction

Rejected because only the selected local Linux Unix socket is implemented; remote transport support is explicitly out of scope.

### Keep bounded handling only for inspection

Rejected because version and inventory responses are also untrusted engine-controlled JSON and require equivalent limits and redaction.

## Rationale

One private request path gives version, inventory, and inspection consistent timeout, size, status, decode, and logging behavior while keeping endpoint selection separate. Allowlisted Ferrum DTOs keep unrelated engine fields out of application state, and negotiated versions remain compatible with the client's canonical maximum.

## Consequences

- The adapter owns Hyper/Hyperlocal HTTP request and bounded JSON response handling for all three operations.
- Limits constrain response buffering; oversized, timed-out, unsuccessful, or invalid JSON responses fail without exposing raw body content.
- Engine-reported maximum API version and selected request API version are distinct values.
- Local Unix-socket/context selection policy is unchanged. Remote transports, Podman, and rootless support remain unsupported and are not implied by this ADR.
