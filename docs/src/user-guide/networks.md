# Networks

The Networks page lists selectable network names from the selected local Docker
Unix socket on Linux. It can create a network by name through
`POST /v{negotiated}/networks/create`, sending only the required `Name` field.
Docker's default bridge driver is used; no driver, IPAM, or options controls
are exposed. Names must be non-whitespace, at most 255 UTF-8 bytes, and contain
no control characters. A successful create refreshes the current Networks
inventory. Selecting a network opens inspection, which displays its
ID, name, driver, scope, creation time, and the `internal`, `attachable`,
`EnableIPv4`, and `EnableIPv6` flags when reported. Optional fields that the
engine omits are shown as not reported.

Inspect reports an optional point-in-time attached-container count and retains
membership IDs only when the `Containers` map is present and every key is a
full 64-character hexadecimal ID. Endpoint values are discarded. A missing
map means the count is not reported; an empty map means zero. Ferrum never
retains or displays endpoint names, endpoint IDs/configuration, addresses,
labels, options, raw status, subnet/CIDR keys, endpoint data, peers, or services.
The validated membership set is used only to compare exact container IDs against
the current container inventory and to show network-attachment controls.

When the Docker Engine reports verbose IPAM status, inspection also shows
aggregate point-in-time allocation counts across its reported subnets: IPs in
use (including reserved addresses) and dynamically available IPs. These are
allocation counts, not traffic counters. Missing, malformed, negative, or
overflowing metrics, invalid subnet CIDR keys, and duplicate subnets after CIDR
normalization are shown as “Not reported”; attached-container count is a
separate membership snapshot.
Subnet/CIDR keys, addresses, endpoints, and raw status are not exposed. The
Engine API does not expose per-network traffic totals through this endpoint;
per-container/interface counters exist but are not aggregated here.

Engine-provided network names/metadata and attached-container names are
untrusted text. Ferrum visibly escapes selected controls, bidi/zero-width and
invisible-format characters, Unicode tags, and Hangul fillers in network
rows/details, membership actions, and confirmations. Engine-sourced fields
whose raw or escaped text exceeds 4 KiB are shown as `[display text omitted]`.
It retains original values for exact comparisons;
connect/disconnect/remove requests remain bound to validated network/container
IDs.

This page supports network list/inspection, name-only default-bridge creation,
explicit removal of selected unused custom networks, and connecting or
disconnecting containers from selected local custom networks. Docker's
predefined `bridge`, `host`, and `none` networks cannot be removed or changed
through this UI.

Network removal is enabled only when the selected inventory entry has a fresh
matching inspection whose `attached_container_count` is exactly `Some(0)`;
positive or unreported counts, mismatched IDs/names, and stale inspections
cannot submit. The user must confirm a prompt naming the exact network name and
full 64-character hexadecimal ID. Ferrum sends negotiated
`DELETE /v{api}/networks/{id}` with an eight-second deadline and no force or
prune parameters. The inspect count is a point-in-time snapshot, not an atomic
deletion guarantee: Docker may reject a request if dependencies change, and a
timeout or transport error does not prove the network remains.

Connect and disconnect controls require a fresh inspection of a selected local
custom network with complete validated membership IDs. A candidate container
must appear in the current inventory with the same full 64-character
hexadecimal ID and a reported `Running` state. Stopped, paused, and unknown
states disable both actions; this keeps the displayed membership comparison
bound to active network endpoints rather than treating a successful request
for an inactive container as a visible attachment. A stale or incomplete
membership map disables these actions rather than guessing.

The Networks-page refresh reads both network and container inventories.
Container-change events invalidate the cached state and trigger a refresh when
the page is current; actions remain disabled while inventory is stale or a
refresh fails. Use Refresh to retry after a failed inventory read.
Before submitting either operation, Ferrum asks for explicit confirmation that
names the network and container and displays both full IDs. Disconnect
confirmation warns that it can interrupt connectivity.

