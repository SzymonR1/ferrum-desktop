# Ferrum

Ferrum is a native Rust desktop application for managing existing
Docker-compatible container engines. It does not bundle an engine, install
privileged services, or use Electron or Node.js. Its experimental embedded
terminal does not launch an external terminal.

## Current status

The Linux local-engine implementation provides container inventory, filtering,
sorting, inspection, historical and live container logs, lifecycle actions, and
external-change synchronization. An experimental embedded terminal provides an
interactive exec session for an explicitly selected container. Terminal access
is limited to the selected local Docker-compatible Unix socket; closing a
session does not stop or remove its container. Terminal output is untrusted:
OSC 52 clipboard and title requests are ignored, and clipboard operations are
user initiated. Terminal contents are never logged. Actions are offered only
for safe known container states; Remove requires confirmation. Container-only
event updates use bounded delivery and report when synchronization is
unavailable, while manual refresh remains available. Logs are plain text,
bounded in memory, and stream only from the selected local Unix socket.

Lifecycle request outcomes also emit minimal structured local trace events with
the action and a validated full container ID when available. `request_error`
means Ferrum did not receive success confirmation; the engine state may still
be changing. Raw errors, endpoints, names, and daemon response bodies are
excluded. These events are not a persistent or tamper-proof audit store.

