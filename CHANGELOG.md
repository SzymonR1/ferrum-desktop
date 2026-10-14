# Changelog

All notable project changes are documented here. The project has not published a release.

## Unreleased
- Added static local audit records for embedded terminal exec opens. The
  existing bounded, private JSONL store durably syncs `terminal_exec/exec/started`
  before an exec request; if that write fails, the request is not submitted and
  the UI receives a static failure notice. After open success/failure, a static
  `success`/`request_error` outcome is best-effort; outcome persistence failure
  does not change the terminal result. Only a validated full container ID may
  be included; shell/command, terminal input/output, raw Engine errors,
  credentials, paths, sockets/endpoints, and daemon bodies are excluded. No
  session-content or resize audit is added. Deterministic fake-Unix API coverage
  checks marker ordering and secret-sentinel exclusion. Issue #19 remains
  open/partial; no complete or tamper-proof audit claim is made.
- Added an ignored Linux local-Engine Compose build smoke through Ferrum's
  production `compose_build` worker. It creates a unique temporary Compose
  project and generated image from `FROM scratch` plus one small `COPY` input,
  preflights the exact image reference as absent, and pins the Compose CLI to
  the `EngineClient::connect_default()` local Unix socket. The test has a
  120-second worker bound, requests cancellation on timeout, and inspects image
  inventory after any submitted or uncertain outcome. Cleanup is armed only
  when inventory contains one full SHA-256 image ID whose sole tag is the exact
  generated reference; `DisposableImageTag` removes by that ID on the same
  socket using `--no-prune`, without force, with a five-second child bound.
  Ambiguous inventory or extra tags leave the image untouched for manual
  inspection. This test-owned image mutation passed on 2026-10-14 on Fedora
  Linux 44 x86_64 with Docker Engine 29.9.0/API 1.56, Compose 5.6.0, and Buildx
  0.38.0 through the default local Unix context. It proves one local Compose
  build path only: it does not test page visuals, `down`, registry pulls, or
  Podman. Issue #18 remains open/partial.
- Added confirmed one-service Compose scaling for the current imported,
  explicitly revalidated ordered files/profiles on the selected Linux local
  Docker Unix socket. Validation keeps the selected-profile service list for
  existing lifecycle eligibility and obtains a separate `--profile '*'
  config --services` inventory for all declared services. One all-profile
  `ps --all` snapshot supplies page status and accurate scale counts, including
  stopped containers; revalidation clears prior status/counts and requires a
  fresh refresh after validation before status-gated actions. The UI offers
  one initially blank control per declared service, including inactive-profile
  services, with integer targets 1–32; scale-to-zero and project-wide scaling
  are unavailable. The worker revalidates sealed files with the
  selected profiles, checks all-profile service membership/count, then submits
  exact `up --detach --no-build --pull never --no-deps --scale SERVICE=N
  SERVICE` argv without any global `--profile` flag. Explicit target activation
  does not start same-profile siblings. Confirmation warns that `up` can start
  stopped containers and run configured service code on the selected Engine;
  removal or configuration/image-driven recreation discards writable-layer changes, and
  mounted volumes follow Compose semantics. No build, pull, volume/orphan
  removal, or force recreation is requested. Submitted failures/timeouts remain
  uncertain and require successful status refresh before retry; explicit
  cancellation or page/context invalidation records static
  `scale/not_submitted`. Audit excludes
  service, project, count, path, profile, socket, and output data. The ignored
  live smoke uses a unique project with a profile-gated target and same-profile
  sibling, preflights project/network absence, and uses a preinstalled image
  without mounts/ports/pull/build. It invokes the production project `Stop`
  and `Start` worker only after a successful submitted scale-up and fresh
  inventory confirms the generated target service. It verifies the sole target
  reaches `exited` after Stop and `running` after Start while the sibling
  remains absent. Project-scoped `down` cleanup is attempted only when a fresh
  inventory still contains one or two expected target containers. If scale-up
  is not confirmed successful/submitted or the inventory is ambiguous or has
  unexpected resources, cleanup is skipped for manual inspection. Cleanup
  requests no force, prune, volume removal, or image removal. The expanded
  smoke passed on 2026-10-14 against Fedora Linux 44 x86_64, Docker Engine
  29.9.0/API 1.56, and Compose v5.6.0 via the local `default` Unix-socket
  context (1 passed, 391 filtered); the cleanup succeeded and generated-project
  container inventory was empty. The prior 2026-10-13 run covered scaling
  only. No broader platform support or full Issue #18 completion is claimed.
  Compose audit writes use one 32-entry bounded FIFO; operations fail closed
  before submission when a start marker cannot be queued. Cancellation audit
  remains best-effort if the queue or durable store is unavailable, and shutdown
  attempts a bounded drain of accepted records.
- Added opt-in Linux local-Docker image-pull progress smokes using an
  in-process loopback OCI registry and generated image references. The
  anonymous smoke uses no credentials; a separate smoke sends only a generated
  synthetic Basic credential directly as Engine AuthConfig and verifies the
  fixture accepted its exact authorization. Both use the existing loopback
  HTTP/insecure-registry setup, preflight references, probe a missing manifest,
  and arm cleanup only after inventory confirms a full image ID and its sole
  exact tag. Cleanup is pinned to the same validated local Unix socket and
  removes by verified full image ID without force or prune. If another tag is
  added, Docker refuses non-forced removal and the image may remain for manual
  cleanup. Neither smoke changes daemon configuration. The authenticated smoke
  does not validate helper selection, per-pull UI opt-in, real secrets, TLS,
  external/private registry deployment, or production authentication.
  Issues #16 and #19 remain partial/open; no broader support is claimed.
