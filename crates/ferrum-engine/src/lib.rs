#![warn(missing_docs)]

//! Docker-compatible API access kept separate from Ferrum's UI and domain model.

mod contexts;

pub use contexts::{ContextTransport, DockerContext};

#[cfg(target_os = "linux")]
use std::{env, path::PathBuf, time::Duration};

#[cfg(target_os = "linux")]
use contexts::{
    ContextCommandError, StartupSource, current_cli_context, discover_contexts,
    resolve_startup_source, valid_context_name,
};

#[cfg(target_os = "linux")]
use bollard::API_DEFAULT_VERSION;
use bollard::{Docker, errors::Error as BollardError, query_parameters::ListContainersOptions};
use ferrum_core::{ContainerDetails, ContainerMount, ContainerState, ContainerSummary};
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
    /// Docker CLI context discovery is required but unavailable.
    #[error(
        "Docker CLI contexts could not be read. Install Docker CLI or check its context configuration."
    )]
    ContextCatalogUnavailable,
    /// The requested context is not present in the validated Docker CLI catalog.
    #[error("Docker context not found. Choose a context reported by the Docker CLI.")]
    UnknownContext,
    /// The selected context uses a transport Ferrum does not support.
    #[error(
        "This Linux build supports local Unix-socket Docker contexts only; remote TCP, HTTP, TLS, and SSH contexts are not enabled."
    )]
    UnsupportedContextTransport,
    /// The selected context does not contain a usable local Unix socket endpoint.
    #[error("The selected Docker context does not provide a valid local Unix socket endpoint.")]
    InvalidContextEndpoint,
    /// The selected context could not return its container inventory.
    #[error(
        "The selected Docker context could not return its container inventory. Check its engine health and access permissions."
    )]
    ContextInventoryRequest,
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
    /// The selected Docker context could not inspect a container.
    #[error(
        "The selected Docker context could not inspect the container. Check its engine health and access permissions."
    )]
    ContextInspectContainerRequest,
    /// The engine failed to inspect a container.
    #[error(
        "The engine at the {endpoint_label} could not inspect the container. Check its health and access permissions."
    )]
    InspectContainerRequest {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// Underlying client error, omitted from user-facing display.
        #[source]
        source: BollardError,
    },
    /// An inspection response omitted its required container identifier.
    #[error("The engine at the {endpoint_label} returned container details without an identifier.")]
    MissingInspectedContainerId {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
}

/// An asynchronous connection to one local Docker-compatible engine.
#[derive(Clone)]
pub struct EngineClient {
    docker: Docker,
    info: EngineInfo,
    selected_context: Option<String>,
}

