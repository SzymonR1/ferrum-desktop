# Security policy

Ferrum treats access to a Docker-compatible engine socket or named pipe as privileged. The application must not elevate privileges, relax socket permissions, store plaintext credentials, or send remote engine traffic without explicit secure transport and identity validation.

## Reporting a vulnerability

Please use the private vulnerability-reporting feature for this repository in GitHub Security Advisories when it is available. If that feature is unavailable, contact the repository owner through GitHub before sharing details publicly. Do not include credentials, private keys, or production endpoint secrets in public issues.

## Current scope

The initial application slice performs read-only local container inventory requests. Remote engine connections, credentials, lifecycle operations, Compose, logs, terminal streams, archive handling, managed engines, and release updates are not implemented. See [`docs/security/threat-model.md`](docs/security/threat-model.md) for current risks and planned controls.