- Extracted the generated OCI fixture and loopback registry into the
  test-only `ferrum-test-support` workspace crate, shared by Engine and app
  tests as dev-dependencies only. Added an ignored Linux fake-helper-to-real-
  Engine pull smoke that exercises `FakeHelper` through
  `authorization_with_binary` into the current local Docker Unix socket and
  synthetic Basic-auth loopback registry. It probes a missing manifest, checks
  exact synthetic authorization, bounds pull progress, and removes the image
  only by the verified full SHA-256 image ID, after confirming the generated
  tag is its sole tag. Cleanup uses the same validated local Unix socket as
  the `EngineClient`, without force or prune; if another tag is added, Docker
  refuses removal and the generated image may remain for manual cleanup.
  The smoke passed on 2026-10-14 on Fedora Linux 44 x86_64 with Docker Engine
  29.9.0/API 1.56 through the default local Unix context. It imports only a
  generated image and uses synthetic credentials; Settings selection,
  per-pull UI consent, real credentials, TLS, and external registries remain
  unvalidated. Run with:
  `cargo test -p ferrum-app --bin ferrum registry_auth::tests::fake_helper_authenticates_generated_loopback_pull_through_local_engine -- --exact --ignored`.
  Issues #16 and #19 remain partial/open.
- Added confirmed service-scoped Compose pause/unpause for the current imported,
  successfully revalidated ordered files/profiles on the selected Linux local
  Docker Unix socket. Pause is offered only for a declared service with a
  successful current status row exactly `running`; unpause requires exactly
  `paused`. Stopped, unknown, missing, or undeclared services are ineligible.
  Both require successful current status before confirmation and a fresh
  read-only status recheck by the worker after validating the selected-file
  sealed snapshots. Commands target only the validated service using argument
  arrays and the existing isolated Compose environment. Confirmation identifies
  the service, import label, ordered safe basenames, and profiles, explains that
  pause suspends processes and unpause resumes them, and states neither action
  creates or removes containers, networks, or volumes. A post-spawn failure or
  timeout is uncertain; retry remains blocked until status refresh succeeds.
  Audit stores only static pause/unpause action/outcome values, without service,
  path, or project metadata. Controlled fake-CLI/headless tests only; no live
  mutation was run because no disposable Engine is designated, and native
  visual validation was not performed. No Podman, remote, Windows/macOS, or
  full Issue #18/#19 completion claim; Issue #18 remains open/partial.
- Added confirmed Compose **Create and start (local images)** for the current
  imported, successfully revalidated ordered file/profile selection and a
  successful current status refresh on the selected Linux local Docker socket.
  It submits sealed file snapshots
  with fixed `up --detach --no-build --pull never` arguments, creates/starts
  containers and networks and may create missing project volumes. It may
  recreate existing containers when configuration/image changes require it and
  uses local images only: no build, pull, orphan removal, volume removal, or
  force recreation. Project services may write to configured volumes and host
  mounts. Missing images require separate Build/Pull actions; Compose pulls are
  anonymous-only. Confirmation warns about trusted files, resource creation,
  recreation, and the local-image limit.
  Compose project configuration and service code run on the selected Engine
  without a sandbox. The 30-second command records only static `up` and
  `started`, `success`, `not_submitted`, `request_error`, or `uncertain`
  outcomes without paths or project metadata; any post-spawn failure/timeout is
  uncertain. A successful current status snapshot is required before every
  confirmation and keeps the action disabled while status is pending or failed.
  Fake-CLI/headless coverage only; no live mutating test ran because no disposable
  Engine is designated. No Podman, remote, Windows/macOS, or full Issue #18/#19
  support is claimed.
- Hardened confirmed local Compose mutations against selected-file path races.
  Each operation snapshots the explicitly selected ordered YAML files into
  write-sealed Linux memfds, validates and submits using the same descriptors,
  and reuses them for restart status checks while retaining the original base
  project directory. Snapshot failures fail closed and create no named on-disk
  copy; host swap/core dumps remain outside Ferrum's control.
  Recursive include/extends and environment sources, plus Dockerfiles/build
  contexts, remain live trusted inputs and are not sandboxed. Added fake-CLI
  source-replacement coverage and an opt-in Compose CLI smoke using temporary
  files without an Engine.
- Added confirmed image pulls for the current imported, successfully revalidated
  Compose file/profile selection on Linux local Docker Unix sockets using
  `docker --host unix://<selected-socket> compose --progress plain
  --parallel 4 --project-directory <base-parent> --file ... --profile ...
  pull --ignore-buildable`. Compose caps concurrency at four service pulls; the
  process has a two-hour deadline and a combined 1 MiB stdout/stderr cap.
  Exceeding either terminates the CLI with an uncertain image-store result. The
  UI shows a bounded, rate-limited summary of recognized progress phases/counts,
  never raw CLI output or image names. Pulls are anonymous only through private
  Docker config/HOME and a cleared environment; saved Docker credentials and
  credential helpers are not used. Pulls do not build images or start/create
  services; only the Engine image store changes, consuming bandwidth and disk.
  Cancellation kills only Ferrum's CLI process group; daemon-side work may
  continue and image state may be uncertain. Best-effort audit records only
  static `compose_pull`/`pull` and `started`, `success`, `not_submitted`,
  `request_error`, or `uncertain` values, never file/project/service/image/
  credential/output data. Issue #18 remains partial/open.
