#![warn(missing_docs)]

//! Docker-compatible API access kept separate from Ferrum's UI and domain model.

mod contexts;
#[cfg(target_os = "linux")]
mod exec;
#[cfg(target_os = "linux")]
mod image_pull;
#[cfg(target_os = "linux")]
mod logs;

pub use contexts::{ContextTransport, DockerContext};
#[cfg(target_os = "linux")]
pub use exec::{ExecResizeHandle, ExecSession, TerminalSize};
#[cfg(target_os = "linux")]
pub use image_pull::ImagePullStream;
#[cfg(target_os = "linux")]
pub use logs::{ContainerLogStream, ContainerLogsOptions};
#[cfg(target_os = "linux")]
use std::{env, path::PathBuf, time::Duration};

#[cfg(target_os = "linux")]
use http_body_util::{BodyExt, Empty, Full, Limited};
#[cfg(target_os = "linux")]
use hyper::body::Incoming;
#[cfg(target_os = "linux")]
use hyper::{Method, Request, body::Bytes};
#[cfg(target_os = "linux")]
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
#[cfg(target_os = "linux")]
use hyperlocal::{UnixConnector, Uri};
#[cfg(target_os = "linux")]
use tokio::time::timeout;

#[cfg(target_os = "linux")]
use bollard::API_DEFAULT_VERSION;
#[cfg(target_os = "linux")]
use contexts::{
    ContextCommandError, StartupSource, current_cli_context, discover_contexts,
    resolve_startup_source, valid_context_name,
};

use ferrum_core::{
    ContainerAction, ContainerChange, ContainerDetails, ContainerMount, ContainerState,
    ContainerSummary, ImageDetails, ImageSummary, NetworkDetails, NetworkSummary, VolumeDetails,
    VolumeSummary, VolumeUsage, is_untrusted_display_character,
};
#[cfg(target_os = "linux")]
use serde::Serialize;
use serde::{
    Deserialize,
    de::{DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use thiserror::Error;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestJsonError {
    RequestBuild,
    Transport,
    UnsuccessfulStatus,
    BodyRead,
    BodyTooLarge,
    JsonDecode,
    Timeout,
}
#[cfg(target_os = "linux")]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(target_os = "linux")]
const VOLUME_USAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "linux")]
const LIFECYCLE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "linux")]
const MAX_INSPECT_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_LIST_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_VERSION_RESPONSE_BYTES: usize = 64 * 1024;
#[cfg(target_os = "linux")]
const MAX_NETWORK_NAME_BYTES: usize = 255;
#[cfg(target_os = "linux")]
const MAX_NETWORK_CREATE_BODY_BYTES: usize = 1024;
#[cfg(target_os = "linux")]
const MAX_NETWORK_CREATE_RESPONSE_BYTES: usize = 64 * 1024;
#[cfg(target_os = "linux")]
const MAX_VOLUME_CREATE_REQUEST_BYTES: usize = 16 * 1024;
#[cfg(target_os = "linux")]
const MAX_VOLUME_CREATE_RESPONSE_BYTES: usize = 64 * 1024;

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

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageListItem {
    id: Option<String>,
    repo_tags: Option<Vec<String>>,
    created: Option<i64>,
    size: Option<u64>,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageInspectResponse {
    id: Option<String>,
    repo_tags: Option<Vec<String>>,
    created: Option<String>,
    size: Option<u64>,
    architecture: Option<String>,
    os: Option<String>,
    author: Option<String>,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeListResponse {
    volumes: Option<Vec<VolumeResponse>>,
}
#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeResponse {
    name: Option<String>,
    driver: Option<String>,
    scope: Option<String>,
    created_at: Option<String>,
    mountpoint: Option<String>,
}

#[cfg(target_os = "linux")]
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeCreateRequest<'a> {
    name: &'a str,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeUsageData {
    size: Option<i64>,
    ref_count: Option<i64>,
}

