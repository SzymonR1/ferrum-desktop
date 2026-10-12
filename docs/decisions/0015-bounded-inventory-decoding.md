# ADR 0015: Bounded decoded Engine inventories

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

ADR 0004 bounds Linux local Engine JSON response bodies and request deadlines.
A byte cap alone does not bound memory used by untrusted JSON arrays: compact
rows and nested strings amplify into vector allocations. Per-array limits alone
also fail to bound the product of row count and nested `Names`/`RepoTags` items.
Container, image, network, and volume inventories are engine-controlled and
feed UI state and destructive-operation guards.

## Decision

Cap each typed inventory sequence at 65,536 items. The bounded visitor reserves
at most the lower of the sequence size hint and cap, and rejects one extra
item rather than truncating or returning partial data.

Before typed deserialization, a recursive streaming preflight counts every
array item across the complete inventory JSON response. Reject more than
262,144 total array items with the existing safe typed error. This response-wide
budget prevents nested arrays from multiplying allocations across rows.

The 16 MiB response-body cap and eight-second timeout from ADR 0004 remain
unchanged. Empty arrays remain successful. Missing/null optional nested arrays
remain absent, and missing/null `Volumes` remains an empty inventory as
permitted by the Docker Engine schema. Inspection, history, usage, and mutation
response contracts are unchanged.

## Alternatives

- **Rely on the body-size cap alone:** rejected because compact rows can amplify
  into much larger vector allocations.
- **Apply only an independent per-array cap:** rejected because the same nested
  cap can multiply across thousands of inventory rows.
- **Truncate an over-limit inventory:** rejected because incomplete results
  could mislead users and undermine dependency checks or action gating.
- **Decode first and validate afterward:** rejected because allocation
  amplification occurs during typed deserialization, before a length check.

## Rationale

The per-sequence bound preserves existing list behavior for ordinary responses;
the response-wide preflight bounds nested fan-out before typed vectors exist.
The 262,144-item budget is four times the existing sequence limit, while the
16 MiB body cap still bounds scalar text. Both limits fail closed.

## Consequences

- A sequence over 65,536 items or a response with over 262,144 total array
  items is unavailable rather than partially displayed; errors remain static.
- The response-wide count includes top-level rows and nested list data such as
  container `Names` and image-list `RepoTags`; missing/null semantics are
  unchanged.
- Fake-Unix tests cover exact and over-limit sequence/aggregate cases, including
  nested fan-out bodies below the existing response-byte limit. They require no
  Engine and mutate no resources.
- This decision adds no Podman/rootless or remote endpoint support. Existing platform and endpoint limitations remain as documented in ADR 0004 and Issue #10.

## Relationship to ADR 0004

This ADR supplements the accepted body-size/time-bound transport decision with a decoded-item bound. ADR 0004's historical record is unchanged; its 16 MiB inventory body and eight-second request bounds continue to apply.
