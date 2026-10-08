#![warn(missing_docs)]

//! Docker-compatible API access kept separate from Ferrum's UI and domain model.

#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use std::{env, path::PathBuf};

#[cfg(target_os = "linux")]
use bollard::API_DEFAULT_VERSION;
use bollard::{Docker, errors::Error as BollardError, query_parameters::ListContainersOptions};
use ferrum_core::{ContainerState, ContainerSummary};
use thiserror::Error;

#[cfg(target_os = "linux")]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Version information reported by the connected engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineInfo {
    /// Engine software version.
    pub version: String,
    /// Highest API version reported by the engine.
    pub api_version: String,
    /// Safe, non-secret label for the selected local endpoint.
    pub endpoint_label: &'static str,
}

/// A safe, user-facing engine connectivity or response error.
#[derive(Debug, Error)]
pub enum EngineError {
    /// No known local Docker socket exists on this machine.
    #[error(
        "No local Docker socket was found. Start Docker Engine or set DOCKER_HOST to a local Unix socket."
    )]
    NoLocalSocket,
    /// Engine connectivity is intentionally unsupported outside Linux in this release.
    #[error("Ferrum currently supports Linux local-engine connections only.")]
    UnsupportedPlatform,
    /// The configured endpoint requires an unimplemented remote transport.
    #[error(
        "This Linux build supports local Unix sockets only; remote TCP, HTTP, TLS, and SSH endpoints are not enabled."
    )]
    UnsupportedRemoteTransport,
    /// A configured endpoint or socket path cannot be represented safely.
    #[error(
        "The configured local engine endpoint is not valid UTF-8 and cannot be passed to the Docker API client."
    )]
    InvalidEndpointEncoding,
    /// The configured or discovered engine did not answer the API request.
    #[error(
        "The engine at the {endpoint_label} did not respond. Check that it is running and the current user can access it."
    )]
    Unavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// Underlying client error, omitted from user-facing display to avoid leaking endpoint data.
        #[source]
        source: BollardError,
    },
    /// The engine did not report its software version.
    #[error("The engine at the {endpoint_label} did not report its software version.")]
    MissingEngineVersion {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine did not report its supported API version.
    #[error("The engine at the {endpoint_label} did not report its API version.")]
    MissingApiVersion {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// A container summary was missing its required engine identifier.
    #[error(
        "The engine at the {endpoint_label} returned a container without an identifier; the inventory could not be displayed safely."
    )]
    MissingContainerId {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine failed to return its container inventory.
    #[error(
        "The engine at the {endpoint_label} could not return its container inventory. Check its health and access permissions."
    )]
    InventoryRequest {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// Underlying client error, omitted from user-facing display to avoid leaking endpoint data.
        #[source]
        source: BollardError,
    },
}

/// An asynchronous connection to one local Docker-compatible engine.
#[derive(Clone)]
pub struct EngineClient {
    docker: Docker,
    info: EngineInfo,
}

impl EngineClient {
    /// Connects to a local Docker-compatible engine on Linux. Remote transports
    /// and non-Linux engine support are out of scope for this implementation.
    ///
    /// `DOCKER_HOST`, when set, must identify a local Unix socket. Otherwise
    /// the default search checks `$XDG_RUNTIME_DIR/docker.sock` before
    /// `/var/run/docker.sock`. All API requests have an eight-second timeout.
    /// Socket permissions are left to the operating system; this method never
    /// elevates privileges.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unsupported platforms/endpoints, missing or
    /// inaccessible sockets, failed API negotiation, or incomplete version data.
    pub async fn connect_default() -> Result<Self, EngineError> {
        #[cfg(not(target_os = "linux"))]
        {
            return Err(EngineError::UnsupportedPlatform);
        }

        #[cfg(target_os = "linux")]
        {
            if let Some(host) = env::var_os("DOCKER_HOST") {
                let host = host
                    .into_string()
                    .map_err(|_| EngineError::InvalidEndpointEncoding)?;
                if is_remote_endpoint(&host) {
                    return Err(EngineError::UnsupportedRemoteTransport);
                }

                let endpoint_label = configured_endpoint_label(&host);
                let docker =
                    Docker::connect_with_defaults().map_err(|source| EngineError::Unavailable {
                        endpoint_label,
                        source,
                    })?;
                return Self::from_client(docker, endpoint_label).await;
            }

            let mut last_error = None;
            let mut found_socket = false;

            for (endpoint_label, path) in local_socket_candidates() {
                if !path.exists() {
                    continue;
                }

                found_socket = true;
                match Self::connect_unix_socket(&path, endpoint_label).await {
                    Ok(client) => return Ok(client),
                    Err(error) => last_error = Some(error),
                }
            }

            if let Some(error) = last_error {
                return Err(error);
            }
            if !found_socket {
                return Err(EngineError::NoLocalSocket);
            }

            Err(EngineError::NoLocalSocket)
        }
    }

