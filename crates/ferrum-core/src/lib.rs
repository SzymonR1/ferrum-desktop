#![warn(missing_docs)]

//! Ferrum's engine-independent domain model.

/// Returns whether a character can disrupt the visible identity or layout of plain text.
///
/// This includes control characters and selected bidirectional, zero-width, and
/// invisible formatting characters used in untrusted engine text.
#[must_use]
pub fn is_untrusted_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00ad}'
                | '\u{061c}'
                | '\u{200b}'
                | '\u{034f}'
                | '\u{200c}'
                | '\u{200d}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{180e}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{feff}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
        )
}

/// A container lifecycle state reported by a Docker-compatible engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerState {
    /// The container has been created but not started.
    Created,
    /// The container is restarting.
    Restarting,
    /// The container is running.
    Running,
    /// The container is paused.
    Paused,
    /// The container has exited.
    Exited,
    /// The container is being removed.
    Removing,
    /// The container has failed.
    Dead,
    /// The container is stopping.
    Stopping,
    /// The engine reported a state Ferrum does not yet recognize.
    Unknown(String),
}
/// An action that can be performed on a container.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerAction {
    /// Start a stopped container.
    Start,
    /// Stop a running container.
    Stop,
    /// Restart a container.
    Restart,
    /// Pause a running container.
    Pause,
    /// Resume a paused container.
    Unpause,
    /// Remove a container without deleting its volumes.
    Remove,
}

impl ContainerAction {
    /// Returns safe user-facing text for this action.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
            Self::Pause => "Pause",
            Self::Unpause => "Unpause",
            Self::Remove => "Remove",
        }
    }

    /// Returns whether this action is destructive.
    #[must_use]
    pub const fn is_destructive(self) -> bool {
        matches!(self, Self::Remove)
    }
}

impl std::fmt::Display for ContainerAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

/// A bounded progress update reported while an image is being pulled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImagePullProgress {
    /// Sanitized status text suitable for display.
    pub status: String,
    /// Number of bytes reported as downloaded, if provided by the engine.
    pub current_bytes: Option<u64>,
    /// Total number of bytes reported for the current progress item, if provided.
    pub total_bytes: Option<u64>,
}

/// A container lifecycle change reported by the engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerChange {
    /// Identifier of the container that changed.
    pub container_id: String,
}

impl ContainerState {
    /// Converts an engine state into a known lifecycle state without discarding
    /// unfamiliar values.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("created") {
            Self::Created
        } else if value.eq_ignore_ascii_case("restarting") {
            Self::Restarting
        } else if value.eq_ignore_ascii_case("running") {
            Self::Running
        } else if value.eq_ignore_ascii_case("paused") {
            Self::Paused
        } else if value.eq_ignore_ascii_case("exited") {
            Self::Exited
        } else if value.eq_ignore_ascii_case("removing") {
            Self::Removing
        } else if value.eq_ignore_ascii_case("dead") {
            Self::Dead
        } else if value.eq_ignore_ascii_case("stopping") {
            Self::Stopping
        } else {
            Self::Unknown(value.to_owned())
        }
    }

    /// Returns the engine's state text for display.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Created => "created",
            Self::Restarting => "restarting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Exited => "exited",
            Self::Removing => "removing",
            Self::Dead => "dead",
            Self::Stopping => "stopping",
            Self::Unknown(value) => value,
        }
    }

    /// Indicates whether the container is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Engine-independent summary information shown in Ferrum's container list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerSummary {
    /// Full engine identifier; never used as a display name unless needed.
    pub id: String,
    /// A human-readable container name.
    pub name: String,
    /// Image reference, when reported by the engine.
    pub image: Option<String>,
    /// Lifecycle state, when reported by the engine.
    pub state: Option<ContainerState>,
    /// Human-readable engine status, when reported by the engine.
    pub status: Option<String>,
}

/// Engine-independent image information shown in Ferrum's image list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageSummary {
    /// Full engine identifier for the image.
    pub id: String,
    /// Repository tags reported for the image.
    pub repo_tags: Vec<String>,
    /// Creation time as Unix seconds, when reported by the engine.
    pub created_unix_seconds: Option<i64>,
    /// Image size in bytes, when reported by the engine.
    pub size_bytes: Option<u64>,
}
/// Engine-independent information about one local volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeSummary {
    /// Volume name.
    pub name: String,
    /// Storage driver, when reported by the engine.
    pub driver: Option<String>,
    /// Driver scope, when reported by the engine.
    pub scope: Option<String>,
}
/// Read-only summary information about one local Docker network.
///
/// Only the documented scalar inventory fields are retained. Docker's network
/// list endpoint does not provide the attached-container count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkSummary {
    /// Engine-assigned network identifier.
    pub id: String,
    /// Human-readable network name.
    pub name: String,
    /// Network driver reported by the engine, when available.
    pub driver: Option<String>,
    /// Network scope reported by the engine, when available.
    pub scope: Option<String>,
    /// Engine-reported creation time, when available.
    pub created_at: Option<String>,
    /// Whether the network is internal, when reported.
    pub internal: Option<bool>,
    /// Whether containers can be attached while running, when reported.
    pub attachable: Option<bool>,
    /// Whether IPv4 is enabled, when reported.
    pub enable_ipv4: Option<bool>,
    /// Whether IPv6 is enabled, when reported.
    pub enable_ipv6: Option<bool>,
}