- Added explicitly confirmed Compose builds for the current imported,
  successfully revalidated ordered files/profiles on a selected Linux local
  Docker Unix socket. The fixed command is
  `docker --host unix://<selected-socket> compose --progress plain --parallel 2
  --project-directory <base-parent> --file ... --profile ... build`; Ferrum adds
  no `--push`, command-line `--ssh`, builder options, or extra command-line build
  args. Compose-file `build.args` remain active and may expose secrets; do not
  store credentials there. Compose-file `build.ssh` may grant SSH-agent access,
  and `build.secrets` may expose configured secret contents to build instructions.
  Inspect both and build only trusted projects. Saved Docker credentials/helpers,
  proxy environment, and user Docker config are not passed.
  Private Docker config/HOME, cleared environment,
  disabled `.env`/remote Git/OCI loaders, Linux child limits, two-hour deadline,
  and combined 1 MiB output cap apply. Only bounded, rate-limited phase/count
  summaries are shown; raw output is not persisted. Builds execute
  project-controlled instructions
  with access to Compose-defined contexts, may read beyond selected YAML, fetch
  remote contexts/base images, execute arbitrary builder/network code, consume
  resources, and change the selected Engine image store; existing local image tags
  may be reassigned. Review `.dockerignore`; build only trusted files; this is not
  sandboxed or path-contained. Anonymous registry access only; credentialed private
  registries may fail.
  Preflight cancellation prevents build submission; post-submit cancellation
  kills only Ferrum's CLI process group and builder/daemon work may continue,
  leaving state uncertain. No automatic retry. Static audit stores only
  `compose_build`/`build` start/outcome values. Issue #18 remains partial/open.
- Added explicit saved Compose registrations through the existing bounded,
  descriptor-pinned SettingsStore. Schema v2 migrates v1/versionless files with
  an empty registry, enforces a 256 KiB serialized settings cap and 32-entry
  limit, and validates labels, ordered absolute UTF-8 paths, and profiles.
  Restored rows are unselected and unvalidated; save and forget are explicit
  local persistence actions with no Compose/Engine operation. Forget retains a
  session-only entry. Invalid persisted registrations are skipped with a static
  SettingsStore, headless Compose UI, and app persistence regressions passed
  locally; no live Compose/Engine access or full Issue #18 acceptance is claimed.
- Added read-only live Compose log follow for one listed service using the
  selected imported/revalidated files, profiles, pinned local Unix socket, and a
  fresh status-membership check. `logs --follow --tail 200 --no-color SERVICE`
  runs in the isolated CLI environment, with a dedicated 16-event backpressure
  queue, 4096-byte raw-line cap, escaped rolling 200-line/256 KiB UI buffer, and
  process-group cancellation on Stop, state/page/context invalidation, and shutdown.
  Follow output is transient, excluded from audit/tracing/files, and is not
  automatically retried; no total stream deadline is claimed. Fake-CLI/headless
  tests only; no real service logs or native visual workflow were validated.
  Issue #18 remains partial/open.
- Corrected the threat model, settings, architecture diagram, and Engine API docs
  to distinguish anonymous-by-default Linux local-Docker pull and push flows
  with separate per-operation helper opt-ins from deferred credential
  persistence, other engines, and remote identity support.
- Extended the confirmed imported-configuration Compose lifecycle slice with
  service-scoped `restart --no-deps SERVICE` on Linux local Docker Unix sockets.
  Restart is offered only beside a service in the current successful status
  snapshot, revalidates the selected ordered files/profiles, then performs a
  fresh read-only `compose ps --all --format json` membership check against the
  same pinned socket before mutation. Failed status or absent membership submits
  no restart. Argument arrays preserve file/profile order and base-parent
  directory; no project label or Engine-reported `Project` metadata authorizes
  scope. Confirmation names the service, imported label, ordered safe file
  basenames, profiles, and action; cancellation submits nothing.
  `--no-deps` keeps the mutation scoped to the selected service; Docker Compose
  restart does not apply configuration changes. Audit records contain only
  static action/outcome values, never service/project/path data. Existing
  single-flight ownership, context gating, stale-result rejection, status
  refresh, and shutdown's preflight/started-audit-before-command invariant are
  retained. Controlled fake-CLI/headless regressions were added; no live
  lifecycle command, user project, service log, or resource mutation was run.
  Trusted local include/extends reads remain unsandboxed under user
  permissions. Issue #18 remains partial/open; Issue #10 remains open/blocked.
- Fixed imported-configuration selection so choosing a session entry restores
  its ordered files/profiles and invalidates stale validation. An unsubmitted
  confirmation is cleared on page invalidation and cannot be confirmed while
  the engine or Compose configuration is unavailable.
- Added transient in-memory Compose configuration imports for a currently
  validated ordered file/profile selection. Unique local display labels (up to
  256 UTF-8 bytes) and absolute file/profile selections are retained for this
  session only, with a 32-entry cap; duplicate/empty/oversized names are
  rejected. Imported configurations appear separately from Engine-discovered
  projects, and their labels or per-container `Project` metadata are not
  treated as canonical identity, file ownership, or authorization. Selecting
  an import cancels in-flight validation/status/log tasks, restores exact file
  order and profiles, and invalidates old results; explicit revalidation is
  required before status/log or lifecycle actions. Import itself starts no
  CLI/Engine operation; neither import nor selection mutates, copies, or
  persists files. Documentation preserves the warning that Compose `include`
  and `extends.file` can read trusted local files outside the explicit selection.
