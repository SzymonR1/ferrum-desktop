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

## Read-only Docker endpoint and context smokes

On 2026-10-13, two ignored live tests passed against Docker Engine 29.9.0/API
1.56 via `unix:///var/run/docker.sock`:

- `current_local_docker_context_connects_and_returns_a_valid_inventory`
  discovered and connected to the current `default` context, read engine
  information, and listed containers.
- `local_engine_returns_a_well_formed_container_inventory` used a direct
  `DOCKER_HOST` override, read engine information and inventory, and inspected
  one existing container. It requires at least one existing container.

The current-context and direct-host tests above do not exercise non-current
contexts. Separately, a read-only smoke on 2026-10-11 used a temporary
`DOCKER_CONFIG` with a non-current `ferrum-readonly-context-probe` context
pointing to the same local Unix engine. The two existing network list/inspect
and verbose-IPAM tests passed (2/2), then the temporary config was removed.
This validates selection of a named non-current context against the same
daemon; it does not validate a distinct engine, remote context, or UI
context-switch workflow.

```sh
cargo test -p ferrum-engine --test live_engine current_local_docker_context_connects_and_returns_a_valid_inventory -- --exact --ignored
DOCKER_CONTEXT='' DOCKER_HOST=unix:///var/run/docker.sock cargo test -p ferrum-engine --test live_engine local_engine_returns_a_well_formed_container_inventory -- --exact --ignored
```

Fake-Unix Engine inventory tests exercise the public container, image, network,
and volume list methods. Each typed sequence is capped at 65,536 entries; a
preflight counts array entries recursively across the complete response and
rejects totals above 262,144 before DTO decoding. Tests cover exact and excess
per-array limits, nested high-fan-out `Names`/`RepoTags` below the 16 MiB body
cap, and exact/over aggregate-counter boundaries. The default suite does not
contact a live Engine or mutate resources.

## Engine connection handshake

Fake-Unix fixtures exercise the Linux handshake and first inventory request on a
temporary local socket passed through the `DOCKER_HOST` selection path. They
verify negotiated API-version use, successful empty inventories, typed and
redacted inventory errors, and that a version response missing its API version
fails connection before any inventory request. The fixtures validate Ferrum's
protocol and error mapping only; they do not establish compatibility with
Docker, Podman, or cross-platform endpoints. Run them with:

```sh
cargo test -p ferrum-engine connection_handshake_tests
```


## Compose mutation input snapshots

Confirmed Compose mutations use the same write-sealed memfd descriptors for
preflight validation and the submitted command; service restart and scale also
reuse them for the fresh read-only status query.
A controlled fake-CLI regression replaces selected source files after preflight
and verifies the operation still receives the bytes that were validated. Run
it with:

```sh
cargo test -p ferrum-app compose::tests::subprocess::compose_mutation_uses_the_validated_file_snapshot -- --exact
```

The optional real-CLI smoke checks ordered base/override files and a local
`include` through `/proc/self/fd` while retaining the original project
directory. It runs only `docker compose config --services` on owned temporary
files; it requires Docker CLI/Compose but does not contact an Engine, registry,
or user project:

```sh
cargo test -p ferrum-app compose::tests::subprocess::local_compose_validation_reads_the_sealed_operation_snapshot -- --exact --ignored
```

## Opt-in production Compose build smoke

`local_compose_build_uses_production_worker_and_cleans_only_verified_image`
is an ignored Linux integration test that calls Ferrum's production
`compose_build` worker, not a fake CLI path. It requires Docker CLI/Compose and
an accessible local Docker-compatible Engine. It creates a unique temporary
project and generated local image using `FROM scratch` and one small `COPY`
input; it requests no `RUN`, pull, `up`, container, network, volume, port, host
mount, remote context, build args, SSH, or secret. Compose is pinned to the same
validated local Unix socket returned by `EngineClient::connect_default()`.

Before submission, the exact generated image reference must be absent. The
worker is bounded to 120 seconds; timeout triggers cancellation, a bounded
10-second join, then task abort if needed. After a submitted or uncertain
outcome, inventory is inspected before cleanup. `DisposableImageTag` is armed
only for one exact reference with a full SHA-256 image ID and no other tags; it
removes only that ID on the same socket, using `--no-prune`, no force, and a
five-second child bound. Failed inventory, ambiguity, timeout, or additional
tags do not broaden cleanup; a generated test image may need manual removal.
Local Engine builder cache may remain. This is a test-owned image mutation, not
a page-visual test, `down` test, registry-pull test, or Podman test.

Run explicitly, and only where that local Engine mutation is acceptable:

```sh
cargo test -p ferrum-app --bin ferrum compose::tests::subprocess::local_compose_build_uses_production_worker_and_cleans_only_verified_image -- --exact --ignored
```

The smoke passed on 2026-10-14 on Fedora Linux 44 x86_64 against Docker Engine
29.9.0/API 1.56, Compose 5.6.0, and Buildx 0.38.0 through the default local
Unix context. It created and cleaned only its generated image; Issue #18
remains partial/open.