The **Images** page provides image listing and inspection, anonymous pulls by
default with opt-in per-pull Secret Service/pass helper authentication, local
tagging/removal, and anonymous push of one existing local `repository:tag`
through the Linux local Docker Unix socket. Helper credentials are requested
only after the per-pull opt-in, sent to the selected local Engine, and never
persisted by Ferrum; push and Buildx remain credential-free. Treat the helper,
Engine, and its mirror configuration as trusted because the Engine receives
credentials and may forward them according to its configured mirrors. Tagging
requires confirmation because Docker Engine v1.56 may reassign an existing
`repository:tag`; the image data remains. Removal confirmation snapshots the
selected image ID and known tags, warns that removing the final reference can
delete image data, and uses `noprune=true` without `force`. Push reinspects the
selected image ID and tag before submission and requires confirmation warning
that image layers and metadata are sent and may contain secrets. Ferrum holds
one image-mutation gate for pull, push, tag, and removal. Hiding pull/push
progress or invalidating Images/context suppresses stale updates while draining
a live response. If a push request/stream ends ambiguously without a terminal
response, Ferrum keeps the image-mutation gate latched for the rest of the
session; verify registry state before restarting or using another client.
Closing Ferrum does not guarantee daemon-side transfer cancellation. A timeout
or request error does not prove engine or registry state remained unchanged.
Local Docker Buildx builds remain available on Linux but do not use the registry
credential helper; private base-image authentication is unsupported. See the
Images guide for Buildx's isolated configuration and limitations.
Issue [#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) is partial
and remains open; helper-assisted pulls have not been live-engine/registry
validated. Dependency [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10)
remains blocked and open; Podman/rootless and remote image endpoints remain
unsupported or unvalidated. No live build or image mutation is claimed by tests;
native image-page visual validation is not claimed. See the
[Images guide](docs/src/user-guide/images.md).

The **Networks** page lists, inspects, creates, and removes networks through the
selected local Docker Unix socket on Linux. Creation sends only `Name` and uses
Docker's default bridge driver; removal is restricted to selected custom
networks with a fresh matching inspection reporting zero attached containers,
and requires explicit full-ID confirmation. Neither operation uses force or
prune. Connect/disconnect controls are limited to selected local custom
networks whose fresh inspection contains a complete map of validated full IDs
and whose target has an exact full-ID match in the current inventory with
`Running` state. Stopped, paused, and unknown states are disabled. Actions
require confirmation naming the network and container. Attachment requests
send only the validated `Container` ID, no endpoint configuration or force
flag, use an eight-second deadline, and are not retried.
The Networks page refresh reads both inventories; container events and failed
inventory reads keep attachment controls disabled until fresh state is available.
Inspect retains attached IDs only when every `Containers` key is a full 64-character hexadecimal ID; endpoint values are discarded. Aggregate IP allocation counts are shown when available; they are point-in-time values, not per-network traffic totals. Credential access is not provided.
Podman/rootless and remote endpoints remain unsupported or unvalidated under [#10](https://github.com/SzymonR1/ferrum-desktop/issues/10); this is not a release. Native UI click/visual validation remains unverified.
See the [Networks guide](docs/src/user-guide/networks.md).


An X11-backend screenshot captured via XWayland showed actual container names,
images, states, and statuses. A no-engine X11 launch showed a connection error
rather than an empty list. The default app launch on this Wayland-configured
host connected and loaded inventory, but the local desktop inspection provider
does not support native accessibility or screenshots, so the Wayland surface
itself has not been visually verified. UI lifecycle action clicks have not
been visually validated. See the [platform support matrix](docs/src/platform-support.md)
for evidence and limitations. This is not a release or platform-support claim.

## Build and run

```sh
cargo build --workspace
cargo run -p ferrum-app
```

The engine adapter targets Linux local Unix sockets. Startup precedence is an
explicit `DOCKER_CONTEXT` (a local Unix-socket Docker CLI context), a local
`DOCKER_HOST`, the current non-default Docker CLI context, then
`$XDG_RUNTIME_DIR/docker.sock`, `$XDG_RUNTIME_DIR/podman/podman.sock`, and
`/var/run/docker.sock`. Context selection requires Docker CLI. The Podman path
is a rootless user-socket discovery candidate only. If the user has enabled
Podman's systemd socket activation, connecting may activate its API service;
Ferrum does not invoke or enable a systemd unit. Podman runtime/API compatibility
remains unvalidated. Direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH values and
remote Docker contexts return typed unsupported errors without fallback to local
socket discovery. Ferrum does not read remote TLS/SSH credentials or elevate
privileges. Issue #10 remains open for Podman and broader endpoint validation.
Windows and macOS remain deferred to issue #21/M8.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo doc --workspace --no-deps
mdbook build docs
```

A read-only live-engine inventory test uses this exact filter:
```sh
cargo test -p ferrum-engine --test live_engine local_engine_returns_a_well_formed_container_inventory -- --exact --ignored
```
It reads version and inventory and may inspect one existing container; it does not mutate resources.
The ignored local image list/inspect smoke test is read-only and requires
`FERRUM_TEST_IMAGE` to name an image already installed locally; it does not pull
or modify images. Pull, push, tag, and removal behavior is covered only by
fake-socket contract tests and headless UI tests; no live image mutation is
performed.
The ignored opt-in volume list/inspect/usage test requires `FERRUM_TEST_VOLUME` to
name an existing volume on the selected local Docker Unix socket on Linux. It
lists and inspects the volume, then reads its matching usage data; it remains
read-only and never mutates or cleans up resources. The ignored read-only
network list/inspect test requires
`FERRUM_TEST_NETWORK` to name an existing local network. It lists networks and
inspects the matching listed ID; it performs no mutation or cleanup. A separate
ignored live create test preflights that its unique test-owned default bridge
name is absent, then requires a full 64-character hex ID and inventory entry
matching that ID and name before arming cleanup. If ownership cannot be
confirmed, it sends no cleanup request and the created network may remain; once
verified, cleanup uses only that ID in the current local Docker context.

The optional ignored volume create/remove live test is mutating and must run
only against a disposable Docker Engine using a local Unix socket. It preflights
a generated name, verifies the create response/inventory name, and requires
exact-name usage `RefCount == 0` before cleanup. Docker volumes are
name-addressed, not identified by an immutable ID; a concurrent delete/recreate
under the same name cannot be distinguished. These checks are best-effort, not
a stable ownership proof, and cleanup may target a same-name replacement if
another actor races the test. Do not run this test against an Engine containing
valuable user data. If initial name/response checks fail, the test sends no
cleanup request and the generated volume may remain. A Drop fallback uses
argument-array Docker CLI `--host unix://...` on the same pinned socket without
force if the test unwinds before removal submission; there is no retry after
submission.

```sh
cargo test -p ferrum-engine live_volume_create_tests::local_engine_creates_and_removes_unique_volume -- --exact --ignored
```

Default and fake-socket tests do not contact a live engine. Native UI
click/visual validation remains unverified; see the testing guide for exact
commands.


An ignored lifecycle integration test is mutating; see the testing guide for its exact command and prerequisites. It requires a locally present `FERRUM_TEST_IMAGE` containing `/bin/sleep`; tests do not pull images and clean up only the created container by its returned full ID.

## Documentation

- [Build and testing guide](docs/src/developer-guide/building.md)
- [Architecture](docs/src/architecture/overview.md)
- [Security threat model](docs/security/threat-model.md)
- [Platform support](docs/src/platform-support.md)
- [Architecture decisions](docs/decisions/)
- [Embedded terminal guide](docs/src/user-guide/terminal.md)
- [Networks guide](docs/src/user-guide/networks.md)
- [Volumes guide](docs/src/user-guide/volumes.md)
- [Performance and accessibility evidence](docs/src/performance.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

No release artifacts or installers have been published.