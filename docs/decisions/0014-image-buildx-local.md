# ADR 0014: Local Docker Buildx image builds

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Ferrum's Linux Images page needs a deliberately limited way to build a local image. Reimplementing Docker context collection would duplicate Docker's `.dockerignore` rules and archive behavior, creating compatibility and security risks. Dockerfile instructions can execute arbitrary builder commands, so this operation must be explicit, locally scoped, and bounded. Registry credentials, proxy secrets, and persistent build output are outside the selected scope.

## Decision

Use the Docker CLI's Buildx plugin with argument-array execution, never a shell. Require the Docker CLI and Buildx plugin in `PATH`; only accept a local directory with a regular root-level `Dockerfile` and a bounded `repository:tag` target. Reject uppercase repository characters client-side and leave complete repository grammar validation to Docker CLI. Invoke Buildx with the selected local context, using Docker CLI's `.dockerignore` and tar-context behavior rather than reproducing those semantics in Ferrum.

Explicitly pin the CLI to `EngineClient::local_socket_path()` with `--host unix://<socket>`. Use a private empty temporary Docker config and isolated `HOME`; remove inherited Docker endpoint/context/TLS/custom-header/API/platform variables and proxy variables. Do not read user credentials or provide registry authentication, build arguments, secrets, SSH access, push, remote contexts, a custom builder, or privileged entitlements. Private base images may fail because authentication is unsupported.

Require explicit confirmation for every build, naming the selected Docker
context or a safe endpoint-source label when no named context is selected,
without displaying the socket path. Explain arbitrary Dockerfile command
execution and possible reassignment of an existing tag. Disable context
switching while the confirmation is open and while a build is active. Stream
output as plain text, sanitize controls and invisible formatting, cap lines and
retained output in memory, and never persist build output to logs or audit
storage. Build outcome tracing is process-local and not durable audit history.
Cancellation is best effort: terminate the Docker CLI process group, then
refresh image inventory after every terminal or cancellation result because
builder execution and image/tag state may be uncertain.

This decision is Linux and local Docker/Buildx only. It does not claim Podman/rootless support or broader endpoint/platform compatibility; issue #10 remains open and blocked. Issues #16 and #19 remain open and ready. No release or broader support claim is made.

## Alternatives

- **Reimplement Docker context filtering and archive creation:** rejected because Ferrum would need to duplicate `.dockerignore` matching and Docker's context/tar semantics, risking divergence and incomplete behavior.
- **Build directly through a remote daemon or remote context:** rejected because remote endpoint identity, credential handling, and context transfer are outside the approved local slice.
- **Use a custom builder or add build credentials/secrets/SSH forwarding:** rejected because these expand the privileged inputs and credential surface beyond the requested scope.
- **Add registry authentication or read the user's Docker configuration:** rejected because builds must not import ambient user credentials or proxy secrets.

## Rationale

Docker CLI Buildx already owns the established context packaging and `.dockerignore` behavior. Pinning its host to the same selected Unix socket used by EngineClient avoids re-resolving a mutable context name. A clean private CLI environment, explicit confirmation, bounded sanitized output, and minimal traces constrain accidental exposure without implying that a Dockerfile is safe to execute.

## Consequences

- Docker CLI and Buildx must be installed and available in `PATH`; missing tooling is reported as unavailable.
- Local directories and root-level Dockerfiles are the only supported build inputs; private bases may fail without authentication.
- Builds may execute arbitrary commands and reassign tags, so each build requires confirmation.
- Cancellation cannot guarantee that builder-side work stopped. Inventory refresh is necessary after every terminal result, and callers must treat request errors or cancellation as potentially state-changing.
- Build output is transient bounded UI data, not a diagnostic or audit log. Build traces contain only static action and outcome.
- Podman/rootless, remote contexts/endpoints, platform selection, push, and broader compatibility remain unsupported or unvalidated. This ADR is proposed and does not close issues #10, #16, or #19.
