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

Inspect adds an optional point-in-time count of attached containers from the inspect response's `Containers` map. The list response may omit attached-container information; a missing map in inspect means the count is not reported, while a present empty map means zero. Ferrum retains and displays the count only: it does not retain or display container IDs, names, endpoint records, MAC/IP addresses, labels, options, IPAM, peers, services, status, or raw response bodies.

This slice offers network list/inspect and default bridge creation only. It
does not provide network usage statistics, remove, connect, or disconnect
operations, and does not read or send credentials. It supports only a Linux
local Docker Unix socket; Podman/rootless and remote endpoints remain
unsupported or unvalidated, and Windows named pipes and macOS are outside the
current engine scope. Issues [#17](https://github.com/SzymonR1/ferrum-desktop/issues/17)
and [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remain open;
this is not completion of either issue or a release.

Default tests, headless UI tests, and fake-socket protocol tests do not contact
or mutate a live engine. The optional ignored read-only live test requires
`FERRUM_TEST_NETWORK` to name an existing network on the selected local Docker
context. It lists networks, then inspects the matching listed network ID:

```sh
FERRUM_TEST_NETWORK=my-existing-network cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_network_read_only -- --exact --ignored
```

The separate opt-in create test preflights that its generated name is absent,
then creates one test-owned network. It arms cleanup only after the returned
ID is a full 64-character hex ID and a subsequent inventory entry matches both
that exact ID and the generated name. Cleanup removes only that verified ID.
Run it only against a disposable local Docker Engine:

```sh
cargo test -p ferrum-engine --test live_engine local_engine_creates_default_bridge_network_and_cleans_up_exact_id -- --exact --ignored
```

If ownership cannot be confirmed, the test fails without issuing a cleanup
request, so a created network may remain for manual inspection and cleanup.
After ownership is confirmed, the cleanup guard retries only that ID and never
removes by name or runs a broad prune. Headless Networks tests cover the UI
behavior; native UI click/visual validation remains unverified. See the
[testing guide](../developer-guide/testing.md) for test distinctions and the
[platform support matrix](../platform-support.md) for current compatibility
limits.
