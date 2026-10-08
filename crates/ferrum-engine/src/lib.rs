#![warn(missing_docs)]

//! Docker-compatible API access kept separate from Ferrum's UI and domain model.

mod contexts;

pub use contexts::{ContextTransport, DockerContext};

#[cfg(target_os = "linux")]
use std::{env, path::PathBuf, time::Duration};

#[cfg(target_os = "linux")]
use http_body_util::{BodyExt, Empty, Limited};
#[cfg(target_os = "linux")]
use hyper::{Method, Request, body::Bytes};
#[cfg(target_os = "linux")]
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
#[cfg(target_os = "linux")]
use hyperlocal::{UnixConnector, Uri};
#[cfg(target_os = "linux")]
use tokio::time::timeout;

#[cfg(target_os = "linux")]
use contexts::{
    ContextCommandError, StartupSource, current_cli_context, discover_contexts,
    resolve_startup_source, valid_context_name,
};

#[cfg(target_os = "linux")]
use bollard::API_DEFAULT_VERSION;
use ferrum_core::{ContainerDetails, ContainerMount, ContainerState, ContainerSummary};
use serde::{Deserialize, de::DeserializeOwned};
use thiserror::Error;

#[cfg(target_os = "linux")]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(target_os = "linux")]
const MAX_INSPECT_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_LIST_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_VERSION_RESPONSE_BYTES: usize = 64 * 1024;

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionResponse {
    version: Option<String>,
    api_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContainerListItem {
    id: Option<String>,
    names: Option<Vec<String>>,
    image: Option<String>,
    state: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectResponse {
    id: Option<String>,
    name: Option<String>,
    image: Option<String>,
    created: Option<String>,
    state: Option<InspectState>,
    restart_count: Option<i64>,
    mounts: Option<Vec<InspectMount>>,
    config: Option<InspectConfig>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectState {
    status: Option<String>,
    exit_code: Option<i64>,
    started_at: Option<String>,
    finished_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectMount {
    #[serde(rename = "Type")]
    typ: Option<String>,
    source: Option<String>,
    destination: Option<String>,
    mode: Option<String>,
    #[serde(rename = "RW")]
    rw: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectConfig {
    image: Option<String>,
}
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
    /// The engine returned an invalid, oversized, or unavailable inspection response.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe container details.")]
    InspectResponseUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The selected context returned an invalid or unavailable inspection response.
    #[cfg(target_os = "linux")]
    #[error("The selected Docker context could not return safe container details.")]
    ContextInspectResponseUnavailable,
    /// A configured endpoint or socket path cannot be represented safely.
    #[error(
        "The configured local engine endpoint is not valid UTF-8 and cannot be passed to the HTTP client."
    )]
    InvalidEndpointEncoding,
    /// The configured or discovered engine did not answer the API request.
    #[error(
        "The engine at the {endpoint_label} did not respond. Check that it is running and the current user can access it."
    )]
    Unavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine reported an API version that cannot be negotiated safely.
    #[error("The engine at the {endpoint_label} reported an invalid API version.")]
    InvalidApiVersion {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
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
    info: EngineInfo,
    api_version: String,
    selected_context: Option<String>,
    #[cfg(target_os = "linux")]
    socket_path: PathBuf,
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
                    let path = host
                        .strip_prefix("unix://")
                        .filter(|path| !path.is_empty())
                        .map(PathBuf::from)
                        .ok_or(EngineError::UnsupportedRemoteTransport)?;
                    return Self::connect_unix_socket(&path, endpoint_label).await;
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
        #[cfg(target_os = "linux")]
        {
            let endpoint_label = self.info.endpoint_label;
            let path = format!("/v{}/containers/json?all=true", self.api_version);
            let containers: Vec<ContainerListItem> = request_json(
                &self.socket_path,
                &path,
                MAX_LIST_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|()| {
                if self.selected_context.is_some() {
                    EngineError::ContextInventoryRequest
                } else {
                    EngineError::InventoryRequest { endpoint_label }
                }
            })?;
            containers
                .into_iter()
                .map(|container| map_container_summary(container, endpoint_label))
                .collect()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(EngineError::UnsupportedPlatform)
        }
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
        #[cfg(target_os = "linux")]
        {
            let path = inspect_request_path(&self.api_version, id)
                .map_err(|()| self.inspection_error())?;
            let response: InspectResponse = request_json(
                &self.socket_path,
                &path,
                MAX_INSPECT_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|()| self.inspection_error())?;
            map_container_details(response, self.info.endpoint_label)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    #[cfg(target_os = "linux")]
    async fn connect_unix_socket(
        path: &std::path::Path,
        endpoint_label: &'static str,
    ) -> Result<Self, EngineError> {
        path.to_str().ok_or(EngineError::InvalidEndpointEncoding)?;
        let socket_path = path.to_path_buf();
        let response: VersionResponse = request_json(
            &socket_path,
            "/version",
            MAX_VERSION_RESPONSE_BYTES,
            REQUEST_TIMEOUT,
        )
        .await
        .map_err(|()| EngineError::Unavailable { endpoint_label })?;
        let version = response
            .version
            .filter(|value| !value.trim().is_empty())
            .ok_or(EngineError::MissingEngineVersion { endpoint_label })?;
        let api_version = response
            .api_version
            .filter(|value| !value.trim().is_empty())
            .ok_or(EngineError::MissingApiVersion { endpoint_label })?;
        let client_api_version = API_DEFAULT_VERSION.to_string();
        let negotiated_api_version = negotiate_api_version(&client_api_version, &api_version)
            .ok_or(EngineError::InvalidApiVersion { endpoint_label })?;

        Ok(Self {
            info: EngineInfo {
                version,
                api_version,
                endpoint_label,
            },
            api_version: negotiated_api_version,
            selected_context: None,
            socket_path,
        })
    }

    #[cfg(target_os = "linux")]
    fn inspection_error(&self) -> EngineError {
        if self.selected_context.is_some() {
            EngineError::ContextInspectResponseUnavailable
        } else {
            EngineError::InspectResponseUnavailable {
                endpoint_label: self.info.endpoint_label,
            }
        }
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

#[cfg(target_os = "linux")]
async fn request_json<T: DeserializeOwned>(
    socket_path: &std::path::Path,
    request_path: &str,
    max_response_bytes: usize,
    request_timeout: Duration,
) -> Result<T, ()> {
    timeout(request_timeout, async {
        let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let request = Request::builder()
            .method(Method::GET)
            .uri(Uri::new(socket_path, request_path))
            .body(Empty::<Bytes>::new())
            .map_err(|_| ())?;
        let response = client.request(request).await.map_err(|_| ())?;
        if !response.status().is_success() {
            return Err(());
        }
        let body = Limited::new(response.into_body(), max_response_bytes)
            .collect()
            .await
            .map_err(|_| ())?
            .to_bytes();
        serde_json::from_slice(&body).map_err(|_| ())
    })
    .await
    .map_err(|_| ())?
}

#[cfg(target_os = "linux")]
fn inspect_request_path(api_version: &str, id: &str) -> Result<String, ()> {
    if id.is_empty() || id.len() > 4096 {
        return Err(());
    }
    let mut path = String::with_capacity(api_version.len() + id.len().saturating_mul(3) + 32);
    path.push_str("/v");
    path.push_str(api_version);
    path.push_str("/containers/");
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            path.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(path, "%{byte:02X}").map_err(|_| ())?;
        }
    }
    path.push_str("/json");
    Ok(path)
}

fn negotiate_api_version(client: &str, server: &str) -> Option<String> {
    fn components(version: &str) -> Option<(u64, u64)> {
        let (major, minor) = version.split_once('.')?;
        if major.is_empty()
            || minor.is_empty()
            || !major.bytes().all(|byte| byte.is_ascii_digit())
            || !minor.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        Some((major.parse().ok()?, minor.parse().ok()?))
    }
    let client_components = components(client)?;
    let server_components = components(server)?;
    Some(if client_components <= server_components {
        client.to_owned()
    } else {
        server.to_owned()
    })
}

fn map_container_summary(
    container: ContainerListItem,
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
    let state = container.state.map(|value| ContainerState::parse(&value));
    Ok(ContainerSummary {
        id,
        name,
        image: container.image,
        state,
        status: container.status,
    })
}
fn map_container_details(
    response: InspectResponse,
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
            let state = state
                .status
                .filter(|status| !status.is_empty())
                .map(|status| ContainerState::parse(&status));
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

    fn response(json: &str) -> InspectResponse {
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
    fn preserves_unrecognized_status_and_maps_other_fields() {
        let details = map_container_details(
            response(
                r#"{"Id":"container-id","Name":"/worker","Image":"sha256:digest","Created":"created","State":{"Status":"checkpointing","ExitCode":7,"StartedAt":"started","FinishedAt":"finished"},"RestartCount":2,"Mounts":[],"Config":{"Image":"registry/worker:latest","Env":["SECRET=value"]}}"#,
            ),
            ENDPOINT,
        )
        .expect("id is present");

        assert_eq!(details.id, "container-id");
        assert_eq!(details.name.as_deref(), Some("worker"));
        assert_eq!(
            details.state,
            Some(ContainerState::Unknown("checkpointing".to_owned()))
        );
        assert_eq!(details.image.as_deref(), Some("registry/worker:latest"));
        assert_eq!(details.created.as_deref(), Some("created"));
        assert_eq!(details.restart_count, Some(2));
        assert_eq!(details.exit_code, Some(7));
        assert_eq!(details.started_at.as_deref(), Some("started"));
        assert_eq!(details.finished_at.as_deref(), Some("finished"));
        assert_eq!(details.mounts, Some(Vec::new()));
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
    #[test]
    fn maps_list_summary_without_exposing_command_or_labels() {
        let item: ContainerListItem = serde_json::from_str(
            r#"{"Id":"container-id","Names":["/web"],"Image":"registry/app:tag","State":"running","Status":"Up 5 minutes","Command":"secret-command","Labels":{"credential":"secret-label"}}"#,
        )
        .expect("list item should deserialize");

        let summary = map_container_summary(item, ENDPOINT).expect("id is present");
        assert_eq!(summary.id, "container-id");
        assert_eq!(summary.name, "web");
        assert_eq!(summary.image, Some("registry/app:tag".to_owned()));
        assert_eq!(summary.state, Some(ContainerState::Running));
        assert_eq!(summary.status, Some("Up 5 minutes".to_owned()));

        let debug = format!("{summary:?}");
        assert!(!debug.contains("secret-command"));
        assert!(!debug.contains("secret-label"));
        assert!(!debug.contains("credential"));
    }

    #[test]
    fn negotiates_the_lower_valid_api_version() {
        assert_eq!(
            negotiate_api_version("1.41", "1.45"),
            Some("1.41".to_owned())
        );
        assert_eq!(
            negotiate_api_version("1.49", "1.45"),
            Some("1.45".to_owned())
        );
        assert_eq!(negotiate_api_version("1.invalid", "1.45"), None);
        assert_eq!(negotiate_api_version("1.49", "bad"), None);
    }

    #[cfg(target_os = "linux")]
    mod request_json_tests {
        use super::*;
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::UnixListener,
        };

        async fn fixture(
            response: Vec<u8>,
        ) -> (
            std::path::PathBuf,
            tokio::task::JoinHandle<()>,
            std::path::PathBuf,
        ) {
            let directory = std::env::temp_dir().join(format!(
                "ferrum-request-json-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after the Unix epoch")
                    .as_nanos()
            ));
            std::fs::create_dir(&directory).expect("temporary directory should be created");
            let socket_path = directory.join("engine.sock");
            let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).await;
                stream
                    .write_all(&response)
                    .await
                    .expect("fixture response should be written");
            });
            (socket_path, server, directory)
        }

        async fn cleanup(
            socket_path: &std::path::Path,
            server: tokio::task::JoinHandle<()>,
            directory: &std::path::Path,
        ) {
            server.await.expect("fixture server should finish");
            std::fs::remove_file(socket_path).expect("socket should be removed");
            std::fs::remove_dir(directory).expect("temporary directory should be removed");
        }

        fn chunked(body: &[u8]) -> Vec<u8> {
            let body = std::str::from_utf8(body).expect("fixture JSON is UTF-8");
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{body}\r\n0\r\n\r\n",
                body.len()
            )
            .into_bytes()
        }

        #[tokio::test]
        async fn accepts_chunked_response_and_deserializes_body() {
            let (socket, server, directory) = fixture(chunked(br#"{"value":"decoded"}"#)).await;
            let result: Result<serde_json::Value, ()> =
                request_json(&socket, "/version", 1024, Duration::from_secs(1)).await;
            cleanup(&socket, server, &directory).await;
            assert_eq!(
                result.expect("chunked body should decode")["value"],
                "decoded"
            );
        }

        #[tokio::test]
        async fn rejects_response_over_body_limit() {
            let body = br#"{"value":"response exceeds limit"}"#;
            let (socket, server, directory) = fixture(chunked(body)).await;
            let result: Result<serde_json::Value, ()> =
                request_json(&socket, "/version", 8, Duration::from_secs(1)).await;
            cleanup(&socket, server, &directory).await;
            assert!(result.is_err(), "oversized body must be rejected");
        }

        #[tokio::test]
        async fn rejects_delayed_body_after_timeout() {
            let directory = std::env::temp_dir().join(format!(
                "ferrum-request-json-timeout-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after the Unix epoch")
                    .as_nanos()
            ));
            std::fs::create_dir(&directory).expect("temporary directory should be created");
            let socket = directory.join("engine.sock");
            let listener = UnixListener::bind(&socket).expect("Unix socket should bind");
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).await;
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .expect("headers should be written");
                tokio::time::sleep(Duration::from_millis(40)).await;
                let _ = stream.write_all(br#"{"value":"slow"}"#).await;
            });

            let result: Result<serde_json::Value, ()> =
                request_json(&socket, "/version", 1024, Duration::from_millis(10)).await;
            cleanup(&socket, server, &directory).await;
            assert!(result.is_err(), "delayed response body must time out");
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn request_inspect_accepts_chunked_http_and_preserves_unknown_state() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::UnixListener,
        };

        let directory = std::env::temp_dir().join(format!(
            "ferrum-inspect-chunked-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("unique temporary directory should be created");
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let mut request = [0_u8; 2048];
            let _ = stream
                .read(&mut request)
                .await
                .expect("request should be read");
            let body = br#"{"Id":"container-id","State":{"Status":"checkpointing"}}"#;
            let chunks = format!(
                "{:X}\r\n{}\r\n0\r\n\r\n",
                body.len(),
                std::str::from_utf8(body).expect("fixture JSON is UTF-8")
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{chunks}"
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("chunked response should be written");
        });

        let result: Result<InspectResponse, ()> = request_json(
            &socket_path,
            "/v1.45/containers/container-id/json",
            MAX_INSPECT_RESPONSE_BYTES,
            REQUEST_TIMEOUT,
        )
        .await;
        server.await.expect("fixture server should finish");
        std::fs::remove_file(&socket_path).expect("socket should be removed");
        std::fs::remove_dir(&directory).expect("directory should be removed");
        let response = result.expect("chunked inspect response should deserialize");
        let details = map_container_details(response, ENDPOINT).expect("ID is present");
        assert_eq!(
            details.state,
            Some(ContainerState::Unknown("checkpointing".to_owned()))
        );
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