impl EngineClient {
    /// Connects to a local Docker-compatible engine on Linux. Remote transports
    /// and non-Linux engine support are out of scope for this implementation.
    ///
    /// A non-empty `DOCKER_CONTEXT` takes precedence and must name a local
    /// Unix-socket context. Otherwise `DOCKER_HOST` retains its existing local
    /// Unix-socket behavior; absent that, a current non-default CLI context is
    /// used before checking `$XDG_RUNTIME_DIR/docker.sock` and `/var/run/docker.sock`.
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
            let docker_context = env::var_os("DOCKER_CONTEXT")
                .map(|value| value.into_string())
                .transpose()
                .map_err(|_| EngineError::InvalidEndpointEncoding)?;
            let docker_host = env::var_os("DOCKER_HOST")
                .map(|value| value.into_string())
                .transpose()
                .map_err(|_| EngineError::InvalidEndpointEncoding)?;
            let contexts = if docker_context
                .as_deref()
                .is_some_and(|name| !name.is_empty())
            {
                Some(
                    discover_contexts()
                        .await
                        .map_err(|_| EngineError::ContextCatalogUnavailable)?,
                )
            } else if docker_host.as_deref().is_none_or(str::is_empty) {
                match discover_contexts().await {
                    Ok(contexts) => Some(contexts),
                    Err(ContextCommandError::CliUnavailable) => None,
                    Err(_) => return Err(EngineError::ContextCatalogUnavailable),
                }
            } else {
                None
            };
            let current = if docker_context
                .as_deref()
                .is_some_and(|name| !name.is_empty())
                || docker_host.as_deref().is_some_and(|host| !host.is_empty())
            {
                None
            } else if let Some(contexts) = contexts.as_deref() {
                Some(
                    current_cli_context(contexts)
                        .map_err(|_| EngineError::ContextCatalogUnavailable)?,
                )
            } else {
                None
            };
            match resolve_startup_source(docker_context.as_deref(), docker_host.as_deref(), current)
            {
                StartupSource::ExplicitContext => {
                    return Self::connect_catalog_context(
                        contexts
                            .as_deref()
                            .ok_or(EngineError::ContextCatalogUnavailable)?,
                        docker_context.as_deref().unwrap_or_default(),
                    )
                    .await;
                }
                StartupSource::CurrentContext => {
                    if let Some(current) = current {
                        return Self::connect_context_record(current).await;
                    }
                }
                StartupSource::DockerHost => {
                    let host = docker_host.as_deref().unwrap_or_default();
                    if is_remote_endpoint(host) {
                        return Err(EngineError::UnsupportedRemoteTransport);
                    }
                    let endpoint_label = configured_endpoint_label(host);
                    let docker = Docker::connect_with_defaults().map_err(|source| {
                        EngineError::Unavailable {
                            endpoint_label,
                            source,
                        }
                    })?;
                    return Self::from_client(docker, endpoint_label).await;
                }
                StartupSource::DefaultDiscovery => {}
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

    /// Lists Docker CLI contexts without exposing their endpoint data.
    ///
    /// # Errors
    ///
    /// Returns a safe error if the Docker CLI is unavailable or cannot provide
    /// a valid context catalog.
    pub async fn available_contexts() -> Result<Vec<DockerContext>, EngineError> {
        #[cfg(target_os = "linux")]
        {
            discover_contexts()
                .await
                .map_err(|_| EngineError::ContextCatalogUnavailable)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Connects to one named Docker CLI context when it uses a local Unix socket.
    ///
    /// # Errors
    ///
    /// Returns a safe error when Docker CLI is unavailable, the context name is
    /// unknown, its transport is unsupported, or its engine cannot be reached.
    pub async fn connect_context(name: &str) -> Result<Self, EngineError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            return Err(EngineError::UnsupportedPlatform);
        }
        #[cfg(target_os = "linux")]
        {
            if !valid_context_name(name) {
                return Err(EngineError::UnknownContext);
            }
            let contexts = discover_contexts()
                .await
                .map_err(|_| EngineError::ContextCatalogUnavailable)?;
            Self::connect_catalog_context(&contexts, name).await
        }
    }

    #[cfg(target_os = "linux")]
    async fn connect_catalog_context(
        contexts: &[DockerContext],
        name: &str,
    ) -> Result<Self, EngineError> {
        let context = contexts
            .iter()
            .find(|context| context.name() == name)
            .ok_or(EngineError::UnknownContext)?;
        Self::connect_context_record(context).await
    }

    #[cfg(target_os = "linux")]
    async fn connect_context_record(context: &DockerContext) -> Result<Self, EngineError> {
        match context.transport() {
            ContextTransport::LocalUnix => {}
            ContextTransport::Unsupported => {
                return Err(EngineError::UnsupportedContextTransport);
            }
            ContextTransport::Invalid => return Err(EngineError::InvalidContextEndpoint),
        }
        let path = context
            .socket_path()
            .ok_or(EngineError::InvalidContextEndpoint)?;
        let mut client = Self::connect_unix_socket(path, "selected Docker context").await?;
        client.selected_context = Some(context.name().to_owned());
        Ok(client)
    }

    /// Returns server version details captured during connection.
    #[must_use]
    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// Returns the selected Docker CLI context name, if one is active.
    #[must_use]
    pub fn selected_context_name(&self) -> Option<&str> {
        self.selected_context.as_deref()
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
            .map_err(|source| {
                if self.selected_context.is_some() {
                    EngineError::ContextInventoryRequest
                } else {
                    EngineError::InventoryRequest {
                        endpoint_label,
                        source,
                    }
                }
            })?;

        containers
            .into_iter()
            .map(|container| map_container(container, endpoint_label))
            .collect()
    }
    /// Returns read-only details for a container identified by its engine ID.
    ///
    /// The engine response is reduced to the documented domain fields; secret-
    /// bearing configuration such as environment variables and labels is not
    /// exposed.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if inspection fails or the response omits
    /// the container identifier.
    pub async fn inspect_container(&self, id: &str) -> Result<ContainerDetails, EngineError> {
        let response = self
            .docker
            .inspect_container(id, None)
            .await
            .map_err(|source| {
                if self.selected_context.is_some() {
                    EngineError::ContextInspectContainerRequest
                } else {
                    EngineError::InspectContainerRequest {
                        endpoint_label: self.info.endpoint_label,
                        source,
                    }
                }
            })?;
        map_container_details(response, self.info.endpoint_label)
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
            selected_context: None,
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
        .and_then(normalize_container_name)
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
fn map_container_details(
    response: bollard::models::ContainerInspectResponse,
    endpoint_label: &'static str,
) -> Result<ContainerDetails, EngineError> {
    let id = response
        .id
        .ok_or(EngineError::MissingInspectedContainerId { endpoint_label })?;
    let name = response.name.and_then(normalize_container_name);
    let (state, exit_code, started_at, finished_at) =
        response.state.map_or((None, None, None, None), |state| {
            let exit_code = state.exit_code;
            let started_at = state.started_at;
            let finished_at = state.finished_at;
            let state = state.status.and_then(map_inspected_state);
            (state, exit_code, started_at, finished_at)
        });

    let image = response
        .config
        .and_then(|config| config.image)
        .or(response.image);

    Ok(ContainerDetails {
        id,
        name,
        image,
        created: response.created,
        state,
        restart_count: response.restart_count,
        exit_code,
        started_at,
        finished_at,
        mounts: response.mounts.map(|mounts| {
            mounts
                .into_iter()
                .map(|mount| ContainerMount {
                    kind: mount.typ,
                    source: mount.source,
                    destination: mount.destination,
                    mode: mount.mode,
                    read_write: mount.rw,
                })
                .collect()
        }),
    })
}

fn map_inspected_state(
    status: bollard::config::ContainerStateStatusEnum,
) -> Option<ContainerState> {
    use bollard::config::ContainerStateStatusEnum as Status;

    match status {
        Status::EMPTY => None,
        Status::CREATED => Some(ContainerState::Created),
        Status::RESTARTING => Some(ContainerState::Restarting),
        Status::RUNNING => Some(ContainerState::Running),
        Status::PAUSED => Some(ContainerState::Paused),
        Status::EXITED => Some(ContainerState::Exited),
        Status::REMOVING => Some(ContainerState::Removing),
        Status::DEAD => Some(ContainerState::Dead),
        Status::STOPPING => Some(ContainerState::Stopping),
    }
}

fn normalize_container_name(name: String) -> Option<String> {
    let name = if name.starts_with('/') {
        name.trim_start_matches('/').to_owned()
    } else {
        name
    };
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod container_inspection_tests {
    use super::*;

    const ENDPOINT: &str = "test endpoint";

    fn response(json: &str) -> bollard::models::ContainerInspectResponse {
        serde_json::from_str(json).expect("inspect response should deserialize")
    }

    #[test]
    fn maps_reported_details_without_exposing_other_response_fields() {
        let response = response(
            r#"{
                "Id":"container-id",
                "Name":"/web",
                "Image":"sha256:digest",
                "RestartCount":3,
                "State":{"Status":"running","ExitCode":0,"StartedAt":"started","FinishedAt":"finished"},
                "Mounts":[{"Type":"bind","Source":"/host/data","Destination":"/data","Mode":"ro","RW":false}],
                "Created":"2024-01-02T03:04:05Z",
                "Config":{"Image":"image-ref","Env":["SECRET=value"],"Labels":{"secret":"value"},"Cmd":["secret"]}
            }"#,
        );

        let details = map_container_details(response, ENDPOINT).expect("id is present");
        assert_eq!(
            details,
            ContainerDetails {
                id: "container-id".to_owned(),
                name: Some("web".to_owned()),
                image: Some("image-ref".to_owned()),
                created: Some("2024-01-02T03:04:05Z".to_owned()),
                state: Some(ContainerState::Running),
                restart_count: Some(3),
                exit_code: Some(0),
                started_at: Some("started".to_owned()),
                finished_at: Some("finished".to_owned()),
                mounts: Some(vec![ContainerMount {
                    kind: Some("bind".to_owned()),
                    source: Some("/host/data".to_owned()),
                    destination: Some("/data".to_owned()),
                    mode: Some("ro".to_owned()),
                    read_write: Some(false),
                }]),
            }
        );
        let debug = format!("{details:?}");
        assert!(!debug.contains("SECRET=value"));
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("Cmd"));
    }

    #[test]
    fn prefers_configured_image_reference_and_falls_back_to_digest() {
        let configured = map_container_details(
            response(
                r#"{"Id":"container-id","Image":"sha256:digest","Config":{"Image":"registry/app:tag"}}"#,
            ),
            ENDPOINT,
        )
        .expect("id is present");
        assert_eq!(configured.image.as_deref(), Some("registry/app:tag"));

        let digest_only = map_container_details(
            response(r#"{"Id":"container-id","Image":"sha256:digest","Config":{}}"#),
            ENDPOINT,
        )
        .expect("id is present");
        assert_eq!(digest_only.image.as_deref(), Some("sha256:digest"));
    }

    #[test]
    fn retains_absent_values_and_distinguishes_missing_from_empty_mounts() {
        let absent = response(r#"{"Id":"container-id"}"#);
        let details = map_container_details(absent, ENDPOINT).expect("id is present");
        assert_eq!(details.name, None);
        assert_eq!(details.image, None);
        assert_eq!(details.created, None);
        assert_eq!(details.state, None);
        assert_eq!(details.restart_count, None);
        assert_eq!(details.exit_code, None);
        assert_eq!(details.started_at, None);
        assert_eq!(details.finished_at, None);
        assert_eq!(details.mounts, None);

        let empty_mounts = response(r#"{"Id":"container-id","Mounts":[]}"#);
        assert_eq!(
            map_container_details(empty_mounts, ENDPOINT)
                .expect("id is present")
                .mounts,
            Some(Vec::new())
        );
    }

    #[test]
    fn treats_empty_state_as_unreported() {
        let details = map_container_details(
            response(r#"{"Id":"container-id","State":{"Status":""}}"#),
            ENDPOINT,
        )
        .expect("id is present");

        assert_eq!(details.state, None);
    }

    #[test]
    fn rejects_missing_identifier_and_normalizes_empty_names() {
        let error = map_container_details(response(r#"{"Name":"/ignored"}"#), ENDPOINT)
            .expect_err("inspection ID is required");
        assert!(matches!(
            error,
            EngineError::MissingInspectedContainerId {
                endpoint_label: ENDPOINT
            }
        ));

        let details =
            map_container_details(response(r#"{"Id":"container-id","Name":"///"}"#), ENDPOINT)
                .expect("id is present");
        assert_eq!(details.name, None);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod context_selection_tests {
    use super::*;
    use crate::contexts::parse_context_records;

    #[tokio::test]
    async fn unknown_named_context_is_reported_without_connecting() {
        let catalog = parse_context_records(
            "{\"Name\":\"default\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/unused.sock\"}",
        )
        .expect("the context catalog should parse");
        assert!(matches!(
            EngineClient::connect_catalog_context(&catalog, "missing").await,
            Err(EngineError::UnknownContext)
        ));
    }

    #[tokio::test]
    async fn remote_contexts_are_rejected_without_fallback() {
        let catalog = parse_context_records(
            "{\"Name\":\"remote\",\"Current\":true,\"DockerEndpoint\":\"tcp://engine.example:2376\"}",
        )
        .expect("the context catalog should parse");
        assert!(matches!(
            EngineClient::connect_context_record(&catalog[0]).await,
            Err(EngineError::UnsupportedContextTransport)
        ));
    }
}