- Added bounded, escaped optional per-container `Project` metadata to the
  read-only Compose status parser and virtualized container rows. Missing values
  render as `Not reported`; rows remain independent, and the label is untrusted
  engine-reported metadata that proves neither file ownership nor authorization
  and does not scope lifecycle actions. Deterministic parser and headless UI
  regressions cover mixed, omitted, invalid, escaped, stale, and cleared values.
  This field was not observed from an engine in this cycle.
- Added explicit Compose profile selection for structural validation,
  read-only status, and one-shot service-log snapshots. Input is
  comma-separated, trims outer whitespace per token, and blank means no
  explicit profiles. The input is limited to 1024 UTF-8 bytes, 16 profiles,
  and 64 bytes per name; empty entries and duplicates are rejected. Names
  follow `[A-Za-z0-9][A-Za-z0-9_.-]+`; standalone `*` enables all profiles. Ferrum
  passes repeated `--profile <name>` argv pairs before `config`, `ps`, or
  `logs`, never via an environment variable, and reuses the selection for the
  status recheck before logs. Invalid or changed selection clears results,
  cancels in-flight work, and requires revalidation. Private Docker config/HOME
  and the cleared environment remain in effect. This profile-selection slice did
  not add persistent project storage, build/pull, live follow, or project-file
  mutation. Imported configurations may use the separately documented start/
  stop slice. Issue #18 remains partial/open.
- Added a bounded read-only declared-service inventory to successful Linux
  Compose validation using the same `config --services` command, selected
  ordered files/profile argv, and isolated CLI environment. The output includes
  every service declared in the files, including profile-gated services
  regardless of the active profile selection. Service data is capped by total
  bytes, row count, and name size; malformed or duplicate output fails with a
  static error, and display names are escaped. The virtualized list distinguishes
  declarations from existing containers and never triggers Engine status/log
  calls. File/profile changes, validation failure/cancellation, and stale
  results cannot retain or restore an obsolete list. The ignored Compose v5.6.0
  temporary-project smoke verifies all declared names, including a service in
  an inactive profile, without Engine access or mutation. Issue #18 remains
  open/partial.
- Added a read-only one-shot Compose service-log snapshot for a listed service.
  Ferrum rechecks membership with fresh status; Compose `--tail 200` is per
  container, while Ferrum keeps the first 200 combined lines/256 KiB without a
  global chronological merge. Output is escaped/transient, cancellation/stale-
  state safe, and not persisted/audited. This snapshot did not add live follow
  or lifecycle operations; Issue #18 remains partial/open.

- Added automatic Linux discovery of the documented rootless Podman user
  socket at `$XDG_RUNTIME_DIR/podman/podman.sock`, after the XDG Docker socket
  and before the system Docker socket. This is socket-path discovery only;
  Podman/runtime compatibility remains unvalidated. Connecting may activate a
  service through the user's existing systemd socket unit; Ferrum does not
  enable or invoke units. Issues #10/#11 remain open.

- Added confirmed Linux local Docker Buildx builds pinned to the selected Unix socket with an empty private CLI config. Confirmations name the selected context or a safe endpoint-source label and lock context switching until completion; no socket path is shown. Client validation rejects uppercase repository characters while Docker CLI validates complete repository grammar. Docker CLI handles `.dockerignore`; credentials, proxy settings, build args/secrets/SSH, push, and remote contexts are excluded, so private bases may fail. Build output is sanitized, bounded, transient, and never persisted; static build outcome metadata is included in the local durable redacted audit when persistence succeeds. Best-effort cancellation refreshes inventory. Issues #16/#19 remain open/ready, #10 open/blocked; no release or broader support claim.
- Buildx confirmation no longer blocks its own start; other image mutations still gate start. Pre-submission rejected or failed starts record only static `not_submitted` outcomes in the existing best-effort audit.
- Buildx process-spawn failures are now audited as `not_submitted`; failures after a child starts remain `request_error`.

- Bound typed Linux Engine inventory arrays to 65,536 items each and count all
  array items in each inventory response against a 262,144-item aggregate
  budget before typed decoding. Excess per-array or aggregate fan-out fails the
  complete inventory without truncation. The 16 MiB body and eight-second
  request limits remain unchanged.

