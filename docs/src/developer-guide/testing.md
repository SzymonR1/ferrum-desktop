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

Image-build regressions exercise only temporary files and a controlled helper
process. They do not invoke Docker or Buildx against an engine, build an image,
or mutate image resources. Tests cover local-context and target validation,
private config permissions and cleanup, socket-pinned command construction,
credential isolation, bounded and escaped output, process failure/cancellation,
explicit confirmation, context-switch gating, and static non-persistent build
outcome tracing.

The named tests are:

- `private_build_config_is_owner_only_and_empty`
- `build_request_rejects_invalid_context_and_target`
- `build_target_rejects_uppercase_repository_names`
- `validate_context_requires_regular_root_dockerfile_and_rejects_symlink_or_missing_file`
- `build_command_uses_local_socket_and_private_config`
- `build_process_streams_safe_bounded_output_and_reports_failure`
- `build_cancellation_stops_the_process_group`
- `build_target_is_bounded_and_rejects_invalid_controls`
- `build_confirmation_requires_explicit_acceptance_and_cancel_submits_nothing`
- `build_confirmation_gates_other_image_mutations`
- `build_confirmation_names_the_selected_engine_source`
- `build_start_progress_success_error_and_cancel_have_terminal_transitions`
- `build_output_is_bounded_and_escaped`
- `records_static_image_build_outcomes`
- `event_reconciliation_tests::pending_image_build_confirmation_disables_context_switching`
- `event_reconciliation_tests::pending_image_build_disables_context_selector_accessibly`
- `event_reconciliation_tests::active_image_build_disables_context_switching`
- `event_reconciliation_tests::build_engine_source_label_distinguishes_configured_socket_from_discovery`

Run the focused checks with:

```sh
cargo test -p ferrum-app --bin ferrum image_build::tests
cargo test -p ferrum-app --bin ferrum images::tests::build_
cargo test -p ferrum-app --bin ferrum event_reconciliation_tests::pending_image_build
cargo test -p ferrum-app --bin ferrum event_reconciliation_tests::active_image_build_disables_context_switching -- --exact
cargo test -p ferrum-app --bin ferrum audit::tests::records_static_image_build_outcomes
```

Builds require Docker CLI and the Buildx plugin in `PATH`; this Linux local
Docker slice does not validate Podman/rootless or broader endpoint/platform
support. Issues #16 and #19 remain open and ready, issue #10 remains open and
blocked, and no release or general support claim is made.

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

Fake-socket regressions verify that duplicate/conflicting volume-usage
`VolumeUsage`, `Items`, `Name`, and `UsageData` keys plus repeated exact-name
items produce unavailable usage, never a false zero-reference result. Run the
focused regression with:

```sh
cargo test -p ferrum-engine volume_api_tests
```

Headless Volumes UI and fake-socket protocol tests need no live engine and
perform no mutations. UI coverage checks reported zero versus unavailable
values, separate usage errors, and stale responses. The native Volumes page has
not been visually validated.

The optional ignored live volume create/remove test is mutating and requires
the current local Docker CLI context to use a Unix socket:

```sh
cargo test -p ferrum-engine live_volume_create_tests::local_engine_creates_and_removes_unique_volume -- --exact --ignored
```

EngineClient uses the socket path stored in the discovered current context. The
test preflights its generated name, verifies the create response and inventory,
and requires the exact-name usage snapshot to report `RefCount == 0`. It
disarms the cleanup guard immediately before calling
`EngineClient::remove_volume`, then confirms inventory absence. The Drop
fallback uses argument-array Docker CLI `--host unix://...` pinned to the same
socket without force, only if the test unwinds before the removal request is
submitted. There is no name-based retry after submission; a failed or uncertain
removal may leave the volume. If ownership cannot be confirmed, no removal
request is sent. The read-only `FERRUM_TEST_VOLUME` test is separate.
Fake-socket and headless UI tests do not contact a live engine.

The optional ignored network list/inspect test is read-only and requires
`FERRUM_TEST_NETWORK` to name an existing network on the selected local Docker
context. It lists networks, then inspects the matching listed network ID:

```sh
FERRUM_TEST_NETWORK=my-existing-network cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_network_read_only -- --exact --ignored
```

An optional ignored live test creates and removes one uniquely named default
bridge network through the current local Docker Unix context. It preflights
that the generated name is absent, then requires the returned ID to be a full
64-character hex ID and a subsequent inventory entry with exactly that ID and
generated name before arming the cleanup guard:

