# Volumes

The Volumes page lists and inspects volumes for the selected local Docker Unix
socket on Linux. Select a volume to inspect its name, optional driver, scope,
creation time, and host mountpoint. Missing optional fields are shown as not
reported. Selected controls and invisible formatting characters in driver,
scope, creation time, and mountpoint are shown as visible `\u{...}` escapes;
unsafe volume names are rejected on creation.

Engine-returned volume names and optional detail values are untrusted display
text. Ferrum escapes selected control, bidi, zero-width, invisible-format,
Unicode-tag, and Hangul-filler characters. Volume names and each optional
detail value—driver, scope, creation time, and host mountpoint—use the same
4 KiB raw-or-escaped display bound at the UI boundary. Longer values are shown
as `[display text omitted]`. This is presentation hardening, not secret
detection or redaction: raw names remain unchanged for identity comparisons
and API request values. Ferrum displays the mountpoint as untrusted text and
never accesses, opens, reads, or browses that path.

## Create a volume

The create form sends only `Name` to the negotiated
`POST /v{api}/volumes/create` endpoint. Docker selects its default local volume
driver; driver, labels, and options are not configurable. Names must be
nonblank, at most 4096 UTF-8 bytes, contain no control or selected invisible
formatting characters, and not be `.` or `..`. The request body is limited to
16 KiB and the response to 64 KiB.
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
Ambiguous usage data—including duplicate/conflicting `VolumeUsage`, `Items`,
`Name`, or `UsageData` keys and repeated exact-name items—is unavailable. The
zero-reference removal gate remains disabled unless one unambiguous snapshot
reports `RefCount == 0`.

The usage request does not retain or display labels or options. A reference count
is a point-in-time snapshot and does not guarantee that a volume is safe to
remove. The mountpoint remains untrusted text and is never accessed.

## Remove a volume

Removal is enabled only when the selected volume is still in the loaded
inventory, a matching inspection returns the exact name, and usage for that
exact name reports `RefCount == 0`. Positive or unavailable counts keep the
control disabled. Confirmation names the volume and warns that the count can
change before deletion. Ferrum sends the negotiated
`DELETE /v{api}/volumes/{name}` without a `force` parameter; this deletes the
volume and its data. Docker may reject removal if a container starts using the
volume before the request arrives. A timeout or transport error may follow
deletion. After every result, Ferrum marks inventory stale; it refreshes
immediately if Volumes remains open, or when the page is opened again. A new
inspection/usage snapshot is required before retrying. Only one volume mutation
is active; context switching is blocked while it is pending. Navigating away
does not abort a submitted request. Pruning, modification, connection,
disconnection, and filesystem browsing are not offered.

Only Linux with the selected local Docker Unix socket is covered. Podman and
rootless behavior are unvalidated, remote endpoints are unsupported, and
Windows and macOS volume support is not implemented. Headless UI and fake-socket
tests cover the interface and protocol. This is a partial display-hardening
slice; issues [#17](https://github.com/SzymonR1/ferrum-desktop/issues/17) and
[#19](https://github.com/SzymonR1/ferrum-desktop/issues/19) remain open/ready,
while [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10) remains
open/blocked for Podman/rootless behavior and broader endpoint validation. No
endpoint/platform support or release is claimed; native Volumes visual
validation remains unverified.

The optional ignored live list/inspect/usage test requires `FERRUM_TEST_VOLUME`
to name an existing volume on the current local Docker context. It is
read-only and does not clean up resources:

```sh
FERRUM_TEST_VOLUME=my-existing-volume cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_volume_read_only -- --exact --ignored
```

The optional ignored live create/remove test is mutating and requires the
current local Docker context to use a Unix socket. It preflights a generated
name, creates it with EngineClient, and arms cleanup only after the create
response and inventory confirm that exact name. Before removal, the test
requires the exact-name usage snapshot to report `RefCount == 0`; it then
disarms the guard, calls `EngineClient::remove_volume`, and verifies the name
is absent. If the test unwinds before the removal request is submitted, the
Drop fallback uses an argument-array Docker CLI `--host unix://...` request on
the same pinned socket without force. Once removal is submitted there is no
retry by name; a failed or uncertain request may leave the volume. If ownership
cannot be confirmed, no removal or cleanup request is sent:

```sh
cargo test -p ferrum-engine live_volume_create_tests::local_engine_creates_and_removes_unique_volume -- --exact --ignored
```

Default fake-socket and headless UI tests do not contact a live engine. See the
[testing guide](../developer-guide/testing.md) for prerequisites and details.
