# Networks

The Networks page lists selectable network names from the selected local
Docker-compatible Unix socket on Linux. Selecting a network opens inspection,
which displays its ID, name, driver, scope, creation time, and the `internal`,
`attachable`, `EnableIPv4`, and `EnableIPv6` flags when reported. Optional
fields that the engine omits are shown as not reported.

Inspect adds an optional point-in-time count of attached containers from the inspect response's `Containers` map. The list response may omit attached-container information; a missing map in inspect means the count is not reported, while a present empty map means zero. Ferrum retains and displays the count only: it does not retain or display container IDs, names, endpoint records, MAC/IP addresses, labels, options, IPAM, peers, services, status, or raw response bodies.

This slice does not provide network usage statistics or create, remove, connect, or disconnect operations, and does not read or send credentials. It does not claim Podman/rootless compatibility or support remote endpoints. Issues [#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) and [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remain open; this is not completion of either issue or a release.

Default tests, headless UI tests, and fake-socket protocol tests do not contact or mutate a live engine. An optional ignored live test requires `FERRUM_TEST_NETWORK` to name an existing network on the selected local Docker context. It lists networks, then inspects the matching listed network ID; it performs no mutation or cleanup.

Run the opt-in read-only test with an existing network name:

```sh
FERRUM_TEST_NETWORK=my-existing-network cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_network_read_only -- --exact --ignored
```

Podman/rootless behavior and remote endpoints remain unsupported or unvalidated. Windows named pipes and macOS remain outside the current Linux-only engine scope. See the [testing guide](../developer-guide/testing.md) for test distinctions and the [platform support matrix](../platform-support.md) for current compatibility limits.
