# Security policy

Ferrum treats access to a Linux Docker-compatible Unix socket as privileged. Windows/macOS engine integration is outside the current scope. The application must not elevate privileges, relax socket permissions, store plaintext credentials, or send remote engine traffic without explicit secure transport and identity validation.

## Reporting a vulnerability

Please use the private vulnerability-reporting feature for this repository in GitHub Security Advisories when it is available. If that feature is unavailable, contact the repository owner through GitHub before sharing details publicly. Do not include credentials, private keys, or production endpoint secrets in public issues.

## Current scope

The Linux application provides container inventory, inspection, bounded historical/live logs, and state-gated lifecycle actions over the selected local Docker-compatible Unix socket. Remove requires explicit confirmation and does not force-remove containers or remove volumes. Remote engine connections, credentials, Compose, terminal streams, archive handling, managed engines, and release updates are not implemented. See [`docs/security/threat-model.md`](docs/security/threat-model.md) for current risks and controls.
