# Testing Ferrum

Run the deterministic workspace tests and checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo doc --workspace --no-deps
mdbook build docs
```

The default suite does not require a host engine. Ignored live-engine tests for
version, inventory, and Docker CLI contexts are read-only. The general
local-engine inventory test may inspect an existing container; these tests do
not mutate resources.

The opt-in image list/inspect test is read-only and requires
`FERRUM_TEST_IMAGE` to name an image already present on the current local Docker
context. The image does not need a shell; the test never pulls or mutates image
resources. For example, after manually installing `busybox:1.37.0` if needed:

```sh
docker pull busybox:1.37.0
FERRUM_TEST_IMAGE=busybox:1.37.0 cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_preinstalled_image_read_only -- --exact --ignored
```

The pull is an optional manual prerequisite, not part of the test. A smoke run
against an image already present needs only the second command.

The lifecycle and logs tests below are separate opt-in ignored tests and mutate
the engine by creating one disposable container.

The lifecycle integration test is a separate opt-in, ignored test and mutates
the engine by creating one disposable container and exercising its lifecycle.
It requires `FERRUM_TEST_IMAGE` to name an image already present locally that
contains `/bin/sleep`. The test never pulls an image. A manual prerequisite, if
needed, is to pull a suitable image yourself, for example:

```sh
docker pull busybox:1.37.0
```

Run only the exact ignored lifecycle test (the default suite skips it):

```sh
FERRUM_TEST_IMAGE=busybox:1.37.0 cargo test -p ferrum-engine --test live_engine local_engine_lifecycle_actions_emit_events_and_update_state -- --exact --ignored
```

The test creates one uniquely named container with `--pull never`, `--network none`, no host mounts, and no published ports. It uses the full ID from the Docker create response/cidfile as its only cleanup authority. On failure, best-effort cleanup may force-remove only that validated ID; it never removes volumes. The successful EngineClient Remove action is exercised after stopping the container and does not force-remove or delete volumes. Run only against a disposable local engine.

The ignored live logs test is mutating and opt-in. It requires `FERRUM_TEST_IMAGE`
to name a locally installed image containing `/bin/sh` and `/bin/sleep`; it
never pulls an image. Run exactly:

```sh
FERRUM_TEST_IMAGE=busybox:1.37.0 cargo test -p ferrum-engine --test live_engine local_engine_logs_stream_history_follow_and_cancel -- --exact --ignored
```

It creates one uniquely named disposable container with `--pull never`,
`--network none`, no mounts, and no published ports. Cleanup uses only that
container's validated full ID; failure cleanup may force-remove only that ID
and never removes volumes. Run only against a disposable local engine.

The live logs test exercises history, follow, and cancellation behavior; it
does not require the UI or visually validate the Logs view. Closing the Logs
view cancels its stream.

The engine integration targets Linux local Unix sockets. Podman/rootless
behavior remains unvalidated, remote transports remain unsupported, and Windows
named pipes and macOS remain deferred to issue #21/M8.

The `audit::tests` module captures lifecycle trace events in memory and checks
action/outcome fields, full-ID validation, and omission of untrusted identifiers.
Run those behavior tests directly with:

```sh
cargo test -p ferrum-app audit::tests
```

Audit events are process tracing output only; the app does not persist a
tamper-proof audit history.

## Accessibility and performance samples

The workspace includes headless AccessKit tests for labels, focus, keyboard
activation, large-inventory virtualization, and stream error/reconnect states.

The Images page has headless `egui_kittest` coverage for navigation, searchable
and virtualized rows, image selection and details, loading/error/empty states,
keyboard interaction, and stale-result rejection. Run only these UI tests with:

```sh
cargo test -p ferrum-app images::tests
```

These tests use fixed image data and do not call an engine. The separate
opt-in ignored `local_engine_lists_and_inspects_preinstalled_image_read_only`
test exercises image list and inspect requests against the local Docker Unix
socket. On 2026-10-09, it listed and inspected the preinstalled
`hello-world:latest` image and confirmed its ID appeared in the inventory
without pulling or modifying images. The native Images page was not clicked or
visually captured because the desktop provider cannot enumerate Ferrum or take
screenshots.
Manual release-mode performance samples are opt-in and have no timing
thresholds. See the [performance and accessibility evidence](../performance.md)
for the host, dataset, methodology, results, and known validation limits.