- Added a native Rust workspace with an eframe desktop entry point.
- Added an asynchronous Linux local Docker-compatible inventory and typed inspection path using negotiated API requests.
- Added state-gated container lifecycle actions and bounded Docker container-event synchronization with manual refresh fallback.
- Added application-owned container summaries, connection and empty states, architecture decisions, developer documentation, and CI configuration.
- Added Linux settings persistence for the startup auto-connect preference, with safe migration, private permissions, atomic replacement, and a settings window.
- Recorded the agreed Linux settings-path anchoring design in ADR 0009 and updated the settings, threat-model, and temporary-directory regression guidance. Issues #19 and #10 remain open; no release is claimed.
- Added Linux Docker CLI context catalog and session-only selection for local Unix-socket contexts; remote contexts remain unsupported.
- Added live container filtering and sorting. Podman/rootless behavior remains unvalidated; remote transports are unsupported. Windows named pipes and macOS remain deferred to issue #21/M8.
- Documented and regression-tested explicit rejection of direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH endpoints without local fallback; remote transports remain unsupported under issue #10.
- Added bounded historical and followed container logs in the inspection dialog for local Unix-socket engines, with non-TTY stdout/stderr labels and combined labels for TTY rows, TTY-aware decoding, and in-memory limits.
- Added Copy, plain-text export to a user-selected path (the save dialog defaults to `container-logs.txt` and offers a text-file filter; the extension is not enforced), preserving source labels for retained visible rows, plus Pause/Resume and auto-scroll controls.
- Copy and Export escape terminal control characters in visible container-log rows as literal `\xNN`.
- Hardened Linux visible-log export with sibling-temporary-file replacement: symlinks and non-regular targets remain rejected, new files are private, existing ordinary rwx bits are preserved, and hard-link aliases keep their original contents. Failed writes or replacement leave the prior target intact.
- Added an experimental embedded interactive exec terminal for the Linux local Docker-compatible Unix-socket slice. Terminal output is untrusted; raw OSC strings are capped at 1 KiB before parsing, and after the cap Ferrum discards the tail until the actual BEL, CAN, SUB, or ESC terminator is passed to the parser. This keeps parser memory bounded while a string remains open. OSC 52 clipboard and title requests are ignored, clipboard operations are user initiated, terminal contents are not logged, and closing the session does not stop or remove its container. Podman/rootless and remote compatibility remain open under issue #10.
- Added headless AccessKit coverage and opt-in release-mode measurements for large inventory filtering/sorting and bounded log-buffer throughput; native screen-reader validation remains unverified.
- Added an Images page with case-insensitive search, virtualized listing, and asynchronous image inspection, anonymous public-reference pulls by default, local image tagging, and confirmed image removal through the Linux local Docker Unix socket. Tagging changes only the local repository/tag association; every tag request requires confirmation because Docker Engine v1.56 may reassign an existing `repository:tag`, while the image data remains. Removal confirmation snapshots the selected ID and known tags, warns that removing the final reference may delete image data, and notes that the non-force request can be rejected for images used by containers or with other tags. The request omits `force` and uses `noprune=true` to preserve untagged parent images. Only one image mutation (pull/tag/remove) is in flight at a time. Page/context invalidation ignores stale UI results without aborting or claiming to undo a submitted removal. Headless UI and fake-socket contract coverage never contact a real engine or mutate images; no live pull/tag/remove operation or native image-page visual validation is claimed. Build/push, Podman/rootless, and remote endpoints remain out of scope; issues #16 and #10 remain open.
- Hiding pull progress now suppresses UI updates without aborting Ferrum's response reader; the image-mutation gate stays closed until a terminal event, including after page/context invalidation.
- Added opt-in, pull-only Linux registry authentication using a persisted `SecretService`/`pass`/`None` helper choice and a one-pull UI checkbox that defaults off and resets on start, target/helper changes, or invalidation. Ferrum invokes only the selected Docker credential-helper binary after opt-in, validates Docker Hub/custom registry keys, bounds helper execution to 30 seconds and 64 KiB of stdout, discards helper errors, and never falls back anonymously after explicit helper failure. It does not read Docker config or persist credentials. Engine pulls use Moby-compatible padded URL-safe `X-Registry-Auth` AuthConfig JSON for username/password or identity token; push and Buildx remain credential-free. The local Engine receives the credential and its configured mirrors may forward it. Zeroize protects helper output, secret, JSON, and encoded buffers; HTTP HeaderValue copies cannot be guaranteed wiped. Fake-helper and fake-engine tests do not contact real helpers or registries; no live auth integration is claimed. Issue #16 remains partial/open.
- Added a read-only Volumes page for listing and inspecting selected local Docker Unix-socket volumes on Linux, with virtualized search and asynchronous inspection. It displays allowlisted metadata; the mountpoint is plain text and is not accessed. Volume creation, removal, pruning, modification, connection, and disconnection are unavailable. Podman/rootless and remote endpoints remain unsupported or unvalidated; issue #17 remains open.
- Added read-only per-volume usage through `GET /v{api}/system/df?verbose=true&type=volume` on the selected local Docker Unix socket. Ferrum reports `Size` in bytes and `RefCount` as a point-in-time container reference count, distinguishes zero from unavailable negative or missing values, and accepts usage only for exact volume names in the inventory. The usage request has a scoped 30-second total request deadline and a 16 MiB response limit; only volume-usage calls receive this longer bound, while volume list and inspect retain their existing eight-second timeout. Labels/options are not retained and mountpoints are not accessed. A zero reference count is not a deletion guarantee. Podman/rootless, remote endpoints, Windows, and macOS remain unsupported or unvalidated; issues #17 and #10 remain open, and this is not a release claim.
- Added read-only local volume list/inspect/usage coverage. An opt-in ignored live test requires `FERRUM_TEST_VOLUME` to name a pre-existing volume and reads its matching usage data. The test does not create, remove, modify, or clean up resources; UI and fake-socket tests do not contact an engine. Native Volumes page visual validation remains outstanding.
- Added a Networks page for local Docker Unix-socket network inventory and inspection on Linux, displaying allowlisted metadata and an optional attached-container count. Endpoint values and other non-allowlisted network fields are discarded; Podman/rootless and remote endpoints remain unvalidated, and issues #17 and #10 remain open.
- Added name-only default-bridge network creation through the negotiated API endpoint. Ferrum sends only `Name`, exposes no driver/IPAM/options controls, and refreshes inventory after creation; native UI click/visual validation remains unverified.
- Added bounded fake-Unix HTTP and headless UI coverage for name-only network creation, redacted failures, validation, stale/in-flight results, success/error display, inventory refresh, and context-switch gating. The ignored live create test preflights its unique name, then arms cleanup only after a full ID and exact inventory name match; normal and `Drop` cleanup use the same pinned `EngineClient` and exact ID without name removal or prune. Unconfirmed ownership may leave the network for manual inspection. The read-only list/inspect test remains opt-in.
- Added exact-full-ID network removal through the negotiated Linux local Docker Unix-socket API. The bounded `DELETE /v{api}/networks/{id}` omits force and prune parameters and surfaces typed redacted errors; daemon rejection, transport errors, and timeouts can leave state uncertain.
- Added Networks-page removal for selected custom networks only after a fresh matching inspection reports exactly zero attached containers. Predefined `bridge`, `host`, and `none` networks and unavailable/positive counts are disabled; confirmation names the full ID and network, one mutation is allowed at a time, context switching is blocked while pending, and success refreshes current inventory. Fake-socket and headless state/UI tests cover the request and safeguards; the optional live create/remove test verifies exact ID/name ownership and removes only that ID through the selected pinned `EngineClient`, with a non-forced exact-ID `Drop` fallback. If identity cannot be confirmed, no removal is sent and the network may remain. Issues #17 and #10 remain open; no release is claimed.

