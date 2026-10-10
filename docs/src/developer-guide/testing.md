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
Settings tests cover missing/corrupt settings, file-symlink rejection without
changing target permissions, non-regular and hard-linked file rejection, and
refusal to chmod or write through symlinked settings directories. The hard-link
regression links a `0644` sibling sentinel as `settings.toml` and verifies that
load rejects it without changing the sentinel's contents or mode; load also
normalizes the opened final settings directory to `0700`. Single-link
versionless legacy settings continue to migrate in memory.

Temporary-directory path regressions verify the anchored-path contract. Create
a store, then replace the selected parent path and confirm that loads and saves
cannot reach the replacement target. Include an XDG/HOME symlink alias that
exists when the store is initialized and confirm that Ferrum honors that
selected location.

Use temporary directory trees to check that loads with missing directories use
defaults without creating directories, while saves create remaining components
relative to the retained descriptor. Insert symlinks in the remaining directory
components and at the final app directory; load and save must reject them
without changing or writing through their targets. Loads reject symlinked,
non-regular, and multiply linked settings files. Saves reject a pre-existing
symlink or non-regular entry before writing; if the destination changes
concurrently into a symlink, `renameat` replaces the link entry without
following its target. Verify that every directory Ferrum creates in the
remaining path ends at exactly `0700`, settings and temporary files stay `0600`,
and pre-existing ancestors keep their modes. The existing final app settings
directory is set to `0700` on load and save.

The subprocess regression
`settings::tests::saving_under_restrictive_umask_normalizes_only_new_directories`
uses a missing `XDG_CONFIG_HOME` below a parent-controlled temporary tree and
umask `0200`. The parent sets both `XDG_CONFIG_HOME` and `HOME` to that same
temporary path, so the child cannot read or modify ambient user settings. The
test verifies that settings are written at
`$XDG_CONFIG_HOME/ferrum/settings.toml` and that the fallback
`$HOME/.config/ferrum/settings.toml` path is not created. It also checks save
success, exact modes for both created directories and the settings file, and
unchanged modes on pre-existing ancestors. Run it directly with:

```sh
cargo test -p ferrum-app settings::tests::saving_under_restrictive_umask_normalizes_only_new_directories -- --exact
```

