# ADR 0005: Container lifecycle actions and event synchronization

- **Status:** Accepted for Linux local-Unix sockets only
- **Date:** 2026-10-08

## Context

Ferrum's inspection view needs to support carefully scoped container lifecycle operations without turning the engine adapter into a general-purpose management API. External changes also need to appear in the UI without replacing the existing bounded asynchronous delivery path. Engine responses and event streams are untrusted; lifecycle failures must not leak raw response bodies, and event processing must remain bounded and cancellable.

## Decision

The `ContainerAction` set is Start, Stop, Restart, Pause, Unpause, and Remove. The inspection dialog exposes actions only when the known container state makes that action safe. Remove requires explicit confirmation. Lifecycle requests use the negotiated API version and a bounded 30-second wait so Stop/Restart can complete the engine's graceful shutdown interval. Start and Stop treat HTTP 304 as idempotent success; other non-2xx outcomes become safe typed errors. A timeout warns that the container may still be changing and directs users to refresh before retrying. Remove issues DELETE without force or volume-removal parameters. Raw response bodies are never logged or exposed.

External synchronization consumes Docker container-only JSONL events. Each event line is capped at 64 KiB and each transport data frame at 1 MiB; changes are delivered to the UI through its existing bounded channel and backpressure behavior. A fresh inventory snapshot is fetched after initial subscription and reconnect, and any open inspection is reloaded so state-gated actions do not remain stale. The event task is cancelled when the engine context changes or the application tears down. Each outage gets at most three retries, with waits of 250 ms, 500 ms, and 1 s; a successful subscription resets the retry budget. Once retries are exhausted, the UI reports event synchronization unavailable. Manual inventory refresh also revalidates any open inspection.

### Implementation clarification (2026-10-12)

Each local Docker event subscription now includes a `since` timestamp captured
at subscription time. A read-only Docker Engine 29.9.0/API 1.56 probe returned
256 retained container-event rows when `since` was omitted, even though no live
container events occurred during the application startup interval. Ferrum relies
on its inventory snapshot after initial subscription and reconnect to cover
earlier state, while the event stream carries only subsequent changes. The same
cutoff is applied on reconnect; no event history is required to reconstruct
current inventory.

The Engine cursor uses whole Unix seconds, so events from earlier within that
same second may still be emitted. The immediate inventory snapshot and normal
event refresh path reconcile this small overlap; the cutoff prevents replay of
the older retained backlog without suppressing events after subscription.

This decision applies only to the existing Linux local Unix-socket engine path. Remote transports, Podman/rootless behavior, Windows, and macOS are not enabled or validated by lifecycle support.

## Alternatives considered

### Unrestricted remove options

Rejected: force removal and volume deletion are not needed for the scoped action and could expand destructive effects. Remove therefore sends neither force nor volume-removal parameters and requires confirmation in the UI.

### Unbounded or detached event processing

Rejected: unbounded buffering can exhaust resources, while a task detached from context/application lifetime can continue consuming events for a stale engine. The bounded channel and explicit cancellation preserve existing ownership and backpressure.

### Retry indefinitely or silently stop synchronization

Rejected: unbounded retry obscures degraded connectivity and can outlive useful context. Bounded retries expose synchronization loss while keeping manual refresh available.

## Consequences

- Lifecycle status requests are available through state-gated inspection actions; Remove is explicitly confirmed and cannot request volume removal.
- Start/Stop are idempotent for the engine's 304 response; other unsuccessful statuses are represented as safe typed errors.
- Event synchronization is bounded in line size, delivery, retry count, and task lifetime.
- Event-stream exhaustion is visible and does not disable manual refresh.
- Existing local endpoint restrictions remain in force; this ADR makes no broader engine or platform support claim.