- Added structured local tracing events for completed container lifecycle requests, recording action, outcome, and validated full IDs only; raw errors, names, endpoints, and daemon bodies are excluded. Request errors do not prove the engine state remained unchanged. These process events are not a persistent audit store.
- Hardened Linux settings paths with a retained directory descriptor: loads and saves no longer re-resolve parent paths after initialization; existing XDG/HOME symlink aliases are honored and pinned, and no-follow suffix traversal keeps later operations anchored. [ADR 0009](docs/decisions/0009-settings-path-anchoring.md) remains unchanged; [ADR 0010](docs/decisions/0010-settings-directory-permissions.md) supersedes it only for new-directory permissions: each remaining-path component Ferrum creates is requested and normalized to exactly `0700` through its opened `O_NOFOLLOW` descriptor, while pre-existing ancestors remain unchanged and the final app settings directory still becomes `0700` on save. Under umask `0200`, a subprocess regression verifies both created directories at `0700`, the settings file at `0600`, and preservation of existing ancestors; it overrides both `XDG_CONFIG_HOME` and `HOME` with an isolated temporary path and checks the XDG destination. Saves reject existing symlinked or non-regular settings entries, and temporary files are reset to `0600` through their descriptors. This Linux-only behavior leaves issues #19 and #10 open and does not claim a release.
- Hardened Linux settings loads by setting the opened final `ferrum` directory to `0700` before opening settings, and rejecting non-regular or multi-link settings files before changing file permissions or reading. Added regressions proving hard-linked sibling permissions remain unchanged and umask-test symlink roots cannot receive settings; [ADR 0011](docs/decisions/0011-settings-load-permissions.md) records the load-time policy.
- Added ephemeral local tracing outcomes for network create/remove requests with static action and outcome fields, plus a validated 64-character hexadecimal network ID only; names, endpoints, errors, and daemon response bodies are excluded. These process events are not a persistent or tamper-proof audit store.
- Added ephemeral image mutation trace outcomes for `success`, `not_submitted` local validation rejection, and `request_error` results across pull/tag/remove; image IDs, references, tags, endpoints, raw errors, and daemon bodies are excluded. These process events are not persistent or tamper-proof.
- Added confirmed connect/disconnect for selected custom networks on the Linux local Docker Unix-socket path. Controls require complete validated membership IDs and an exact current-inventory container in the `Running` state; stopped, paused, and unknown states are disabled. Container changes invalidate cached state; the Networks refresh loads both inventories and keeps actions disabled until current data is available. Requests send only `Container`, omit endpoint configuration/`Force`, use an eight-second bound, and are not retried. After every result Ferrum refreshes network/container inventory and re-inspects the selected network. Issues #17 and #10 remain open, and this is not a release.
- Added an opt-in ignored live attachment smoke test using a preinstalled `FERRUM_TEST_IMAGE` containing `/bin/sleep`, a uniquely named custom network, and a generated container created with Docker CLI `--host` from `EngineClient::local_socket_path()` plus `--network bridge --pull never`; this pins setup to the connected daemon rather than re-resolving a mutable context name. It starts the container to verify membership, disconnects it, then stops/removes only the owned IDs without force or prune; default tests skip it.
- Extended network mutation traces to record static connect/disconnect outcomes and validated network/container IDs only; names, endpoints, errors, and daemon bodies remain excluded. These events are not persistent or tamper-proof.

