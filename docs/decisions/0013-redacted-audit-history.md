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
from container lifecycle, image pull/push/tag/removal and Buildx image builds,
volume create/removal, network create/removal/connect/disconnect operations,
and embedded terminal exec opens. Store it as `audit.jsonl`
in the same descriptor-anchored private Ferrum configuration directory used by
settings, and open it lazily on the first supported record. Require a `0700`
directory and a regular `0600` file. Do not follow symlink targets or accept
non-regular or multiply linked audit files.

Append one JSON object per line and sync record data. Each record contains only
a schema version, timestamp in Unix milliseconds, and a static event/action/outcome,
with an already-validated full resource ID only when available. Terminal exec
uses `terminal_exec` / `exec` and static `started`, `success`, or `request_error`
outcomes. Persist its `started` record before submitting the Engine exec request;
if that record cannot be durably written, do not submit exec and show a static
failure notice. Persist the terminal outcome after the open request; an outcome
write failure does not change the already-observed terminal result. Never persist
shell/command details, terminal input/output, names, image references or tags,
endpoints, credentials, raw errors, daemon bodies, or filesystem paths.

Cap the file at 16 MiB. Do not automatically delete, truncate, or rotate older
records. When the cap is reached or persistence fails, claim no durable record
and emit a structured warning with only a static `audit_error` category; do not
log raw storage errors or paths. Failure to persist the required terminal
`started` marker blocks exec submission. Persistence failure for a final record
or an already-submitted action leaves its observed Engine/UI result unchanged.
Existing process trace events remain separate from this file.

Serialize appends with nonblocking file-lock attempts bounded to 250 ms. If the
lock remains contended, classify persistence as `write` and apply the same
terminal-start versus already-submitted outcome policy above.

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
for final outcomes and actions already submitted to the Engine: a failure emits
a static warning but does not change the already-observed Engine/UI result.
Terminal exec is an explicit exception: its synced `started` record is a
prerequisite to submission, so failure to persist it blocks the exec request
and produces a static UI failure.

## Consequences

- The record is local history only: it is not tamper-proof, does not capture
  mutations made outside Ferrum, and does not prove the engine's final state.
- When the file reaches 16 MiB or cannot be safely opened/appended/synced, later
  final outcomes may have no durable record. A static warning is emitted, while
  the already-observed Engine/UI result for an already-submitted action remains
  unchanged. A terminal exec whose `started` record cannot be persisted is not
  submitted and instead produces a static UI failure.
- Cancellation or shutdown while an exec-open request is in flight may leave
  only its `started` record; that records an attempt, not whether the Engine
  began a shell.
- Only validated full IDs already available to the mutation trace may appear;
  no names or other resource descriptors are included.
- This is a partial Issue #19 slice. Issues #19 and #17 remain open/ready; #10
  remains open/blocked for Podman/rootless behavior and broader endpoint
  validation. No release or support for other endpoints or platforms is
  claimed.
- Existing UI safety constraints and native visual-validation limitations
  remain unchanged.