The default suite must use temporary paths and must not change the user's real
XDG or HOME settings tree. [ADR 0010](../architecture/decisions.md#adr-0010-linux-settings-directory-permissions) supersedes ADR 0009 only for newly created
remaining-path directory modes; [ADR 0011](../architecture/decisions.md#adr-0011-linux-settings-load-permissions-and-hard-link-rejection) records load-time directory permissions and hard-link rejection. If a
umask removes owner-read permission required to open a new directory, save
fails before temporary-file creation or settings writes. There is no
interprocess serialization between `mkdirat` and `openat`: an actor with
write/search access to the parent (subject to sticky-directory rules) may
replace a just-created entry with a real directory during that interval.
`O_NOFOLLOW` rejects symlinks, not real-directory replacements. If an ordinary
unprivileged process opens a replacement owned by another user, descriptor-
based mode normalization fails with `EPERM` and save stops before writing; a
process that owns the replacement or has `CAP_FOWNER` can change its mode.
Later filesystem operations remain anchored to the opened descriptor. Keep the
migration, default, corrupt-file, and unknown-secret-field checks.

The inherited-marker regression
`settings::tests::inherited_valid_marker_cannot_write_through_symlinked_test_roots`
passes a matching parent PID/nonce while the test root or `existing-xdg-parent`
is a symlink. It verifies that Ferrum writes no settings through either link
and that the helper process umask remains `027` (a `0750` directory probe).
Marker validation also rejects a pre-existing derived XDG root.

Run the focused load and marker checks directly:

```sh
cargo test -p ferrum-app settings::tests::loading_rejects_hardlinked_settings_without_changing_sibling_permissions -- --exact
cargo test -p ferrum-app settings::tests::inherited_valid_marker_cannot_write_through_symlinked_test_roots -- --exact
cargo test -p ferrum-app settings::tests::inherited_umask_child_marker_cannot_touch_ambient_settings_or_umask -- --exact
```

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

The ignored read-only volume test requires `FERRUM_TEST_VOLUME` to name a
volume already present on the selected local Docker Unix socket:

```sh
FERRUM_TEST_VOLUME=my-existing-volume cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_volume_read_only -- --exact --ignored
```

It lists volumes, inspects the exact configured name, and reads volume usage
with `GET /v{api}/system/df?verbose=true&type=volume`. Ferrum accepts usage only
for a response name that exactly matches the loaded inventory entry. The
volume-usage request has a scoped 30-second total request deadline and a 16 MiB
response limit. Only volume-usage calls receive this longer bound; volume list
and inspect requests retain their existing eight-second timeout. This opt-in
test remains read-only: it does not create, remove, modify, or clean up any
resource.
Headless Volumes UI and fake-socket protocol tests need no live engine and
perform no mutations. UI coverage checks reported zero versus unavailable
values, separate usage errors, and stale responses. The native Volumes page has
not been visually validated.

The optional ignored network list/inspect test is read-only and requires
`FERRUM_TEST_NETWORK` to name an existing network on the selected local Docker
context. It lists networks, then inspects the matching listed network ID:

```sh
FERRUM_TEST_NETWORK=my-existing-network cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_network_read_only -- --exact --ignored
```

It does not create, remove, connect, disconnect, modify, or clean up resources.
This live test is distinct from the default suite, which skips ignored tests,
and from headless UI/fake-socket protocol tests, which do not contact a live
engine. Network list results may omit attached-container information. Inspect
may report an optional point-in-time attached-container count; no container
identifiers, names, endpoint data, or other non-allowlisted network fields are
retained or displayed. There are no live mutation tests for networks.

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
and virtualized rows, image selection/details, loading/error/empty states,
keyboard interaction, and stale-result rejection. Tag UI coverage checks the
accessible repository/tag fields, bounded target validation, disabled invalid
targets, explicit confirmation because Docker Engine v1.56 may reassign an
existing `repository:tag` while image data remains, cancel-without-request
behavior, loading, safe success/error display, single-request gating, stale
generation rejection, and successful-inventory-refresh state. Removal UI
coverage checks the accessible confirmation with a snapshot of the selected
image ID and known tags, final-reference warning, non-force/in-use and
multi-tag warnings, `noprune=true` parent preservation, Cancel-without-request,
successful request state, safe errors, stale invalidation, and one-mutation-at-
a-time gating across pull/tag/remove. Hidden-progress coverage verifies the
reader and gate stay active until a terminal event; invalidation hides late
events without releasing the gate early. These tests use fixed image data and
local state/events only; they never contact an engine or delete an image.

Pull presentation tests use fixed progress values. Tag and removal tests
exercise fake completion events only; they do not send requests or mutate image
resources. No live pull, tag, or image-removal operation is claimed. Tests must
not pull, tag, delete, or otherwise mutate user images or engines.

Engine image pull, tag, and removal contract tests use fake Unix sockets, not a
Docker engine or registry, and never pull, tag, or delete an image. Tag cases
cover encoded request paths/queries, success response, omitted force/auth
fields, invalid input without a connection, and safe typed status rejection.
Pull cases cover query encoding, chunked progress, response/event errors,
malformed or oversized input, and invalid references. Removal cases cover the
encoded image ID, DELETE with `noprune=true`, omitted force/auth fields, success,
invalid input without a connection, and safe rejection/status without response
body exposure. Run the image protocol tests with:

```sh
cargo test -p ferrum-engine image_pull::tests
cargo test -p ferrum-engine image_api_tests
```

The separate opt-in ignored
`local_engine_lists_and_inspects_preinstalled_image_read_only` test exercises
image list and inspect requests against the local Docker Unix socket. It is
read-only and does not tag, pull, or modify images. No live image tag, pull, or
removal request or image mutation test is provided.

Image pull, tag, and removal requests use the validated local Docker Unix-socket
endpoint; private-registry authentication is not implemented.


Manual release-mode performance samples are opt-in and have no timing
thresholds. See the [performance and accessibility evidence](../performance.md)
for the host, dataset, methodology, results, and known validation limits.
