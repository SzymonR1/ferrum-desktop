# ADR 0013: Redacted local audit history

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Ferrum emits structured process traces for some mutation outcomes, but those
traces are ephemeral and are not durable history. Issue #19 calls for audit
coverage, while a complete cross-platform, multi-engine solution is outside the
validated Linux local-Docker slice. Mutation results can also be uncertain:
a transport/request error does not prove that the engine state remained
unchanged.

## Decision

Add a separate, bounded Linux-local JSONL sink for Ferrum-initiated outcomes
from container lifecycle, image pull/tag/removal, volume create/removal, and
network create/removal/connect/disconnect operations. Store it as `audit.jsonl`
in the same descriptor-anchored private Ferrum configuration directory used by
settings, and open it lazily on the first supported outcome. Require a `0700`
directory and a regular `0600` file. Do not follow symlink targets or accept
non-regular or multiply linked audit files.

Append one JSON object per line and sync record data. Each record contains only
a schema version, timestamp in Unix milliseconds, a static event/action/outcome,
and already-validated full resource IDs where available. Never persist names,
image references or tags, endpoints, credentials, raw errors, daemon response
bodies, or terminal/log contents.

Cap the file at 16 MiB. Do not automatically delete, truncate, or rotate older
records. When the cap is reached or persistence fails, claim no durable record,
emit only a static process warning, and leave the already-returned engine
result unchanged. Existing process trace events remain separate from this file.

## Alternatives

- **Rely only on process tracing:** rejected because events disappear with the
  process and do not provide a local persistent history.
- **Persist complete engine requests/responses or descriptive resource data:**
  rejected because names, references, endpoint data, errors, and daemon bodies
  can contain sensitive or untrusted information that is not needed for this
  bounded outcome record.
- **Rotate or delete old records automatically:** rejected because it would
  silently remove prior local history and complicate the fixed storage bound.
- **Claim a tamper-proof or complete audit system:** rejected because a local
  file controlled within the same user/filesystem trust boundary cannot provide
  tamper evidence or capture activity outside Ferrum.
- **Extend to Podman/rootless, remote engines, or other platforms:** deferred;
  those paths are not validated by this decision.

## Rationale

A narrowly allowlisted outcome record provides durable local context while
avoiding persistence of resource names, endpoint material, raw failures, or
engine-provided bodies. Anchoring file access to the existing settings
filesystem descriptor carries forward the established private-directory
boundary. A hard size ceiling and refusal to overwrite existing history keep
storage bounded without silently replacing records. Persistence is best-effort
and must never turn an audit-file failure into a changed engine-action result.

## Consequences

- The record is local history only: it is not tamper-proof, does not capture
  mutations made outside Ferrum, and does not prove the engine's final state.
- When the file reaches 16 MiB or cannot be safely opened/appended/synced, later
  outcomes may have no durable record. A static warning is emitted, while the
  engine result remains unchanged.
- Only validated full IDs already available to the mutation trace may appear;
  no names or other resource descriptors are included.
- This is a partial Issue #19 slice. Issues #19 and #17 remain open/ready; #10
  remains open/blocked for Podman/rootless behavior and broader endpoint
  validation. No release or support for other endpoints or platforms is
  claimed.
- Existing UI safety constraints and native visual-validation limitations
  remain unchanged.