## Compose service pause/unpause

Controlled fake-CLI and headless tests cover service-scoped `pause SERVICE`
and `unpause SERVICE` on the selected Linux local Docker Unix socket. Pause is
eligible only for a declared service whose current status row is exactly
`running`; unpause requires exactly `paused`. Stopped, unknown, absent, or
undeclared services are ineligible. Tests cover imported/revalidated selection,
status gating before confirmation and the worker's fresh status recheck after
sealed-snapshot validation, exact service-targeted argv, confirmation and
cancellation, static action/outcome audit records without service/path/project
metadata, and retry blocking after a post-spawn failure until status refresh
succeeds. The operation uses the existing private, cleared Compose environment,
argv, 30-second bound, and child-process cleanup. Pause suspends processes and
unpause resumes them; neither operation creates or removes containers,
networks, or volumes.

No live mutation was run because no disposable Engine is designated. There is
no native visual validation, and these tests do not establish Podman, remote,
Windows, or macOS support or full completion of Issues #18/#19.

The controlled fake-CLI and headless regressions cover one accessible control
per all-declared service rather than per replica row; inactive-profile services
are included, and all-profile `ps --all` counts include stopped containers.
Regular lifecycle eligibility remains selected-profile-only. Tests cover target
bounds, undeclared services, selected-profile validation and all-profile status
argv versus profile-free scale `up` argv, exact
`up --detach --no-build --pull never --no-deps --scale SERVICE=N SERVICE`
arguments, sibling safety, stale confirmation counts, sealed-file validation,
fresh same-socket status, uncertain retries, and revalidation invalidating
status/counts until a fresh refresh. Headless confirmation regressions cover
stopped-container starts, configured service code execution, writable-layer loss
on configuration/image-driven recreation, and mounted-volume semantics.
Explicit cancel and page/context invalidation tests assert static
`scale/not_submitted` audit data without dynamic operation metadata.

The optional ignored live test preflights that a unique generated project and
its default network are unused, then scales the profile-gated `scale-probe`
service up to exactly two replicas and down to one. A same-profile sibling
service verifies that explicit target activation does not start siblings. The
smoke then invokes the production project `Stop` and `Start` worker with only
the `scale-check` profile selected, verifying the sole target container is
`exited` after Stop and `running` after Start while the sibling remains absent.
It uses an already-installed `FERRUM_TEST_IMAGE` with `/bin/sleep`, no host
mounts or published ports, and requests no pull or build. Read-only
inventory/status and generated-project cleanup may use `--profile '*'`; scale
mutations never do. A scoped cleanup runs `compose down` only with the
generated project name, directory, and known Compose file; it does not
force-remove, prune, or remove volumes:

```sh
env -u DOCKER_HOST DOCKER_CONTEXT=default FERRUM_TEST_IMAGE=ghcr.io/catthehacker/ubuntu:rust-latest cargo test -p ferrum-app --bin ferrum compose::tests::subprocess::local_compose_service_scale_scales_and_cleans_only_verified_project -- --exact --ignored
```

The prior scale-only version passed on 2026-10-13 against Fedora Linux 44
x86_64, Docker Engine 29.9.0/API 1.56, and Compose v5.6.0 via the local
`default` Unix-socket context. That run verified scale-up/down, sibling absence,
and scoped cleanup; it did not exercise Stop/Start.

The expanded smoke passed on 2026-10-14 against the same environment (1 passed,
391 filtered). It verified scale-up to two replicas and down to one, Stop to
`exited`, Start back to `running`, sibling absence, and successful scoped
cleanup with no generated-project containers remaining. This opt-in smoke
covers only this local Docker Engine path: it does not establish UI, Podman,
remote, Windows, macOS, or full Issue #18 acceptance. Do not run it against an
Engine containing valuable resources; it touches only its preflighted,
uniquely named generated project.

## Compose up with local images

The confirmed Compose `up` path has controlled fake-CLI and headless UI
coverage. Regressions exercise selected ordered files/profiles and socket
arguments, sealed-snapshot submission, confirmation gating, and warnings about
volume creation and writes to mounted data. They also cover static audit
outcomes and post-spawn failure classification without changing Engine
resources. Headless state tests require a successful status snapshot before
`up`, reject confirmation while status is pending or failed, and keep retries
blocked until status succeeds after an uncertain outcome and page
invalidation/re-import. No live mutating integration test ran because no
disposable Engine is designated; fixtures do not establish live Engine or
broader platform behavior.