#[cfg(target_os = "linux")]
// Walk the SystemDataUsageResponse envelope and retain only the requested
// item's size and reference count; every item name is dropped after matching.
struct VolumeUsageResponseSeed<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> DeserializeSeed<'de> for VolumeUsageResponseSeed<'a> {
    type Value = Option<VolumeUsage>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(VolumeUsageResponseVisitor {
            requested_name: self.requested_name,
        })
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageResponseVisitor<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> Visitor<'de> for VolumeUsageResponseVisitor<'a> {
    type Value = Option<VolumeUsage>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a Docker system data usage response")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut usage = None;
        let mut volume_usage_seen = false;
        let mut ambiguous = false;
        while let Some(field) = map.next_key::<String>()? {
            if field == "VolumeUsage" {
                let value = map.next_value_seed(VolumeUsageInventorySeed {
                    requested_name: self.requested_name,
                })?;
                ambiguous |= volume_usage_seen;
                volume_usage_seen = true;
                if !ambiguous {
                    usage = value;
                }
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(if ambiguous { None } else { usage })
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageInventorySeed<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> DeserializeSeed<'de> for VolumeUsageInventorySeed<'a> {
    type Value = Option<VolumeUsage>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VolumeUsageInventoryVisitor {
            requested_name: self.requested_name,
        })
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageInventoryVisitor<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> Visitor<'de> for VolumeUsageInventoryVisitor<'a> {
    type Value = Option<VolumeUsage>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a volume usage object or null")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut usage = None;
        let mut items_seen = false;
        let mut ambiguous = false;
        while let Some(field) = map.next_key::<String>()? {
            if field == "Items" {
                let value = map.next_value_seed(VolumeUsageItemsSeed {
                    requested_name: self.requested_name,
                })?;
                ambiguous |= items_seen;
                items_seen = true;
                if !ambiguous {
                    usage = value;
                }
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(if ambiguous { None } else { usage })
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageItemsSeed<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> DeserializeSeed<'de> for VolumeUsageItemsSeed<'a> {
    type Value = Option<VolumeUsage>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(VolumeUsageItemsVisitor {
            requested_name: self.requested_name,
        })
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageItemsVisitor<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> Visitor<'de> for VolumeUsageItemsVisitor<'a> {
    type Value = Option<VolumeUsage>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of volume usage items or null")
    }

    fn visit_seq<A>(self, mut items: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut usage = None;
        let mut ambiguous_match = false;
        while let Some(item_usage) = items.next_element_seed(VolumeUsageItemSeed {
            requested_name: self.requested_name,
        })? {
            if let Some(item_usage) = item_usage {
                if usage.is_some() {
                    ambiguous_match = true;
                } else {
                    usage = Some(item_usage);
                }
            }
        }
        if ambiguous_match {
            Ok(Some(VolumeUsage {
                size_bytes: None,
                reference_count: None,
            }))
        } else {
            Ok(usage)
        }
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageItemSeed<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> DeserializeSeed<'de> for VolumeUsageItemSeed<'a> {
    type Value = Option<VolumeUsage>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(VolumeUsageItemVisitor {
            requested_name: self.requested_name,
        })
    }
}

#[cfg(target_os = "linux")]
struct VolumeUsageItemVisitor<'a> {
    requested_name: &'a str,
}

#[cfg(target_os = "linux")]
impl<'de, 'a> Visitor<'de> for VolumeUsageItemVisitor<'a> {
    type Value = Option<VolumeUsage>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a volume usage item")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut matches_requested_name = None;
        let mut name_seen = false;
        let mut requested_name_seen = false;
        let mut usage_data_seen = false;
        let mut ambiguous = false;
        let mut usage_data = None;
        while let Some(field) = map.next_key::<String>()? {
            match field.as_str() {
                "Name" => {
                    let name = map.next_value::<Option<String>>()?;
                    let matches = name.as_deref() == Some(self.requested_name);
                    ambiguous |= name_seen;
                    name_seen = true;
                    requested_name_seen |= matches;
                    matches_requested_name = Some(matches);
                    if !matches {
                        usage_data = None;
                    }
                }
                "UsageData" if matches_requested_name == Some(false) => {
                    map.next_value::<IgnoredAny>()?;
                }
                "UsageData" => {
                    let value = map.next_value::<Option<VolumeUsageData>>()?;
                    ambiguous |= usage_data_seen;
                    usage_data_seen = true;
                    if !ambiguous {
                        usage_data = value;
                    }
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }

        let usage = if requested_name_seen && ambiguous {
            Some(VolumeUsage {
                size_bytes: None,
                reference_count: None,
            })
        } else if matches_requested_name == Some(true) {
            Some(VolumeUsage {
                size_bytes: usage_data
                    .as_ref()
                    .and_then(|usage| usage.size)
                    .and_then(|size| u64::try_from(size).ok()),
                reference_count: usage_data
                    .and_then(|usage| usage.ref_count)
                    .and_then(|count| u64::try_from(count).ok()),
            })
        } else {
            None
        };
        Ok(usage)
    }
}

#[cfg(target_os = "linux")]
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
    tty: Option<bool>,
}
#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NetworkResponse {
    id: Option<String>,
    name: Option<String>,
    driver: Option<String>,
    scope: Option<String>,
    created: Option<String>,
    internal: Option<bool>,
    attachable: Option<bool>,
    #[serde(rename = "EnableIPv4")]
    enable_ipv4: Option<bool>,
    #[serde(rename = "EnableIPv6")]
    enable_ipv6: Option<bool>,
}
#[cfg(target_os = "linux")]
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct NetworkCreateRequest<'a> {
    name: &'a str,
}

#[cfg(target_os = "linux")]
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct NetworkAttachmentRequest<'a> {
    container: &'a str,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NetworkCreateResponse {
    id: Option<String>,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NetworkInspectResponse {
    #[serde(flatten)]
    network: NetworkResponse,
    #[serde(default)]
    containers: Option<NetworkContainerMembership>,
}

#[cfg(target_os = "linux")]
struct NetworkContainerMembership {
    count: usize,
    ids: Option<Vec<String>>,
}

#[cfg(target_os = "linux")]
impl<'de> Deserialize<'de> for NetworkContainerMembership {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct MembershipVisitor;
        impl<'de> serde::de::Visitor<'de> for MembershipVisitor {
            type Value = NetworkContainerMembership;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map of attached container endpoints")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut count = 0usize;
                let mut ids = Vec::new();
                let mut valid = true;
                while let Some(id) = map.next_key::<String>()? {
                    map.next_value::<IgnoredAny>()?;
                    count = count.saturating_add(1);
                    if is_full_container_id(&id) {
                        ids.push(id);
                    } else {
                        valid = false;
                    }
                }
                Ok(NetworkContainerMembership {
                    count,
                    ids: valid.then_some(ids),
                })
            }
        }
        deserializer.deserialize_map(MembershipVisitor)
    }
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
    /// The engine could not return a valid, bounded image inventory.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe image information.")]
    ImageInventoryUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// An image summary omitted its required engine identifier.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} returned an image without an identifier.")]
    MissingImageId {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine could not return a valid, bounded volume inventory.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe volume information.")]
    VolumeInventoryUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Volume inspection failed or returned an invalid, oversized response.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe volume details.")]
    VolumeInspectionUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Volume creation failed or returned an invalid or oversized response.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not create or confirm the volume.")]
    VolumeCreationUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The local engine rejected volume creation.
    #[cfg(target_os = "linux")]
    #[error("Volume creation was rejected at the {endpoint_label} (HTTP {status}).")]
    VolumeCreationRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// Volume creation timed out and may already have taken effect.
    #[cfg(target_os = "linux")]
    #[error("Volume creation at the {endpoint_label} timed out; the volume may have been created.")]
    VolumeCreationTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Volume removal failed or the engine returned an invalid response.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not confirm volume removal.")]
    VolumeRemovalUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Volume removal timed out and may already have taken effect.
    #[cfg(target_os = "linux")]
    #[error("Volume removal at the {endpoint_label} timed out; the volume may have been removed.")]
    VolumeRemovalTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The local engine rejected volume removal.
    #[cfg(target_os = "linux")]
    #[error("Volume removal was rejected at the {endpoint_label} (HTTP {status}).")]
    VolumeRemovalRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },

    /// The volume name supplied to an inspection, creation, or removal request is invalid.
    #[cfg(target_os = "linux")]
    #[error("The volume name for the engine request is invalid.")]
    InvalidVolumeName,
    /// The engine could not return valid, bounded volume usage information.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe volume usage information.")]
    VolumeUsageUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine could not return a valid, bounded network inventory.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe network information.")]
    NetworkInventoryUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Network inspection failed or returned an invalid, oversized response.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe network details.")]
    NetworkInspectionUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The network identifier supplied to an inspection or removal request
    /// is invalid.
    #[cfg(target_os = "linux")]
    #[error("The network identifier for the engine request is invalid.")]
    InvalidNetworkId,
    /// The network name supplied for creation is invalid.
    #[cfg(target_os = "linux")]
    #[error(
        "The network name must be non-whitespace, at most 255 UTF-8 bytes, and contain no control characters."
    )]
    InvalidNetworkName,
    /// The engine could not return a valid, bounded network creation response.
    #[cfg(target_os = "linux")]
    #[error("Network creation is unavailable at the {endpoint_label}.")]
    NetworkCreationUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The network creation request exceeded its bounded wait; creation may have succeeded.
    #[cfg(target_os = "linux")]
    #[error(
        "Network creation at the {endpoint_label} timed out; the network may have been created."
    )]
    NetworkCreationTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The local engine rejected the network creation request.
    #[cfg(target_os = "linux")]
    #[error("Network creation was rejected at the {endpoint_label} (HTTP {status}).")]
    NetworkCreationRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// The engine's result for a network removal request could not be confirmed.
    #[cfg(target_os = "linux")]
    #[error(
        "The engine at the {endpoint_label} could not confirm whether the network was removed."
    )]
    NetworkRemovalUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Network removal exceeded its bounded wait; the daemon may have applied it.
    #[cfg(target_os = "linux")]
    #[error(
        "Network removal at the {endpoint_label} timed out; the network may have been removed."
    )]
    NetworkRemovalTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The daemon rejected network removal, for example because the network is now in use.
    #[cfg(target_os = "linux")]
    #[error("Network removal was rejected at the {endpoint_label} (HTTP {status}).")]
    NetworkRemovalRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// A network attachment request could not reach the local engine.
    #[cfg(target_os = "linux")]
    #[error("Network {action} is unavailable at the {endpoint_label}.")]
    NetworkAttachmentUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// Static request action: `connect` or `disconnect`.
        action: &'static str,
    },
    /// A network attachment request exceeded its bounded wait.
    #[cfg(target_os = "linux")]
    #[error(
        "Network {action} at the {endpoint_label} timed out; attachment state may have changed."
    )]
    NetworkAttachmentTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// Static request action: `connect` or `disconnect`.
        action: &'static str,
    },
    /// The local engine rejected the network attachment request.
    #[cfg(target_os = "linux")]
    #[error("Network {action} was rejected at the {endpoint_label} (HTTP {status}).")]
    NetworkAttachmentRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// Static request action: `connect` or `disconnect`.
        action: &'static str,
        /// HTTP response status.
        status: u16,
    },

    /// Image inspection failed or returned an invalid, oversized response.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} could not return safe image details.")]
    ImageInspectionUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The image identifier supplied to an engine request is invalid.
    #[cfg(target_os = "linux")]
    #[error("The image identifier for the engine request is invalid.")]
    InvalidImageId,
    /// The image reference supplied for a pull is invalid.
    #[cfg(target_os = "linux")]
    #[error("The image reference for the pull request is invalid.")]
    InvalidImageReference,
    /// The repository or tag supplied for an image tag operation is invalid.
    #[cfg(target_os = "linux")]
    #[error("The image repository and tag target is invalid.")]
    InvalidImageTagTarget,
    /// The image tag request could not reach the local engine.
    #[cfg(target_os = "linux")]
    #[error("Image tagging is unavailable at the {endpoint_label}.")]
    ImageTagUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The image tag request exceeded its bounded wait; the tag may still change.
    #[cfg(target_os = "linux")]
    #[error("Image tagging at the {endpoint_label} timed out; the tag may still have changed.")]
    ImageTagTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The local engine rejected the image tag request.
    #[cfg(target_os = "linux")]
    #[error("The image tag request was rejected at the {endpoint_label} (HTTP {status}).")]
    ImageTagRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// The image removal request could not reach the local engine.
    #[cfg(target_os = "linux")]
    #[error("Image removal is unavailable at the {endpoint_label}.")]
    ImageRemovalUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Image removal timed out; the image state may have changed.
    #[cfg(target_os = "linux")]
    #[error("Image removal at the {endpoint_label} timed out; image state may have changed.")]
    ImageRemovalTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The local engine safely rejected the image removal request.
    #[cfg(target_os = "linux")]
    #[error("Image removal was rejected at the {endpoint_label} (HTTP {status}).")]
    ImageRemovalRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// The engine could not open the image pull progress stream.
    #[cfg(target_os = "linux")]
    #[error("Image pull progress is unavailable at the {endpoint_label}.")]
    ImagePullUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine rejected the image pull request.
    #[cfg(target_os = "linux")]
    #[error("The image pull request was rejected at the {endpoint_label} (HTTP {status}).")]
    ImagePullRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// Opening the image pull stream exceeded its bounded wait.
    #[cfg(target_os = "linux")]
    #[error("Opening image pull progress at the {endpoint_label} timed out.")]
    ImagePullTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Image pull progress ended, stalled, or returned invalid or oversized data.
    #[cfg(target_os = "linux")]
    #[error("Image pull progress from the {endpoint_label} was interrupted.")]
    ImagePullStreamInterrupted {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine reported that the image pull failed.
    #[cfg(target_os = "linux")]
    #[error("The engine could not pull the requested image.")]
    ImagePullFailed,
    /// An image inspection response omitted its required engine identifier.
    #[cfg(target_os = "linux")]
    #[error("The engine at the {endpoint_label} returned image details without an identifier.")]
    MissingInspectedImageId {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// A lifecycle action could not be sent to the selected engine.
    #[error("The {action} action could not reach the {endpoint_label}; check engine access.")]
    ContainerActionUnavailable {
        /// Action requested by the caller.
        action: ContainerAction,
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// A lifecycle request exceeded its bounded wait; the action may still complete.
    #[error(
        "The {action} action at the {endpoint_label} timed out; the container may still be changing. Refresh to confirm its state."
    )]
    ContainerActionTimedOut {
        /// Action requested by the caller.
        action: ContainerAction,
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The selected engine rejected a lifecycle action.
    #[error(
        "The {action} action was rejected at the {endpoint_label} (HTTP {status}); check container state and permissions."
    )]
    ContainerActionRejected {
        /// Action requested by the caller.
        action: ContainerAction,
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// The container event stream could not be opened.
    #[error("Container events are unavailable at the {endpoint_label}.")]
    ContainerEventsUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The container event stream ended or returned invalid event data.
    #[error("Container events from the {endpoint_label} were interrupted.")]
    ContainerEventsInterrupted {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Log options are invalid; no request was sent.
    #[cfg(target_os = "linux")]
    #[error("Container log options are invalid.")]
    InvalidContainerLogsOptions,
    /// A supplied container identifier is invalid.
    #[cfg(target_os = "linux")]
    #[error("The container identifier for the engine request is invalid.")]
    InvalidContainerId,
    /// A local logs request could not be constructed.
    #[cfg(target_os = "linux")]
    #[error("The container logs request could not be built for the {endpoint_label}.")]
    ContainerLogsRequestFailed {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The engine rejected a container log request.
    #[cfg(target_os = "linux")]
    #[error("Container logs are unavailable at the {endpoint_label} (HTTP {status}).")]
    ContainerLogsUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// The engine did not return log response headers within the bounded wait.
    #[cfg(target_os = "linux")]
    #[error("Opening container logs at the {endpoint_label} timed out.")]
    ContainerLogsTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The log response body ended unexpectedly or violated stream bounds.
    #[cfg(target_os = "linux")]
    #[error("Container logs from the {endpoint_label} were interrupted.")]
    ContainerLogStreamInterrupted {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// A terminal exec request or session could not be opened safely.
    #[cfg(target_os = "linux")]
    #[error("A terminal session could not be opened at the {endpoint_label}.")]
    ExecSessionUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The exec request was rejected by the engine.
    #[cfg(target_os = "linux")]
    #[error("A terminal session was rejected at the {endpoint_label} (HTTP {status}).")]
    ExecSessionRejected {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
        /// HTTP response status.
        status: u16,
    },
    /// A terminal session handshake exceeded its bounded wait.
    #[cfg(target_os = "linux")]
    #[error("Opening a terminal session at the {endpoint_label} timed out.")]
    ExecSessionTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// The terminal exec identifier or command is invalid.
    #[cfg(target_os = "linux")]
    #[error("The terminal exec request is invalid.")]
    InvalidExecRequest,
    /// A terminal session resize request could not be completed.
    #[cfg(target_os = "linux")]
    #[error("The terminal could not be resized at the {endpoint_label}.")]
    ExecResizeUnavailable {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// A terminal session resize request exceeded its bounded wait.
    #[cfg(target_os = "linux")]
    #[error("Resizing a terminal at the {endpoint_label} timed out.")]
    ExecResizeTimedOut {
        /// Non-secret description of the endpoint kind.
        endpoint_label: &'static str,
    },
    /// Terminal dimensions must contain nonzero rows and columns.
    #[cfg(target_os = "linux")]
    #[error("Terminal rows and columns must be nonzero.")]
    InvalidTerminalSize,
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
    /// Creates and starts an attached TTY exec session for a container.
    ///
    /// The command is passed as an exact argument array. The returned stream
    /// is raw bidirectional terminal data; dropping it closes only that exec.
    ///
    /// # Errors
    ///
    /// Returns `EngineError::InvalidExecRequest` for invalid identifiers or
    /// commands, `EngineError::InvalidTerminalSize` for zero dimensions, or
    /// `EngineError::ExecSessionRejected`, `EngineError::ExecSessionUnavailable`,
    /// or `EngineError::ExecSessionTimedOut` for engine failures.
    #[cfg(target_os = "linux")]
    pub async fn open_exec_session(
        &self,
        container_id: &str,
        command: &[String],
        initial_size: TerminalSize,
    ) -> Result<ExecSession, EngineError> {
        exec::open_exec_session(self, container_id, command, initial_size).await
    }
    /// Assigns a repository and tag to an existing local image.
    ///
    /// The image identifier must be nonempty and no longer than 4096 bytes.
    /// The repository and tag must be nonempty, contain no ASCII whitespace or
    /// control characters, and together fit within 512 bytes including `:`.
    /// The tag must follow Docker's tag grammar. The engine validates the
    /// repository. If that repository/tag already exists, Docker may reassign
    /// it to this image; image data is not removed. No registry credentials are
    /// read or sent.
    ///
    /// # Errors
    ///
    /// Returns `InvalidImageId` or `InvalidImageTagTarget` for invalid input,
    /// or a safe availability, timeout, or HTTP rejection error.
    #[cfg(target_os = "linux")]
    pub async fn tag_image(
        &self,
        image_id: &str,
        repository: &str,
        tag: &str,
    ) -> Result<(), EngineError> {
        if image_id.is_empty() || image_id.len() > 4096 {
            return Err(EngineError::InvalidImageId);
        }
        if !valid_image_tag_target(repository, tag) {
            return Err(EngineError::InvalidImageTagTarget);
        }

        let endpoint_label = self.info.endpoint_label;
        let mut path = resource_request_path(&self.api_version, "images", image_id, "/tag")
            .map_err(|_| EngineError::InvalidImageId)?;
        path.push_str("?repo=");
        path.push_str(&encode_query_component(repository));
        path.push_str("&tag=");
        path.push_str(&encode_query_component(tag));
        let response = timeout(REQUEST_TIMEOUT, async {
            let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
            let request = Request::builder()
                .method(Method::POST)
                .uri(Uri::new(&self.socket_path, &path))
                .body(Empty::<Bytes>::new())
                .map_err(|_| ())?;
            client.request(request).await.map_err(|_| ())
        })
        .await
        .map_err(|_| EngineError::ImageTagTimedOut { endpoint_label })?
        .map_err(|_| EngineError::ImageTagUnavailable { endpoint_label })?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(EngineError::ImageTagRejected {
                endpoint_label,
                status: response.status().as_u16(),
            })
        }
    }
    /// Opens an anonymous Docker image pull and streams bounded progress.
    ///
    /// The reference must be nonempty, at most 512 bytes, and contain no ASCII
    /// whitespace or control characters. Registry credentials are never read
    /// or sent. Dropping the returned stream cancels Ferrum's HTTP response
    /// stream, but does not guarantee that the daemon-side pull stops. Progress
    /// reads use a 60-second idle timeout.
    ///
    /// # Errors
    ///
    /// Returns `InvalidImageReference` for an invalid reference,
    /// `ImagePullRejected` when the engine rejects the request, or a safe
    /// availability/timeout error when the stream cannot be opened.
    #[cfg(target_os = "linux")]
    pub async fn open_image_pull(&self, reference: &str) -> Result<ImagePullStream, EngineError> {
        image_pull::open_image_pull(self, reference).await
    }
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
            Self::connect_with_startup_environment(docker_context, docker_host).await
        }
    }

    #[cfg(target_os = "linux")]
    async fn connect_with_startup_environment(
        docker_context: Option<String>,
        docker_host: Option<String>,
    ) -> Result<Self, EngineError> {
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
        match resolve_startup_source(docker_context.as_deref(), docker_host.as_deref(), current) {
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
                let path = local_socket_path_from_docker_host(host)?;
                let endpoint_label = configured_endpoint_label(host);
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
    /// Returns the local Unix socket used by this client.
    ///
    /// This endpoint is local configuration data, not a credential. Use it
    /// only to direct trusted local tooling to the same already-authorized
    /// daemon; do not display or log it. Normal socket permissions continue to
    /// enforce access.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn local_socket_path(&self) -> &std::path::Path {
        &self.socket_path
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
            .map_err(|_| {
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
    /// Returns all images, including images unused by containers.
    ///
    /// Only the image identifier, repository tags, creation time, and size are
    /// retained from the engine response.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the inventory request fails, the response
    /// is invalid or oversized, or an image omits its identifier.
    pub async fn list_images(&self) -> Result<Vec<ImageSummary>, EngineError> {
        #[cfg(target_os = "linux")]
        {
            let endpoint_label = self.info.endpoint_label;
            let path = format!("/v{}/images/json?all=true", self.api_version);
            let images: Vec<ImageListItem> = request_json(
                &self.socket_path,
                &path,
                MAX_LIST_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| EngineError::ImageInventoryUnavailable { endpoint_label })?;
            images
                .into_iter()
                .map(|image| map_image_summary(image, endpoint_label))
                .collect()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(EngineError::UnsupportedPlatform)
        }
    }
    /// Lists local networks from the connected Docker-compatible engine.
    ///
    /// Only identity and the allowlisted scalar network fields are retained.
    /// Attached-container counts are not provided by the network list endpoint
    /// and are available only as a point-in-time count from inspection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the bounded local Unix-socket request
    /// fails, the response is invalid or oversized, or any item lacks a
    /// non-empty ID or name. Unsupported platforms return
    /// `UnsupportedPlatform`.
    pub async fn list_networks(&self) -> Result<Vec<NetworkSummary>, EngineError> {
        #[cfg(target_os = "linux")]
        {
            let endpoint_label = self.info.endpoint_label;
            let path = format!("/v{}/networks", self.api_version);
            let networks: Vec<NetworkResponse> = request_json(
                &self.socket_path,
                &path,
                MAX_LIST_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| EngineError::NetworkInventoryUnavailable { endpoint_label })?;
            networks
                .into_iter()
                .map(|network| {
                    map_network(network)
                        .map_err(|()| EngineError::NetworkInventoryUnavailable { endpoint_label })
                })
                .collect()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Creates a default bridge network on the connected Linux local Docker
    /// Unix socket.
    ///
    /// Only the required `Name` field is sent; Docker's default bridge driver is
    /// used. The encoded body is bounded to 1024 bytes and successful response
    /// bodies to 64 KiB. Names must be non-whitespace, at most 255 UTF-8 bytes,
    /// and contain no control characters. The returned value is the exact
    /// network ID reported by the engine. Ferrum does not read or send
    /// credentials.
    ///
    /// # Errors
    ///
    /// Returns `InvalidNetworkName` without sending a request for invalid
    /// names, or a safe typed availability, timeout, or HTTP rejection error.
    /// Successful responses must contain a safe non-empty network ID.
    /// Unsupported platforms return `UnsupportedPlatform`.
    pub async fn create_network(&self, name: &str) -> Result<String, EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !valid_network_name(name) {
                return Err(EngineError::InvalidNetworkName);
            }
            let endpoint_label = self.info.endpoint_label;
            let body = serde_json::to_vec(&NetworkCreateRequest { name })
                .map_err(|_| EngineError::NetworkCreationUnavailable { endpoint_label })?;
            if body.len() > MAX_NETWORK_CREATE_BODY_BYTES {
                return Err(EngineError::InvalidNetworkName);
            }
            let path = format!("/v{}/networks/create", self.api_version);
            timeout(REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Full::new(Bytes::from(body)))
                    .map_err(|_| EngineError::NetworkCreationUnavailable { endpoint_label })?;
                let response = client
                    .request(request)
                    .await
                    .map_err(|_| EngineError::NetworkCreationUnavailable { endpoint_label })?;
                if !response.status().is_success() {
                    return Err(EngineError::NetworkCreationRejected {
                        endpoint_label,
                        status: response.status().as_u16(),
                    });
                }
                let body = Limited::new(response.into_body(), MAX_NETWORK_CREATE_RESPONSE_BYTES)
                    .collect()
                    .await
                    .map_err(|_| EngineError::NetworkCreationUnavailable { endpoint_label })?
                    .to_bytes();
                let response: NetworkCreateResponse = serde_json::from_slice(&body)
                    .map_err(|_| EngineError::NetworkCreationUnavailable { endpoint_label })?;
                response
                    .id
                    .filter(|id| network_id_path_safe(id))
                    .ok_or(EngineError::NetworkCreationUnavailable { endpoint_label })
            })
            .await
            .map_err(|_| EngineError::NetworkCreationTimedOut { endpoint_label })?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Inspects one local network by its engine ID or name.
    ///
    /// The identifier must be non-empty, at most 4096 bytes, and not `.` or
    /// `..`. Endpoint values are discarded. Full 64-character hexadecimal
    /// container IDs are retained only when every `Containers` map key is
    /// valid; the point-in-time attached-container count remains separate.
    ///
    /// # Errors
    ///
    /// Returns `InvalidNetworkId` before connecting for an invalid identifier,
    /// or a safe typed error if the bounded request fails, the response is
    /// invalid or oversized, or it lacks a non-empty ID or name. Unsupported
    /// platforms return `UnsupportedPlatform`.
    pub async fn inspect_network(&self, id: &str) -> Result<NetworkDetails, EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !network_id_path_safe(id) {
                return Err(EngineError::InvalidNetworkId);
            }
            let path = resource_request_path(&self.api_version, "networks", id, "")
                .map_err(|_| EngineError::InvalidNetworkId)?;
            let endpoint_label = self.info.endpoint_label;
            let response: NetworkInspectResponse = request_json(
                &self.socket_path,
                &path,
                MAX_INSPECT_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| EngineError::NetworkInspectionUnavailable { endpoint_label })?;
            let summary = map_network(response.network)
                .map_err(|()| EngineError::NetworkInspectionUnavailable { endpoint_label })?;
            Ok(NetworkDetails {
                id: summary.id,
                name: summary.name,
                driver: summary.driver,
                scope: summary.scope,
                created_at: summary.created_at,
                internal: summary.internal,
                attachable: summary.attachable,
                enable_ipv4: summary.enable_ipv4,
                enable_ipv6: summary.enable_ipv6,
                attached_container_count: response
                    .containers
                    .as_ref()
                    .map(|containers| containers.count),
                attached_container_ids: response.containers.and_then(|containers| containers.ids),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Removes one network by its exact full Docker ID on the connected Linux
    /// local Docker Unix socket.
    ///
    /// The request is bounded to the standard engine request deadline and sends
    /// no force or prune parameters. The inspected attached-container count is
    /// only a point-in-time observation; Docker may reject removal if network
    /// dependencies change before this request.
    ///
    /// # Errors
    ///
    /// Returns `InvalidNetworkId` before connecting unless the ID is exactly
    /// 64 hexadecimal characters. Other failures return safe typed availability,
    /// timeout, or HTTP rejection errors. A timeout or transport error does not
    /// prove the network remains.
    /// Unsupported platforms return `UnsupportedPlatform`.
    pub async fn remove_network(&self, id: &str) -> Result<(), EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !is_full_network_id(id) {
                return Err(EngineError::InvalidNetworkId);
            }
            let path = resource_request_path(&self.api_version, "networks", id, "")
                .map_err(|_| EngineError::InvalidNetworkId)?;
            let endpoint_label = self.info.endpoint_label;
            timeout(REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(Method::DELETE)
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Empty::<Bytes>::new())
                    .map_err(|_| EngineError::NetworkRemovalUnavailable { endpoint_label })?;
                let response = client
                    .request(request)
                    .await
                    .map_err(|_| EngineError::NetworkRemovalUnavailable { endpoint_label })?;
                if !response.status().is_success() {
                    return Err(EngineError::NetworkRemovalRejected {
                        endpoint_label,
                        status: response.status().as_u16(),
                    });
                }
                Ok(())
            })
            .await
            .map_err(|_| EngineError::NetworkRemovalTimedOut { endpoint_label })?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Connects a container to a network over the selected Linux local Docker
    /// Unix socket.
    ///
    /// Both identifiers must be full 64-character hexadecimal IDs. Sends only
    /// the `Container` field, with no endpoint configuration, and does not retry.
    /// The request has the standard eight-second deadline.
    ///
    /// # Errors
    ///
    /// Returns `InvalidNetworkId` or `InvalidContainerId` before I/O for invalid
    /// IDs, or a safe typed availability, timeout, or HTTP rejection error.
    /// A timeout or transport error does not prove that the engine did not
    /// change attachment state. Unsupported platforms return
    /// `UnsupportedPlatform`.
    pub async fn connect_container_to_network(
        &self,
        network_id: &str,
        container_id: &str,
    ) -> Result<(), EngineError> {
        self.mutate_network_attachment(network_id, container_id, "connect")
            .await
    }

    /// Disconnects a container from a network over the selected Linux local
    /// Docker Unix socket.
    ///
    /// Both identifiers must be full 64-character hexadecimal IDs. Sends only
    /// the `Container` field; it omits `Force`, retaining Docker's default
    /// `false`. The request has the standard eight-second deadline and is not
    /// retried.
    ///
    /// # Errors
    ///
    /// Returns `InvalidNetworkId` or `InvalidContainerId` before I/O for invalid
    /// IDs, or a safe typed availability, timeout, or HTTP rejection error.
    /// A timeout or transport error does not prove that the engine did not
    /// change attachment state. Unsupported platforms return
    /// `UnsupportedPlatform`.
    pub async fn disconnect_container_from_network(
        &self,
        network_id: &str,
        container_id: &str,
    ) -> Result<(), EngineError> {
        self.mutate_network_attachment(network_id, container_id, "disconnect")
            .await
    }

    async fn mutate_network_attachment(
        &self,
        network_id: &str,
        container_id: &str,
        action: &'static str,
    ) -> Result<(), EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !is_full_network_id(network_id) {
                return Err(EngineError::InvalidNetworkId);
            }
            if !is_full_container_id(container_id) {
                return Err(EngineError::InvalidContainerId);
            }
            let suffix = if action == "connect" {
                "/connect"
            } else {
                "/disconnect"
            };
            let path = resource_request_path(&self.api_version, "networks", network_id, suffix)
                .map_err(|_| EngineError::InvalidNetworkId)?;
            let endpoint_label = self.info.endpoint_label;
            let body = serde_json::to_vec(&NetworkAttachmentRequest {
                container: container_id,
            })
            .map_err(|_| EngineError::NetworkAttachmentUnavailable {
                endpoint_label,
                action,
            })?;
            timeout(REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Full::new(Bytes::from(body)))
                    .map_err(|_| EngineError::NetworkAttachmentUnavailable {
                        endpoint_label,
                        action,
                    })?;
                let response = client.request(request).await.map_err(|_| {
                    EngineError::NetworkAttachmentUnavailable {
                        endpoint_label,
                        action,
                    }
                })?;
                if !response.status().is_success() {
                    return Err(EngineError::NetworkAttachmentRejected {
                        endpoint_label,
                        action,
                        status: response.status().as_u16(),
                    });
                }
                Ok(())
            })
            .await
            .map_err(|_| EngineError::NetworkAttachmentTimedOut {
                endpoint_label,
                action,
            })?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (network_id, container_id, action);
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Lists local volumes. Only name, driver, and scope are retained.
    ///
    /// A missing or null `Volumes` field is treated as an empty inventory, as
    /// permitted by the Docker Engine API schema. Request failures and invalid
    /// or oversized responses remain errors.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the bounded local-socket request fails or
    /// the response is invalid or oversized. Unsupported platforms return
    /// `UnsupportedPlatform`.
    pub async fn list_volumes(&self) -> Result<Vec<VolumeSummary>, EngineError> {
        #[cfg(target_os = "linux")]
        {
            let endpoint_label = self.info.endpoint_label;
            let path = format!("/v{}/volumes", self.api_version);
            let response: VolumeListResponse = request_json(
                &self.socket_path,
                &path,
                MAX_LIST_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| EngineError::VolumeInventoryUnavailable { endpoint_label })?;
            response
                .volumes
                .unwrap_or_default()
                .into_iter()
                .map(|volume| {
                    let name = volume
                        .name
                        .filter(|name| volume_name_path_safe(name))
                        .ok_or(EngineError::VolumeInventoryUnavailable { endpoint_label })?;
                    Ok(VolumeSummary {
                        name,
                        driver: volume.driver.map(escape_volume_display_text),
                        scope: volume.scope.map(escape_volume_display_text),
                    })
                })
                .collect()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Inspects a named local volume and retains only documented display fields.
    ///
    /// The mountpoint is untrusted path text and is not accessed by Ferrum.
    ///
    /// # Errors
    ///
    /// Returns `InvalidVolumeName` before connecting for an invalid name, or a
    /// safe typed error for unavailable, invalid, or oversized responses.
    /// Unsupported platforms return `UnsupportedPlatform`.
    pub async fn inspect_volume(&self, name: &str) -> Result<VolumeDetails, EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !volume_name_path_safe(name) {
                return Err(EngineError::InvalidVolumeName);
            }
            let path = resource_request_path(&self.api_version, "volumes", name, "")
                .map_err(|_| EngineError::InvalidVolumeName)?;
            let endpoint_label = self.info.endpoint_label;
            let response: VolumeResponse = request_json(
                &self.socket_path,
                &path,
                MAX_INSPECT_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| EngineError::VolumeInspectionUnavailable { endpoint_label })?;
            let name = response
                .name
                .filter(|returned_name| {
                    returned_name == name && volume_name_path_safe(returned_name)
                })
                .ok_or(EngineError::VolumeInspectionUnavailable { endpoint_label })?;
            Ok(VolumeDetails {
                name,
                driver: response.driver.map(escape_volume_display_text),
                scope: response.scope.map(escape_volume_display_text),
                created_at: response.created_at.map(escape_volume_display_text),
                mountpoint: response.mountpoint.map(escape_volume_display_text),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Creates a named volume using the engine's default driver and options.
    ///
    /// Ferrum sends only `Name`. The returned mountpoint is untrusted text and
    /// is never accessed.
    ///
    /// # Errors
    ///
    /// Returns `InvalidVolumeName` before connecting for an unsafe name, or a
    /// safe typed availability, timeout, or HTTP rejection error. The response
    /// must identify the requested name. Unsupported platforms return
    /// `UnsupportedPlatform`.
    pub async fn create_volume(&self, name: &str) -> Result<VolumeDetails, EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !volume_name_path_safe(name) {
                return Err(EngineError::InvalidVolumeName);
            }
            let endpoint_label = self.info.endpoint_label;
            let body = serde_json::to_vec(&VolumeCreateRequest { name })
                .map_err(|_| EngineError::VolumeCreationUnavailable { endpoint_label })?;
            if body.len() > MAX_VOLUME_CREATE_REQUEST_BYTES {
                return Err(EngineError::InvalidVolumeName);
            }
            let path = format!("/v{}/volumes/create", self.api_version);
            timeout(REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Full::new(Bytes::from(body)))
                    .map_err(|_| EngineError::VolumeCreationUnavailable { endpoint_label })?;
                let response = client
                    .request(request)
                    .await
                    .map_err(|_| EngineError::VolumeCreationUnavailable { endpoint_label })?;
                if !response.status().is_success() {
                    return Err(EngineError::VolumeCreationRejected {
                        endpoint_label,
                        status: response.status().as_u16(),
                    });
                }
                let body = Limited::new(response.into_body(), MAX_VOLUME_CREATE_RESPONSE_BYTES)
                    .collect()
                    .await
                    .map_err(|_| EngineError::VolumeCreationUnavailable { endpoint_label })?
                    .to_bytes();
                let response: VolumeResponse = serde_json::from_slice(&body)
                    .map_err(|_| EngineError::VolumeCreationUnavailable { endpoint_label })?;
                let name = response
                    .name
                    .filter(|returned_name| {
                        returned_name.as_str() == name && volume_name_path_safe(returned_name)
                    })
                    .ok_or(EngineError::VolumeCreationUnavailable { endpoint_label })?;
                Ok(VolumeDetails {
                    name,
                    driver: response.driver.map(escape_volume_display_text),
                    scope: response.scope.map(escape_volume_display_text),
                    created_at: response.created_at.map(escape_volume_display_text),
                    mountpoint: response.mountpoint.map(escape_volume_display_text),
                })
            })
            .await
            .map_err(|_| EngineError::VolumeCreationTimedOut { endpoint_label })?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err(EngineError::UnsupportedPlatform)
        }
    }
    /// Removes a named local volume on the connected Linux local Docker Unix socket.
    ///
    /// The caller must check current dependencies and obtain explicit confirmation.
    /// Ferrum never sets Docker's `force` parameter. The dependency snapshot is not
    /// atomic with this request, so Docker may reject removal if the volume becomes
    /// in use before the request is processed.
    ///
    /// # Errors
    ///
    /// Returns `InvalidVolumeName` before connecting for an unsafe name, or a safe
    /// typed availability, timeout, or HTTP rejection error. A timeout or transport
    /// error does not establish whether the volume remains. Unsupported platforms
    /// return `UnsupportedPlatform`.
    pub async fn remove_volume(&self, name: &str) -> Result<(), EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !volume_name_path_safe(name) {
                return Err(EngineError::InvalidVolumeName);
            }
            let path = resource_request_path(&self.api_version, "volumes", name, "")
                .map_err(|_| EngineError::InvalidVolumeName)?;
            let endpoint_label = self.info.endpoint_label;
            timeout(REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(Method::DELETE)
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Empty::<Bytes>::new())
                    .map_err(|_| EngineError::VolumeRemovalUnavailable { endpoint_label })?;
                let response = client
                    .request(request)
                    .await
                    .map_err(|_| EngineError::VolumeRemovalUnavailable { endpoint_label })?;
                if !response.status().is_success() {
                    return Err(EngineError::VolumeRemovalRejected {
                        endpoint_label,
                        status: response.status().as_u16(),
                    });
                }
                Ok(())
            })
            .await
            .map_err(|_| EngineError::VolumeRemovalTimedOut { endpoint_label })?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Returns read-only point-in-time usage for a named local volume.
    ///
    /// Only the matching volume name and its size and reference count are
    /// retained from Docker's verbose volume usage report. The request is
    /// restricted to the volume type and never requests all disk usage data.
    /// This filtered read has a bounded 30-second total request wait because
    /// system usage calculation can be slower than ordinary engine reads.
    ///
    /// # Errors
    ///
    /// Returns `InvalidVolumeName` before connecting for an invalid name, or
    /// `VolumeUsageUnavailable` if the request exceeds its 30-second bound or
    /// the engine response is unavailable, invalid, or oversized. Unsupported
    /// platforms return `UnsupportedPlatform`.
    pub async fn volume_usage(&self, name: &str) -> Result<Option<VolumeUsage>, EngineError> {
        #[cfg(target_os = "linux")]
        {
            if !volume_name_path_safe(name) {
                return Err(EngineError::InvalidVolumeName);
            }
            let endpoint_label = self.info.endpoint_label;
            let path = format!("/v{}/system/df?verbose=true&type=volume", self.api_version);
            let usage = request_json_with_decoder(
                &self.socket_path,
                &path,
                MAX_LIST_RESPONSE_BYTES,
                VOLUME_USAGE_REQUEST_TIMEOUT,
                |body| {
                    let mut deserializer = serde_json::Deserializer::from_slice(body);
                    let usage = VolumeUsageResponseSeed {
                        requested_name: name,
                    }
                    .deserialize(&mut deserializer)
                    .map_err(|_| RequestJsonError::JsonDecode)?;
                    deserializer
                        .end()
                        .map_err(|_| RequestJsonError::JsonDecode)?;
                    Ok(usage)
                },
            )
            .await
            .map_err(|_| EngineError::VolumeUsageUnavailable { endpoint_label })?;
            Ok(usage)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Returns read-only details for an image identified by its engine ID.
    ///
    /// The engine response is reduced to the documented image fields; labels,
    /// configuration, and environment data are not exposed.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the identifier is invalid, inspection
    /// fails, the response is invalid or oversized, or the response omits the
    /// image identifier.
    pub async fn inspect_image(&self, id: &str) -> Result<ImageDetails, EngineError> {
        #[cfg(target_os = "linux")]
        {
            let endpoint_label = self.info.endpoint_label;
            let path = image_inspect_request_path(&self.api_version, id)
                .map_err(|_| EngineError::InvalidImageId)?;
            let response: ImageInspectResponse = request_json(
                &self.socket_path,
                &path,
                MAX_INSPECT_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| EngineError::ImageInspectionUnavailable { endpoint_label })?;
            map_image_details(response, endpoint_label)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            Err(EngineError::UnsupportedPlatform)
        }
    }
    /// Removes an image by its engine identifier without forcing removal.
    ///
    /// Untagged parent images are preserved. The engine may reject images in
    /// use or carrying other tags.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the identifier is invalid, the request
    /// is unavailable or times out, or the engine rejects removal.
    pub async fn remove_image(&self, image_id: &str) -> Result<(), EngineError> {
        #[cfg(target_os = "linux")]
        {
            if image_id.is_empty() || image_id.len() > 4096 {
                return Err(EngineError::InvalidImageId);
            }
            let endpoint_label = self.info.endpoint_label;
            let path = image_remove_request_path(&self.api_version, image_id)
                .map_err(|_| EngineError::InvalidImageId)?;
            let result = timeout(LIFECYCLE_REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(Method::DELETE)
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Empty::<Bytes>::new())
                    .map_err(|_| ())?;
                client.request(request).await.map_err(|_| ())
            })
            .await;
            let response = match result {
                Ok(Ok(response)) => response,
                Ok(Err(())) => return Err(EngineError::ImageRemovalUnavailable { endpoint_label }),
                Err(_) => return Err(EngineError::ImageRemovalTimedOut { endpoint_label }),
            };
            if response.status().is_success() {
                Ok(())
            } else {
                Err(EngineError::ImageRemovalRejected {
                    endpoint_label,
                    status: response.status().as_u16(),
                })
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = image_id;
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
            let path =
                inspect_request_path(&self.api_version, id).map_err(|_| self.inspection_error())?;
            let response: InspectResponse = request_json(
                &self.socket_path,
                &path,
                MAX_INSPECT_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await
            .map_err(|_| self.inspection_error())?;
            map_container_details(response, self.info.endpoint_label)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            Err(EngineError::UnsupportedPlatform)
        }
    }
    /// Performs a lifecycle action on a container.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the action could not be sent or the
    /// engine rejected it.
    pub async fn perform_container_action(
        &self,
        id: &str,
        action: ContainerAction,
    ) -> Result<(), EngineError> {
        #[cfg(target_os = "linux")]
        {
            let endpoint_label = self.info.endpoint_label;
            let suffix = match action {
                ContainerAction::Start => "/start",
                ContainerAction::Stop => "/stop",
                ContainerAction::Restart => "/restart",
                ContainerAction::Pause => "/pause",
                ContainerAction::Unpause => "/unpause",
                ContainerAction::Remove => "",
            };
            let path = container_request_path(&self.api_version, id, suffix).map_err(|_| {
                EngineError::ContainerActionUnavailable {
                    action,
                    endpoint_label,
                }
            })?;
            let method = if action == ContainerAction::Remove {
                Method::DELETE
            } else {
                Method::POST
            };
            let result = timeout(LIFECYCLE_REQUEST_TIMEOUT, async {
                let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
                let request = Request::builder()
                    .method(method)
                    .uri(Uri::new(&self.socket_path, &path))
                    .body(Empty::<Bytes>::new())
                    .map_err(|_| ())?;
                client.request(request).await.map_err(|_| ())
            })
            .await;
            let response = match result {
                Ok(Ok(response)) => response,
                Ok(Err(())) => {
                    return Err(EngineError::ContainerActionUnavailable {
                        action,
                        endpoint_label,
                    });
                }
                Err(_) => {
                    return Err(EngineError::ContainerActionTimedOut {
                        action,
                        endpoint_label,
                    });
                }
            };
            let status = response.status();
            if status.is_success()
                || (status == hyper::StatusCode::NOT_MODIFIED
                    && matches!(action, ContainerAction::Start | ContainerAction::Stop))
            {
                Ok(())
            } else {
                Err(EngineError::ContainerActionRejected {
                    action,
                    endpoint_label,
                    status: status.as_u16(),
                })
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (id, action);
            Err(EngineError::UnsupportedPlatform)
        }
    }

    /// Opens the engine's incremental container event stream.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error if the event endpoint cannot be opened.
    #[cfg(target_os = "linux")]
    pub async fn subscribe_container_changes(&self) -> Result<ContainerChangeStream, EngineError> {
        let endpoint_label = self.info.endpoint_label;
        let path = format!(
            "/v{}/events?filters=%7B%22type%22%3A%5B%22container%22%5D%7D",
            self.api_version
        );
        let result = timeout(REQUEST_TIMEOUT, async {
            let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
            let request = Request::builder()
                .method(Method::GET)
                .uri(Uri::new(&self.socket_path, &path))
                .body(Empty::<Bytes>::new())
                .map_err(|_| ())?;
            client.request(request).await.map_err(|_| ())
        })
        .await;
        let response = match result {
            Ok(Ok(response)) if response.status().is_success() => response,
            _ => return Err(EngineError::ContainerEventsUnavailable { endpoint_label }),
        };
        Ok(ContainerChangeStream {
            body: response.into_body(),
            endpoint_label,
            pending: Vec::with_capacity(MAX_EVENT_LINE_BYTES),
            frame: None,
            frame_offset: 0,
            finished: false,
        })
    }
    /// Opens a historical or follow stream of container logs.
    ///
    /// The caller must provide the TTY setting reported by container inspect;
    /// TTY output is a raw combined stream, while non-TTY output uses Docker's
    /// multiplexed stdout/stderr framing. Docker exposes this endpoint for
    /// `json-file` and `journald` logging drivers. Response headers are bounded
    /// by an eight-second timeout; the body remains cancellable and has no
    /// total-stream timeout.
    ///
    /// # Errors
    ///
    /// Returns an options or identifier error before I/O, a safe error if the
    /// request cannot be opened, or an interrupted error for malformed or
    /// oversized stream data.
    #[cfg(target_os = "linux")]
    pub async fn open_container_logs(
        &self,
        id: &str,
        tty: bool,
        options: ContainerLogsOptions,
    ) -> Result<ContainerLogStream, EngineError> {
        logs::open_container_logs(
            &self.socket_path,
            &self.api_version,
            self.info.endpoint_label,
            id,
            tty,
            options,
        )
        .await
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
        .map_err(|_| EngineError::Unavailable { endpoint_label })?;
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

/// Incremental stream of container changes from a Docker-compatible engine.
#[cfg(target_os = "linux")]
pub struct ContainerChangeStream {
    body: Incoming,
    endpoint_label: &'static str,
    pending: Vec<u8>,
    frame: Option<Bytes>,
    frame_offset: usize,
    finished: bool,
}

#[cfg(target_os = "linux")]
impl ContainerChangeStream {
    /// Returns the next container change, or `None` when the stream ends.
    ///
    /// # Errors
    ///
    /// Returns an interrupted error for transport, decoding, or size failures.
    pub async fn next_change(&mut self) -> Result<Option<ContainerChange>, EngineError> {
        loop {
            if let Some(data) = self.frame.as_ref() {
                let (parsed, consumed) =
                    process_event_data(&mut self.pending, &data[self.frame_offset..])
                        .map_err(|()| self.interrupted())?;
                self.frame_offset += consumed;
                if self.frame_offset == data.len() {
                    self.frame = None;
                    self.frame_offset = 0;
                }
                if parsed.is_some() {
                    return Ok(parsed);
                }
                continue;
            }
            if self.finished {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let parsed =
                    parse_container_event(&self.pending).map_err(|()| self.interrupted())?;
                self.pending.clear();
                return Ok(parsed);
            }
            match self.body.frame().await {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        if data.len() > MAX_EVENT_FRAME_BYTES {
                            return Err(self.interrupted());
                        }
                        self.frame = Some(data.clone());
                    }
                }
                Some(Err(_)) => return Err(self.interrupted()),
                None => self.finished = true,
            }
        }
    }

    fn interrupted(&self) -> EngineError {
        EngineError::ContainerEventsInterrupted {
            endpoint_label: self.endpoint_label,
        }
    }
}
#[cfg(target_os = "linux")]
const MAX_EVENT_LINE_BYTES: usize = 64 * 1024;

#[cfg(target_os = "linux")]
// Reject unusually large aggregated Hyper frames instead of retaining or scanning them.
const MAX_EVENT_FRAME_BYTES: usize = 1024 * 1024;

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
struct ContainerEvent<'a> {
    #[serde(rename = "Type", borrow)]
    event_type: Option<&'a str>,
    #[serde(rename = "Actor", borrow)]
    actor: Option<ContainerEventActor<'a>>,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
struct ContainerEventActor<'a> {
    #[serde(rename = "ID", borrow)]
    id: Option<&'a str>,
}

#[cfg(target_os = "linux")]
fn process_event_data(
    pending: &mut Vec<u8>,
    data: &[u8],
) -> Result<(Option<ContainerChange>, usize), ()> {
    let mut remaining = data;
    while let Some(newline) = remaining.iter().position(|byte| *byte == b'\n') {
        if pending.len() + newline > MAX_EVENT_LINE_BYTES {
            return Err(());
        }
        pending.extend_from_slice(&remaining[..newline]);
        remaining = &remaining[newline + 1..];
        if pending.last() == Some(&b'\r') {
            pending.pop();
        }
        let parsed = parse_container_event(pending)?;
        pending.clear();
        if parsed.is_some() {
            return Ok((parsed, data.len() - remaining.len()));
        }
    }
    if pending.len() + remaining.len() > MAX_EVENT_LINE_BYTES {
        return Err(());
    }
    pending.extend_from_slice(remaining);
    Ok((None, data.len()))
}

fn parse_container_event(line: &[u8]) -> Result<Option<ContainerChange>, ()> {
    let event: ContainerEvent<'_> = serde_json::from_slice(line).map_err(|_| ())?;
    if event.event_type != Some("container") {
        return Ok(None);
    }
    let container_id = event
        .actor
        .and_then(|actor| actor.id)
        .filter(|id| !id.is_empty())
        .ok_or(())?;
    Ok(Some(ContainerChange {
        container_id: container_id.to_owned(),
    }))
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
fn valid_image_tag_target(repository: &str, tag: &str) -> bool {
    let tag_bytes = tag.as_bytes();
    !repository.is_empty()
        && !tag.is_empty()
        && repository.len().saturating_add(1).saturating_add(tag.len()) <= 512
        && !repository
            .bytes()
            .chain(tag.bytes())
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        && tag_bytes.len() <= 128
        && (tag_bytes[0].is_ascii_alphanumeric() || tag_bytes[0] == b'_')
        && tag_bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}
#[cfg(target_os = "linux")]
fn local_socket_path_from_docker_host(host: &str) -> Result<PathBuf, EngineError> {
    if is_remote_endpoint(host) {
        return Err(EngineError::UnsupportedRemoteTransport);
    }
    host.strip_prefix("unix://")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or(EngineError::UnsupportedRemoteTransport)
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
) -> Result<T, RequestJsonError> {
    request_json_with_decoder(
        socket_path,
        request_path,
        max_response_bytes,
        request_timeout,
        |body| serde_json::from_slice(body).map_err(|_| RequestJsonError::JsonDecode),
    )
    .await
}

#[cfg(target_os = "linux")]
async fn request_json_with_decoder<T, F>(
    socket_path: &std::path::Path,
    request_path: &str,
    max_response_bytes: usize,
    request_timeout: Duration,
    decode: F,
) -> Result<T, RequestJsonError>
where
    F: FnOnce(&[u8]) -> Result<T, RequestJsonError>,
{
    match timeout(request_timeout, async {
        let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let request = Request::builder()
            .method(Method::GET)
            .uri(Uri::new(socket_path, request_path))
            .body(Empty::<Bytes>::new())
            .map_err(|_| RequestJsonError::RequestBuild)?;
        let response = client
            .request(request)
            .await
            .map_err(|_| RequestJsonError::Transport)?;
        if !response.status().is_success() {
            return Err(RequestJsonError::UnsuccessfulStatus);
        }
        let body = Limited::new(response.into_body(), max_response_bytes)
            .collect()
            .await
            .map_err(|error| {
                if error.is::<http_body_util::LengthLimitError>() {
                    RequestJsonError::BodyTooLarge
                } else {
                    RequestJsonError::BodyRead
                }
            })?
            .to_bytes();
        decode(&body)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(RequestJsonError::Timeout),
    }
}

#[cfg(target_os = "linux")]
fn inspect_request_path(api_version: &str, id: &str) -> Result<String, RequestJsonError> {
    container_request_path(api_version, id, "/json")
}

#[cfg(target_os = "linux")]
fn container_request_path(
    api_version: &str,
    id: &str,
    suffix: &str,
) -> Result<String, RequestJsonError> {
    resource_request_path(api_version, "containers", id, suffix)
}

#[cfg(target_os = "linux")]
fn image_inspect_request_path(api_version: &str, id: &str) -> Result<String, RequestJsonError> {
    resource_request_path(api_version, "images", id, "/json")
}

#[cfg(target_os = "linux")]
fn image_remove_request_path(
    api_version: &str,
    image_id: &str,
) -> Result<String, RequestJsonError> {
    let mut path = resource_request_path(api_version, "images", image_id, "")?;
    path.push_str("?noprune=true");
    Ok(path)
}

#[cfg(target_os = "linux")]
fn encode_query_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len().saturating_mul(3));
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

#[cfg(target_os = "linux")]
fn volume_name_path_safe(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 4096
        && !matches!(name, "." | "..")
        && !name.chars().any(is_untrusted_display_character)
}
#[cfg(target_os = "linux")]
fn escape_volume_display_text(value: String) -> String {
    let escape_count = value
        .chars()
        .filter(|character| is_untrusted_display_character(*character))
        .count();
    if escape_count == 0 {
        return value;
    }
    let capacity = value.len().saturating_add(escape_count.saturating_mul(7));
    let mut escaped = String::with_capacity(capacity);
    for character in value.chars() {
        if is_untrusted_display_character(character) {
            let mut code = character as u32;
            let mut digits = [0_u8; 6];
            let mut start = digits.len();
            loop {
                start -= 1;
                digits[start] = b"0123456789abcdef"[(code & 0xf) as usize];
                code >>= 4;
                if code == 0 {
                    break;
                }
            }
            escaped.push_str("\\u{");
            for digit in &digits[start..] {
                escaped.push(char::from(*digit));
            }
            escaped.push('}');
        } else {
            escaped.push(character);
        }
    }
    escaped
}

#[cfg(target_os = "linux")]
fn resource_request_path(
    api_version: &str,
    resource: &str,
    id: &str,
    suffix: &str,
) -> Result<String, RequestJsonError> {
    if id.is_empty() || id.len() > 4096 {
        return Err(RequestJsonError::RequestBuild);
    }
    let mut path = String::with_capacity(
        api_version.len() + resource.len() + id.len().saturating_mul(3) + suffix.len() + 4,
    );
    path.push_str("/v");
    path.push_str(api_version);
    path.push('/');
    path.push_str(resource);
    path.push('/');
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            path.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(path, "%{byte:02X}").map_err(|_| RequestJsonError::RequestBuild)?;
        }
    }
    path.push_str(suffix);
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

#[cfg(target_os = "linux")]
fn network_id_path_safe(id: &str) -> bool {
    !id.is_empty() && id.len() <= 4096 && !matches!(id, "." | "..")
}
#[cfg(target_os = "linux")]
fn is_full_network_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(target_os = "linux")]
fn is_full_container_id(id: &str) -> bool {
    is_full_network_id(id)
}

#[cfg(target_os = "linux")]
fn valid_network_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NETWORK_NAME_BYTES
        && !name.trim().is_empty()
        && !name.chars().any(char::is_control)
}

#[cfg(target_os = "linux")]
fn map_network(response: NetworkResponse) -> Result<NetworkSummary, ()> {
    let id = response
        .id
        .filter(|id| network_id_path_safe(id))
        .ok_or(())?;
    let name = response.name.filter(|name| !name.is_empty()).ok_or(())?;
    Ok(NetworkSummary {
        id,
        name,
        driver: response.driver,
        scope: response.scope,
        created_at: response.created,
        internal: response.internal,
        attachable: response.attachable,
        enable_ipv4: response.enable_ipv4,
        enable_ipv6: response.enable_ipv6,
    })
}

#[cfg(target_os = "linux")]
fn map_image_summary(
    image: ImageListItem,
    endpoint_label: &'static str,
) -> Result<ImageSummary, EngineError> {
    let id = image
        .id
        .filter(|id| !id.is_empty())
        .ok_or(EngineError::MissingImageId { endpoint_label })?;
    Ok(ImageSummary {
        id,
        repo_tags: image.repo_tags.unwrap_or_default(),
        created_unix_seconds: image.created,
        size_bytes: image.size,
    })
}

#[cfg(target_os = "linux")]
fn map_image_details(
    response: ImageInspectResponse,
    endpoint_label: &'static str,
) -> Result<ImageDetails, EngineError> {
    let id = response
        .id
        .filter(|id| !id.is_empty())
        .ok_or(EngineError::MissingInspectedImageId { endpoint_label })?;
    Ok(ImageDetails {
        id,
        repo_tags: response.repo_tags.unwrap_or_default(),
        created: response.created,
        size_bytes: response.size,
        architecture: response.architecture,
        os: response.os,
        author: response.author,
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
            let state = state.status.map(|status| ContainerState::parse(&status));
            (state, exit_code, started_at, finished_at)
        });
    let (config_image, tty) = response
        .config
        .map_or((None, None), |config| (config.image, config.tty));
    let image = config_image.or(response.image);

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
        tty,
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
                tty: None,
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
    fn preserves_explicitly_empty_state_status() {
        let details = map_container_details(
            response(r#"{"Id":"container-id","State":{"Status":""}}"#),
            ENDPOINT,
        )
        .expect("id is present");

        assert_eq!(details.state, Some(ContainerState::Unknown(String::new())));
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
            let result: Result<serde_json::Value, RequestJsonError> =
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
            let result: Result<serde_json::Value, RequestJsonError> =
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
            let (headers_sent, headers_written) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).await;
                let headers_result = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = headers_sent.send(headers_result.is_ok());
                if headers_result.is_ok() {
                    let mut buffer = [0_u8; 128];
                    loop {
                        match tokio::time::timeout(Duration::from_secs(1), stream.read(&mut buffer))
                            .await
                        {
                            Ok(Ok(0)) => break,
                            Ok(Ok(_)) => {}
                            Ok(Err(_)) | Err(_) => {
                                panic!("client should close after the body-read timeout")
                            }
                        }
                    }
                }
            });

            let request_socket = socket.clone();
            let request = tokio::spawn(async move {
                request_json::<serde_json::Value>(
                    &request_socket,
                    "/version",
                    1024,
                    Duration::from_millis(250),
                )
                .await
            });
            assert!(
                headers_written
                    .await
                    .expect("server should report its header write"),
                "response headers should reach the client before body timeout"
            );
            let result = request.await.expect("request task should finish");
            cleanup(&socket, server, &directory).await;
            assert_eq!(
                result,
                Err(RequestJsonError::Timeout),
                "body read must be included in the total request timeout"
            );
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

        let result: Result<InspectResponse, RequestJsonError> = request_json(
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
mod image_api_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{UnixListener, UnixStream},
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "ferrum-images-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("temporary directory should be created");
        directory
    }

    fn client(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    async fn fixture(
        response: Vec<u8>,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        tokio::task::JoinHandle<String>,
    ) {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let request = read_request(&mut stream).await;
            // An oversized-body request may close the socket as soon as the
            // response limit is reached.
            let _ = stream.write_all(&response).await;
            request
        });
        (socket_path, directory, server)
    }

    async fn read_request(stream: &mut UnixStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream
                .read(&mut buffer)
                .await
                .expect("request should be readable");
            assert_ne!(count, 0, "client should send HTTP headers");
            request.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8(request).expect("HTTP request should be UTF-8")
    }

    fn cleanup(socket_path: &std::path::Path, directory: &std::path::Path) {
        std::fs::remove_file(socket_path).expect("socket should be removed");
        std::fs::remove_dir(directory).expect("directory should be removed");
    }

    #[tokio::test]
    async fn lists_images_from_versioned_endpoint_and_maps_allowlisted_fields() {
        let body = br#"[{"Id":"sha256:image-id","RepoTags":["registry.example/app:stable"],"Created":1720000000,"Size":9876,"Labels":{"credential":"private-label"},"Containers":4},{"Id":"sha256:untagged","RepoTags":null,"Created":1720000001}]"#;
        let (socket, directory, server) = fixture(http_response("200 OK", body)).await;

        let images = client(socket.clone())
            .list_images()
            .await
            .expect("image inventory should be returned");
        let request = server.await.expect("fixture server should finish");
        assert!(
            request.starts_with("GET /v1.45/images/json?all=true HTTP/1.1\r\n"),
            "unexpected image list request: {request}"
        );
        assert_eq!(
            images,
            vec![
                ferrum_core::ImageSummary {
                    id: "sha256:image-id".to_owned(),
                    repo_tags: vec!["registry.example/app:stable".to_owned()],
                    created_unix_seconds: Some(1_720_000_000),
                    size_bytes: Some(9_876),
                },
                ferrum_core::ImageSummary {
                    id: "sha256:untagged".to_owned(),
                    repo_tags: Vec::new(),
                    created_unix_seconds: Some(1_720_000_001),
                    size_bytes: None,
                },
            ]
        );
        let debug = format!("{images:?}");
        assert!(!debug.contains("private-label"));
        assert!(!debug.contains("credential"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn inspects_encoded_image_id_and_maps_only_allowlisted_fields() {
        let body = br#"{"Id":"sha256:image-id","RepoTags":["registry.example/app:stable"],"Created":"2024-07-03T12:00:00Z","Size":9876,"Architecture":"amd64","Os":"linux","Author":"maintainer","Config":{"Env":["TOKEN=private-env"],"Labels":{"credential":"private-label"}},"ContainerConfig":{"Env":["OTHER=secret"]},"RootFS":{"Layers":["private-layer"]}}"#;
        let (socket, directory, server) = fixture(http_response("200 OK", body)).await;

        let details = client(socket.clone())
            .inspect_image("a/b c")
            .await
            .expect("image details should be returned");
        let request = server.await.expect("fixture server should finish");
        assert!(
            request.starts_with("GET /v1.45/images/a%2Fb%20c/json HTTP/1.1\r\n"),
            "unexpected image inspection request: {request}"
        );
        assert_eq!(
            details,
            ferrum_core::ImageDetails {
                id: "sha256:image-id".to_owned(),
                repo_tags: vec!["registry.example/app:stable".to_owned()],
                created: Some("2024-07-03T12:00:00Z".to_owned()),
                size_bytes: Some(9_876),
                architecture: Some("amd64".to_owned()),
                os: Some("linux".to_owned()),
                author: Some("maintainer".to_owned()),
            }
        );
        let debug = format!("{details:?}");
        assert!(!debug.contains("private-env"));
        assert!(!debug.contains("private-label"));
        assert!(!debug.contains("OTHER=secret"));
        assert!(!debug.contains("private-layer"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_input_and_responses_missing_image_ids() {
        let missing_socket = std::env::temp_dir().join("ferrum-image-id-must-not-connect.sock");
        let engine = client(missing_socket);
        assert!(matches!(
            engine.inspect_image("").await,
            Err(EngineError::InvalidImageId)
        ));

        let (socket, directory, server) = fixture(http_response(
            "200 OK",
            br#"[{"RepoTags":["ignored:tag"]}]"#,
        ))
        .await;
        let error = client(socket.clone())
            .list_images()
            .await
            .expect_err("list items require an image ID");
        let request = server.await.expect("fixture server should finish");
        assert!(request.starts_with("GET /v1.45/images/json?all=true HTTP/1.1\r\n"));
        assert!(matches!(
            error,
            EngineError::MissingImageId {
                endpoint_label: "test endpoint"
            }
        ));
        cleanup(&socket, &directory);

        let (socket, directory, server) =
            fixture(http_response("200 OK", br#"{"RepoTags":[]}"#)).await;
        let error = client(socket.clone())
            .inspect_image("image-id")
            .await
            .expect_err("inspection response requires an image ID");
        let request = server.await.expect("fixture server should finish");
        assert!(request.starts_with("GET /v1.45/images/image-id/json HTTP/1.1\r\n"));
        assert!(matches!(
            error,
            EngineError::MissingInspectedImageId {
                endpoint_label: "test endpoint"
            }
        ));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn maps_malformed_http_and_oversized_responses_to_safe_errors() {
        let (socket, directory, server) = fixture(http_response("200 OK", b"{malformed")).await;
        let error = client(socket.clone())
            .list_images()
            .await
            .expect_err("malformed JSON should fail safely");
        let _ = server.await.expect("fixture server should finish");
        assert!(matches!(
            error,
            EngineError::ImageInventoryUnavailable {
                endpoint_label: "test endpoint"
            }
        ));
        cleanup(&socket, &directory);

        let private_body = b"private daemon response body";
        let (socket, directory, server) =
            fixture(http_response("500 Internal Server Error", private_body)).await;
        let error = client(socket.clone())
            .inspect_image("image-id")
            .await
            .expect_err("HTTP errors should not be treated as image details");
        let _ = server.await.expect("fixture server should finish");
        assert!(matches!(
            error,
            EngineError::ImageInspectionUnavailable {
                endpoint_label: "test endpoint"
            }
        ));
        assert!(!format!("{error:?}").contains("private daemon"));
        assert!(!error.to_string().contains("private daemon"));
        cleanup(&socket, &directory);

        let oversized = vec![b' '; MAX_INSPECT_RESPONSE_BYTES + 1];
        let (socket, directory, server) = fixture(http_response("200 OK", &oversized)).await;
        let error = client(socket.clone())
            .inspect_image("image-id")
            .await
            .expect_err("oversized inspection JSON should be rejected");
        let _ = server.await.expect("fixture server should finish");
        assert!(matches!(
            error,
            EngineError::ImageInspectionUnavailable {
                endpoint_label: "test endpoint"
            }
        ));
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn removes_image_with_encoded_id_and_preserves_untagged_parents() {
        let (socket, directory, server) =
            fixture(http_response("200 OK", br#"{"Untagged":[],"Deleted":[]}"#)).await;
        client(socket.clone())
            .remove_image("a/b c")
            .await
            .expect("image removal should succeed");
        let request = server.await.expect("fixture server should finish");
        assert!(
            request.starts_with("DELETE /v1.45/images/a%2Fb%20c?noprune=true HTTP/1.1\r\n"),
            "unexpected image removal request: {request}"
        );
        let request_line = request.lines().next().unwrap_or_default();
        assert!(!request_line.contains("force"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_image_id_before_connecting() {
        let missing_socket = std::env::temp_dir().join("ferrum-image-remove-must-not-connect.sock");
        let engine = client(missing_socket);
        assert!(matches!(
            engine.remove_image("").await,
            Err(EngineError::InvalidImageId)
        ));
        assert!(matches!(
            engine.remove_image(&"x".repeat(4097)).await,
            Err(EngineError::InvalidImageId)
        ));
    }

    #[tokio::test]
    async fn image_removal_rejection_is_typed_and_does_not_expose_response_body() {
        let (socket, directory, server) = fixture(http_response(
            "409 Conflict",
            b"private engine response body",
        ))
        .await;
        let error = client(socket.clone())
            .remove_image("image-id")
            .await
            .expect_err("409 should be rejected");
        let request = server.await.expect("fixture server should finish");
        assert!(request.starts_with("DELETE /v1.45/images/image-id?noprune=true HTTP/1.1\r\n"));
        assert!(matches!(
            error,
            EngineError::ImageRemovalRejected {
                endpoint_label: "test endpoint",
                status: 409
            }
        ));
        assert!(!format!("{error:?}").contains("private engine response body"));
        assert!(!error.to_string().contains("private engine response body"));
        cleanup(&socket, &directory);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod volume_api_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "ferrum-volumes-{}-{timestamp}-{sequence}",
            std::process::id(),
            timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos(),
        ));
        std::fs::create_dir(&directory).expect("temporary directory should be created");
        directory
    }

    fn client(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn fixture(
        response: Vec<u8>,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        tokio::task::JoinHandle<String>,
    ) {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let header_end = loop {
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    break position + 4;
                }
                let count = stream
                    .read(&mut buffer)
                    .await
                    .expect("request headers should be read");
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            };
            let headers =
                std::str::from_utf8(&request[..header_end]).expect("headers should be UTF-8");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim())
                })
                .map(|value| value.parse::<usize>().expect("valid content length"))
                .unwrap_or(0);
            let request_end = header_end + content_length;
            while request.len() < request_end {
                let count = stream
                    .read(&mut buffer)
                    .await
                    .expect("request body should be read");
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).expect("request should be UTF-8");
            let _ = stream.write_all(&response).await;
            request
        });
        (socket_path, directory, server)
    }

    fn response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut result = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        result.extend_from_slice(body);
        result
    }

    fn cleanup(socket: &std::path::Path, directory: &std::path::Path) {
        std::fs::remove_file(socket).expect("socket should be removed");
        std::fs::remove_dir(directory).expect("directory should be removed");
    }

    #[tokio::test]
    async fn lists_volumes_from_versioned_endpoint_and_maps_allowlisted_fields() {
        let body = br#"{"Volumes":[{"Name":"data","Driver":"local","Scope":"local","Labels":{"secret":"label"},"Options":{"secret":"option"},"UsageData":{"Size":7},"Status":{"secret":"status"}}]}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let volumes = client(socket.clone())
            .list_volumes()
            .await
            .expect("inventory");
        let request = server.await.expect("server");
        assert!(request.starts_with("GET /v1.45/volumes HTTP/1.1\r\n"));
        assert_eq!(
            volumes,
            vec![VolumeSummary {
                name: "data".to_owned(),
                driver: Some("local".to_owned()),
                scope: Some("local".to_owned()),
            }]
        );
        assert!(!format!("{volumes:?}").contains("secret"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn creates_volume_with_name_only_on_versioned_endpoint_and_maps_allowlisted_fields() {
        let name = "ferrum-data";
        let body = br#"{"Name":"ferrum-data","Driver":"local","Scope":"local","CreatedAt":"created","Mountpoint":"/var/lib/docker/volumes/ferrum-data/_data","Labels":{"secret":"label"},"Options":{"secret":"option"},"Status":{"secret":"status"}}"#;
        let (socket, directory, server) = fixture(response("201 Created", body)).await;
        let details = client(socket.clone())
            .create_volume(name)
            .await
            .expect("the named volume should be created");
        let request = server.await.expect("server should finish");
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .expect("HTTP request should contain headers and a body");
        assert!(headers.starts_with("POST /v1.45/volumes/create HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("content-type: application/json")
        );
        let request_body: serde_json::Value =
            serde_json::from_str(body).expect("request body should be valid JSON");
        assert_eq!(request_body["Name"], name);
        assert_eq!(request_body.as_object().expect("JSON object").len(), 1);
        assert_eq!(
            details,
            VolumeDetails {
                name: name.to_owned(),
                driver: Some("local".to_owned()),
                scope: Some("local".to_owned()),
                created_at: Some("created".to_owned()),
                mountpoint: Some("/var/lib/docker/volumes/ferrum-data/_data".to_owned()),
            }
        );
        assert!(!format!("{details:?}").contains("secret"));
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn volume_creation_escapes_untrusted_display_metadata() {
        let body = br#"{"Name":"ferrum-data","Driver":"local\u202e-driver","Scope":"local\nscope","CreatedAt":"date\u200b","Mountpoint":"/host\u202e/path"}"#;
        let (socket, directory, server) = fixture(response("201 Created", body)).await;
        let details = client(socket.clone())
            .create_volume("ferrum-data")
            .await
            .expect("the matching volume should be created");
        server.await.expect("server should finish");

        assert_eq!(details.driver.as_deref(), Some("local\\u{202e}-driver"));
        assert_eq!(details.scope.as_deref(), Some("local\\u{a}scope"));
        assert_eq!(details.created_at.as_deref(), Some("date\\u{200b}"));
        assert_eq!(details.mountpoint.as_deref(), Some("/host\\u{202e}/path"));
        assert!(!format!("{details:?}").contains('\u{202e}'));
        assert!(!format!("{details:?}").contains('\u{200b}'));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_volume_create_names_before_connecting() {
        let directory = temporary_directory();
        let socket = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket).expect("Unix socket should bind");
        let engine = client(socket.clone());
        let too_long = "x".repeat(4097);
        for name in ["", "  ", ".", "..", too_long.as_str(), "volume\nname"] {
            assert!(matches!(
                engine.create_volume(name).await,
                Err(EngineError::InvalidVolumeName)
            ));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn volume_create_body_is_bounded_for_the_largest_accepted_name() {
        let name = "\"".repeat(4096);
        let response_body = serde_json::to_vec(&serde_json::json!({ "Name": name }))
            .expect("response should serialize");
        let (socket, directory, server) = fixture(response("201 Created", &response_body)).await;
        let details = client(socket.clone())
            .create_volume(&name)
            .await
            .expect("the longest accepted name should be bounded");
        let request = server.await.expect("server should finish");
        let (_, request_body) = request
            .split_once("\r\n\r\n")
            .expect("HTTP request should contain a body");
        assert!(request_body.len() <= MAX_VOLUME_CREATE_REQUEST_BYTES);
        let decoded_request: serde_json::Value =
            serde_json::from_str(request_body).expect("request body should be valid JSON");
        assert_eq!(decoded_request["Name"], name);
        assert_eq!(decoded_request.as_object().expect("JSON object").len(), 1);
        assert_eq!(details.name, name);
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn volume_create_rejections_and_responses_are_safe() {
        let private_body = b"private daemon response volume-secret";
        let (socket, directory, server) = fixture(response("409 Conflict", private_body)).await;
        let error = client(socket.clone())
            .create_volume("volume-secret")
            .await
            .expect_err("daemon rejection should be surfaced");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::VolumeCreationRejected { status: 409, .. }
        ));
        assert!(!format!("{error:?}").contains("private daemon response"));
        assert!(!error.to_string().contains("volume-secret"));
        cleanup(&socket, &directory);

        let (socket, directory, server) =
            fixture(response("201 Created", br#"{"Name":"other-volume"}"#)).await;
        let error = client(socket.clone())
            .create_volume("expected-volume")
            .await
            .expect_err("a response for a different name should fail");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::VolumeCreationUnavailable { .. }
        ));
        assert!(!error.to_string().contains("other-volume"));
        cleanup(&socket, &directory);

        let oversized = vec![b' '; MAX_VOLUME_CREATE_RESPONSE_BYTES + 1];
        let (socket, directory, server) = fixture(response("201 Created", &oversized)).await;
        assert!(matches!(
            client(socket.clone()).create_volume("volume").await,
            Err(EngineError::VolumeCreationUnavailable { .. })
        ));
        server.await.expect("server should finish");
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn removes_volume_through_versioned_name_path_without_force() {
        let (socket, directory, server) = fixture(response("204 No Content", b"")).await;
        client(socket.clone())
            .remove_volume("data")
            .await
            .expect("an unused volume should be removed");
        let request = server.await.expect("server should finish");
        assert!(request.starts_with("DELETE /v1.45/volumes/data HTTP/1.1\r\n"));
        let request_line = request.lines().next().expect("HTTP request line");
        assert!(!request_line.contains('?'));
        assert!(!request.to_ascii_lowercase().contains("force"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn volume_removal_rejection_is_typed_and_redacted() {
        let private_body = b"private daemon body volume-secret";
        let (socket, directory, server) = fixture(response("409 Conflict", private_body)).await;
        let error = client(socket.clone())
            .remove_volume("volume-secret")
            .await
            .expect_err("an in-use volume should be rejected");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::VolumeRemovalRejected { status: 409, .. }
        ));
        assert!(!format!("{error:?}").contains("private daemon body"));
        assert!(!error.to_string().contains("volume-secret"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_volume_removal_names_before_connecting() {
        let directory = temporary_directory();
        let socket = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket).expect("Unix socket should bind");
        let engine = client(socket.clone());
        let too_long = "x".repeat(4097);
        for name in ["", "  ", ".", "..", too_long.as_str(), "volume\nname"] {
            assert!(matches!(
                engine.remove_volume(name).await,
                Err(EngineError::InvalidVolumeName)
            ));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn accepts_missing_or_null_volume_lists_as_empty() {
        for body in [br#"{}"#.as_slice(), br#"{"Volumes":null}"#.as_slice()] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert!(
                client(socket.clone())
                    .list_volumes()
                    .await
                    .expect("inventory")
                    .is_empty()
            );
            let request = server.await.expect("server");
            assert!(request.starts_with("GET /v1.45/volumes HTTP/1.1\r\n"));
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn rejects_volume_list_entries_without_safe_names() {
        for body in [
            br#"{"Volumes":[{"Driver":"local","Scope":"local"}]}"#.as_slice(),
            br#"{"Volumes":[{"Name":"","Driver":"local","Scope":"local"}]}"#.as_slice(),
            br#"{"Volumes":[{"Name":"..","Driver":"local","Scope":"local"}]}"#.as_slice(),
            br#"{"Volumes":[{"Name":"   ","Driver":"local","Scope":"local"}]}"#.as_slice(),
            br#"{"Volumes":[{"Name":"unsafe\u202ename","Driver":"local","Scope":"local"}]}"#
                .as_slice(),
        ] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert!(matches!(
                client(socket.clone()).list_volumes().await,
                Err(EngineError::VolumeInventoryUnavailable { .. })
            ));
            server.await.expect("server");
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn inspects_encoded_volume_name_and_maps_allowlisted_fields() {
        let body = br#"{"Name":"a/b c","Driver":"local","Scope":"local","CreatedAt":"created","Mountpoint":"/host/data","Labels":{"secret":"label"},"Options":{"secret":"option"},"UsageData":{"Size":7},"Status":{"secret":"status"}}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let details = client(socket.clone())
            .inspect_volume("a/b c")
            .await
            .expect("volume details");
        let request = server.await.expect("server");
        assert!(request.starts_with("GET /v1.45/volumes/a%2Fb%20c HTTP/1.1\r\n"));
        assert_eq!(
            details,
            VolumeDetails {
                name: "a/b c".to_owned(),
                driver: Some("local".to_owned()),
                scope: Some("local".to_owned()),
                created_at: Some("created".to_owned()),
                mountpoint: Some("/host/data".to_owned()),
            }
        );
        assert!(!format!("{details:?}").contains("secret"));
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn volume_inspection_escapes_metadata_and_requires_exact_name() {
        let body = br#"{"Name":"data","Driver":"local\u202e-driver","Scope":"local\nscope","CreatedAt":"date\u200b","Mountpoint":"/host\u202e/path"}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let details = client(socket.clone())
            .inspect_volume("data")
            .await
            .expect("matching volume details should be safe to display");
        server.await.expect("server should finish");

        assert_eq!(details.driver.as_deref(), Some("local\\u{202e}-driver"));
        assert_eq!(details.scope.as_deref(), Some("local\\u{a}scope"));
        assert_eq!(details.created_at.as_deref(), Some("date\\u{200b}"));
        assert_eq!(details.mountpoint.as_deref(), Some("/host\\u{202e}/path"));
        assert!(!format!("{details:?}").contains('\u{202e}'));
        cleanup(&socket, &directory);

        let (socket, directory, server) =
            fixture(response("200 OK", br#"{"Name":"other-volume"}"#)).await;
        let error = client(socket.clone())
            .inspect_volume("data")
            .await
            .expect_err("inspect response must identify the requested name");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::VolumeInspectionUnavailable { .. }
        ));
        assert!(!error.to_string().contains("other-volume"));
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn rejects_whitespace_only_inspection_names_before_connecting() {
        let directory = temporary_directory();
        let socket = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket).expect("Unix socket should bind");
        for name in [" ", "\t"] {
            assert!(matches!(
                client(socket.clone()).inspect_volume(name).await,
                Err(EngineError::InvalidVolumeName)
            ));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn reports_usage_only_for_an_exact_volume_name_and_keeps_allowlisted_fields() {
        let body = br#"{"VolumeUsage":{"Items":[{"Name":"data-copy","UsageData":{"Size":900,"RefCount":8},"Labels":{"secret":"label"}},{"Name":"data","UsageData":{"Size":7,"RefCount":2},"Options":{"secret":"option"},"Status":{"secret":"status"},"extra":"secret"}]}}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let usage = client(socket.clone())
            .volume_usage("data")
            .await
            .expect("volume usage");
        let request = server.await.expect("server");
        assert!(request.starts_with("GET /v1.45/system/df?verbose=true&type=volume HTTP/1.1\r\n"));
        assert_eq!(
            usage,
            Some(VolumeUsage {
                size_bytes: Some(7),
                reference_count: Some(2),
            })
        );
        assert!(!format!("{usage:?}").contains("secret"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn leaves_ambiguous_duplicate_volume_usage_unavailable() {
        for body in [
            br#"{"VolumeUsage":{"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":0},"Name":"other","Name":"data"}]}}"#.as_slice(),
            br#"{"VolumeUsage":{"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":0},"UsageData":{"Size":9,"RefCount":2}}]}}"#.as_slice(),
            br#"{"VolumeUsage":{"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":0}},{"Name":"data","UsageData":{"Size":9,"RefCount":2}}]}}"#.as_slice(),
        ] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert_eq!(
                client(socket.clone())
                    .volume_usage("data")
                    .await
                    .expect("valid HTTP response with ambiguous usage"),
                Some(VolumeUsage {
                    size_bytes: None,
                    reference_count: None,
                })
            );
            server.await.expect("server should finish");
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn rejects_duplicate_volume_usage_envelopes() {
        for body in [
            br#"{"VolumeUsage":{"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":3}}]},"VolumeUsage":{"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":0}}]}}"#.as_slice(),
            br#"{"VolumeUsage":{"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":3}}],"Items":[{"Name":"data","UsageData":{"Size":7,"RefCount":0}}]}}"#.as_slice(),
        ] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert_eq!(
                client(socket.clone())
                    .volume_usage("data")
                    .await
                    .expect("valid HTTP response with duplicate wrapper"),
                None
            );
            server.await.expect("server should finish");
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn maps_missing_negative_and_zero_usage_values() {
        for (name, body, expected) in [
            (
                "missing-data",
                br#"{"VolumeUsage":{"Items":[{"Name":"missing-data"}]}}"#.as_slice(),
                VolumeUsage {
                    size_bytes: None,
                    reference_count: None,
                },
            ),
            (
                "missing-fields",
                br#"{"VolumeUsage":{"Items":[{"Name":"missing-fields","UsageData":{"Size":12}}]}}"#.as_slice(),
                VolumeUsage {
                    size_bytes: Some(12),
                    reference_count: None,
                },
            ),
            (
                "negative",
                br#"{"VolumeUsage":{"Items":[{"Name":"negative","UsageData":{"Size":-1,"RefCount":-2}}]}}"#.as_slice(),
                VolumeUsage {
                    size_bytes: None,
                    reference_count: None,
                },
            ),
            (
                "zero",
                br#"{"VolumeUsage":{"Items":[{"Name":"zero","UsageData":{"Size":0,"RefCount":0}}]}}"#.as_slice(),
                VolumeUsage {
                    size_bytes: Some(0),
                    reference_count: Some(0),
                },
            ),
        ] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert_eq!(
                client(socket.clone())
                    .volume_usage(name)
                    .await
                    .expect("volume usage"),
                Some(expected),
                "usage for {name}"
            );
            server.await.expect("server");
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn returns_none_when_the_requested_volume_is_absent_from_the_snapshot() {
        let body = br#"{"VolumeUsage":{"Items":[{"Name":"data-copy","UsageData":{"Size":4,"RefCount":1}},{"Name":"database","UsageData":{"Size":5,"RefCount":2}}]}}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        assert_eq!(
            client(socket.clone())
                .volume_usage("data")
                .await
                .expect("volume usage"),
            None
        );
        server.await.expect("server");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn maps_volume_usage_request_decode_size_and_status_failures_safely() {
        for (status, body) in [
            (
                "500 Internal Server Error",
                br#"{"message":"private daemon body"}"#.as_slice(),
            ),
            ("200 OK", b"{".as_slice()),
        ] {
            let (socket, directory, server) = fixture(response(status, body)).await;
            let error = client(socket.clone())
                .volume_usage("data")
                .await
                .expect_err("invalid response should fail");
            server.await.expect("server");
            assert!(matches!(error, EngineError::VolumeUsageUnavailable { .. }));
            assert!(!error.to_string().contains("private"));
            assert!(!format!("{error:?}").contains("private"));
            cleanup(&socket, &directory);
        }

        let body = vec![b' '; MAX_LIST_RESPONSE_BYTES + 1];
        let (socket, directory, server) = fixture(response("200 OK", &body)).await;
        assert!(matches!(
            client(socket.clone()).volume_usage("data").await,
            Err(EngineError::VolumeUsageUnavailable { .. })
        ));
        server.await.expect("server");
        cleanup(&socket, &directory);

        let error = client(std::path::PathBuf::from(
            "/path/that/does/not/exist/engine.sock",
        ))
        .volume_usage("data")
        .await
        .expect_err("unavailable socket should fail");
        assert!(matches!(error, EngineError::VolumeUsageUnavailable { .. }));
    }

    #[tokio::test]
    async fn rejects_invalid_usage_names_without_connecting_to_the_socket() {
        let directory = temporary_directory();
        let socket = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket).expect("Unix socket should bind");
        let engine = client(socket.clone());
        let too_long = "x".repeat(4097);
        for name in ["", ".", "..", too_long.as_str()] {
            assert!(matches!(
                engine.volume_usage(name).await,
                Err(EngineError::InvalidVolumeName)
            ));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_inspection_names_before_socket_connection() {
        let engine = client(std::path::PathBuf::from("/path/that/does/not/exist"));
        let too_long = "x".repeat(4097);
        for name in ["", ".", "..", too_long.as_str()] {
            assert!(matches!(
                engine.inspect_volume(name).await,
                Err(EngineError::InvalidVolumeName)
            ));
        }
    }

    #[tokio::test]
    async fn rejects_inspection_response_without_volume_name() {
        let (socket, directory, server) =
            fixture(response("200 OK", br#"{"Driver":"local"}"#)).await;
        let error = client(socket.clone())
            .inspect_volume("data")
            .await
            .expect_err("inspection without a name should be rejected");
        server.await.expect("server");
        assert!(matches!(
            error,
            EngineError::VolumeInspectionUnavailable { .. }
        ));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn maps_http_and_malformed_volume_responses_to_safe_errors() {
        for (status, body) in [
            (
                "500 Internal Server Error",
                br#"{"message":"private"}"#.as_slice(),
            ),
            ("200 OK", b"{".as_slice()),
        ] {
            let (socket, directory, server) = fixture(response(status, body)).await;
            let result = client(socket.clone()).list_volumes().await;
            server.await.expect("server");
            assert!(matches!(
                result,
                Err(EngineError::VolumeInventoryUnavailable { .. })
            ));
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn rejects_oversized_volume_inventory_safely() {
        let body = vec![b' '; MAX_LIST_RESPONSE_BYTES + 1];
        let (socket, directory, server) = fixture(response("200 OK", &body)).await;
        assert!(matches!(
            client(socket.clone()).list_volumes().await,
            Err(EngineError::VolumeInventoryUnavailable { .. })
        ));
        server.await.expect("server");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn inspects_volume_rejects_http_and_oversized_responses_safely() {
        let (socket, directory, server) =
            fixture(response("404 Not Found", b"private daemon body")).await;
        let error = client(socket.clone())
            .inspect_volume("volume")
            .await
            .expect_err("HTTP error should be rejected");
        server.await.expect("server");
        assert!(matches!(
            error,
            EngineError::VolumeInspectionUnavailable { .. }
        ));
        assert!(!error.to_string().contains("private"));
        cleanup(&socket, &directory);

        let body = vec![b' '; MAX_INSPECT_RESPONSE_BYTES + 1];
        let (socket, directory, server) = fixture(response("200 OK", &body)).await;
        assert!(matches!(
            client(socket.clone()).inspect_volume("volume").await,
            Err(EngineError::VolumeInspectionUnavailable { .. })
        ));
        server.await.expect("server");
        cleanup(&socket, &directory);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod container_logs_http_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    use ferrum_core::{ContainerLogLine, ContainerLogSource};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{UnixListener, UnixStream},
    };

    fn temporary_directory() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "ferrum-logs-{}-{timestamp}-{sequence}",
            std::process::id(),
        ));
        std::fs::create_dir(&directory).expect("temporary directory should be created");
        directory
    }

    fn client(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn read_request(stream: &mut UnixStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream
                .read(&mut buffer)
                .await
                .expect("request should be readable");
            assert_ne!(count, 0, "client should send HTTP headers");
            request.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8(request).expect("HTTP request should be UTF-8")
    }

    async fn fixture_socket() -> (std::path::PathBuf, std::path::PathBuf, UnixListener) {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        (socket_path, directory, listener)
    }

    fn cleanup(socket_path: &std::path::Path, directory: &std::path::Path) {
        std::fs::remove_file(socket_path).expect("socket should be removed");
        std::fs::remove_dir(directory).expect("directory should be removed");
    }

    fn frame(source: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![source, 0, 0, 0];
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    async fn write_fragmented(stream: &mut UnixStream, bytes: &[u8]) {
        for byte in bytes {
            stream
                .write_all(&[*byte])
                .await
                .expect("fragment should be written");
            tokio::task::yield_now().await;
        }
    }

    fn line(source: ContainerLogSource, text: &str) -> ContainerLogLine {
        ContainerLogLine {
            source,
            text: text.to_owned(),
        }
    }
    fn inspect_response(json: &str) -> InspectResponse {
        serde_json::from_str(json).expect("inspect response should deserialize")
    }

    #[tokio::test]
    async fn rejects_invalid_options_and_container_ids_before_transport() {
        let directory = temporary_directory();
        let engine = client(directory.join("missing.sock"));
        for options in [
            ContainerLogsOptions {
                stdout: false,
                stderr: false,
                ..ContainerLogsOptions::default()
            },
            ContainerLogsOptions {
                tail: 0,
                ..ContainerLogsOptions::default()
            },
            ContainerLogsOptions {
                tail: 5_001,
                ..ContainerLogsOptions::default()
            },
        ] {
            let error = match engine
                .open_container_logs("container", false, options)
                .await
            {
                Err(error) => error,
                Ok(_) => panic!("invalid log options should fail before transport"),
            };
            assert!(matches!(error, EngineError::InvalidContainerLogsOptions));
        }

        let error = match engine
            .open_container_logs("", false, ContainerLogsOptions::default())
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("empty container ID should fail before transport"),
        };
        assert!(matches!(error, EngineError::InvalidContainerId));

        let error = match engine
            .open_container_logs("container", false, ContainerLogsOptions::default())
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("missing socket should fail"),
        };
        assert!(matches!(
            error,
            EngineError::Unavailable {
                endpoint_label: "test endpoint"
            }
        ));
        std::fs::remove_dir(directory).expect("temporary directory should be removed");
    }

    #[tokio::test]
    async fn opens_logs_with_negotiated_path_encoded_id_and_ordered_query() {
        let (socket, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let request = read_request(&mut stream).await;
            assert!(request.starts_with(
                "GET /v1.45/containers/a%2Fb%20c/logs?stdout=true&stderr=false&timestamps=true&follow=false&tail=17 HTTP/1.1\r\n"
            ), "{request}");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .expect("response should be written");
        });
        let result = client(socket.clone())
            .open_container_logs(
                "a/b c",
                false,
                ContainerLogsOptions {
                    stdout: true,
                    stderr: false,
                    timestamps: true,
                    follow: false,
                    tail: 17,
                },
            )
            .await;
        server.await.expect("fixture server should finish");
        assert!(result.is_ok(), "empty stream should open");
        drop(result);
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_http_500_without_exposing_response_body() {
        let (socket, directory, listener) = fixture_socket().await;
        let secret = "private-daemon-response";
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{secret}",
                secret.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("response should be written");
        });
        let error = match client(socket.clone())
            .open_container_logs("container", false, ContainerLogsOptions::default())
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("500 response should fail"),
        };
        server.await.expect("fixture server should finish");
        assert!(matches!(
            error,
            EngineError::ContainerLogsUnavailable {
                endpoint_label: "test endpoint",
                status: 500
            }
        ));
        assert!(!format!("{error:?}").contains(secret));
        assert!(!error.to_string().contains(secret));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn decodes_interleaved_multiplexed_lines_and_preserves_timestamp_prefixes() {
        let (socket, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            write_fragmented(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .await;
            for (stream_id, payload) in [
                (1, b"2026-01-02T03:04:05Z out\n".as_slice()),
                (2, b"2026-01-02T03:04:06Z err\n".as_slice()),
            ] {
                let data = frame(stream_id, payload);
                let chunk = format!("{:X}\r\n", data.len());
                write_fragmented(&mut stream, chunk.as_bytes()).await;
                write_fragmented(&mut stream, &data).await;
                write_fragmented(&mut stream, b"\r\n").await;
            }
            write_fragmented(&mut stream, b"0\r\n\r\n").await;
        });
        let mut logs = client(socket.clone())
            .open_container_logs("container", false, ContainerLogsOptions::default())
            .await
            .expect("stream should open");
        let first = logs.next_line().await.expect("first line should parse");
        let second = logs.next_line().await.expect("second line should parse");
        let end = logs.next_line().await.expect("stream should finish");
        server.await.expect("fixture server should finish");
        assert_eq!(
            first,
            Some(line(ContainerLogSource::Stdout, "2026-01-02T03:04:05Z out"))
        );
        assert_eq!(
            second,
            Some(line(ContainerLogSource::Stderr, "2026-01-02T03:04:06Z err"))
        );
        assert_eq!(end, None);
        drop(logs);
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn keeps_partial_stdout_and_stderr_lines_separate_across_frames() {
        let (socket, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            let mut body = frame(1, b"out");
            body.extend(frame(2, b"err\n"));
            body.extend(frame(1, b"put"));
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(headers.as_bytes())
                .await
                .expect("headers should be written");
            stream
                .write_all(&body)
                .await
                .expect("body should be written");
        });
        let mut logs = client(socket.clone())
            .open_container_logs("container", false, ContainerLogsOptions::default())
            .await
            .expect("stream should open");

        assert_eq!(
            logs.next_line().await.expect("stderr line should parse"),
            Some(line(ContainerLogSource::Stderr, "err"))
        );
        assert_eq!(
            logs.next_line().await.expect("stdout line should parse"),
            Some(line(ContainerLogSource::Stdout, "output"))
        );
        assert_eq!(logs.next_line().await.expect("stream should finish"), None);
        server.await.expect("fixture server should finish");
        drop(logs);
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn decodes_raw_tty_lines_and_replaces_invalid_utf8_lossily() {
        let (socket, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            let body = b"hello\r\n\xffx\nlast";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            write_fragmented(&mut stream, response.as_bytes()).await;
            write_fragmented(&mut stream, body).await;
        });
        let mut logs = client(socket.clone())
            .open_container_logs("container", true, ContainerLogsOptions::default())
            .await
            .expect("stream should open");
        assert_eq!(
            logs.next_line().await.expect("line should parse"),
            Some(line(ContainerLogSource::Combined, "hello"))
        );
        assert_eq!(
            logs.next_line().await.expect("line should parse"),
            Some(line(ContainerLogSource::Combined, "�x"))
        );
        assert_eq!(
            logs.next_line()
                .await
                .expect("unterminated final TTY line should flush"),
            Some(line(ContainerLogSource::Combined, "last"))
        );
        assert_eq!(logs.next_line().await.expect("stream should finish"), None);
        server.await.expect("fixture server should finish");
        drop(logs);
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_stream_headers_and_oversized_frames_or_lines() {
        let invalid = frame(9, b"bad\n");
        let oversized_frame = {
            let mut bytes = vec![1, 0, 0, 0];
            bytes.extend_from_slice(&(1024_u32 * 1024 + 1).to_be_bytes());
            bytes
        };
        for response_body in [
            invalid,
            oversized_frame,
            frame(1, &vec![b'x'; 64 * 1024 + 1]),
        ] {
            let (socket, directory, listener) = fixture_socket().await;
            let body_len = response_body.len();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let _ = read_request(&mut stream).await;
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
                );
                stream
                    .write_all(headers.as_bytes())
                    .await
                    .expect("headers should be written");
                stream
                    .write_all(&response_body)
                    .await
                    .expect("body should be written");
            });
            let mut logs = client(socket.clone())
                .open_container_logs("container", false, ContainerLogsOptions::default())
                .await
                .expect("stream should open");
            assert!(
                logs.next_line().await.is_err(),
                "invalid header or exceeded bound should fail"
            );
            drop(logs);
            server.await.expect("fixture server should finish");
            cleanup(&socket, &directory);
        }
    }
    #[tokio::test]
    async fn rejects_lossy_expansion_beyond_the_line_output_limit() {
        let (socket, directory, listener) = fixture_socket().await;
        let mut payload = vec![0xff; (64 * 1024 / 3) + 1];
        payload.push(b'\n');
        let body = frame(1, &payload);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(headers.as_bytes())
                .await
                .expect("headers should be written");
            stream
                .write_all(&body)
                .await
                .expect("body should be written");
        });
        let mut logs = client(socket.clone())
            .open_container_logs("container", false, ContainerLogsOptions::default())
            .await
            .expect("stream should open");
        let error = logs
            .next_line()
            .await
            .expect_err("lossy-expanded lines must remain within the UI message bound");
        assert!(matches!(
            error,
            EngineError::ContainerLogStreamInterrupted {
                endpoint_label: "test endpoint"
            }
        ));
        drop(logs);
        server.await.expect("fixture server should finish");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn dropping_open_stream_cancels_body_read() {
        let (socket, directory, listener) = fixture_socket().await;
        let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n").await.expect("headers should be written");
            let mut byte = [0];
            let closed =
                tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut byte))
                    .await
                    .expect("dropping stream should close socket")
                    .map(|count| count == 0)
                    .unwrap_or(false);
            let _ = closed_tx.send(closed);
        });
        let logs = client(socket.clone())
            .open_container_logs("container", false, ContainerLogsOptions::default())
            .await
            .expect("stream should open");
        drop(logs);
        assert!(closed_rx.await.expect("server should observe closure"));
        server.await.expect("fixture server should finish");
        cleanup(&socket, &directory);
    }

    #[test]
    fn maps_config_tty_true_false_and_absence_without_defaulting() {
        let true_details = map_container_details(
            inspect_response(r#"{"Id":"a","Config":{"Tty":true}}"#),
            "test",
        )
        .expect("id is present");
        let false_details = map_container_details(
            inspect_response(r#"{"Id":"b","Config":{"Tty":false}}"#),
            "test",
        )
        .expect("id is present");
        let absent_details =
            map_container_details(inspect_response(r#"{"Id":"c","Config":{}}"#), "test")
                .expect("id is present");
        assert_eq!(true_details.tty, Some(true));
        assert_eq!(false_details.tty, Some(false));
        assert_eq!(absent_details.tty, None);
    }
}
#[cfg(all(test, target_os = "linux"))]
mod container_lifecycle_http_tests {
    use super::*;
    use ferrum_core::ContainerAction;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "ferrum-lifecycle-{}-{timestamp}-{sequence}",
            std::process::id(),
        ));
        std::fs::create_dir(&directory).expect("temporary directory should be created");
        directory
    }

    fn client(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn read_request(stream: &mut tokio::net::UnixStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream
                .read(&mut buffer)
                .await
                .expect("request should be readable");
            assert_ne!(count, 0, "client should send HTTP headers");
            request.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8(request).expect("HTTP request should be UTF-8")
    }

    async fn fixture_socket() -> (std::path::PathBuf, std::path::PathBuf, UnixListener) {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        (socket_path, directory, listener)
    }

    fn cleanup(socket_path: &std::path::Path, directory: &std::path::Path) {
        std::fs::remove_file(socket_path).expect("socket should be removed");
        std::fs::remove_dir(directory).expect("temporary directory should be removed");
    }

    #[tokio::test]
    async fn lifecycle_actions_use_the_expected_http_method_and_path() {
        let (socket_path, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            let requests = [
                (
                    ContainerAction::Start,
                    "POST",
                    "/v1.45/containers/cid/start",
                ),
                (ContainerAction::Stop, "POST", "/v1.45/containers/cid/stop"),
                (
                    ContainerAction::Restart,
                    "POST",
                    "/v1.45/containers/cid/restart",
                ),
                (
                    ContainerAction::Pause,
                    "POST",
                    "/v1.45/containers/cid/pause",
                ),
                (
                    ContainerAction::Unpause,
                    "POST",
                    "/v1.45/containers/cid/unpause",
                ),
                (ContainerAction::Remove, "DELETE", "/v1.45/containers/cid"),
            ];
            for (action, method, path) in requests {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let request = read_request(&mut stream).await;
                assert!(
                    request.starts_with(&format!("{method} {path} HTTP/1.1\r\n")),
                    "unexpected lifecycle request: {request}"
                );
                assert!(
                    !request.lines().next().unwrap_or_default().contains('?'),
                    "lifecycle request must not add query parameters"
                );
                stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .expect("success response should be written");
                let _ = action;
            }
        });

        let engine = client(socket_path.clone());
        for action in [
            ContainerAction::Start,
            ContainerAction::Stop,
            ContainerAction::Restart,
            ContainerAction::Pause,
            ContainerAction::Unpause,
            ContainerAction::Remove,
        ] {
            engine
                .perform_container_action("cid", action)
                .await
                .expect("204 should succeed for every lifecycle action");
        }
        server.await.expect("fixture server should finish");
        cleanup(&socket_path, &directory);
    }

    #[tokio::test]
    async fn start_and_stop_treat_not_modified_as_idempotent_success() {
        for action in [ContainerAction::Start, ContainerAction::Stop] {
            let (socket_path, directory, listener) = fixture_socket().await;
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let _ = read_request(&mut stream).await;
                stream
                    .write_all(b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .expect("304 response should be written");
            });
            client(socket_path.clone())
                .perform_container_action("cid", action)
                .await
                .expect("start and stop should treat 304 as success");
            server.await.expect("fixture server should finish");
            cleanup(&socket_path, &directory);
        }
    }

    #[tokio::test]
    async fn rejected_action_error_is_typed_and_does_not_expose_response_body() {
        let (socket_path, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let _ = read_request(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 409 Conflict\r\nContent-Length: 32\r\nConnection: close\r\n\r\nprivate engine response body",
                )
                .await
                .expect("rejection response should be written");
        });

        let error = client(socket_path.clone())
            .perform_container_action("cid", ContainerAction::Pause)
            .await
            .expect_err("409 should be rejected");
        assert!(matches!(
            error,
            EngineError::ContainerActionRejected {
                action: ContainerAction::Pause,
                endpoint_label: "test endpoint",
                status: 409
            }
        ));
        assert!(
            !error.to_string().contains("private engine response body"),
            "user-facing errors must not expose untrusted response bodies"
        );
        server.await.expect("fixture server should finish");
        cleanup(&socket_path, &directory);
    }

    #[tokio::test]
    async fn action_path_escapes_container_id_and_remove_has_no_force_or_volume_query() {
        let (socket_path, directory, listener) = fixture_socket().await;
        let server = tokio::spawn(async move {
            for (method, expected_path) in [
                ("POST", "/v1.45/containers/id%2Fwith%20space/start"),
                ("DELETE", "/v1.45/containers/cid"),
            ] {
                let (mut stream, _) = listener.accept().await.expect("client should connect");
                let request = read_request(&mut stream).await;
                assert!(
                    request.starts_with(&format!("{method} {expected_path} HTTP/1.1\r\n")),
                    "unexpected escaped lifecycle request: {request}"
                );
                assert!(
                    !request.lines().next().unwrap_or_default().contains('?'),
                    "remove must not include force or volume query parameters"
                );
                stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .expect("success response should be written");
            }
        });

        let engine = client(socket_path.clone());
        engine
            .perform_container_action("id/with space", ContainerAction::Start)
            .await
            .expect("escaped ID action should succeed");
        engine
            .perform_container_action("cid", ContainerAction::Remove)
            .await
            .expect("remove without query parameters should succeed");
        server.await.expect("fixture server should finish");
        cleanup(&socket_path, &directory);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod container_change_stream_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "ferrum-events-{}-{timestamp}-{sequence}",
            std::process::id(),
        ));
        std::fs::create_dir(&directory).expect("temporary directory should be created");
        directory
    }

    fn client(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn request_headers(stream: &mut tokio::net::UnixStream) {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream
                .read(&mut buffer)
                .await
                .expect("request should be readable");
            assert_ne!(count, 0, "client should send HTTP headers");
            request.extend_from_slice(&buffer[..count]);
        }
        assert!(
            request.starts_with(
                b"GET /v1.45/events?filters=%7B%22type%22%3A%5B%22container%22%5D%7D HTTP/1.1\r\n"
            ),
            "event subscription must filter to container events"
        );
    }

    fn cleanup(socket_path: &std::path::Path, directory: &std::path::Path) {
        std::fs::remove_file(socket_path).expect("socket should be removed");
        std::fs::remove_dir(directory).expect("temporary directory should be removed");
    }

    #[tokio::test]
    async fn parses_split_jsonl_events_incrementally_and_skips_irrelevant_types() {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            request_headers(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("event response headers should be written");
            for part in [
                br#"{"Type":"network","Action":"connect","Actor":{"ID":"ignored"}}"#.as_slice(),
                b"\n",
                br#"{"Type":"container","Action":"start","Actor":{"ID":"cont"#.as_slice(),
                br#"ainer-42"}}"#.as_slice(),
                b"\n",
            ] {
                let chunk = format!("{:X}\r\n", part.len());
                stream
                    .write_all(chunk.as_bytes())
                    .await
                    .expect("chunk header should be written");
                stream
                    .write_all(part)
                    .await
                    .expect("event fragment should be written");
                stream
                    .write_all(b"\r\n")
                    .await
                    .expect("chunk terminator should be written");
            }
            stream
                .write_all(b"0\r\n\r\n")
                .await
                .expect("chunked stream should finish");
        });

        let mut changes = client(socket_path.clone())
            .subscribe_container_changes()
            .await
            .expect("event subscription should start");
        let change = changes
            .next_change()
            .await
            .expect("event line should parse")
            .expect("container event should be emitted");
        assert_eq!(change.container_id, "container-42");
        assert_eq!(
            changes.next_change().await.expect("stream should finish"),
            None
        );
        server.await.expect("fixture server should finish");
        cleanup(&socket_path, &directory);
    }

    #[tokio::test]
    async fn rejects_a_jsonl_line_over_the_size_limit() {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            request_headers(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 65538\r\nConnection: close\r\n\r\n")
                .await
                .expect("event response headers should be written");
            let mut oversized_line = vec![b' '; 65 * 1024 + 1];
            oversized_line.push(b'\n');
            stream
                .write_all(&oversized_line)
                .await
                .expect("oversized event line should be written");
        });

        let mut changes = client(socket_path.clone())
            .subscribe_container_changes()
            .await
            .expect("event subscription should start");
        let error = changes
            .next_change()
            .await
            .expect_err("oversized event line should fail");
        assert!(matches!(
            error,
            EngineError::ContainerEventsInterrupted {
                endpoint_label: "test endpoint"
            }
        ));
        server.await.expect("fixture server should finish");
        cleanup(&socket_path, &directory);
    }

    #[tokio::test]
    async fn dropping_event_stream_cancels_the_http_body() {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            request_headers(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n")
                .await
                .expect("event response headers should be written");
            let mut response = [0; 1];
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                stream.read(&mut response),
            )
            .await
            .expect("dropping stream should promptly cancel response body")
            .expect("socket read should succeed")
        });

        let changes = client(socket_path.clone())
            .subscribe_container_changes()
            .await
            .expect("event subscription should start");
        drop(changes);
        assert_eq!(server.await.expect("fixture server should finish"), 0);
        cleanup(&socket_path, &directory);
    }
    #[test]
    fn rejects_oversized_aggregated_event_frames() {
        let mut pending = Vec::new();
        let frame = vec![b' '; MAX_EVENT_FRAME_BYTES + 1];
        assert!(process_event_data(&mut pending, &frame).is_err());
        assert!(pending.is_empty());
    }

    #[test]
    fn rejects_container_events_without_a_nonempty_id() {
        assert!(parse_container_event(br#"{"Type":"container","Actor":{"ID":""}}"#).is_err());
        assert!(parse_container_event(br#"{"Type":"container","Actor":{}}"#).is_err());
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
    #[test]
    fn rejects_direct_remote_docker_host_schemes_with_safe_typed_error() {
        for host in [
            "tcp://engine.example:2376",
            "http://engine.example:2375",
            "https://engine.example:2376",
            "ssh://user@engine.example",
            "HTTPS://engine.example:2376",
        ] {
            let error = local_socket_path_from_docker_host(host)
                .expect_err("explicit remote Docker endpoints must fail closed");
            assert!(matches!(error, EngineError::UnsupportedRemoteTransport));
            assert!(
                !error.to_string().contains("engine.example"),
                "configured endpoint details must not appear in user-facing errors"
            );
        }
    }

    #[tokio::test]
    async fn remote_direct_hosts_are_rejected_before_default_discovery() {
        for host in [
            "tcp://127.0.0.1:0",
            "http://127.0.0.1:0",
            "https://127.0.0.1:0",
            "ssh://127.0.0.1:0",
        ] {
            let result =
                EngineClient::connect_with_startup_environment(None, Some(host.to_owned())).await;
            assert!(
                matches!(result, Err(EngineError::UnsupportedRemoteTransport)),
                "explicit remote host {host} must be rejected before local discovery"
            );
        }
    }
    #[test]
    fn accepts_local_unix_docker_host_socket() {
        assert_eq!(
            local_socket_path_from_docker_host("unix:///tmp/docker.sock")
                .expect("a local Unix socket is supported"),
            PathBuf::from("/tmp/docker.sock")
        );
    }
}
#[cfg(all(test, target_os = "linux"))]
mod network_api_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "ferrum-networks-{}-{timestamp}-{sequence}",
            std::process::id(),
            timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos(),
        ));
        std::fs::create_dir(&directory).expect("temporary directory should be created");
        directory
    }

    fn client(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn fixture(
        response: Vec<u8>,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        tokio::task::JoinHandle<String>,
    ) {
        let directory = temporary_directory();
        let socket_path = directory.join("engine.sock");
        let listener = UnixListener::bind(&socket_path).expect("Unix socket should bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let header_end = loop {
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    break position + 4;
                }
                let count = stream
                    .read(&mut buffer)
                    .await
                    .expect("request should be read");
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            };
            let headers = std::str::from_utf8(&request[..header_end])
                .expect("request headers should be UTF-8");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim())
                })
                .map(|value| value.parse::<usize>().expect("valid content length"))
                .unwrap_or(0);
            let request_end = header_end + content_length;
            while request.len() < request_end {
                let count = stream
                    .read(&mut buffer)
                    .await
                    .expect("request body should be read");
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).expect("request should be UTF-8");
            let _ = stream.write_all(&response).await;
            request
        });
        (socket_path, directory, server)
    }

    fn response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut result = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        result.extend_from_slice(body);
        result
    }

    fn cleanup(socket: &std::path::Path, directory: &std::path::Path) {
        std::fs::remove_file(socket).expect("socket should be removed");
        std::fs::remove_dir(directory).expect("directory should be removed");
    }
    #[tokio::test]
    async fn creates_network_with_name_only_on_versioned_endpoint() {
        let (socket, directory, server) = fixture(response(
            "201 Created",
            br#"{"Id":"created-network-id","Warning":""}"#,
        ))
        .await;

        let id = client(socket.clone())
            .create_network("^")
            .await
            .expect("network should be created");
        let request = server.await.expect("server should finish");
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .expect("HTTP request should contain headers and a body");
        assert!(
            headers.starts_with("POST /v1.45/networks/create HTTP/1.1\r\n"),
            "unexpected create request: {headers}"
        );
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("content-type: application/json")
        );
        assert_eq!(body, r#"{"Name":"^"}"#);
        assert_eq!(id, "created-network-id");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn create_network_bounds_the_maximum_name_json_body() {
        let name = "\"".repeat(MAX_NETWORK_NAME_BYTES);
        let (socket, directory, server) =
            fixture(response("201 Created", br#"{"Id":"created-network-id"}"#)).await;

        let id = client(socket.clone())
            .create_network(&name)
            .await
            .expect("a maximum-length network name should be accepted");
        let request = server.await.expect("server should finish");
        let (_, body) = request
            .split_once("\r\n\r\n")
            .expect("HTTP request should contain a JSON body");
        let decoded: serde_json::Value =
            serde_json::from_str(body).expect("the request body should be valid JSON");
        assert_eq!(decoded["Name"], name);
        assert_eq!(decoded.as_object().expect("JSON object").len(), 1);
        assert!(body.len() <= MAX_NETWORK_CREATE_BODY_BYTES);
        assert_eq!(id, "created-network-id");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn create_network_http_failures_are_typed_and_redacted() {
        let rejection_body = b"private daemon response test-network";
        let (socket, directory, server) = fixture(response("409 Conflict", rejection_body)).await;
        let error = client(socket.clone())
            .create_network("test-network")
            .await
            .expect_err("HTTP rejection should fail");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::NetworkCreationRejected { status: 409, .. }
        ));
        let debug = format!("{error:?}");
        let display = error.to_string();
        assert!(!debug.contains("private daemon response"));
        assert!(!debug.contains("test-network"));
        assert!(!display.contains("private daemon response"));
        assert!(!display.contains("test-network"));
        cleanup(&socket, &directory);

        let (socket, directory, server) = fixture(response(
            "200 OK",
            br#"{"Id":"..","Warning":"private warning"}"#,
        ))
        .await;
        let error = client(socket.clone())
            .create_network("test-network")
            .await
            .expect_err("unsafe response ID should fail");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::NetworkCreationUnavailable { .. }
        ));
        assert!(!format!("{error:?}").contains("private warning"));
        cleanup(&socket, &directory);

        let oversized = vec![b' '; MAX_NETWORK_CREATE_RESPONSE_BYTES + 1];
        let (socket, directory, server) = fixture(response("201 Created", &oversized)).await;
        assert!(matches!(
            client(socket.clone()).create_network("test-network").await,
            Err(EngineError::NetworkCreationUnavailable { .. })
        ));
        server.await.expect("server should finish");
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn network_connect_disconnect_send_minimal_container_body_to_versioned_paths() {
        for action in ["connect", "disconnect"] {
            let (socket, directory, server) = fixture(response("200 OK", b"")).await;
            let network_id = "a".repeat(64);
            let container_id = "b".repeat(64);
            let engine = client(socket.clone());
            if action == "connect" {
                engine
                    .connect_container_to_network(&network_id, &container_id)
                    .await
                    .expect("connect should succeed");
            } else {
                engine
                    .disconnect_container_from_network(&network_id, &container_id)
                    .await
                    .expect("disconnect should succeed");
            }
            let request = server.await.expect("server should finish");
            let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
            assert!(headers.starts_with(&format!(
                "POST /v1.45/networks/{network_id}/{action} HTTP/1.1\r\n"
            )));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(body).unwrap(),
                serde_json::json!({"Container": container_id})
            );
            assert!(!body.contains("EndpointConfig"));
            assert!(!body.contains("Force"));
            assert!(!headers.to_ascii_lowercase().contains("force"));
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn network_attachment_rejects_invalid_ids_before_socket_access() {
        let engine = client(std::path::PathBuf::from("/path/that/does/not/exist"));
        let valid = "a".repeat(64);
        for invalid in ["", "short"] {
            assert!(matches!(
                engine.connect_container_to_network(invalid, &valid).await,
                Err(EngineError::InvalidNetworkId)
            ));
            assert!(matches!(
                engine
                    .disconnect_container_from_network(&valid, invalid)
                    .await,
                Err(EngineError::InvalidContainerId)
            ));
        }
        let invalid_hex = "g".repeat(64);
        assert!(matches!(
            engine
                .connect_container_to_network(&valid, &invalid_hex)
                .await,
            Err(EngineError::InvalidContainerId)
        ));
    }

    #[tokio::test]
    async fn rejected_network_attachment_is_status_only_and_redacted() {
        let network_id = "a".repeat(64);
        let container_id = "b".repeat(64);
        let (socket, directory, server) =
            fixture(response("409 Conflict", b"private daemon response")).await;
        let error = client(socket.clone())
            .connect_container_to_network(&network_id, &container_id)
            .await
            .expect_err("daemon rejection should be returned");
        server.await.expect("server");
        assert!(matches!(
            error,
            EngineError::NetworkAttachmentRejected {
                status: 409,
                action: "connect",
                ..
            }
        ));
        let message = format!("{error:?} {error}");
        assert!(!message.contains(&network_id));
        assert!(!message.contains(&container_id));
        assert!(!message.contains("private daemon response"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn removes_network_by_exact_id_without_force_prune_or_query() {
        let (socket, directory, server) = fixture(response("204 No Content", b"")).await;
        let id = "a".repeat(64);

        client(socket.clone())
            .remove_network(&id)
            .await
            .expect("an exact-ID network removal should succeed");
        let request = server.await.expect("server should finish");
        let (headers, body) = request
            .split_once("\r\n\r\n")
            .expect("HTTP request should contain headers");
        assert!(
            headers.starts_with(&format!("DELETE /v1.45/networks/{id} HTTP/1.1\r\n")),
            "unexpected network removal request: {headers}"
        );
        assert!(
            !headers.lines().next().unwrap_or_default().contains('?'),
            "network removal must not include query parameters"
        );
        assert!(body.is_empty(), "network removal must not send a body");
        assert!(!headers.to_ascii_lowercase().contains("force"));
        assert!(!headers.to_ascii_lowercase().contains("prune"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn network_removal_rejects_non_full_or_non_hex_ids_before_connecting() {
        let engine = client(std::path::PathBuf::from("/path/that/does/not/exist"));
        let non_hex = "g".repeat(64);
        for id in ["", "network-name", "..", "a/../b", non_hex.as_str()] {
            assert!(matches!(
                engine.remove_network(id).await,
                Err(EngineError::InvalidNetworkId)
            ));
        }
    }

    #[tokio::test]
    async fn rejected_network_removal_is_typed_and_redacts_id_and_daemon_body() {
        let id = "b".repeat(64);
        let (socket, directory, server) =
            fixture(response("409 Conflict", b"private daemon response")).await;
        let error = client(socket.clone())
            .remove_network(&id)
            .await
            .expect_err("the daemon rejection should be surfaced");
        server.await.expect("server should finish");
        assert!(matches!(
            error,
            EngineError::NetworkRemovalRejected { status: 409, .. }
        ));
        assert!(!format!("{error:?}").contains(&id));
        assert!(!format!("{error:?}").contains("private daemon response"));
        assert!(!error.to_string().contains(&id));
        assert!(!error.to_string().contains("private daemon response"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn invalid_network_names_are_rejected_before_socket_connection() {
        let engine = client(std::path::PathBuf::from("/path/that/does/not/exist"));
        let too_long = "n".repeat(MAX_NETWORK_NAME_BYTES + 1);
        let too_long_utf8 = "ü".repeat(MAX_NETWORK_NAME_BYTES / "ü".len() + 1);
        for name in [
            "",
            "  ",
            "\t",
            "newline\nname",
            "\0",
            too_long.as_str(),
            too_long_utf8.as_str(),
        ] {
            assert!(matches!(
                engine.create_network(name).await,
                Err(EngineError::InvalidNetworkName)
            ));
        }
    }

    #[tokio::test]
    async fn lists_networks_from_versioned_endpoint_and_maps_only_allowlisted_fields() {
        let body = br#"[{"Id":"network-id","Name":"bridge","Driver":"bridge","Scope":"local","Created":"created","Internal":false,"Attachable":true,"EnableIPv4":true,"EnableIPv6":false,"Labels":{"secret":"label"},"Options":{"secret":"option"},"IPAM":{"secret":"ipam"},"Ingress":true,"Status":{"secret":"status"}}]"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let networks = client(socket.clone())
            .list_networks()
            .await
            .expect("inventory");
        let request = server.await.expect("server");
        assert!(request.starts_with("GET /v1.45/networks HTTP/1.1\r\n"));
        assert_eq!(
            networks,
            vec![NetworkSummary {
                id: "network-id".to_owned(),
                name: "bridge".to_owned(),
                driver: Some("bridge".to_owned()),
                scope: Some("local".to_owned()),
                created_at: Some("created".to_owned()),
                internal: Some(false),
                attachable: Some(true),
                enable_ipv4: Some(true),
                enable_ipv6: Some(false),
            }]
        );
        assert!(!format!("{networks:?}").contains("secret"));
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn list_requires_nonempty_network_identity_and_preserves_missing_optionals() {
        for body in [
            br#"[{"Name":"bridge"}]"#.as_slice(),
            br#"[{"Id":"","Name":"bridge"}]"#.as_slice(),
            br#"[{"Id":"..","Name":"bridge"}]"#.as_slice(),
            br#"[{"Id":"id"}]"#.as_slice(),
            br#"[{"Id":"id","Name":""}]"#.as_slice(),
        ] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert!(matches!(
                client(socket.clone()).list_networks().await,
                Err(EngineError::NetworkInventoryUnavailable { .. })
            ));
            server.await.expect("server");
            cleanup(&socket, &directory);
        }

        let (socket, directory, server) =
            fixture(response("200 OK", br#"[{"Id":"id","Name":"bridge"}]"#)).await;
        let summary = client(socket.clone())
            .list_networks()
            .await
            .expect("inventory")
            .remove(0);
        assert_eq!(summary.driver, None);
        assert_eq!(summary.enable_ipv4, None);
        server.await.expect("server");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn inspection_encodes_id_and_discards_endpoint_keys_and_values() {
        let body = br#"{"Id":"network-id","Name":"bridge","Driver":"bridge","Scope":"local","Created":"created","Internal":true,"Attachable":false,"EnableIPv4":true,"EnableIPv6":true,"Containers":{"secret-container-id":{"Name":"secret-name","EndpointID":"secret-endpoint","MacAddress":"secret-mac","IPv4Address":"secret-ipv4","IPv6Address":"secret-ipv6"}},"Labels":{"secret":"label"},"Options":{"secret":"option"},"IPAM":{"secret":"ipam"},"Peers":[{"secret":"peer"}],"Services":{"secret":"service"},"Status":{"secret":"status"}}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let details = client(socket.clone())
            .inspect_network("a/b c")
            .await
            .expect("details");
        let request = server.await.expect("server");
        assert!(request.starts_with("GET /v1.45/networks/a%2Fb%20c HTTP/1.1\r\n"));
        assert_eq!(
            details,
            NetworkDetails {
                id: "network-id".to_owned(),
                name: "bridge".to_owned(),
                driver: Some("bridge".to_owned()),
                scope: Some("local".to_owned()),
                created_at: Some("created".to_owned()),
                internal: Some(true),
                attachable: Some(false),
                enable_ipv4: Some(true),
                enable_ipv6: Some(true),
                attached_container_count: Some(1),
                attached_container_ids: None,
            }
        );
        let debug = format!("{details:?}");
        for secret in [
            "secret-container-id",
            "secret-name",
            "secret-endpoint",
            "secret-mac",
            "secret-ipv4",
            "secret-ipv6",
        ] {
            assert!(!debug.contains(secret));
        }
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn missing_null_and_empty_containers_have_distinct_count_shapes() {
        for (containers, expected) in [
            ("", None),
            (r#","Containers":null"#, None),
            (r#","Containers":{}"#, Some(0)),
        ] {
            let body = format!(r#"{{"Id":"id","Name":"bridge"{containers}}}"#);
            let (socket, directory, server) = fixture(response("200 OK", body.as_bytes())).await;
            assert_eq!(
                client(socket.clone())
                    .inspect_network("id")
                    .await
                    .expect("details")
                    .attached_container_count,
                expected
            );
            server.await.expect("server");
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn inspection_retains_only_validated_attachment_ids_and_marks_incomplete_membership() {
        let id = "c".repeat(64);
        let body = format!(
            r#"{{"Id":"network","Name":"custom","Containers":{{"{id}":{{"Name":"private-name","EndpointID":"private-endpoint","MacAddress":"private-mac","IPv4Address":"private-ip"}}}}}}"#
        );
        let (socket, directory, server) = fixture(response("200 OK", body.as_bytes())).await;
        let details = client(socket.clone())
            .inspect_network("network")
            .await
            .expect("details");
        assert_eq!(details.attached_container_count, Some(1));
        assert_eq!(details.attached_container_ids, Some(vec![id.clone()]));
        let debug = format!("{details:?}");
        assert!(!debug.contains("private-name"));
        assert!(!debug.contains("private-endpoint"));
        assert!(!debug.contains("private-mac"));
        assert!(!debug.contains("private-ip"));
        server.await.expect("server");
        cleanup(&socket, &directory);

        let body = br#"{"Id":"network","Name":"custom","Containers":{"short-id":{"Name":"private-name"}}}"#;
        let (socket, directory, server) = fixture(response("200 OK", body)).await;
        let details = client(socket.clone())
            .inspect_network("network")
            .await
            .expect("details");
        assert_eq!(details.attached_container_count, Some(1));
        assert_eq!(details.attached_container_ids, None);
        assert!(!format!("{details:?}").contains("short-id"));
        server.await.expect("server");
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn rejects_invalid_network_ids_before_socket_connection() {
        let engine = client(std::path::PathBuf::from("/path/that/does/not/exist"));
        let too_long = "x".repeat(4097);
        for id in ["", ".", "..", too_long.as_str()] {
            assert!(matches!(
                engine.inspect_network(id).await,
                Err(EngineError::InvalidNetworkId)
            ));
        }
    }

    #[tokio::test]
    async fn rejects_inspection_without_nonempty_identity() {
        for body in [
            br#"{"Name":"bridge"}"#.as_slice(),
            br#"{"Id":"","Name":"bridge"}"#.as_slice(),
            br#"{"Id":"id"}"#.as_slice(),
            br#"{"Id":"id","Name":""}"#.as_slice(),
        ] {
            let (socket, directory, server) = fixture(response("200 OK", body)).await;
            assert!(matches!(
                client(socket.clone()).inspect_network("id").await,
                Err(EngineError::NetworkInspectionUnavailable { .. })
            ));
            server.await.expect("server");
            cleanup(&socket, &directory);
        }
    }

    #[tokio::test]
    async fn maps_http_malformed_and_oversized_network_responses_to_safe_errors() {
        for (status, body) in [
            ("404 Not Found", b"private daemon body".as_slice()),
            ("200 OK", b"{".as_slice()),
        ] {
            let (socket, directory, server) = fixture(response(status, body)).await;
            let error = client(socket.clone())
                .inspect_network("id")
                .await
                .expect_err("invalid response should fail");
            server.await.expect("server");
            assert!(matches!(
                error,
                EngineError::NetworkInspectionUnavailable { .. }
            ));
            assert!(!error.to_string().contains("private"));
            cleanup(&socket, &directory);
        }

        for (cap, method) in [
            (MAX_LIST_RESPONSE_BYTES, 0_u8),
            (MAX_INSPECT_RESPONSE_BYTES, 1_u8),
        ] {
            let body = vec![b' '; cap + 1];
            let (socket, directory, server) = fixture(response("200 OK", &body)).await;
            let result = if method == 0 {
                client(socket.clone()).list_networks().await.map(|_| ())
            } else {
                client(socket.clone())
                    .inspect_network("id")
                    .await
                    .map(|_| ())
            };
            assert!(result.is_err());
            server.await.expect("server");
            cleanup(&socket, &directory);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod live_volume_create_tests {
    use super::*;
    use std::{
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_VOLUME_NAME: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    #[ignore = "creates and removes one unique volume through the exact local Unix socket used by EngineClient"]
    async fn local_engine_creates_and_removes_unique_volume() {
        let contexts = EngineClient::available_contexts()
            .await
            .expect("Docker CLI context discovery should succeed");
        let context = contexts
            .iter()
            .find(|context| context.is_current())
            .expect("Docker CLI should report its current context");
        assert_eq!(
            context.transport(),
            ContextTransport::LocalUnix,
            "volume creation requires the current local Unix Docker context"
        );
        let socket_path = context
            .socket_path()
            .expect("a local Unix context should expose its socket path internally")
            .to_path_buf();
        let socket_uri = format!(
            "unix://{}",
            socket_path
                .to_str()
                .expect("the local socket path should be valid UTF-8")
        );
        let engine =
            EngineClient::connect_unix_socket(&socket_path, "selected local Docker Unix socket")
                .await
                .expect("the exact current local Docker socket should connect");

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let sequence = NEXT_VOLUME_NAME.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "ferrum-live-volume-{}-{timestamp}-{sequence}",
            std::process::id()
        );
        let existing_volumes = engine
            .list_volumes()
            .await
            .expect("the local volume inventory should be readable before creation");
        assert!(
            volume_name_is_available(&name, &existing_volumes),
            "the generated test volume name must not already exist"
        );

        let created = engine
            .create_volume(&name)
            .await
            .expect("the unique test-owned volume should be created");
        let created_volumes = engine
            .list_volumes()
            .await
            .expect("the local volume inventory should be readable after creation");
        let verified_name = verify_created_volume(&created, &name, &created_volumes)
            .expect("the response and inventory must identify the exact generated volume name");
        let mut volume = DisposableVolume::new(verified_name, socket_uri);
        let usage = engine
            .volume_usage(volume.name())
            .await
            .expect("the generated volume usage should be readable");
        assert_eq!(
            usage.and_then(|usage| usage.reference_count),
            Some(0),
            "the owned test volume should have no container references before removal"
        );

        volume.disarm();
        engine
            .remove_volume(volume.name())
            .await
            .expect("EngineClient should remove the verified test-owned volume");

        let remaining = engine
            .list_volumes()
            .await
            .expect("the local volume inventory should be readable after cleanup");
        assert!(
            !remaining.iter().any(|listed| listed.name == volume.name()),
            "the test-owned volume should be absent after exact-name cleanup"
        );
    }

    struct DisposableVolume {
        name: VerifiedVolumeName,
        socket_uri: String,
        cleanup_required: bool,
    }

    struct VerifiedVolumeName(String);

    impl VerifiedVolumeName {
        fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl DisposableVolume {
        fn new(name: VerifiedVolumeName, socket_uri: String) -> Self {
            Self {
                name,
                socket_uri,
                cleanup_required: true,
            }
        }

        fn name(&self) -> &str {
            self.name.as_str()
        }

        fn disarm(&mut self) {
            self.cleanup_required = false;
        }
    }

    impl Drop for DisposableVolume {
        fn drop(&mut self) {
            if self.cleanup_required {
                let _ = Command::new("docker")
                    .args([
                        "--host",
                        self.socket_uri.as_str(),
                        "volume",
                        "rm",
                        self.name.as_str(),
                    ])
                    .env_remove("DOCKER_CONTEXT")
                    .env_remove("DOCKER_HOST")
                    .env_remove("DOCKER_TLS_VERIFY")
                    .env_remove("DOCKER_CERT_PATH")
                    .output();
            }
        }
    }

    fn volume_name_is_available(name: &str, volumes: &[VolumeSummary]) -> bool {
        !volumes.iter().any(|volume| volume.name == name)
    }

    fn verify_created_volume(
        created: &VolumeDetails,
        requested_name: &str,
        volumes: &[VolumeSummary],
    ) -> Option<VerifiedVolumeName> {
        (created.name == requested_name
            && volumes.iter().any(|volume| volume.name == requested_name))
        .then(|| VerifiedVolumeName(requested_name.to_owned()))
    }

    #[test]
    fn live_volume_cleanup_requires_preflight_and_verified_created_name() {
        let summary = |name: &str| VolumeSummary {
            name: name.to_owned(),
            driver: None,
            scope: None,
        };
        let details = |name: &str| VolumeDetails {
            name: name.to_owned(),
            driver: None,
            scope: None,
            created_at: None,
            mountpoint: None,
        };
        let name = "ferrum-live-volume-123";

        assert!(volume_name_is_available(name, &[summary("another-volume")]));
        assert!(!volume_name_is_available(name, &[summary(name)]));
        assert!(verify_created_volume(&details(name), name, &[summary(name)]).is_some());
        assert!(
            verify_created_volume(&details("different-volume"), name, &[summary(name)]).is_none()
        );
        assert!(
            verify_created_volume(&details(name), name, &[summary("another-volume")]).is_none()
        );
    }
}
