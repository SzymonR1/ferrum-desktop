# Volumes

The Volumes page is a read-only inventory for the selected local Docker Unix
socket on Linux. Select a volume to inspect its name, optional driver, scope,
creation time, and host mountpoint. Missing optional fields are shown as not
reported. Ferrum displays the mountpoint as untrusted text and does not open,
read, or browse that path.

Ferrum reads per-volume usage with the negotiated Engine API request
`GET /v{api}/system/df?verbose=true&type=volume`. It keeps usage data only when
the response volume name exactly matches a name in the loaded volume inventory.
`Size` is shown in bytes and is available for local volume drivers. For other
drivers the engine may return `-1`, which means unavailable. Zero is a valid
reported value; negative or missing values are not reported. `RefCount` is the
point-in-time number of containers the engine reports as referencing that
volume. The request has a 30-second bound and a 16 MiB response limit. Ferrum
issues it asynchronously and does not retry it. Other JSON requests keep their
eight-second deadlines. If usage retrieval fails, Ferrum keeps the inspected
details visible and shows a separate usage error.

This request does not retain or display labels or options, and it does not
access volume mountpoints. A reference count is only a snapshot. Even a count
of zero does not guarantee that a volume is safe to delete because container
state can change after the response. Ferrum does not offer volume creation,
removal, pruning, modification, connection, or disconnection.

Only Linux with the selected local Docker Unix socket is covered. Podman and
rootless behavior are unvalidated, remote endpoints are unsupported, and
Windows and macOS volume support is not implemented. Headless UI and fake-socket
tests cover the interface and protocol; the native Volumes page has not been
visually validated. This slice does not complete issues
[#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) or
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10), and it is not a
release claim.

The optional ignored live test requires `FERRUM_TEST_VOLUME` to name a volume
that already exists on the selected local Docker socket. It lists volumes,
inspects that exact name, and reads its usage from the Engine API. It does not
create, remove, modify, or clean up any resource. Headless UI and fake-socket
tests do not contact a live engine. See the [testing guide](../developer-guide/testing.md)
for the exact command.

For the optional ignored live check, set `FERRUM_TEST_VOLUME` to an existing
volume name and run:

```sh
FERRUM_TEST_VOLUME=my-existing-volume cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_volume_read_only -- --exact --ignored
```

It lists volumes, inspects the named volume, and reads per-volume usage. It
performs no create, remove, modify, or cleanup operation.
