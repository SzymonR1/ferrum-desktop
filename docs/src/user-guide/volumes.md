# Volumes

The Volumes page is a read-only inventory for the selected local Docker-compatible Unix socket on Linux. Select a volume to inspect its name, optional driver, scope, creation time, and host mountpoint. Missing optional fields are shown as not reported. The mountpoint is plain text only; Ferrum does not open or browse it.

Volume list and inspect endpoints do not provide usage data. Ferrum does not show estimated size or reference counts. This slice does not offer volume creation, removal, connection/disconnection, network operations, or usage reporting. Do not infer compatibility with Podman/rootless engines or remote endpoints. Issues [#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) and [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remain open.

The headless UI and fake-socket protocol coverage does not mutate an engine. An optional ignored live test requires `FERRUM_TEST_VOLUME` to name a pre-existing volume on the selected local Docker context. It only lists volumes and inspects that named volume; it creates, removes, modifies, or cleans up nothing. See the [testing guide](../developer-guide/testing.md) for the exact command.

For the optional ignored live check, set `FERRUM_TEST_VOLUME` to an existing
volume name and run:

```sh
FERRUM_TEST_VOLUME=my-existing-volume cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_volume_read_only -- --exact --ignored
```

It lists and inspects only; it performs no create, remove, modify, or cleanup.