Settings tests cover missing/corrupt settings, file-symlink rejection without
changing target permissions, non-regular and hard-linked file rejection, and
refusal to chmod or write through symlinked settings directories. The hard-link
regression links a `0644` sibling sentinel as `settings.toml` and verifies that
load rejects it without changing the sentinel's contents or mode; load also
normalizes the opened final settings directory to `0700`. Versionless and v1
settings migrate with an empty Compose registration list. Deterministic temp
store tests round-trip valid saved records, validate count/path/profile/label
bounds, enforce the 256 KiB serialized cap before atomic replacement, and
verify oversize failure leaves an existing settings file unchanged. These
tests use temporary directories only and contact no Engine or Compose CLI.

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

The opt-in image list/inspect/history test is read-only and requires
`FERRUM_TEST_IMAGE` to name an image already present on the current local Docker
context. The image does not need a shell; the test never pulls or mutates image
resources. For example, after manually installing `busybox:1.37.0` if needed:

```sh
docker pull busybox:1.37.0
FERRUM_TEST_IMAGE=busybox:1.37.0 cargo test -p ferrum-engine --test live_engine local_engine_lists_inspects_and_reads_history_for_preinstalled_image_read_only -- --exact --ignored
```

The pull is an optional manual prerequisite, not part of the test. A smoke run
against an image already present needs only the second command.

The 2026-10-11 read-only history smoke passed against Docker Engine 29.9.0/API
1.56 using the preinstalled `ghcr.io/catthehacker/ubuntu:rust-latest` image
and current local Unix context; it read history without mutating the image.

## Opt-in local Docker image-pull progress smokes

`local_engine_loopback_registry_pull_reports_progress_and_cleans_only_test_tag`
and `local_engine_loopback_registry_synthetic_basic_auth_pull_reports_progress`
are ignored Linux integration tests for the Engine pull stream, not the Images
page or credential-helper path. Both require the current Docker context to use
a local Unix socket and the daemon's existing loopback-insecure-registry policy.
Each starts an in-process OCI registry bound only to `127.0.0.1` on an ephemeral
port and changes no daemon configuration. Both preflight generated references
as absent and probe a missing manifest that must reach the fixture, be rejected,
and leave both references absent. Each Engine progress stream has a 30-second
total deadline and a 1,024-record cap; the smoke retains only whether a
non-empty status was observed. On a completed pull, cleanup is armed only after
inventory confirms a full SHA-256 image ID and exactly the generated tag. It
pins cleanup to the same validated local Unix socket as the EngineClient and
removes only the verified full image ID using `docker image rm --no-prune`
without force. If another tag is added, Docker refuses non-forced ID removal
and the image may remain for manual cleanup. If inventory cannot confirm
ownership, the test does not attempt removal. Explicit cleanup and Drop fallback
use a CLI child with a five-second deadline and attempt to kill and reap it on
timeout.

The authenticated test creates a nonce username and synthetic secret in memory,
constructs `RegistryAuth` directly, and authenticates both the missing-manifest
probe and pull. The fixture rejects requests without the exact expected Basic
authorization using HTTP 401 and a Basic challenge, and records only whether a
valid header was seen. Temporary secret/header strings are zeroized where
practical; neither user credentials nor Docker config/helpers are read. This is
synthetic direct Engine AuthConfig transport over the existing loopback
HTTP/insecure-registry setup. It does not validate helper selection, per-pull UI
opt-in, real secrets, TLS, external/private registry deployment, or production
authentication. Issues #16 and #19 remain partial/open.

No container, external registry, user image, or daemon setting is used. A failed
pull stream is dropped and the fixture stopped, but Engine-side cancellation is
not guaranteed; interrupted pulls may leave untagged blob data or an unknown
outcome. Run only against a disposable local Docker Engine:

```sh
env -u DOCKER_HOST DOCKER_CONTEXT=default cargo test -p ferrum-engine --test live_engine local_engine_loopback_registry_pull_reports_progress_and_cleans_only_test_tag -- --exact --ignored
env -u DOCKER_HOST DOCKER_CONTEXT=default cargo test -p ferrum-engine --test live_engine local_engine_loopback_registry_synthetic_basic_auth_pull_reports_progress -- --exact --ignored
```