- Added name-only volume creation and guarded removal through the negotiated API on Linux's selected local Docker Unix socket. Creation sends only `Name` and uses Docker's default local driver. Removal requires a matching inspection, an exact-name usage snapshot with `RefCount == 0`, and explicit confirmation; it sends no `force` parameter. Context switching is blocked while either volume mutation is pending; after every result, inventory is marked stale and refreshes immediately if Volumes remains open, otherwise on the next visit. Volume prune, modification, connection, and disconnection remain unavailable. Issues #17 and #10 remain open.
- Added bounded fake-socket and headless UI coverage for create/removal validation, visible confirmation/cancel, stale results, zero/positive/unavailable reference-count gates, errors, inventory refresh, context-switch gating, display escaping, and redacted audit outcomes. The ignored live create/remove test verifies a unique owned volume and exercises EngineClient removal on its pinned local Unix socket. Native Volumes-page visual validation remains outstanding; no release is claimed.
- Volume create/remove results emit ephemeral structured process-tracing outcomes only; names, mountpoints, endpoints, error text, and daemon bodies are excluded. These traces are not a durable or tamper-proof audit store.
- Hardened untrusted container-log presentation: C0/C1 controls and selected bidi, zero-width, and invisible formatting characters are escaped before Logs rows, Copy, or Export. The escaped representation counts against the existing 8 MiB retention budget; a line that would exceed it is dropped. This prevents formatting-based display spoofing but does not detect or redact arbitrary log secrets.
- Bounded and hardened engine-controlled container, image, and network display text. Selected controls, bidi/zero-width/invisible-format characters, Unicode tags, and Hangul fillers render as visible escapes, including the image tag-confirmation ID; fields over the 4 KiB raw-or-escaped display limit show `[display text omitted]`. Raw values remain available for matching and ID-based requests. Headless AccessKit regressions verify the displayed values and unchanged mutation IDs; issues #17 and #19 remain open.
- Hardened engine-returned volume-name and optional-detail presentation: selected control, bidi, zero-width, invisible-format, Unicode-tag, and Hangul-filler characters are escaped. The EngineClient omits optional volume metadata whose raw or escaped form exceeds 4 KiB before retaining it; those fields show as unavailable, while the UI independently applies its 4 KiB display cap and uses `[display text omitted]` for over-limit values that reach rendering. Raw names remain unchanged for identity and requests, and the mountpoint is never accessed. This is presentation hardening, not secret detection or redaction; issue #19 remains open, #10 remains open/blocked for Podman/rootless and broader endpoint validation, and native Volumes visual validation remains outstanding. No release is claimed.
- Hardened volume-usage parsing so duplicate/conflicting `VolumeUsage`, `Items`, `Name`, or `UsageData` keys and repeated exact-name records cannot report an ambiguous zero reference count. Such usage remains unavailable and cannot enable removal. Issue #17 remains open.
- Added an anonymous-only image push for one existing local `repository:tag` through the Linux local Docker Unix socket. Confirmation names the image/target and warns that layers and metadata may disclose secrets; Ferrum reinspects the image/tag before submission, reads no registry credentials, and streams bounded sanitized progress. Ambiguous request/stream failures keep image mutations latched for the application session without automatic retry or unlock. Fake-socket and headless UI tests do not contact a registry or mutate real images; no live push is claimed.
- Extended Linux local Docker Compose validation from one selected file to an
  ordered set: choose one base file, then append local overrides in user-chosen
  order (one-based order, base at 1; maximum 16 total). Ferrum passes the base
  first and each override as a separate `--file`; Compose merges in supplied
  order. The base parent is `--project-directory`, anchoring relative paths.
  The UI numbers files and allows removing overrides. Choosing a new base
  clears current overrides so relative-path semantics remain explicit.
  `Validate selected files` reruns validation after an override is removed.
  See Docker's [Compose CLI reference](https://docs.docker.com/reference/cli/docker/compose)
  for ordered merge/path resolution and [`config`](https://docs.docker.com/reference/cli/docker/compose/config)
  for the validation flags. Private-config preflight, structural-only
  `config --quiet --no-interpolate --no-env-resolution`, engine-independent
  The new multi-file CLI smoke uses a controlled temporary base/override pair;
  it passed with Compose v5.6.0 without an engine or user project mutation.
- Bounded Linux Compose resource use: reject selected base-and-override files
  above 16 MiB in aggregate before launching Docker/Compose (exactly 16 MiB is
  accepted), using a point-in-time check that is not atomic against same-UID
  changes. After version preflight, Compose reopens the original selected
  paths for validation. Apply per-process 4 GiB `RLIMIT_AS` and 512 MiB
  `RLIMIT_DATA` ceilings to Compose version, discovery, validation, and status
  subprocesses, retaining lower inherited limits. These per-process limits
  apply while reading selected and included files; they bound each process,
  not aggregate process-tree RSS, and are not a sandbox. The child environment
  sets Go's 384 MiB `GOMEMLIMIT`, a soft target that may be exceeded to avoid
  GC thrashing. Included local `include`/`extends.file` reads remain outside
  the selected-file size check and remain under the logged-in user's
  permissions. Resource exhaustion can produce a generic validation or status failure.
  Issues #18 and #19 remain partial/open; no full issue completion, release,
  or broader support claim is made.
- Clarified the Compose validation trust boundary: included projects may still
  read their default `.env` and explicit `include.env_file` paths outside the
  base directory despite `COMPOSE_DISABLE_ENV_FILE=1` and the
  no-interpolation/no-env-resolution flags. These reads are under the logged-in
  user's permissions, are not path-confined, and require validating only
  trusted Compose files.
- Added read-only Linux local Docker Compose container status for successfully
  validated selected base/override files using `docker compose ps --all
  --format json`, pinned to the selected local Unix socket. The UI displays only
  bounded escaped container name, service, state, and optional health; IDs,
  commands, ports, and unknown fields are discarded. Empty results are distinct
  from query errors, results are manually refreshed, and no project or engine
  resources are mutated. The private environment disables the selected `.env`;
  environment-dependent status may differ from a normal shell invocation.
  Headless/fake-CLI regression coverage and the Compose v5.6.0 temporary-project
  smoke passed on Docker Engine 29.9.0 without creating containers. Issue #18
  remains partial/open; lifecycle, import, and log support remain outstanding.
- Optional ignored validation smokes cover single-file relative paths and an
  ordered temporary base/override pair. The multi-file smoke passed with
  Compose v5.6.0; it contacted no engine, touched no user project, and performed
  no project mutations.
- Moved the Compose file picker to `rfd::AsyncFileDialog`; pending, cancelled, and stale file-selection results are covered by headless state tests.
- Disabled remote Git/OCI `include`/`extends.file` loaders during Compose validation to prevent remote fetches; an opt-in loopback safety test verifies no connection is made.
- Display the detected Compose CLI version from file-validation preflight when no engine context is selected or connected.
- Added read-only Linux local Docker Compose discovery: `docker compose version --short` reports the CLI version, and `docker compose ls --all --format json` lists projects with the listing pinned to the selected local Docker Unix socket. Compose lifecycle and import operations remain unsupported; issue #18 remains partial/open and issue #10 remains blocked/open.
- Added read-only verbose network IPAM usage on the Linux local Docker Unix socket, displaying aggregate point-in-time used/reserved IPs and dynamically available IPs when reported. Subnet keys must parse as IPv4/IPv6 CIDRs and are normalized before duplicate detection; invalid CIDRs or duplicate normalized subnet keys make both aggregate metrics unavailable. Missing, malformed, negative, or overflowing count values and checked-sum overflow make only the affected metric unavailable; the other metric may remain available. Subnet/CIDR and endpoint details are not retained. These are allocation counts, not per-network traffic totals; per-container/interface counters are not aggregated. Podman/rootless and remote compatibility remain unvalidated; issues #17 and #10 remain open.
- Added read-only Linux local Docker Unix-socket image layer history through the negotiated `GET /v{api}/images/{id}/history`, keyed to the exact selected image ID in current inventory. The eight-second request, 16 MiB response body, and 65,536-row limits keep processing bounded; a sequence visitor filters rows during deserialization and rejects excess rows rather than materializing all response rows. Only validated full SHA-256 layer IDs, nonnegative creation timestamps, and nonnegative sizes are retained; `CreatedBy` command text, tags, comments, and other daemon fields are discarded. Added deterministic fake-socket and headless UI coverage for request routing, safe response filtering, loading/empty/error states, stale results, and row-limit boundaries. The opt-in ignored live read-only history smoke passed on 2026-10-11 against Docker Engine 29.9.0/API 1.56 using a preinstalled image; no image mutation or release is claimed. Issue #16 remains open; issue #10 remains open/blocked.
- Added a bounded Linux-local JSONL copy of redacted Ferrum mutation outcomes, with private permissions, per-record sync, a 16 MiB hard cap, and nonblocking interprocess-lock acquisition bounded to 250 ms. Persistent-audit failures or lock contention do not alter engine outcomes. This local history is not tamper-proof and does not complete Issue #19. The durable redacted audit includes image push and Buildx build outcomes after integration; build output remains transient, bounded, and never persisted. It excludes image IDs, references/tags, endpoints, raw errors, daemon bodies, and build output. Issues #16, #18, and #19 remain open/partial; issue #10 remains open/blocked for Podman/rootless and broader endpoint validation. No release or platform/endpoint support is claimed.
- Network connect/disconnect audit outcomes are retained even when invalid IDs are omitted; audit handling does not change action outcomes. Issue #19 remains open/partial.
- Added anonymous-by-default Linux local Docker image-push authentication as a separate per-push opt-in in the existing confirmation. Ferrum uses the selected `SecretService`/`pass` helper only after explicit selection, sends `X-Registry-Auth` only for that request, and fails before Engine submission without anonymous fallback. The helper enum is the only persisted setting; credentials are not read from Docker config or persisted by Ferrum. The Engine receives the credential and configured mirrors may forward it. Anonymous pushes remain unchanged and Buildx remains credential-free. New fake Unix-engine, headless UI, and redacted-audit tests verify the push request and one-operation choice; existing fake-helper tests cover bounded helper protocol handling. No live helper, registry, push, or image mutation was performed. Issue #16 remains partial/open.
- Bounded local Docker container-event subscriptions to the current Unix timestamp so retained pre-subscription history no longer causes a startup refresh storm. The existing initial/reconnect snapshots cover prior state; the second-resolution cutoff may include earlier events from that same second. Fake Unix-stream coverage checks the time boundary, and an end-to-end fake-engine smoke reduced a 256-event backlog from 40 inventory requests in eight seconds to two expected startup/snapshot requests. No user resources were changed; Issue #20 remains open/blocked for native accessibility and remaining performance acceptance.
- Bound local network inspection membership to 16,384 `Containers` entries independently of the 4 MiB response-body limit. Larger maps make the entire inspection unavailable instead of returning a partial membership set. Fake-Unix API tests cover the exact and over-limit boundaries.
