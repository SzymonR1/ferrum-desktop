# Security policy

Ferrum treats access to a Linux Docker-compatible Unix socket as privileged. Windows/macOS engine integration is outside the current scope. The application must not elevate privileges, relax socket permissions, store plaintext credentials, or send remote engine traffic without explicit secure transport and identity validation. The experimental embedded terminal is limited to the selected local socket; terminal output is untrusted, OSC 52 clipboard and title requests are ignored, user clipboard actions are explicit, and terminal contents must not be logged.

## Reporting a vulnerability

Please use the private vulnerability-reporting feature for this repository in GitHub Security Advisories when it is available. If that feature is unavailable, contact the repository owner through GitHub before sharing details publicly. Do not include credentials, private keys, or production endpoint secrets in public issues.

## Current scope

The Linux application provides container inventory, inspection, bounded historical/live logs, state-gated lifecycle actions, image tagging, and an experimental embedded exec terminal over the selected local Docker-compatible Unix socket. Remove requires explicit confirmation and does not force-remove containers or remove volumes. Every image-tag request requires explicit confirmation because Docker Engine v1.56 may reassign an existing `repository:tag`; the image data remains. Tagging changes only the local image-store association and sends no registry credentials or authentication. Remote engine connections, credentials, Compose, archive handling, managed engines, and release updates are not implemented. The terminal does not launch an external terminal; closing its exec session does not stop or remove the container. Issue #10 remains open for Podman/rootless and remote compatibility. See [`docs/security/threat-model.md`](docs/security/threat-model.md) for current risks and controls.