    /// Returns server version details captured during connection.
    #[must_use]
    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// Returns every container, including stopped containers, from the
    /// connected engine.
    ///
    /// # Errors
    ///
    /// Returns an API error when the engine cannot provide the inventory or a
    /// response omits a container identifier.
    pub async fn list_containers(&self) -> Result<Vec<ContainerSummary>, EngineError> {
        let options = ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let endpoint_label = self.info.endpoint_label;
        let containers = self
            .docker
            .list_containers(Some(options))
            .await
            .map_err(|source| EngineError::InventoryRequest {
                endpoint_label,
                source,
            })?;

        containers
            .into_iter()
            .map(|container| map_container(container, endpoint_label))
            .collect()
    }

    #[cfg(target_os = "linux")]
    async fn connect_unix_socket(
        path: &std::path::Path,
        endpoint_label: &'static str,
    ) -> Result<Self, EngineError> {
        let path = path.to_str().ok_or(EngineError::InvalidEndpointEncoding)?;
        let docker =
            Docker::connect_with_unix(path, REQUEST_TIMEOUT.as_secs(), API_DEFAULT_VERSION)
                .map_err(|source| EngineError::Unavailable {
                    endpoint_label,
                    source,
                })?;
        Self::from_client(docker, endpoint_label).await
    }

    #[cfg(target_os = "linux")]
    async fn from_client(
        docker: Docker,
        endpoint_label: &'static str,
    ) -> Result<Self, EngineError> {
        let docker = docker
            .with_timeout(REQUEST_TIMEOUT)
            .negotiate_version()
            .await
            .map_err(|source| EngineError::Unavailable {
                endpoint_label,
                source,
            })?;
        let server_version = docker
            .version()
            .await
            .map_err(|source| EngineError::Unavailable {
                endpoint_label,
                source,
            })?;
        let version = server_version
            .version
            .filter(|value| !value.trim().is_empty())
            .ok_or(EngineError::MissingEngineVersion { endpoint_label })?;
        let api_version = server_version
            .api_version
            .filter(|value| !value.trim().is_empty())
            .ok_or(EngineError::MissingApiVersion { endpoint_label })?;

        Ok(Self {
            docker,
            info: EngineInfo {
                version,
                api_version,
                endpoint_label,
            },
        })
    }
}

#[cfg(target_os = "linux")]
fn local_socket_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut candidates = Vec::with_capacity(2);
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        candidates.push((
            "user runtime Docker socket",
            PathBuf::from(runtime_dir).join("docker.sock"),
        ));
    }
    candidates.push((
        "system Docker socket",
        PathBuf::from("/var/run/docker.sock"),
    ));
    candidates
}
#[cfg(target_os = "linux")]
fn configured_endpoint_label(host: &str) -> &'static str {
    if host.starts_with("unix://") {
        "configured Unix socket"
    } else {
        "configured endpoint"
    }
}

#[cfg(target_os = "linux")]
fn is_remote_endpoint(host: &str) -> bool {
    ["tcp://", "http://", "https://", "ssh://"]
        .iter()
        .any(|scheme| {
            host.get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

fn map_container(
    container: bollard::models::ContainerSummary,
    endpoint_label: &'static str,
) -> Result<ContainerSummary, EngineError> {
    let id = container
        .id
        .ok_or(EngineError::MissingContainerId { endpoint_label })?;
    let name = container
        .names
        .and_then(|names| names.into_iter().next())
        .map(|name| name.trim_start_matches('/').to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.chars().take(12).collect());
    let state = container
        .state
        .map(|value| ContainerState::parse(value.as_ref()));

    Ok(ContainerSummary {
        id,
        name,
        image: container.image,
        state,
        status: container.status,
    })
}