The anonymous smoke passed on 2026-10-13. The synthetic-auth smoke passed on
the same Fedora Linux 44 x86_64 host against Docker Engine 29.9.0/API 1.56
through the local `default` context. Both assert at least one non-empty Engine
status; numeric byte counters are not claimed. The
authenticated smoke sends nonce-generated credentials directly as
`RegistryAuth` and verifies that the loopback fixture accepts the expected
Basic header; it does not exercise the configured helper or per-pull UI opt-in.
Docker Engine's `X-Registry-Auth` AuthConfig is documented in the
[Engine API reference](https://docs.docker.com/reference/api/engine/version/v1.56/).
No real credentials are used, and the smoke does not validate TLS or production
registry setup; CNCF Distribution requires TLS for Basic auth
([configuration](https://distribution.github.io/distribution/about/configuration/)).

Image-build regressions exercise only temporary files and controlled helper
processes. They do not invoke Docker or Buildx against an engine, build an image,
or mutate image resources. Tests cover local-context and target validation,
private config permissions and cleanup, socket-pinned command construction,
credential isolation, bounded and escaped output, process spawn failure versus
post-start failure/cancellation, explicit confirmation, context-switch gating,
and static durable build-outcome classification.

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
- `event_reconciliation_tests::build_runner_spawn_failure_is_persisted_as_not_submitted`
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
support. Issues #16 and #19 remain partial/open; #10 remains blocked/open.
No release or general support claim is made.

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
The fake-socket `volume_api_tests` also verify that control-heavy metadata with
an escaped form over 4 KiB and raw optional fields over 4 KiB are omitted before
they are retained in `VolumeSummary` or `VolumeDetails`.

Headless Volumes UI and fake-socket protocol tests need no live engine and
perform no mutations. UI coverage checks reported zero versus unavailable
values, separate usage errors, and stale responses. Headless AccessKit volume
cases verify names and optional driver, scope, creation-time, and host-
mountpoint details are escaped and bounded to 4 KiB of raw or escaped text;
over-limit text is omitted. Raw names stay exact for identity and requests, and
the mountpoint is never accessed. The volume display cases include
`engine_supplied_volume_names_are_escaped_before_accessible_rendering`,
`oversized_engine_volume_names_are_omitted_from_accessible_rendering`, and
`oversized_engine_volume_details_are_omitted_from_accessible_rendering`.
The native Volumes page has not been visually validated.

Network IPAM usage fake-socket regressions exercise the `verbose=true` inspect
request, aggregation across subnets, missing/empty status, missing or invalid
counts, invalid IPv4/IPv6 CIDR keys, equivalent subnet keys after normalization,
duplicate status/IPAM/subnet/metric fields, repeated CIDR records, and checked
addition overflow. Malformed allocation usage remains unavailable without
hiding valid network inspection or membership. Headless Networks coverage checks
reported versus unavailable allocation counts and labels them explicitly as
point-in-time allocation rather than traffic. Neither coverage contacts or
mutates an engine.

The optional ignored live volume create/remove test is mutating and requires
the current local Docker CLI context to use a Unix socket:

```sh
cargo test -p ferrum-engine live_volume_create_tests::local_engine_creates_and_removes_unique_volume -- --exact --ignored
```

This test is mutating and must run only against a disposable local Docker Engine
using a Unix socket. EngineClient uses the socket path stored in the discovered
current context. The test preflights a generated name, verifies the create
response and inventory names, and requires the exact-name usage snapshot to
report `RefCount == 0`. Docker volumes have no immutable ID in this path; a
concurrent delete/recreate under the same name cannot be distinguished. The
generated-name checks are best-effort, not a stable ownership proof, and
cleanup by name may target a same-name replacement if another actor races the
test. If initial name/response checks fail, no cleanup is issued and the
generated volume may remain. The Drop fallback uses the same pinned socket
without force only if the test unwinds before removal submission. There is no
name-based retry after submission. The read-only `FERRUM_TEST_VOLUME` test is
separate. Fake-socket and headless UI tests do not contact a live engine.

The optional ignored network list/inspect test is read-only and requires
`FERRUM_TEST_NETWORK` to name an existing network on the selected local Docker
context. It lists networks, then inspects the matching listed network ID:

```sh
FERRUM_TEST_NETWORK=my-existing-network cargo test -p ferrum-engine --test live_engine local_engine_lists_and_inspects_existing_network_read_only -- --exact --ignored
```

The optional ignored live IPAM check is also read-only. Set
`FERRUM_TEST_NETWORK` to an existing local network that reports at least one
verbose IPAM allocation count:

```sh
FERRUM_TEST_NETWORK=bridge cargo test -p ferrum-engine --test live_engine local_engine_reads_verbose_ipam_allocation_for_existing_network_read_only -- --exact --ignored
```

It only lists and inspects the selected network; it does not create, connect,
disconnect, or remove resources.

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

The local container-event subscription contract tests use fake Unix sockets and
verify both the container-only filter and a `since` timestamp captured within
two seconds of the current local Unix time. This prevents retained
pre-subscription events from being replayed as live changes; the application
continues to load an inventory snapshot after subscription and reconnect. The
event API cutoff is second-resolution, so a same-second overlap is possible;
the inventory snapshot covers that boundary. These tests do not contact a live
engine or mutate containers. Run them with:

```sh
cargo test -p ferrum-engine container_change_stream_tests
```

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

The embedded terminal's parser tests use synthetic byte streams and no engine.
They verify that raw OSC data passed to vt100 stays within the 1 KiB budget,
including when `ESC ]` and BEL/ST/CAN/SUB boundaries cross output chunks, and
that visible text resumes after an oversized string is discarded. Run them
with:

```sh
cargo test -p ferrum-app --bin ferrum terminal::tests::
```


Headless AccessKit tests verify visible escaping for engine-controlled container
inventory and inspection fields, image tags/inspection author/tag-confirmation
IDs, network names/metadata, attached-container names in membership/confirmation
labels, and engine-supplied volume names and optional details in their
accessible display surfaces. They also verify network actions submit exact IDs,
image tagging submits the original image ID, and volume names remain raw for
identity and request values. Engine-sourced fields whose raw or escaped text
exceeds the 4 KiB limit are omitted. The volume-specific cases are
`engine_supplied_volume_names_are_escaped_before_accessible_rendering`,
`oversized_engine_volume_names_are_omitted_from_accessible_rendering`, and
`oversized_engine_volume_details_are_omitted_from_accessible_rendering`. These
tests use in-memory view state, do not access mountpoints, and do not contact an
engine. Run the volume display regressions with:

```sh
cargo test -p ferrum-app --bin ferrum volume_details
```

Run the other display-safety checks with:

```sh
cargo test -p ferrum-core --test display_safety
cargo test -p ferrum-app --bin ferrum display::tests
cargo test -p ferrum-app --bin ferrum untrusted
cargo test -p ferrum-app --bin ferrum engine_image_id_is_escaped_in_tag_confirmation_and_request_preserves_exact_id
cargo test -p ferrum-app --bin ferrum engine_network_and_container_text_is_escaped_before_accessible_display
cargo test -p ferrum-app --bin ferrum volume_names
```

The engine integration targets Linux local Unix sockets. Podman/rootless
behavior remains unvalidated, remote transports remain unsupported, and Windows
named pipes and macOS remain deferred to issue #21/M8.

Default Compose discovery, validation, lifecycle, and headless UI tests use
controlled CLI fixtures; they do not contact a live engine or mutate projects.

Saved-registration regressions use the real `SettingsStore` in deterministic
temporary directories, not a fake storage echo. They cover validated session
selection save -> settings-file save -> reload -> visible saved row with exact
ordered paths/profiles and no selected/validated/status state; selecting it
requires explicit revalidation. Headless row tests return save/forget actions.
An app-level failed atomic save test verifies both save and forget preserve the
prior in-memory saved status and settings records. These tests invoke no
Docker/Compose CLI and contact no Engine.
Discovery fixtures check `docker compose version --short` and
`docker compose ls --all --format json`, with only the listing pinned to the
selected `EngineClient::local_socket_path()`. Validation preflights
`docker compose version --short` through the private isolated Docker config;
preflight failure is distinct from config-validation failure. The selected
ordered files and repeated profile arguments feed one validation invocation:
`docker compose --project-directory <parent> --file <file> ... config
--no-interpolate --no-env-resolution --services`. Its bounded stdout is parsed
as unique service-name rows; empty output is valid, while malformed, duplicate,
overlong, and excess output fails statically. Fake CLI fixtures verify exact
base/override/profile argument order and the private Docker config/HOME, cleared
environment, disabled selected-project `.env`, and disabled remote loaders.
Diagnostics and selected paths are not exposed. This same invocation supplies
every declared service name from the selected files regardless of active
profiles; no additional CLI process or Engine request is used. Service names
are escaped and shown separately from container status in a virtualized list
with success, empty, error, loading, and stale-result headless coverage. The
inventory is not a running-container list and listing does not trigger status
or logs.
Container-status parser regressions cover optional per-row `Project` metadata,
missing versus reported values, mixed rows, escaping, required nonempty strings,
the field-byte cap, wrong types/null, duplicate keys, and all-or-nothing output.
Headless status coverage verifies per-container `Project` labels and the
`Not reported` marker alongside existing empty, cancellation, clear, and
stale-result behavior. These fixtures do not establish that a live Engine
reported the field.

Compose profile coverage uses controlled CLI fixtures and headless state checks;
it does not contact a live engine or user project. Unit tests cover blank input
as no explicit profiles, whitespace trimming, the 1024-byte input, 16-profile,
and 64-byte/name limits, the allowed name grammar, and standalone `*` wildcard
enabling all profiles; input is bounded before CLI launch. Fake CLI fixtures
assert exact argument-array construction and repeated `--profile <name>` pairs
before `config --services`, `ps`, and `logs`, with no profile environment
variable. The log and service-restart fixtures verify that status rechecks reuse
selected profiles. Headless state tests cover profile-edit invalidation,
invalid-profile gating, cancellation/stale results, revalidation, and keeping
profile input disabled during asynchronous file selection.

Preserve private Docker config/HOME, cleared environment, disabled
selected-project `.env`, and disabled remote Git/OCI loaders. Transient imports
use synthetic path state and headless UI only; they retain no file contents and
persist no registry. Tests cover validation gating, duplicate/name and 32-entry
bounds, ordered path/profile restoration, stale-generation rejection, and
cancellation requests for in-flight validation/status/log tasks.

Lifecycle UI requires an imported, currently selected and explicitly
revalidated configuration. Service restart also requires a service in the
current successful status snapshot. Controlled fake-CLI regressions cover exact
ordered base/override, profile, and pinned-socket arguments; explicit
validation, then fresh read-only `ps --all --format json` membership, then only
`restart --no-deps SERVICE`; and no restart after failed status or absent
membership. The restart command test checks each argv item and the validation /
status / mutation order without invoking a live Docker command. Other lifecycle
tests retain start/stop argv coverage, isolated environment, validation-before-
mutation, safe static errors, and spawn/failure/timeout outcome classification.
Headless tests cover the restart control's visibility/disabled state,
confirmation naming the service/import label/ordered safe files/profiles/action,
and cancellation without creating a task. Existing headless tests cover
confirmation gating while the engine is unavailable, clearing unsubmitted
confirmations on page invalidation, preventing status refresh during a running
lifecycle command, preserving submitted results across page invalidation, and
persisted redacted `started`/`not_submitted` outcomes. Audit regression checks
static `restart` action/outcome fields without service, project, or path data.
Drop-before-first-poll shutdown flushes the initial marker. No live Compose
lifecycle mutation test is run; selecting an import clears stale read-only
results. Issue #18 remains partial/open.
See Docker's [Compose profiles guide](https://docs.docker.com/compose/how-tos/profiles/)
and [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose/).
Headless AccessKit/state tests cover ordered base/override selection, removing
overrides and explicitly revalidating the remaining set, replacing the base
(which clears overrides), the 16-file limit, escaped basename display,
pending/cancelled/stale selection, validation success/failure, the trusted-file
notice, and cancellation. They do not open the native file chooser or claim
native Compose-page visual validation.

Validation accepts one selected base Compose file followed by user-ordered
local override files, using one-based order and a maximum of 16 total files.
The base file's parent anchors project-directory path resolution; Compose
applies `-f` files in order. See Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for ordered merging and path resolution, and [`config`](https://docs.docker.com/reference/cli/docker/compose/config) for the validation flags.
The single-file smoke below covers relative paths. The ordered base/override
smoke uses the real CLI with temporary files, but is not arbitrary user-project
or lifecycle validation.

The validation trust boundary is unchanged with multiple selected files:
Compose can recursively read local `include` and service `extends.file`
references, plus included projects' default `.env` and explicit
`include.env_file` paths. Those reads may be outside the base directory and
occur with the logged-in user's permissions. `COMPOSE_DISABLE_ENV_FILE=1`
disables only the selected project's default `.env`; `--no-interpolate` and
`--no-env-resolution` do not confine included-project reads. Remote Git and OCI
loaders are disabled, so remote references fail without fetching. The CLI/plugin
is unsandboxed; validate only trusted files.

The controlled CLI fixtures verify version-probe and config arguments, the same
private configuration across both commands, selected file/parent paths, a
cleared environment without inherited engine or credential/proxy settings, and
explicitly disabled Git/OCI remote loaders. They also cover plugin-unavailable
classification before config invocation, redaction, output bounds, and
deadline cancellation. Run the Compose tests with:


```sh
cargo test -p ferrum-app --bin ferrum compose::tests::
```

The optional ignored app-level live smoke requires a local Docker-compatible
Unix socket and Compose CLI. It only discovers the caller-selected engine's
Compose version/projects and checks bounded, escaped output; it never creates,
starts, stops, or removes projects:

```sh
cargo test -p ferrum-app --bin ferrum compose::tests::local_compose_discovery_smoke_is_read_only -- --exact --ignored
```

The optional ignored validation smokes require Docker CLI/Compose but no engine.
The single-file smoke creates one controlled Compose YAML and a relative
`./data` directory. The multi-file smoke creates an ordered controlled base and
override pair and verifies the real CLI accepts their relative paths; neither
touches user project files or mutates engine resources:

```sh
cargo test -p ferrum-app --bin ferrum compose::tests::subprocess::local_compose_validation_reads_a_controlled_relative_path_project -- --exact --ignored
cargo test -p ferrum-app --bin ferrum compose::tests::subprocess::local_compose_validation_reads_ordered_temporary_base_and_override_files -- --exact --ignored
```

The optional ignored remote-loader safety smoke uses only a `127.0.0.1`
listener and Git/OCI-shaped `include` references. It passes only when Compose
rejects the references without making a local connection; it contacts no
external host and uses no engine:

```sh
cargo test -p ferrum-app --bin ferrum compose::tests::subprocess::local_compose_validation_rejects_remote_includes_without_network_access -- --exact --ignored
```

See Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
for ordered `-f` merging and path resolution relative to the first/base config
or `--project-directory`, and [`config`](https://docs.docker.com/reference/cli/docker/compose/config) for the validation flags.
The smoke above exercises ordered base/override validation with a real local
Compose CLI and temporary files. It does not test arbitrary projects, engine
operations, or broader CLI/plugin compatibility.

Compose may still read each included project's default `.env` or explicit
`include.env_file`, including paths outside the base directory, under the
logged-in user's permissions. `COMPOSE_DISABLE_ENV_FILE=1` disables only the
selected project's default `.env`; `--no-interpolate` and
`--no-env-resolution` do not confine included-project reads. Remote Git and OCI
loaders are explicitly disabled, so remote include/extends references fail
without fetching. The CLI/plugin remains unsandboxed; validate only trusted
files. Bounded profile selection applies to structural validation, read-only
status, one-shot service logs and live follow, imported-project start/stop/pull/
build, and single-service restart; the declared-service inventory is not
profile-filtered and lists all declarations from selected files. Profiles are
passed as explicit `--profile` arguments and are not inherited from the
environment. Imports remain session-only unless explicitly saved. Saved
registrations use bounded SettingsStore metadata, restore unselected/
unvalidated, and require explicit revalidation; persistence adds no CLI/Engine
request or project-file access.
Compose pull and build use fake-CLI/temp-file fixtures only. Pull regressions
cover exact argv, isolated environment, preflight cancellation, bounded
progress, and uncertain post-spawn outcomes. Build checks exact fixed argv
including spaced paths/profiles, private environment/socket selection,
allowlisted progress, explicit-confirmation cancellation, stale selection,
preflight cancellation, and uncertain submitted cancellation. Shared image
command-runner regressions exercise output overflow and short test-injected
deadline uncertainty. Build audit tests verify static redaction. Neither
operation contacts a live Engine or registry, executes a user project, mutates
an image store, or tests remote contexts. Build uses anonymous registry access
only. Build/up/down, removal, and project-file mutation are not supported except
the confirmed local build slice.

Controlled live-follow fixtures cover exact selected-file/profile/socket argv
and fresh status-before-stream ordering, absent/failed service-membership
refusal, post-status file-size rechecks, split lines, overlong-line recovery,
escaping, bounded queue and rolling output, cancellation, stale-event rejection,
and process-group cleanup. Headless controls test Stop, imported-selection
cancellation, and output retained after EOF. Neither one-shot snapshots nor live
follow read real service logs; no project or Engine resource is mutated.
The engine-free Compose CLI smoke uses owned temporary base/override/include
files and verifies that `config --services` resolves the selected project from
sealed snapshot descriptors while preserving the original project directory.
The ignored snapshot-backed status smoke then uses `prepare_compose_operation`
and the same descriptors for a read-only `compose ps --all --format json`
request on a uniquely named temporary project and the selected local Docker
Unix socket. It creates or changes no Engine resources; on 2026-10-13 it
passed with Docker Engine 29.9.0, Compose 5.6.0, and the `default` context:

```sh
cargo test -p ferrum-app compose::tests::subprocess::local_compose_profile_status_reads_temporary_project_without_mutation -- --exact --ignored --nocapture
```

Compose validation and service listing use no Engine; the opt-in status smoke
is local Docker-only. The native Compose page visual check is unavailable, and
Podman/rootless and remote transports remain unsupported or unvalidated. Issue
#18 remains open/partial; #10 remains blocked. No release/support expansion is
claimed.

The mutation audit tests verify redacted JSONL records for lifecycle, image,
volume, and network outcomes; they also check the 16 MiB hard cap and that
audit files are created privately and reject symlink, hard-link, and non-regular
targets.
Lock-contention coverage holds the audit file lock through a separate open file
descriptor and verifies persistence fails within the bounded wait. No real
engine resource is used. The records are a local Ferrum-request history, not
tamper-proof or proof of final engine state, and do not capture external engine
actions. Separate in-memory lifecycle trace tests check action/outcome fields,
full-ID validation, and omission of untrusted identifiers.
Run the focused audit tests directly:

```sh
cargo test -p ferrum-app --bin ferrum audit::tests
cargo test -p ferrum-app --bin ferrum settings::tests::audit_file_
```

The named regression tests are:
`persists_redacted_audit_records_for_mutation_outcomes`,
`durable_audit_log_stops_at_the_configured_size_limit`,
`audit_persistence_returns_bounded_failure_under_lock_contention`,
`audit_file_is_created_with_private_permissions`,
`audit_file_rejects_symlink_and_hardlink_targets`, and
`audit_file_rejects_non_regular_targets`. This is partial Issue #19 work:
issues #16 and #19 remain partial/open; #10 remains blocked/open for
Podman/rootless and broader endpoint validation. No endpoint/platform support
or release is claimed.

## Accessibility and performance samples

The workspace includes headless AccessKit tests for labels, focus, keyboard
activation, large-inventory virtualization, and stream error/reconnect states.

The Images page has headless `egui_kittest` coverage for navigation, searchable
and virtualized rows, image selection/details, loading/error/empty states,
keyboard interaction, and stale-result rejection. Tag UI coverage checks the
accessible repository/tag fields, bounded target validation, disabled invalid
targets, explicit reassignment confirmation, cancel-without-request behavior,
loading, safe success/error display, single-request gating, stale generation
rejection, and successful-inventory-refresh state. Push UI coverage checks that
the selected image owns the existing local tag, the confirmation snapshots the
image ID and target, cancel submits nothing, anonymous disclosure is explicit,
and the configured-helper checkbox is a one-operation opt-in cleared by cancel
and after the request snapshot. Hidden or invalidated progress retains the
mutation gate until a terminal response; ambiguous outcomes latch it for the
application session. Removal UI checks the selected image ID and known tags,
final-reference and dependency warnings, `noprune=true` parent preservation,
Cancel-without-request, success/error display, and stale invalidation. These
tests use fixed image data and local state/events only; they never contact an
engine or registry or mutate images.

Pull presentation tests use fixed progress values. Tag, push, and removal tests
use fake completion events and do not mutate image resources. Separate opt-in
Engine-level smokes exercise anonymous and synthetic Basic-auth pulls through
the Engine; the fake-helper app smoke separately validates the helper execution
seam. All pull cleanup uses the same validated local Unix socket and verified
full image ID without force or prune, after the generated tag is verified as
sole; a concurrent additional tag makes Docker refuse removal and may leave the
generated image for manual cleanup. These smokes do not exercise the Ferrum UI
or configured production helper/credentials.
No live Ferrum UI pull, tag/removal, or registry-push operation is claimed.
Push tests use fake sockets and headless UI state only; they do not mutate
images or contact a registry. Tests must not pull, tag, push, delete, or
otherwise mutate user images or engines.

Engine image pull, push, tag, removal, and history contract tests use fake Unix
sockets, not a Docker engine or registry. Tag cases cover encoded request
paths/queries, success response, omitted force/auth fields, invalid input
without a connection, and safe typed status rejection. Pull cases cover query
encoding, chunked progress, response/event errors, malformed or oversized input,
and invalid references. Push cases cover the image-ID/tag preflight, exact
explicit tag path, empty anonymous JSON body without `X-Registry-Auth`, the
opt-in `X-Registry-Auth` header without a plaintext credential, bounded
progress, reassignment before submission, HTTP rejection, transport loss after
submission as unknown, error-record draining to EOF, and daemon-text redaction.
Removal cases cover exact-ID DELETE with noprune=true, no force/auth
fields, success, invalid input without a connection, and safe status rejection
without response-body exposure. History cases cover exact encoded routing,
allowlisted fields, discarded command/tag/comment data, empty and malformed/API
errors, invalid IDs without a connection, acceptance of the 65,536-row
boundary, and rejection of excess rows. Run the image protocol tests with:

```sh
cargo test -p ferrum-engine image_pull::tests
cargo test -p ferrum-engine image_push::tests
cargo test -p ferrum-engine image_api_tests
cargo test -p ferrum-core image_push_target_tests
```

The separate opt-in ignored
`local_engine_lists_inspects_and_reads_history_for_preinstalled_image_read_only`
test lists and inspects an already-installed image, then reads history through
the selected local Docker Unix socket. It is read-only and does not pull, tag,
push, remove, or modify images. The separate live pull smokes documented above
are the only live image-transfer operations in this guide.

Image pull, push, tag, and removal use the validated local Docker Unix-socket
endpoint. Pulls and pushes are anonymous by default. Fake-helper tests cover
helper protocol/response parsing; fake Unix-engine tests cover authenticated
headers without invoking real helpers or contacting a registry. The ignored
Engine synthetic-auth smoke constructs `RegistryAuth` directly and does not test
the helper path. A separate ignored app smoke runs the fake helper through
`authorization_with_binary`, then passes its result to a real local Engine and
the shared test-only loopback registry. It passed on 2026-10-14 on Fedora Linux
44 x86_64 with Docker Engine 29.9.0/API 1.56 through the default local Unix
context:

```sh
env -u DOCKER_HOST DOCKER_CONTEXT=default cargo test -p ferrum-app --bin ferrum registry_auth::tests::fake_helper_authenticates_generated_loopback_pull_through_local_engine -- --exact --ignored
```

The helper binary and Basic username/secret are generated and synthetic. The
app smoke pulls a generated image into the test Engine and verifies its full
SHA-256 ID and sole generated tag before arming cleanup. Explicit cleanup and
Drop use the same validated local Unix socket as the EngineClient and remove
only the retained full image ID with `docker image rm --no-prune` and no force.
If another tag is added, Docker refuses non-forced ID removal and the generated
image may remain for manual cleanup. No user image is intentionally targeted.
Registry requests stay on `127.0.0.1` and HTTP/insecure-registry only; no TLS,
saved Docker registry credential config, or external registry is used. The test
does not exercise Settings enum selection or per-pull UI consent. Both
helper-auth paths remain narrow evidence only; Issues #16 and #19 remain
partial/open. Ferrum does not read saved Docker credential entries; Buildx
remains credential-free.

Manual release-mode performance samples are opt-in and have no timing
thresholds. See the [performance and accessibility evidence](../performance.md)
for the host, dataset, methodology, results, and known validation limits.
