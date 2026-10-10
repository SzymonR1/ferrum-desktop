# Volumes

The Volumes page lists and inspects volumes for the selected local Docker Unix
socket on Linux. Select a volume to inspect its name, optional driver, scope,
creation time, and host mountpoint. Missing optional fields are shown as not
reported. Selected controls and invisible formatting characters in driver,
scope, creation time, and mountpoint are shown as visible `\u{...}` escapes;
unsafe volume names are rejected. Ferrum displays the mountpoint as untrusted
text and does not open, read, or browse that path.

## Create a volume

The create form sends only `Name` to the negotiated
`POST /v{api}/volumes/create` endpoint. Docker selects its default local volume
driver; driver, labels, and options are not configurable. Names must be
nonblank, at most 4096 UTF-8 bytes, contain no control or selected invisible
formatting characters, and not be `.` or `..`. The request body is limited to 16 KiB and the response to 64 KiB.
A successful response must identify the requested name. Requests have an
eight-second bound; a timeout or transport error can occur after Docker has
created the volume. Ferrum refreshes the inventory after every create result
and blocks context switching while the request is pending. Navigating away
does not abort a submitted request.

## Usage reporting

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

The usage request does not retain or display labels or options, and it does not
access volume mountpoints. A reference count is only a snapshot. Even a count
of zero does not guarantee that a volume is safe to delete because container
state can change after the response. Ferrum offers no volume removal, pruning,
modification, connection, disconnection, or filesystem browsing.

Only Linux with the selected local Docker Unix socket is covered. Podman and
rootless behavior are unvalidated, remote endpoints are unsupported, and
Windows and macOS volume support is not implemented. Headless UI and fake-socket
tests cover the interface and protocol; the native Volumes page has not been
visually validated. This slice does not complete issues
[#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) or
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10), and it is not a
release claim.

The optional ignored live list/inspect/usage test requires `FERRUM_TEST_VOLUME`
to name an existing volume on the current local Docker context. It is
read-only and does not clean up resources:

```sh
FERRUM_TEST_VOLUME=my-existing-volume cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_volume_read_only -- --exact --ignored
```

The optional ignored create test creates a uniquely named volume through the
current local Docker context. EngineClient and cleanup use the exact same local
Unix socket path; cleanup passes it to Docker CLI as an argument-array
`--host unix://...` option and never uses force. The test preflights the
generated name and arms cleanup only after both the create response and
inventory confirm that exact name. If ownership cannot be confirmed, no cleanup
is sent. Once a cleanup command has returned, the guard is disarmed; a failed
command is not retried by name and the volume may remain:

```sh
cargo test -p ferrum-engine live_volume_create_tests::local_engine_creates_and_removes_unique_volume -- --exact --ignored
```

Default fake-socket and headless UI tests do not contact a live engine. See the
[testing guide](../developer-guide/testing.md) for prerequisites and details.