/// Read-only details about one local Docker network.
///
/// Optional values remain absent when the engine does not report them. The
/// attached-container count is a point-in-time count from inspect only; endpoint
/// identifiers, names, addresses, and values are never retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkDetails {
    /// Engine-assigned network identifier.
    pub id: String,
    /// Human-readable network name.
    pub name: String,
    /// Network driver reported by the engine, when available.
    pub driver: Option<String>,
    /// Network scope reported by the engine, when available.
    pub scope: Option<String>,
    /// Engine-reported creation time, when available.
    pub created_at: Option<String>,
    /// Whether the network is internal, when reported.
    pub internal: Option<bool>,
    /// Whether containers can be attached while running, when reported.
    pub attachable: Option<bool>,
    /// Whether IPv4 is enabled, when reported.
    pub enable_ipv4: Option<bool>,
    /// Whether IPv6 is enabled, when reported.
    pub enable_ipv6: Option<bool>,
    /// Number of containers attached at the instant of inspection, if reported.
    pub attached_container_count: Option<usize>,
}

/// Read-only details reported for one local volume.
///
/// Optional values remain absent when the engine does not report them. The
/// mountpoint is untrusted path text reported by the host engine; callers must
/// not treat it as a safe path to access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeDetails {
    /// Volume name.
    pub name: String,
    /// Storage driver, when reported by the engine.
    pub driver: Option<String>,
    /// Driver scope, when reported by the engine.
    pub scope: Option<String>,
    /// Creation timestamp, when reported by the engine.
    pub created_at: Option<String>,
    /// Untrusted host mountpoint path text, when reported by the engine.
    pub mountpoint: Option<String>,
}

/// Read-only point-in-time usage reported for one local volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeUsage {
    /// Volume size in bytes, when reported as a non-negative integer.
    pub size_bytes: Option<u64>,
    /// Number of references to the volume, when reported as a non-negative integer.
    pub reference_count: Option<u64>,
}

/// Read-only details reported for one image by a Docker-compatible engine.
///
/// Optional values remain absent when the engine does not report them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageDetails {
    /// Full engine identifier.
    pub id: String,
    /// Repository tags reported for the image.
    pub repo_tags: Vec<String>,
    /// Creation timestamp, when reported by the engine.
    pub created: Option<String>,
    /// Image size in bytes, when reported by the engine.
    pub size_bytes: Option<u64>,
    /// CPU architecture, when reported by the engine.
    pub architecture: Option<String>,
    /// Operating system, when reported by the engine.
    pub os: Option<String>,
    /// Image author, when reported by the engine.
    pub author: Option<String>,
}

/// Read-only details reported for one container by a Docker-compatible engine.
///
/// Optional values remain absent when the engine does not report them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerDetails {
    /// Full engine identifier.
    pub id: String,
    /// Human-readable container name, when reported.
    pub name: Option<String>,
    /// Image identifier or reference, when reported.
    pub image: Option<String>,
    /// Creation timestamp, when reported.
    pub created: Option<String>,
    /// Lifecycle state, when reported.
    pub state: Option<ContainerState>,
    /// Number of restarts, when reported.
    pub restart_count: Option<i64>,
    /// Last process exit code, when reported.
    pub exit_code: Option<i64>,
    /// Last start timestamp, when reported.
    pub started_at: Option<String>,
    /// Last finish timestamp, when reported.
    pub finished_at: Option<String>,
    /// Mounts, when the engine reports the mount list. An empty list is distinct
    /// from an unavailable list.
    pub mounts: Option<Vec<ContainerMount>>,
    /// Whether the container was created with a TTY, when reported by the engine.
    pub tty: Option<bool>,
}

/// The output stream associated with a container log line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerLogSource {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// Combined output, as used by containers created with a TTY.
    Combined,
}

/// A complete line emitted by a container log stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerLogLine {
    /// Stream that produced this line.
    pub source: ContainerLogSource,
    /// Log text without its line ending. Invalid UTF-8 is replaced lossily.
    pub text: String,
}

/// Read-only information about a mount reported for a container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerMount {
    /// Mount kind, when reported.
    pub kind: Option<String>,
    /// Source path or volume, when reported.
    pub source: Option<String>,
    /// Destination path inside the container, when reported.
    pub destination: Option<String>,
    /// Mount mode, when reported.
    pub mode: Option<String>,
    /// Whether the mount is writable, when reported.
    pub read_write: Option<bool>,
}
