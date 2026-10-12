# ADR 0015: Bounded decoded Engine inventories

- **Status:** Accepted
- **Date:** 2026-10-12

## Context

ADR 0004 bounds Linux local Engine JSON response bodies and request deadlines. A byte cap alone does not bound the memory used by an untrusted JSON array: many compact rows or nested string entries can amplify into large `Vec` allocations during deserialization. Container, image, network, and volume inventories are engine-controlled and feed UI state and destructive-operation guards.

## Decision

Limit every array decoded by the container, image, network, and volume inventory DTOs to 65,536 elements. This includes top-level inventory rows and the retained per-container `Names` and per-image-list `RepoTags` arrays. The decoder reserves at most the lower of the sequence size hint and the cap, decodes no more than the cap as typed values, then probes one additional item as `IgnoredAny`. If any extra item exists, reject the complete inventory through its existing safe typed error; never truncate or expose a partial list.

The 16 MiB inventory response-body cap and eight-second request timeout from ADR 0004 remain unchanged. Empty arrays remain successful empty inventories. Missing/null optional nested arrays remain absent, and missing/null `Volumes` remains an empty inventory as permitted by the Docker Engine schema. Inspection, history, usage, and mutation response contracts are unchanged.

## Alternatives

- **Rely on the body-size cap alone:** rejected because a compact JSON body can contain many small rows whose vector and per-item allocation overhead substantially exceeds their encoded size.
- **Truncate at the item limit:** rejected because incomplete inventories could mislead users and undermine dependency checks or action gating.
- **Use an unbounded sequence and validate afterward:** rejected because allocation amplification occurs during deserialization, before a post-decode length check.

## Rationale

The 65,536-element cap matches the existing image-history bound and leaves ample headroom above the 1,000-record UI performance target. A shared bounded sequence decoder applies one fail-closed invariant consistently without changing DTO allowlists or public domain models. Probing the first excess value as ignored data detects overflow without materializing it as a typed row.

## Consequences

- An inventory array containing more than 65,536 entries is unavailable rather than partially displayed; the application reports its existing static endpoint-specific error.
- The same limit applies to nested `Names` and `RepoTags` arrays retained from list responses. Other response schemas and the history cap remain unchanged.
- Deterministic fake-Unix tests cover exact-limit acceptance, over-limit rejection for each public list operation, and nested-array overflow. They do not require an Engine or mutate resources.
- This decision adds no Podman/rootless or remote endpoint support. Existing platform and endpoint limitations remain as documented in ADR 0004 and Issue #10.

## Relationship to ADR 0004

This ADR supplements the accepted body-size/time-bound transport decision with a decoded-item bound. ADR 0004's historical record is unchanged; its 16 MiB inventory body and eight-second request bounds continue to apply.