On confirmation, Ferrum sends only
`POST /v{api}/networks/{id}/connect` or `/disconnect` with a JSON body
containing only `Container`. It supplies no endpoint configuration, omits
`Force` so Docker's default `false` applies, uses an eight-second deadline, and
does not retry. Errors expose only safe typed endpoint/status information; raw
daemon bodies and IDs are excluded. A timeout or transport failure can follow
a completed mutation. After either result Ferrum refreshes network and
container inventories and re-inspects the selected network; if refresh fails,
the network list is marked stale and must be reloaded before another action.
Only one network mutation may be active, and context switching is blocked
until its result arrives.

Per-network traffic totals and credential access are not provided; available
IPAM values are point-in-time allocation counts, not traffic. Network operations
are scoped to the selected local Docker Unix socket on Linux. Podman/rootless
behavior remains unvalidated, remote endpoints are unsupported, and Windows
named pipes and macOS are outside the current engine scope; see [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10)
for the remaining endpoint matrix. No general platform/engine or release claim
is made.

Native Networks-page click/visual validation remains unverified because the
available desktop provider cannot enumerate Ferrum or capture its native
surface.

Default tests, headless UI tests, and fake-socket protocol tests do not contact
or mutate a live engine. The optional ignored read-only live test requires
`FERRUM_TEST_NETWORK` to name an existing network on the selected local Docker
context. It lists networks, then inspects the matching listed network ID:

```sh
FERRUM_TEST_NETWORK=my-existing-network cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_network_read_only -- --exact --ignored
```

The separate opt-in test preflights that its generated name is absent, then
creates one test-owned network. It arms cleanup only after a full 64-character
hex ID and a subsequent inventory entry match both that exact ID and the
generated name. It removes only that ID through `EngineClient::remove_network`
and confirms the ID is absent. The `Drop` fallback retains the same
`EngineClient`, which is pinned to the selected Unix socket, and uses only the
verified ID without force or prune. If ownership cannot be confirmed, no
removal is sent and the created network may remain for manual inspection.
Run it only against a disposable local Docker Engine:

```sh
cargo test -p ferrum-engine --test live_engine local_engine_creates_default_bridge_network_and_cleans_up_exact_id -- --exact --ignored
```

Headless Networks tests
cover dependency gates, explicit confirmation, stale results, and mutation
gating; native UI click/visual validation remains unverified. See the
[testing guide](../developer-guide/testing.md) for test distinctions and the
[platform support matrix](../platform-support.md) for current compatibility
limits.

The separate opt-in connect/disconnect live test requires a current local
Unix-socket Docker context and a preinstalled `FERRUM_TEST_IMAGE` containing
`/bin/sleep`. It creates a uniquely named network through `EngineClient`; after
the client pins the socket, Docker CLI `--host` creates a test-owned container
on that same socket with `--network bridge --pull never`, no host mounts, and
no published ports. It never pulls an image. After verifying the exact full
IDs against generated names, the test starts the container, connects and
inspects membership, disconnects and re-inspects, then stops and removes the
container. Cleanup uses only those verified IDs without force or prune.

The network cleanup guard is armed only after the create response's full ID
matches the generated name in a fresh inventory. A create error, mismatched
ID, or failed inventory read does not authorize cleanup; the network may remain
for manual inspection. The container guard is armed only after its full ID
matches the exact generated name. Once armed, both guards use the same pinned
`EngineClient` socket and exact IDs without force or prune. If identity cannot
be verified, no stop/remove request is sent and the test-owned resource may
remain for manual inspection.
Run this mutating test only against a disposable local engine:

```sh
FERRUM_TEST_IMAGE=ghcr.io/catthehacker/ubuntu:rust-latest cargo test -p ferrum-engine --test live_engine local_engine_connects_and_disconnects_network_with_verified_owned_container -- --exact --ignored
```
