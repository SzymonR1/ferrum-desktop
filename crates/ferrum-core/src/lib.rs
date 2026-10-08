#![warn(missing_docs)]

//! Ferrum's engine-independent domain model.

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