```sh
cargo test -p ferrum-engine --test live_engine local_engine_creates_default_bridge_network_and_cleans_up_exact_id -- --exact --ignored
```

Run it only against a disposable local Docker Engine. Once ownership is
verified, the test removes only that ID through `EngineClient::remove_network`
and confirms it absent from refreshed inventory. The `Drop` guard retains the
same `EngineClient` pinned to the selected local Unix socket and only attempts
exact-ID removal if still armed; it uses no force or prune. If ownership cannot
be confirmed, no removal is sent and the network may remain for manual
inspection. The default suite skips this ignored mutating test.

The optional ignored live network connect/disconnect test requires the current
local Docker context to use a Unix socket and `FERRUM_TEST_IMAGE` already
installed with `/bin/sleep`. After EngineClient resolves and pins that socket,
the test passes the same path as Docker CLI `--host` to create a uniquely named
container with `--network bridge --pull never`, no host mounts, and no
published ports; it never pulls an image. It verifies the network and
container IDs against their exact generated names, starts the container,
connects and inspects membership, disconnects and re-inspects, then stops and
removes the container. Cleanup targets only those IDs without force or prune.

The network cleanup guard is armed only after a successful create response's
full ID matches the generated name in fresh inventory. A create error,
mismatched ID, or failed inventory read does not authorize cleanup; the network
may remain for manual inspection. The container guard is armed only after its
full ID matches the exact generated name. Once armed, both guards use the same
pinned `EngineClient` socket and exact IDs without force or prune. If identity
cannot be verified, no stop/remove request is sent and the test-owned resource
may remain.
Run it only on a disposable local engine:

```sh
FERRUM_TEST_IMAGE=ghcr.io/catthehacker/ubuntu:rust-latest cargo test -p ferrum-engine --test live_engine local_engine_connects_and_disconnects_network_with_verified_owned_container -- --exact --ignored
```

Fake-socket network API tests cover versioned exact-ID delete/connect/disconnect
paths, invalid IDs before connection, a minimal `Container` body with no
endpoint configuration or `Force`, no removal force/prune parameters, and safe
typed rejection errors without daemon-body or resource-ID exposure.

Headless Networks UI/state tests cover removal only after a fresh matching
inspection reports `Some(0)`, disable positive/unreported counts and predefined
networks, and require an explicit confirmation/cancel. Attachment controls
require a complete membership map of full IDs plus an exact current-inventory
container entry in the `Running` state; stopped, paused, or unknown states are
disabled. Connect/disconnect confirmations name both full IDs. Tests cover
container-event invalidation, NetworkList refresh of both inventories, stale
list results and read failures, stale selections/results, and single-flight/
context-switch gates. Inspect retains the point-in-time count and only the
membership keys when every key is a validated full ID; endpoint values are
discarded. Network names and endpoint details are not retained from the map.

Fake-socket/headless tests do not contact or mutate a live engine. The native
Networks-page click/visual check remains unverified. Counts and membership are
point-in-time snapshots; Docker can reject a mutation if network dependencies
change after inspection, and timeout/transport errors do not prove that the
engine state is unchanged.

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

The headless `log_buffer_tests` verify that C0/C1 controls and selected
bidirectional, zero-width, and invisible formatting characters are escaped
before row text reaches the UI and before `visible_text` feeds Copy/Export.
Escaped expansion is also rejected if it exceeds the existing 8 MiB retained
buffer limit. This is data-level coverage, not native visual validation or
secret detection. Run the focused tests with:

```sh
cargo test -p ferrum-app --bin ferrum log_buffer_tests
```

Headless AccessKit tests verify visible escaping for engine-controlled container
inventory and inspection fields, image tags/inspection author/tag-confirmation
IDs, network names/metadata, and attached-container names in
membership/confirmation labels. They also verify network actions submit exact
IDs and image tagging submits the original image ID. Engine-sourced fields
whose raw or escaped text exceeds the 4 KiB limit are omitted. These tests use
in-memory view state and do not contact an engine. Run them with:

```sh
cargo test -p ferrum-core --test display_safety
cargo test -p ferrum-app --bin ferrum display::tests
cargo test -p ferrum-app --bin ferrum untrusted
cargo test -p ferrum-app --bin ferrum engine_image_id_is_escaped_in_tag_confirmation_and_request_preserves_exact_id
cargo test -p ferrum-app --bin ferrum engine_network_and_container_text_is_escaped_before_accessible_display
```

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
resources. No live pull, tag, removal, or image build was performed; the
separate optional image list/inspect test is read-only. Tests must not pull,
tag, delete, or otherwise mutate user images or engines.

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
