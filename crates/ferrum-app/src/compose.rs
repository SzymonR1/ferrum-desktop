#[cfg(target_os = "linux")]
use crate::display::escape_untrusted_display_text;
use crate::display::escape_untrusted_display_text_with_limit;
use eframe::egui;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
const MAX_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_PROJECTS: usize = 4096;
#[cfg(target_os = "linux")]
const MAX_PROJECT_FIELD_BYTES: usize = 256;
#[cfg(target_os = "linux")]
const MAX_VERSION_BYTES: usize = 64;
const MAX_COMPOSE_FILENAME_DISPLAY_BYTES: usize = 128;
#[cfg(target_os = "linux")]
const MAX_COMPOSE_SERVICES: usize = 1024;
#[cfg(target_os = "linux")]
const MAX_COMPOSE_SERVICE_NAME_BYTES: usize = 128;
#[cfg(target_os = "linux")]
const MAX_COMPOSE_SERVICE_DISPLAY_BYTES: usize = 1024;
#[cfg(target_os = "linux")]
const MAX_COMPOSE_SERVICE_OUTPUT_BYTES: usize = 128 * 1024;
const COMPOSE_SERVICE_ROW_HEIGHT: f32 = 24.0;
#[cfg(target_os = "linux")]
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(target_os = "linux")]
const LIFECYCLE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_COMPOSE_INPUT_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(target_os = "linux")]
const CHILD_ADDRESS_SPACE_LIMIT: libc::rlim_t = 4 * 1024 * 1024 * 1024;
#[cfg(target_os = "linux")]
const CHILD_DATA_LIMIT: libc::rlim_t = 512 * 1024 * 1024;
#[cfg(target_os = "linux")]
const CHILD_GO_MEMORY_LIMIT: &str = "402653184";

const MAX_COMPOSE_PROFILES: usize = 16;
const MAX_COMPOSE_PROFILE_NAME_BYTES: usize = 64;
const MAX_COMPOSE_PROFILE_INPUT_BYTES: usize = 1024;

fn parse_compose_profiles(input: &str) -> Result<Vec<String>, ()> {
    if input.len() > MAX_COMPOSE_PROFILE_INPUT_BYTES {
        return Err(());
    }
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for token in input.split(',') {
        let name = token.trim();
        if name.is_empty()
            || name.len() > MAX_COMPOSE_PROFILE_NAME_BYTES
            || (name != "*"
                && (name.len() < 2
                    || !name.as_bytes().iter().enumerate().all(|(index, byte)| {
                        if index == 0 {
                            byte.is_ascii_alphanumeric()
                        } else {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
                        }
                    })))
        {
            return Err(());
        }
        if name == "*" && (!profiles.is_empty() || input.split(',').count() != 1) {
            return Err(());
        }
        if profiles.iter().any(|profile| profile == name) {
            return Err(());
        }
        profiles.push(name.to_owned());
        if profiles.len() > MAX_COMPOSE_PROFILES {
            return Err(());
        }
    }
    Ok(profiles)
}
fn valid_profile_list(profiles: &[String]) -> bool {
    if profiles.len() > MAX_COMPOSE_PROFILES {
        return false;
    }
    let input_bytes = profiles
        .iter()
        .fold(0_usize, |total, profile| {
            total.saturating_add(profile.len())
        })
        .saturating_add(profiles.len().saturating_sub(1));
    input_bytes <= MAX_COMPOSE_PROFILE_INPUT_BYTES
        && profiles.iter().enumerate().all(|(index, name)| {
            name.len() <= MAX_COMPOSE_PROFILE_NAME_BYTES
                && (name == "*" && profiles.len() == 1
                    || (name.len() >= 2
                        && name.as_bytes().iter().enumerate().all(|(i, byte)| {
                            if i == 0 {
                                byte.is_ascii_alphanumeric()
                            } else {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
                            }
                        })
                        && !profiles[..index].iter().any(|previous| previous == name)))
        })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeContainer {
    pub(super) name: String,
    pub(super) service: String,
    pub(super) project: Option<String>,
    pub(super) state: String,
    pub(super) health: Option<String>,
}

#[cfg(target_os = "linux")]
const MAX_COMPOSE_CONTAINERS: usize = 4096;
#[cfg(target_os = "linux")]
const MAX_CONTAINER_FIELD_BYTES: usize = 256;
const COMPOSE_CONTAINER_ROW_HEIGHT: f32 = 28.0;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ComposeStatusError {
    DockerCliMissing,
    PrivateConfigSetupFailed,
    QueryFailed,
    InvalidOutput,
}

impl ComposeStatusError {
    fn message(&self) -> &'static str {
        match self {
            Self::DockerCliMissing => "Docker CLI is unavailable.",
            Self::PrivateConfigSetupFailed => {
                "Could not prepare an isolated Docker CLI environment."
            }
            Self::QueryFailed => "Could not read existing Compose containers.",
            Self::InvalidOutput => "Compose returned invalid or excessive container status data.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeStatusResult {
    pub(super) containers: Vec<ComposeContainer>,
    pub(super) outcome: Result<(), ComposeStatusError>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum ComposeStatusState {
    #[default]
    Idle,
    Loading {
        file_name: String,
    },
    Succeeded {
        file_name: String,
    },
    Failed {
        file_name: String,
        error: ComposeStatusError,
    },
    Cancelled {
        file_name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ComposeLifecycleError {
    DockerCliMissing,
    CommandStartFailed,
    PrivateConfigSetupFailed,
    ValidationFailed,
    EngineUnavailable,
    OperationFailed,
}

impl ComposeLifecycleError {
    fn message(&self) -> &'static str {
        match self {
            Self::DockerCliMissing => "Docker Compose CLI is unavailable.",
            Self::CommandStartFailed => "Docker Compose CLI could not be started.",
            Self::PrivateConfigSetupFailed => {
                "Could not prepare an isolated Docker CLI environment."
            }
            Self::ValidationFailed => {
                "Compose configuration validation failed; no lifecycle command was run."
            }
            Self::EngineUnavailable => {
                "The engine or Compose configuration is not ready; no lifecycle command was run."
            }
            Self::OperationFailed => "Compose lifecycle operation failed.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeLifecycleResult {
    pub(super) outcome: Result<(), ComposeLifecycleError>,
    pub(super) submitted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeLifecycleSelection {
    pub(super) imported_label: String,
    pub(super) files: Vec<std::path::PathBuf>,
    pub(super) profiles: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct ComposeLifecycleConfirmation {
    action: ComposeLifecycleAction,
    selection: ComposeLifecycleSelection,
    ordered_files: Vec<String>,
}

#[derive(Default)]
enum ComposeLifecycleState {
    #[default]
    Idle,
    Confirming(ComposeLifecycleConfirmation),
    Running {
        action: ComposeLifecycleAction,
        file_name: String,
    },
    Succeeded {
        action: ComposeLifecycleAction,
        file_name: String,
    },
    Failed {
        action: ComposeLifecycleAction,
        file_name: String,
        error: ComposeLifecycleError,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposeLifecycleAction {
    Start,
    Stop,
}

impl ComposeLifecycleAction {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }
}

pub(super) struct ComposeLifecycleTask {
    pub(super) generation: u64,
    pub(super) action: ComposeLifecycleAction,
    pub(super) selection: ComposeLifecycleSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ComposeLogsError {
    DockerCliMissing,
    PrivateConfigSetupFailed,
    QueryFailed,
    ServiceUnavailable,
    InvalidOutput,
}

impl ComposeLogsError {
    fn message(&self) -> &'static str {
        match self {
            Self::DockerCliMissing => "Docker CLI is unavailable.",
            Self::PrivateConfigSetupFailed => {
                "Could not prepare an isolated Docker CLI environment."
            }
            Self::ServiceUnavailable => {
                "The service is no longer present in the current Compose status. Refresh and retry."
            }
            Self::QueryFailed => "Could not read the selected Compose service logs.",
            Self::InvalidOutput => "Compose returned invalid or excessive log data.",
        }
    }
}

pub(super) struct ComposeLogsResult {
    output: String,
    outcome: Result<(), ComposeLogsError>,
}

impl std::fmt::Debug for ComposeLogsResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComposeLogsResult")
            .field("output_bytes", &self.output.len())
            .field("outcome", &self.outcome)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum ComposeLogsState {
    #[default]
    Idle,
    Loading {
        service: String,
    },
    Succeeded {
        service: String,
    },
    Failed {
        service: String,
        error: ComposeLogsError,
    },
    Cancelled {
        service: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeProject {
    name: String,
    status: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ComposeDiscovery {
    version: String,
    projects: Vec<ComposeProject>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ComposeError {
    DockerCliMissing,
    CommandStartFailed,
    ComposePluginUnavailable,
    PrivateConfigSetupFailed,
    ProjectListFailed { version: String },
}

impl ComposeError {
    pub(super) fn message(&self) -> &'static str {
        match self {
            Self::DockerCliMissing => {
                "Docker CLI is unavailable. Install Docker CLI to discover Compose projects."
            }
            Self::CommandStartFailed => "Docker Compose CLI could not be started.",
            Self::ComposePluginUnavailable => {
                "Docker Compose is unavailable. Install the Docker Compose plugin to discover projects."
            }
            Self::PrivateConfigSetupFailed => {
                "Compose discovery could not start safely. Try again later."
            }
            Self::ProjectListFailed { .. } => {
                "Compose projects could not be read from the selected local Docker Engine."
            }
        }
    }

    fn detected_version(&self) -> Option<&str> {
        match self {
            Self::ProjectListFailed { version } => Some(version),
            Self::DockerCliMissing
            | Self::CommandStartFailed
            | Self::ComposePluginUnavailable
            | Self::PrivateConfigSetupFailed => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposeValidationError {
    DockerCliMissing,
    CommandStartFailed,
    ComposePluginUnavailable,
    PrivateConfigSetupFailed,
    InvalidSelectedFile,
    InputTooLarge,
    InvalidProfiles,
    ValidationFailed,
}

impl ComposeValidationError {
    fn message(self) -> &'static str {
        match self {
            Self::DockerCliMissing => {
                "Docker CLI is unavailable. Install Docker CLI to validate Compose files."
            }
            Self::CommandStartFailed => "Docker Compose CLI could not be started.",
            Self::ComposePluginUnavailable => {
                "Docker Compose is unavailable. Install the Docker Compose plugin to validate Compose files."
            }
            Self::PrivateConfigSetupFailed => {
                "Compose validation could not start safely. Try again later."
            }
            Self::InvalidSelectedFile => "The selected Compose file could not be validated.",
            Self::InputTooLarge => {
                "Selected Compose files exceed the 16 MiB validation input limit."
            }
            Self::InvalidProfiles => "The selected Compose profile input is invalid.",
            Self::ValidationFailed => "Compose file validation failed.",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeValidationResult {
    pub(super) version: Option<String>,
    pub(super) services: Vec<String>,
    pub(super) outcome: Result<(), ComposeValidationError>,
}

impl ComposeValidationResult {
    fn failed(error: ComposeValidationError) -> Self {
        Self {
            version: None,
            services: Vec::new(),
            outcome: Err(error),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum ComposeValidationState {
    #[default]
    Idle,
    SelectingFile {
        override_file: bool,
    },
    Validating {
        file_name: String,
    },
    Succeeded {
        file_name: String,
    },
    Failed {
        file_name: String,
        error: ComposeValidationError,
    },
    Cancelled {
        file_name: String,
    },
    SelectionCancelled,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ComposeActions {
    pub(super) refresh: bool,
    pub(super) refresh_containers: bool,
    pub(super) select_base_file: bool,
    pub(super) add_override_file: bool,
    pub(super) remove_override: Option<usize>,
    pub(super) validate_files: bool,
    pub(super) cancel_validation: bool,
    pub(super) cancel_logs: bool,
    pub(super) logs_service: Option<String>,
    pub(super) cancel_status: bool,
    pub(super) start_imported: bool,
    pub(super) stop_imported: bool,
    pub(super) confirm_lifecycle: bool,
    pub(super) cancel_lifecycle: bool,
    pub(super) profiles_changed: bool,
}
const MAX_COMPOSE_FILES: usize = 16;
const MAX_IMPORTED_COMPOSE_PROJECTS: usize = 32;
const MAX_IMPORTED_COMPOSE_NAME_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportedComposeProject {
    name: String,
    display_name: String,
    files: Vec<std::path::PathBuf>,
    profiles: Vec<String>,
}

fn display_imported_project_name(name: &str) -> String {
    escape_untrusted_display_text_with_limit(name.to_owned(), MAX_IMPORTED_COMPOSE_NAME_BYTES)
        .unwrap_or_else(|| "[name omitted]".to_owned())
}

fn display_compose_basename(path: &std::path::Path) -> String {
    let Some(file_name) = path.file_name() else {
        return "[name omitted]".to_owned();
    };
    escape_untrusted_display_text_with_limit(
        file_name.to_string_lossy().into_owned(),
        MAX_COMPOSE_FILENAME_DISPLAY_BYTES,
    )
    .unwrap_or_else(|| "[name omitted]".to_owned())
}
fn display_compose_file_list(files: &[std::path::PathBuf]) -> String {
    let mut names = String::new();
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            names.push_str(" + ");
        }
        names.push_str(&display_compose_basename(file));
    }
    names
}

#[derive(Default)]
pub(super) struct ComposeView {
    version: Option<String>,
    projects: Vec<ComposeProject>,
    loading: bool,
    loaded: bool,
    error: Option<ComposeError>,
    generation: u64,
    validation_generation: u64,
    validation: ComposeValidationState,
    validated_services: Vec<String>,
    selected_files: Vec<std::path::PathBuf>,
    profile_input: String,
    status_generation: u64,
    status: ComposeStatusState,
    containers: Vec<ComposeContainer>,
    logs_generation: u64,
    logs: ComposeLogsState,
    logs_output: String,
    validated_generation: Option<u64>,
    imported_projects: Vec<ImportedComposeProject>,
    imported_selection: bool,
    selected_import_label: Option<String>,
    import_name_input: String,
    import_feedback: Option<&'static str>,
    lifecycle_generation: u64,
    lifecycle: ComposeLifecycleState,
}

impl ComposeView {
    pub(super) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(super) fn has_loaded(&self) -> bool {
        self.loaded
    }

    pub(super) fn begin_load(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.loaded = false;
        self.version = None;
        self.projects.clear();
        self.error = None;
        self.generation
    }

    pub(super) fn finish_load(
        &mut self,
        generation: u64,
        result: Result<ComposeDiscovery, ComposeError>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        self.loaded = true;
        match result {
            Ok(discovery) => {
                self.version = Some(discovery.version);
                self.projects = discovery.projects;
                self.error = None;
            }
            Err(error) => {
                self.version = error.detected_version().map(str::to_owned);
                self.projects.clear();
                self.error = Some(error);
            }
        }
        true
    }
    pub(super) fn can_refresh_containers(&self) -> bool {
        matches!(&self.validation, ComposeValidationState::Succeeded { .. })
    }
    pub(super) fn request_lifecycle_confirmation(
        &mut self,
        action: ComposeLifecycleAction,
    ) -> bool {
        if !self.can_start_lifecycle() {
            return false;
        }
        let (Some(imported_label), Ok(profiles)) =
            (self.selected_import_label.clone(), self.selected_profiles())
        else {
            return false;
        };
        let selection = ComposeLifecycleSelection {
            imported_label,
            files: self.selected_files.clone(),
            profiles,
        };
        let ordered_files = selection
            .files
            .iter()
            .map(|file| display_compose_basename(file))
            .collect();
        self.lifecycle = ComposeLifecycleState::Confirming(ComposeLifecycleConfirmation {
            action,
            selection,
            ordered_files,
        });
        true
    }

    pub(super) fn confirm_lifecycle(&mut self) -> Option<ComposeLifecycleTask> {
        let ComposeLifecycleState::Confirming(confirmation) = &self.lifecycle else {
            return None;
        };
        let action = confirmation.action;
        if !self.current_imported_selection_validated()
            || self.selected_import_label.as_deref()
                != Some(confirmation.selection.imported_label.as_str())
            || self.selected_files != confirmation.selection.files
            || self.selected_profiles().ok().as_deref()
                != Some(confirmation.selection.profiles.as_slice())
        {
            self.lifecycle = ComposeLifecycleState::Idle;
            return None;
        }
        let selection = confirmation.selection.clone();
        self.invalidate_status();
        self.cancel_service_logs();
        self.lifecycle_generation = self.lifecycle_generation.wrapping_add(1);
        let generation = self.lifecycle_generation;
        let file_name = display_compose_file_list(&selection.files);
        self.lifecycle = ComposeLifecycleState::Running { action, file_name };
        Some(ComposeLifecycleTask {
            generation,
            action,
            selection,
        })
    }

    pub(super) fn cancel_lifecycle_confirmation(&mut self) -> Option<ComposeLifecycleAction> {
        let ComposeLifecycleState::Confirming(confirmation) = &self.lifecycle else {
            return None;
        };
        let action = confirmation.action;
        self.lifecycle = ComposeLifecycleState::Idle;
        Some(action)
    }

    #[cfg(test)]
    fn lifecycle_confirmation(&self) -> Option<&ComposeLifecycleConfirmation> {
        match &self.lifecycle {
            ComposeLifecycleState::Confirming(confirmation) => Some(confirmation),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn lifecycle_confirmation_open(&self) -> bool {
        self.lifecycle_confirmation().is_some()
    }

    pub(super) fn current_lifecycle_selection_matches(
        &self,
        selection: &ComposeLifecycleSelection,
    ) -> bool {
        self.current_imported_selection_validated()
            && self.selected_import_label.as_deref() == Some(selection.imported_label.as_str())
            && self.selected_files == selection.files
            && self
                .selected_profiles()
                .is_ok_and(|current| current == selection.profiles)
    }

    pub(super) fn finish_lifecycle(
        &mut self,
        generation: u64,
        result: ComposeLifecycleResult,
    ) -> bool {
        if generation != self.lifecycle_generation
            || !matches!(&self.lifecycle, ComposeLifecycleState::Running { .. })
        {
            return false;
        }
        let ComposeLifecycleState::Running { action, file_name } =
            std::mem::take(&mut self.lifecycle)
        else {
            return false;
        };
        self.lifecycle = match result.outcome {
            Ok(()) => ComposeLifecycleState::Succeeded { action, file_name },
            Err(error) => ComposeLifecycleState::Failed {
                action,
                file_name,
                error,
            },
        };
        true
    }

    fn current_imported_selection_validated(&self) -> bool {
        self.imported_selection
            && self.selected_import_label.is_some()
            && self.is_current_selection_validated()
    }

    fn can_start_lifecycle(&self) -> bool {
        self.current_imported_selection_validated()
            && !matches!(
                &self.lifecycle,
                ComposeLifecycleState::Confirming(_) | ComposeLifecycleState::Running { .. }
            )
    }

    pub(super) fn begin_status_refresh(
        &mut self,
    ) -> Option<(u64, Vec<std::path::PathBuf>, Vec<String>)> {
        let profiles = self.selected_profiles().ok()?;
        if !self.can_refresh_containers()
            || matches!(&self.status, ComposeStatusState::Loading { .. })
            || matches!(&self.lifecycle, ComposeLifecycleState::Running { .. })
        {
            return None;
        }
        self.status_generation = self.status_generation.wrapping_add(1);
        self.invalidate_logs();
        self.containers.clear();
        let file_name = display_compose_file_list(&self.selected_files);
        self.status = ComposeStatusState::Loading { file_name };
        Some((
            self.status_generation,
            self.selected_files.clone(),
            profiles,
        ))
    }

    pub(super) fn finish_status_refresh(
        &mut self,
        generation: u64,
        result: ComposeStatusResult,
    ) -> bool {
        if generation != self.status_generation
            || !matches!(&self.status, ComposeStatusState::Loading { .. })
            || !self.can_refresh_containers()
        {
            return false;
        }
        let ComposeStatusState::Loading { file_name } = std::mem::take(&mut self.status) else {
            return false;
        };
        match result.outcome {
            Ok(()) => {
                self.containers = result.containers;
                self.status = ComposeStatusState::Succeeded { file_name };
            }
            Err(error) => {
                self.containers.clear();
                self.status = ComposeStatusState::Failed { file_name, error };
            }
        }
        true
    }

    pub(super) fn cancel_status_refresh(&mut self) {
        if let ComposeStatusState::Loading { file_name } = std::mem::take(&mut self.status) {
            self.status_generation = self.status_generation.wrapping_add(1);
            self.containers.clear();
            self.status = ComposeStatusState::Cancelled { file_name };
        }
    }

    fn invalidate_status(&mut self) {
        self.status_generation = self.status_generation.wrapping_add(1);
        self.status = ComposeStatusState::Idle;
        self.containers.clear();
        self.invalidate_logs();
    }
    fn can_request_service_logs(&self, service: &str) -> bool {
        validate_service_selector(service).is_some()
            && matches!(&self.status, ComposeStatusState::Succeeded { .. })
            && self.selected_profiles().is_ok()
            && matches!(&self.validation, ComposeValidationState::Succeeded { .. })
            && self
                .containers
                .iter()
                .any(|container| container.service == service)
            && !matches!(&self.logs, ComposeLogsState::Loading { .. })
    }
    pub(super) fn begin_service_logs(
        &mut self,
        service: &str,
    ) -> Option<(u64, Vec<std::path::PathBuf>, Vec<String>, String)> {
        let profiles = self.selected_profiles().ok()?;
        let service = validate_service_selector(service)?;
        if !matches!(&self.status, ComposeStatusState::Succeeded { .. })
            || !matches!(&self.validation, ComposeValidationState::Succeeded { .. })
            || !self
                .containers
                .iter()
                .any(|container| container.service == service)
            || matches!(&self.logs, ComposeLogsState::Loading { .. })
        {
            return None;
        }
        self.logs_generation = self.logs_generation.wrapping_add(1);
        self.logs_output.clear();
        self.logs = ComposeLogsState::Loading {
            service: service.to_owned(),
        };
        Some((
            self.logs_generation,
            self.selected_files.clone(),
            profiles,
            service.to_owned(),
        ))
    }

    pub(super) fn finish_service_logs(
        &mut self,
        generation: u64,
        result: ComposeLogsResult,
    ) -> bool {
        if generation != self.logs_generation
            || !matches!(&self.logs, ComposeLogsState::Loading { .. })
        {
            return false;
        }
        let ComposeLogsState::Loading { service } = std::mem::take(&mut self.logs) else {
            return false;
        };
        match result.outcome {
            Ok(()) => {
                self.logs_output = result.output;
                self.logs = ComposeLogsState::Succeeded { service };
            }
            Err(error) => self.logs = ComposeLogsState::Failed { service, error },
        }
        true
    }

    pub(super) fn cancel_service_logs(&mut self) {
        if let ComposeLogsState::Loading { service } = std::mem::take(&mut self.logs) {
            self.logs_generation = self.logs_generation.wrapping_add(1);
            self.logs_output.clear();
            self.logs = ComposeLogsState::Cancelled { service };
        }
    }

    fn invalidate_logs(&mut self) {
        self.logs_generation = self.logs_generation.wrapping_add(1);
        self.logs_output.clear();
        self.logs = ComposeLogsState::Idle;
    }

    pub(super) fn begin_file_selection(&mut self) -> u64 {
        self.begin_selection(false)
    }

    pub(super) fn begin_override_selection(&mut self) -> u64 {
        self.begin_selection(true)
    }

    fn begin_selection(&mut self, override_file: bool) -> u64 {
        self.validated_generation = None;
        self.imported_selection = false;
        self.selected_import_label = None;
        self.clear_lifecycle_result();
        self.validated_services.clear();
        self.validation_generation = self.validation_generation.wrapping_add(1);
        self.invalidate_status();
        self.validation = ComposeValidationState::SelectingFile { override_file };
        self.validation_generation
    }

    pub(super) fn selected_files(&self) -> &[std::path::PathBuf] {
        &self.selected_files
    }

    fn selected_profiles(&self) -> Result<Vec<String>, ()> {
        parse_compose_profiles(&self.profile_input)
    }
    fn is_current_selection_validated(&self) -> bool {
        !self.selected_files.is_empty()
            && !self.is_file_selection_running()
            && !self.is_validation_running()
            && self.validated_generation == Some(self.validation_generation)
            && matches!(&self.validation, ComposeValidationState::Succeeded { .. })
    }
    pub(super) fn can_import_current_project(&self, profiles_valid: bool) -> bool {
        self.is_current_selection_validated()
            && profiles_valid
            && !matches!(
                &self.lifecycle,
                ComposeLifecycleState::Confirming(_) | ComposeLifecycleState::Running { .. }
            )
    }

    pub(super) fn import_current_project(&mut self, name: &str) -> bool {
        let name = name.trim();
        if !self.is_current_selection_validated()
            || matches!(
                &self.lifecycle,
                ComposeLifecycleState::Confirming(_) | ComposeLifecycleState::Running { .. }
            )
            || name.is_empty()
            || name.len() > MAX_IMPORTED_COMPOSE_NAME_BYTES
            || self.imported_projects.len() >= MAX_IMPORTED_COMPOSE_PROJECTS
            || self
                .imported_projects
                .iter()
                .any(|project| project.name == name)
        {
            return false;
        }
        let Ok(profiles) = self.selected_profiles() else {
            return false;
        };
        let display_name = display_imported_project_name(name);
        self.imported_projects.push(ImportedComposeProject {
            name: name.to_owned(),
            display_name: display_name.clone(),
            files: self.selected_files.clone(),
            profiles,
        });
        self.imported_selection = true;
        self.selected_import_label = Some(display_name);
        self.clear_lifecycle_result();
        self.import_name_input.clear();
        self.import_feedback = Some("Imported for this session only.");
        true
    }

    fn select_imported_project(&mut self, index: usize) -> bool {
        if matches!(&self.lifecycle, ComposeLifecycleState::Running { .. }) {
            return false;
        }
        let Some((files, profile_input, display_name)) =
            self.imported_projects.get(index).map(|project| {
                (
                    project.files.clone(),
                    project.profiles.join(","),
                    project.display_name.clone(),
                )
            })
        else {
            return false;
        };
        self.validation_generation = self.validation_generation.wrapping_add(1);
        self.validated_generation = None;
        self.invalidate_status();
        self.validated_services.clear();
        self.validation = ComposeValidationState::Idle;
        self.selected_files = files;
        self.profile_input = profile_input;
        self.imported_selection = true;
        self.selected_import_label = Some(display_name);
        self.clear_lifecycle_result();
        self.import_feedback = None;
        true
    }

    pub(super) fn profiles_for_automatic_validation(&mut self) -> Option<Vec<String>> {
        match self.selected_profiles() {
            Ok(profiles) => Some(profiles),
            Err(()) => {
                let file_name = match &self.validation {
                    ComposeValidationState::Validating { file_name } => file_name.clone(),
                    _ => return None,
                };
                self.validation = ComposeValidationState::Failed {
                    file_name,
                    error: ComposeValidationError::InvalidProfiles,
                };
                None
            }
        }
    }

    pub(super) fn profiles_changed(&mut self) {
        self.imported_selection = false;
        self.validated_generation = None;
        self.validated_services.clear();
        self.validation_generation = self.validation_generation.wrapping_add(1);
        self.invalidate_status();
        self.validation = ComposeValidationState::Idle;
        self.clear_lifecycle_result();
    }
    fn clear_lifecycle_result(&mut self) {
        if !matches!(&self.lifecycle, ComposeLifecycleState::Running { .. }) {
            self.lifecycle = ComposeLifecycleState::Idle;
        }
    }
    pub(super) fn finish_validation(
        &mut self,
        generation: u64,
        result: ComposeValidationResult,
    ) -> bool {
        if generation != self.validation_generation
            || !matches!(&self.validation, ComposeValidationState::Validating { .. })
        {
            return false;
        }
        let invalid_file_set = matches!(
            result.outcome,
            Err(ComposeValidationError::InvalidSelectedFile)
        );
        if let Some(version) = result.version {
            self.version = Some(version);
        }
        let ComposeValidationState::Validating { file_name } = std::mem::take(&mut self.validation)
        else {
            unreachable!("the validation state was checked above");
        };
        if invalid_file_set {
            self.selected_files.clear();
        }
        self.validated_generation = result.outcome.is_ok().then_some(generation);
        self.validated_services = if result.outcome.is_ok() {
            result.services
        } else {
            Vec::new()
        };
        self.validation = match result.outcome {
            Ok(()) => ComposeValidationState::Succeeded { file_name },
            Err(error) => ComposeValidationState::Failed { file_name, error },
        };
        true
    }

    pub(super) fn is_validation_running(&self) -> bool {
        matches!(&self.validation, ComposeValidationState::Validating { .. })
    }

    pub(super) fn is_file_selection_running(&self) -> bool {
        matches!(
            &self.validation,
            ComposeValidationState::SelectingFile { .. }
        )
    }

    pub(super) fn finish_file_selection(
        &mut self,
        generation: u64,
        selected_file: Option<&std::path::Path>,
    ) -> bool {
        if generation != self.validation_generation {
            return false;
        }
        let ComposeValidationState::SelectingFile { override_file } = &self.validation else {
            return false;
        };
        let Some(path) = selected_file else {
            self.validation = ComposeValidationState::SelectionCancelled;
            return true;
        };
        if !path.is_absolute()
            || (*override_file && self.selected_files.is_empty())
            || (*override_file && self.selected_files.len() >= MAX_COMPOSE_FILES)
            || (*override_file && self.selected_files.iter().any(|selected| selected == path))
        {
            self.validation = ComposeValidationState::Failed {
                file_name: display_compose_file_list(&self.selected_files),
                error: ComposeValidationError::InvalidSelectedFile,
            };
            return true;
        }
        if *override_file {
            self.selected_files.push(path.to_path_buf());
        } else {
            self.selected_files.clear();
            self.selected_files.push(path.to_path_buf());
        }
        self.imported_selection = false;
        self.selected_import_label = None;
        self.clear_lifecycle_result();
        let file_name = display_compose_file_list(&self.selected_files);
        self.validation = ComposeValidationState::Validating { file_name };
        true
    }

    pub(super) fn remove_override(&mut self, index: usize) {
        if index > 0 && index < self.selected_files.len() {
            self.imported_selection = false;
            self.selected_import_label = None;
            self.validated_generation = None;
            self.invalidate_status();
            self.validated_services.clear();
            self.validation_generation = self.validation_generation.wrapping_add(1);
            self.selected_files.remove(index);
            self.validation = ComposeValidationState::Idle;
            self.clear_lifecycle_result();
        }
    }
    pub(super) fn begin_validation(
        &mut self,
    ) -> Option<(u64, Vec<std::path::PathBuf>, Vec<String>)> {
        if self.selected_files.is_empty()
            || self.is_file_selection_running()
            || self.is_validation_running()
            || matches!(&self.lifecycle, ComposeLifecycleState::Running { .. })
        {
            return None;
        }
        self.validated_services.clear();
        self.validated_generation = None;
        let profiles = match self.selected_profiles() {
            Ok(profiles) => profiles,
            Err(()) => {
                self.validation = ComposeValidationState::Failed {
                    file_name: display_compose_file_list(&self.selected_files),
                    error: ComposeValidationError::InvalidProfiles,
                };
                return None;
            }
        };
        self.clear_lifecycle_result();
        let generation = self.validation_generation;
        let file_name = display_compose_file_list(&self.selected_files);
        self.validation = ComposeValidationState::Validating { file_name };
        Some((generation, self.selected_files.clone(), profiles))
    }

    pub(super) fn cancel_validation(&mut self) {
        self.validated_generation = None;
        self.validated_services.clear();
        if let ComposeValidationState::Validating { file_name } =
            std::mem::take(&mut self.validation)
        {
            self.validation_generation = self.validation_generation.wrapping_add(1);
            self.validation = ComposeValidationState::Cancelled { file_name };
        }
    }

    pub(super) fn invalidate(&mut self) {
        if matches!(&self.lifecycle, ComposeLifecycleState::Confirming(_)) {
            self.lifecycle = ComposeLifecycleState::Idle;
        }
        self.validated_generation = None;
        self.validated_services.clear();
        self.generation = self.generation.wrapping_add(1);
        self.loading = false;
        self.loaded = false;
        self.version = None;
        self.projects.clear();
        self.error = None;
        self.invalidate_status();
        self.validation = ComposeValidationState::Idle;
        self.selected_files.clear();
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        available: bool,
        unavailable_message: &str,
        refresh_enabled: bool,
        validation_enabled: bool,
        status_enabled: bool,
    ) -> ComposeActions {
        let mut actions = ComposeActions::default();
        ui.group(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(224, 174, 109),
                concat!(
                    "Security notice: validate only trusted Compose files. ",
"Compose may recursively read local files referenced by include or service ",
"extends.file, including each included project's default .env and explicit ",
"include.env_file, even outside the selected directory under your permissions. ",
"Disabling the root .env and using --no-interpolate/--no-env-resolution do not ",
"confine these reads. Remote Git/OCI loaders are disabled. The selection is ",
"not a path boundary, and validation is not sandboxed."
                ),
            );
        });
        ui.horizontal(|ui| {
            let label = ui.label("Profiles:");
            let response = ui
                .add_enabled(
                    !self.is_file_selection_running()
                        && !matches!(
                            &self.lifecycle,
                            ComposeLifecycleState::Confirming(_)
                                | ComposeLifecycleState::Running { .. }
                        ),
                    egui::TextEdit::singleline(&mut self.profile_input)
                        .char_limit(MAX_COMPOSE_PROFILE_INPUT_BYTES)
                        .hint_text("comma-separated; * = all, blank = default")
                        .desired_width(260.0),
                )
                .labelled_by(label.id);
            actions.profiles_changed = response.changed();
            if response.changed() {
                self.validated_generation = None;
            }
        });
        ui.label("Profiles apply to validation, status, one-shot logs, and imported-project start/stop; * enables all profiles.");
        let profiles_valid = self.selected_profiles().is_ok();
        if !profiles_valid {
            ui.colored_label(
                egui::Color32::from_rgb(224, 174, 109),
                "Invalid profiles. Use up to 16 unique names (64 bytes each), or * alone to enable all profiles; input is limited to 1024 bytes.",
            );
        }
        let selection_enabled = validation_enabled
            && !self.is_file_selection_running()
            && !self.is_validation_running()
            && !matches!(
                &self.lifecycle,
                ComposeLifecycleState::Confirming(_) | ComposeLifecycleState::Running { .. }
            );
        let validation_action_enabled = selection_enabled && profiles_valid;
        ui.horizontal(|ui| {
            actions.refresh = ui
                .add_enabled(
                    refresh_enabled && !self.loading,
                    egui::Button::new("Refresh Compose projects"),
                )
                .clicked();
            if self.selected_files.is_empty() {
                actions.select_base_file = ui
                    .add_enabled(selection_enabled, egui::Button::new("Choose base file"))
                    .clicked();
            } else {
                actions.select_base_file = ui
                    .add_enabled(
                        selection_enabled,
                        egui::Button::new("Change base file (clears overrides)"),
                    )
                    .clicked();
                actions.add_override_file = ui
                    .add_enabled(
                        selection_enabled && self.selected_files.len() < MAX_COMPOSE_FILES,
                        egui::Button::new("Add override file"),
                    )
                    .clicked();
            }
            if !self.selected_files.is_empty() {
                actions.validate_files = ui
                    .add_enabled(
                        validation_action_enabled,
                        egui::Button::new("Validate selected files"),
                    )
                    .clicked();
            }
            if self.is_validation_running() {
                actions.cancel_validation = ui.button("Cancel validation").clicked();
            }
        });
        if !self.selected_files.is_empty() {
            ui.label("Compose file order (base first, then overrides):");
            for (index, file) in self.selected_files.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", index + 1));
                    ui.label(display_compose_basename(file));
                    if index > 0
                        && ui
                            .add_enabled(
                                selection_enabled,
                                egui::Button::new(format!("Remove override {}", index + 1)),
                            )
                            .clicked()
                    {
                        actions.remove_override = Some(index);
                    }
                });
            }
        } else {
            ui.label("Choose one local Compose YAML base file, then add overrides in order.");
        }
        let mut imported_selection = None;
        ui.separator();
        ui.heading("Import validated configuration");
        ui.label("The local label is only for this session; it is not a Compose project identity.");
        ui.horizontal(|ui| {
            let label = ui.label("Import name:");
            let input = ui
                .add(
                    egui::TextEdit::singleline(&mut self.import_name_input)
                        .char_limit(MAX_IMPORTED_COMPOSE_NAME_BYTES)
                        .hint_text("local display label")
                        .desired_width(220.0),
                )
                .labelled_by(label.id);
            if input.changed() {
                self.import_feedback = None;
            }
            let import_limit_reached =
                self.imported_projects.len() >= MAX_IMPORTED_COMPOSE_PROJECTS;
            if import_limit_reached {
                ui.label("This session has reached the 32-import limit.");
            }
            if ui
                .add_enabled(
                    self.can_import_current_project(profiles_valid)
                        && !import_limit_reached
                        && !actions.select_base_file
                        && !actions.add_override_file
                        && !actions.validate_files
                        && actions.remove_override.is_none(),
                    egui::Button::new("Import validated selection"),
                )
                .clicked()
            {
                let name = self.import_name_input.clone();
                if !self.import_current_project(&name) {
                    self.import_feedback = Some(
                        "Import rejected: names must be unique, non-empty, and at most 256 UTF-8 bytes; the session holds at most 32 imports.",
                    );
                }
            }
        });
        if let Some(feedback) = self.import_feedback {
            ui.label(feedback);
        }
        ui.heading("Imported local projects (this session only)");
        ui.label("Selecting one restores its ordered files and profiles, clears old results, and requires explicit revalidation.");
        for (index, project) in self.imported_projects.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(&project.display_name);
                if ui
                    .add_enabled(
                        !matches!(
                            &self.lifecycle,
                            ComposeLifecycleState::Confirming(_)
                                | ComposeLifecycleState::Running { .. }
                        ),
                        egui::Button::new("Use imported configuration"),
                    )
                    .clicked()
                {
                    imported_selection = Some(index);
                }
            });
        }
        if let Some(index) = imported_selection {
            // FerrumApp owns the async task handles; return cancellations before switching state.
            actions.cancel_validation |=
                self.is_file_selection_running() || self.is_validation_running();
            actions.cancel_status |= matches!(&self.status, ComposeStatusState::Loading { .. });
            actions.cancel_logs |= matches!(&self.logs, ComposeLogsState::Loading { .. });
            self.select_imported_project(index);
        }
        ui.separator();
        ui.heading("Declared services in selected files (all profiles, read-only)");
        ui.label("Local Compose CLI/file-derived inventory includes every declared service, including profile-gated services even when their profiles are inactive. These are not running containers. Listing does not query Engine status or logs.");
        match &self.validation {
            ComposeValidationState::Validating { .. } => {
                ui.spinner();
                ui.label("Loading declared services…");
            }
            ComposeValidationState::Succeeded { .. } if self.validated_services.is_empty() => {
                ui.label("No declared services were returned for the selected files.");
            }
            ComposeValidationState::Succeeded { .. } => {
                egui::ScrollArea::vertical()
                    .id_salt("compose-declared-services")
                    .max_height(240.0)
                    .show_rows(
                        ui,
                        COMPOSE_SERVICE_ROW_HEIGHT,
                        self.validated_services.len(),
                        |ui, rows| {
                            for row in rows {
                                ui.label(self.validated_services[row].as_str());
                            }
                        },
                    );
            }
            ComposeValidationState::Failed { .. } => {
                ui.label("Declared services are unavailable because validation failed.");
            }
            ComposeValidationState::Cancelled { .. } => {
                ui.label("Declared-service inventory cleared because validation was cancelled.");
            }
            _ => {
                ui.label("Validate the selected files to list all declared services.");
            }
        }

        match &self.validation {
            ComposeValidationState::Idle => {}
            ComposeValidationState::SelectingFile { override_file } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if *override_file {
                        "Choosing an override file…"
                    } else {
                        "Choosing a base file…"
                    });
                });
            }
            ComposeValidationState::Validating { file_name } => {
                ui.spinner();
                ui.label(format!("Validating Compose files: {file_name}"));
            }
            ComposeValidationState::Succeeded { file_name } => {
                ui.label(format!("Validated Compose files: {file_name}"));
                ui.label("Structural validation succeeded. Environment interpolation and service env_file resolution were skipped.");
            }
            ComposeValidationState::Failed { file_name, error } => {
                if !file_name.is_empty() {
                    ui.label(format!("Selected Compose files: {file_name}"));
                }
                ui.colored_label(egui::Color32::from_rgb(224, 174, 109), error.message());
            }
            ComposeValidationState::Cancelled { file_name } => {
                ui.label(format!(
                    "Validation cancelled for Compose files: {file_name}"
                ));
            }
            ComposeValidationState::SelectionCancelled => {
                ui.label("File selection cancelled.");
            }
        }
        ui.separator();
        ui.heading("Existing containers (read-only)");
        ui.label(concat!(
            "Status uses Ferrum's isolated CLI environment and skips the selected ",
            "project's default .env; environment-dependent project matching may ",
            "differ from a normal shell."
        ));
        ui.label(concat!(
            "Project is untrusted per-container metadata reported by Compose; it is not a canonical ",
            "identity, does not establish file ownership or authorization, and does not determine lifecycle scope.",
        ));
        actions.refresh_containers = ui
            .add_enabled(
                status_enabled
                    && profiles_valid
                    && !self.loading
                    && self.can_refresh_containers()
                    && !matches!(&self.status, ComposeStatusState::Loading { .. })
                    && !matches!(&self.lifecycle, ComposeLifecycleState::Running { .. }),
                egui::Button::new("Refresh existing containers"),
            )
            .clicked();
        match &self.status {
            ComposeStatusState::Idle => {}
            ComposeStatusState::Loading { .. } => {
                ui.spinner();
                ui.label("Refreshing existing Compose containers…");
                actions.cancel_status = ui.button("Cancel container refresh").clicked();
            }
            ComposeStatusState::Succeeded { .. } if self.containers.is_empty() => {
                ui.label("No existing containers for the selected Compose files.");
            }
            ComposeStatusState::Succeeded { .. } => {
                ui.columns(6, |columns| {
                    columns[0].strong("Container");
                    columns[1].strong("Service");
                    columns[2].strong("Project");
                    columns[3].strong("State");
                    columns[4].strong("Health");
                    columns[5].strong("Recent logs");
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("compose-container-status")
                    .max_height(280.0)
                    .show_rows(
                        ui,
                        COMPOSE_CONTAINER_ROW_HEIGHT,
                        self.containers.len(),
                        |ui, rows| {
                            for row in rows {
                                let container = &self.containers[row];
                                ui.columns(6, |columns| {
                                    columns[0]
                                        .add(egui::Label::new(container.name.as_str()).truncate());
                                    columns[1].add(
                                        egui::Label::new(container.service.as_str()).truncate(),
                                    );
                                    columns[2].add(
                                        egui::Label::new(
                                            container.project.as_deref().unwrap_or("Not reported"),
                                        )
                                        .truncate(),
                                    );
                                    columns[3]
                                        .add(egui::Label::new(container.state.as_str()).truncate());
                                    columns[4].add(
                                        egui::Label::new(
                                            container.health.as_deref().unwrap_or("—"),
                                        )
                                        .truncate(),
                                    );
                                    if columns[5]
                                        .add_enabled(
                                            self.can_request_service_logs(&container.service),
                                            egui::Button::new(format!(
                                                "Snapshot logs for {}",
                                                container.service
                                            )),
                                        )
                                        .clicked()
                                    {
                                        actions.logs_service = Some(container.service.clone());
                                    }
                                });
                            }
                        },
                    );
            }
            ComposeStatusState::Failed { error, .. } => {
                ui.colored_label(egui::Color32::from_rgb(224, 174, 109), error.message());
            }
            ComposeStatusState::Cancelled { .. } => {
                ui.label("Container refresh cancelled.");
            }
        }
        match &self.logs {
            ComposeLogsState::Idle => {}
            ComposeLogsState::Loading { service } => {
                ui.spinner();
                ui.label(format!(
                    "Checking current status and reading recent logs for {service}…"
                ));
                actions.cancel_logs = ui.button("Cancel log snapshot").clicked();
            }
            ComposeLogsState::Succeeded { service } => {
                ui.heading(format!("Recent logs: {service}"));
                ui.label(concat!(
                    "Compose tails up to 200 lines per container. Ferrum shows at most ",
                    "200 combined lines/256 KiB; replicated output may be truncated."
                ));
                if self.logs_output.is_empty() {
                    ui.label("No log output.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&self.logs_output).monospace());
                        });
                }
            }
            ComposeLogsState::Failed { error, .. } => {
                ui.colored_label(egui::Color32::from_rgb(224, 174, 109), error.message());
            }
            ComposeLogsState::Cancelled { .. } => {
                ui.label("Log snapshot cancelled.");
            }
        }
        ui.heading("Imported configuration lifecycle");
        ui.label("Start or stop affects only the currently selected, imported, and explicitly revalidated local files. Start requires existing containers; it does not create, build, or pull. Stop does not remove containers.");
        let lifecycle_enabled = status_enabled && self.can_start_lifecycle();
        ui.horizontal(|ui| {
            actions.start_imported = ui
                .add_enabled(
                    lifecycle_enabled,
                    egui::Button::new("Start imported project"),
                )
                .clicked();
            actions.stop_imported = ui
                .add_enabled(
                    lifecycle_enabled,
                    egui::Button::new("Stop imported project"),
                )
                .clicked();
        });
        match &self.lifecycle {
            ComposeLifecycleState::Idle => {}
            ComposeLifecycleState::Confirming(confirmation) => {
                ui.group(|ui| {
                    ui.heading(format!("Confirm Compose {}", confirmation.action.label()));
                    ui.label(format!(
                        "Imported local label: {}",
                        confirmation.selection.imported_label
                    ));
                    ui.label(format!("Ordered files: {}", confirmation.ordered_files.join(" + ")));
                    ui.label(format!(
                        "Profiles: {}",
                        if confirmation.selection.profiles.is_empty() {
                            "default".to_owned()
                        } else {
                            confirmation.selection.profiles.join(", ")
                        }
                    ));
                    if !status_enabled {
                        ui.label(
                            "The engine or Compose configuration is not ready; reconnect or wait before confirming.",
                        );
                    }
                    actions.confirm_lifecycle = ui
                        .add_enabled(
                            status_enabled,
                            egui::Button::new(format!(
                                "Confirm {}",
                                confirmation.action.label()
                            )),
                        )
                        .clicked();
                    actions.cancel_lifecycle = ui.button("Cancel lifecycle action").clicked();
                });
            }
            ComposeLifecycleState::Running { action, file_name } => {
                ui.spinner();
                ui.label(format!(
                    "Compose {} running for {file_name}…",
                    action.label()
                ));
            }
            ComposeLifecycleState::Succeeded { action, file_name } => {
                ui.label(format!(
                    "Compose {} succeeded for {file_name}.",
                    action.label()
                ));
            }
            ComposeLifecycleState::Failed {
                action,
                file_name,
                error,
            } => {
                ui.label(format!(
                    "Compose {} failed for {file_name}.",
                    action.label()
                ));
                ui.colored_label(egui::Color32::from_rgb(224, 174, 109), error.message());
            }
        }

        if let Some(version) = &self.version {
            ui.horizontal(|ui| {
                ui.label("Docker Compose version:");
                ui.label(version);
            });
        }

        if !available {
            ui.label(unavailable_message);
            return actions;
        }
        if !unavailable_message.is_empty() {
            ui.label(unavailable_message);
        }
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading Compose projects…");
            });
            return actions;
        }
        if let Some(error) = &self.error {
            ui.colored_label(
                egui::Color32::from_rgb(224, 174, 109),
                "Compose projects unavailable",
            );
            ui.label(error.message());
            return actions;
        }
        if !self.loaded {
            ui.label("Compose projects have not been loaded.");
            return actions;
        }

        if self.projects.is_empty() {
            ui.label("No Compose projects found.");
            return actions;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("compose-projects")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Project");
                    ui.strong("Status");
                    ui.end_row();
                    for project in &self.projects {
                        ui.add(egui::Label::new(project.name.as_str()).truncate());
                        ui.add(egui::Label::new(project.status.as_str()).truncate());
                        ui.end_row();
                    }
                });
        });
        actions
    }
}

#[cfg(target_os = "linux")]
async fn probe_compose_version(
    binary: &std::path::Path,
    isolated: &PrivateDockerConfig,
    search_path: &std::ffi::OsStr,
    deadline: Duration,
) -> Result<Vec<u8>, CommandFailure> {
    use tokio::process::Command;

    let mut command = Command::new(binary);
    isolated.configure(&mut command, search_path);
    command.args(["compose", "version", "--short"]);
    run_command_with_timeout(&mut command, deadline).await
}

#[cfg(target_os = "linux")]
async fn discover_with_binary(
    binary: &std::path::Path,
    socket_path: &std::path::Path,
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
) -> Result<ComposeDiscovery, ComposeError> {
    use tokio::process::Command;

    let isolated = PrivateDockerConfig::new(user_compose_plugin)
        .map_err(|_| ComposeError::PrivateConfigSetupFailed)?;
    let version_output = probe_compose_version(binary, &isolated, search_path, COMMAND_TIMEOUT)
        .await
        .map_err(|failure| match failure {
            CommandFailure::Launch => ComposeError::DockerCliMissing,
            CommandFailure::Spawn => ComposeError::CommandStartFailed,
            CommandFailure::Rejected
            | CommandFailure::TimedOut
            | CommandFailure::OutputLimit
            | CommandFailure::Io => ComposeError::ComposePluginUnavailable,
        })?;
    let version = parse_version(&version_output)
        .ok_or(ComposeError::ComposePluginUnavailable)?
        .to_owned();
    let socket_path = match socket_path.to_str() {
        Some(path) => path,
        None => {
            return Err(ComposeError::ProjectListFailed { version });
        }
    };

    let host = format!("unix://{socket_path}");
    let mut listing_command = Command::new(binary);
    isolated.configure(&mut listing_command, search_path);
    listing_command
        .arg("--host")
        .arg(host.as_str())
        .args(["compose", "ls", "--all", "--format", "json"]);
    let listing_output = match run_command_with_timeout(&mut listing_command, COMMAND_TIMEOUT).await
    {
        Ok(output) => output,
        Err(CommandFailure::Launch) => return Err(ComposeError::DockerCliMissing),
        Err(CommandFailure::Spawn) => return Err(ComposeError::CommandStartFailed),
        Err(_) => {
            return Err(ComposeError::ProjectListFailed { version });
        }
    };
    let projects = match parse_projects(&listing_output) {
        Some(projects) => projects,
        None => {
            return Err(ComposeError::ProjectListFailed { version });
        }
    };

    Ok(ComposeDiscovery { version, projects })
}

fn validate_service_selector(service: &str) -> Option<&str> {
    if service.is_empty() || service.len() > 128 {
        return None;
    }
    let mut bytes = service.bytes();
    let first = bytes.next()?;
    if !first.is_ascii_alphanumeric()
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some(service)
}
#[cfg(target_os = "linux")]
fn parse_declared_services(output: &[u8]) -> Option<Vec<String>> {
    use std::collections::HashSet;

    if output.len() > MAX_COMPOSE_SERVICE_OUTPUT_BYTES {
        return None;
    }
    if output.is_empty() {
        return Some(Vec::new());
    }
    let output = std::str::from_utf8(output).ok()?;
    let output = output.strip_suffix('\n').unwrap_or(output);
    let row_count = output.split('\n').count();
    if row_count > MAX_COMPOSE_SERVICES {
        return None;
    }
    let mut services = Vec::with_capacity(row_count);
    let mut identities = HashSet::with_capacity(row_count);
    let mut display_names = HashSet::with_capacity(row_count);
    for line in output.split('\n') {
        let name = line.strip_suffix('\r').unwrap_or(line);
        if name.is_empty()
            || name.len() > MAX_COMPOSE_SERVICE_NAME_BYTES
            || !identities.insert(name)
        {
            return None;
        }
        let display_name = escape_untrusted_display_text_with_limit(
            name.to_owned(),
            MAX_COMPOSE_SERVICE_DISPLAY_BYTES,
        )?;
        if !display_names.insert(display_name.clone()) {
            return None;
        }
        services.push(display_name);
    }
    Some(services)
}
fn parse_containers(output: &[u8]) -> Option<Vec<ComposeContainer>> {
    use std::collections::HashSet;
    let output = output.trim_ascii();
    if output.is_empty() {
        return Some(Vec::new());
    }
    let mut rows = Vec::new();
    let mut identities = HashSet::new();
    let mut display_names = HashSet::new();
    for line in output.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() || rows.len() >= MAX_COMPOSE_CONTAINERS {
            return None;
        }
        let row = serde_json::from_slice::<RawComposeContainer>(line).ok()?;
        if row.name.is_empty()
            || row.service.is_empty()
            || row.state.is_empty()
            || row.name.len() > MAX_CONTAINER_FIELD_BYTES
            || row.service.len() > MAX_CONTAINER_FIELD_BYTES
            || row.state.len() > MAX_CONTAINER_FIELD_BYTES
            || row
                .health
                .as_ref()
                .is_some_and(|value| value.len() > MAX_CONTAINER_FIELD_BYTES)
            || row
                .project
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > MAX_CONTAINER_FIELD_BYTES)
            || !identities.insert(row.name.clone())
        {
            return None;
        }
        let name = escape_untrusted_display_text_with_limit(row.name, MAX_CONTAINER_FIELD_BYTES)?;
        if !display_names.insert(name.clone()) {
            return None;
        }
        let service =
            escape_untrusted_display_text_with_limit(row.service, MAX_CONTAINER_FIELD_BYTES)?;
        let state = escape_untrusted_display_text_with_limit(row.state, MAX_CONTAINER_FIELD_BYTES)?;
        let project = match row.project {
            Some(project) => Some(escape_untrusted_display_text_with_limit(
                project,
                MAX_CONTAINER_FIELD_BYTES,
            )?),
            None => None,
        };
        let health = match row.health.filter(|health| !health.is_empty()) {
            Some(health) => Some(escape_untrusted_display_text_with_limit(
                health,
                MAX_CONTAINER_FIELD_BYTES,
            )?),
            None => None,
        };
        rows.push(ComposeContainer {
            name,
            service,
            project,
            state,
            health,
        });
    }
    Some(rows)
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct RawComposeContainer {
    name: String,
    service: String,
    state: String,
    health: Option<String>,
    project: Option<String>,
}

#[cfg(target_os = "linux")]
impl<'de> serde::Deserialize<'de> for RawComposeContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, IgnoredAny, MapAccess, Visitor};
        use std::fmt;
        struct ContainerVisitor;
        impl<'de> Visitor<'de> for ContainerVisitor {
            type Value = RawComposeContainer;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Docker Compose container status object")
            }
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut name = None;
                let mut service = None;
                let mut state = None;
                let mut health = None;
                let mut seen_health = false;
                let mut project = None;
                let mut seen_exit_code = false;
                let mut seen_command = false;
                let mut seen_id = false;
                let mut seen_publishers = false;
                let mut seen_project = false;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "Name" => {
                            if name.is_some() {
                                return Err(de::Error::duplicate_field("Name"));
                            }
                            name = Some(map.next_value::<String>()?);
                        }
                        "Service" => {
                            if service.is_some() {
                                return Err(de::Error::duplicate_field("Service"));
                            }
                            service = Some(map.next_value::<String>()?);
                        }
                        "State" => {
                            if state.is_some() {
                                return Err(de::Error::duplicate_field("State"));
                            }
                            state = Some(map.next_value::<String>()?);
                        }
                        "Health" => {
                            if seen_health {
                                return Err(de::Error::duplicate_field("Health"));
                            }
                            seen_health = true;
                            health = map.next_value::<Option<String>>()?;
                        }
                        "ExitCode" => {
                            if seen_exit_code {
                                return Err(de::Error::duplicate_field("ExitCode"));
                            }
                            seen_exit_code = true;
                            map.next_value::<IgnoredAny>()?;
                        }
                        "Command" => {
                            if seen_command {
                                return Err(de::Error::duplicate_field("Command"));
                            }
                            seen_command = true;
                            map.next_value::<IgnoredAny>()?;
                        }
                        "ID" => {
                            if seen_id {
                                return Err(de::Error::duplicate_field("ID"));
                            }
                            seen_id = true;
                            map.next_value::<IgnoredAny>()?;
                        }
                        "Publishers" => {
                            if seen_publishers {
                                return Err(de::Error::duplicate_field("Publishers"));
                            }
                            seen_publishers = true;
                            map.next_value::<IgnoredAny>()?;
                        }
                        "Project" => {
                            if seen_project {
                                return Err(de::Error::duplicate_field("Project"));
                            }
                            seen_project = true;
                            project = Some(map.next_value::<String>()?);
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(RawComposeContainer {
                    name: name.ok_or_else(|| de::Error::missing_field("Name"))?,
                    service: service.ok_or_else(|| de::Error::missing_field("Service"))?,
                    state: state.ok_or_else(|| de::Error::missing_field("State"))?,
                    health,
                    project,
                })
            }
        }
        deserializer.deserialize_map(ContainerVisitor)
    }
}

fn parse_version(output: &[u8]) -> Option<&str> {
    if output.len() > MAX_VERSION_BYTES + 1 {
        return None;
    }
    let output = std::str::from_utf8(output).ok()?.trim();
    if output.is_empty()
        || output.len() > MAX_VERSION_BYTES
        || output.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '+' | '_')
        })
    {
        return None;
    }
    Some(output)
}

#[cfg(target_os = "linux")]
fn parse_projects(output: &[u8]) -> Option<Vec<ComposeProject>> {
    use std::collections::HashSet;

    let rows = serde_json::from_slice::<Vec<RawComposeProject>>(output).ok()?;
    if rows.len() > MAX_PROJECTS {
        return None;
    }

    let mut identities = HashSet::with_capacity(rows.len());
    let mut display_names = HashSet::with_capacity(rows.len());
    let mut projects = Vec::with_capacity(rows.len());
    for row in &rows {
        if row.name.is_empty()
            || row.name.len() > MAX_PROJECT_FIELD_BYTES
            || row.status.len() > MAX_PROJECT_FIELD_BYTES
            || !identities.insert(row.name.as_str())
        {
            return None;
        }
        let name = escape_untrusted_display_text(&row.name).into_owned();
        if !display_names.insert(name.clone()) {
            return None;
        }
        let status = escape_untrusted_display_text(&row.status).into_owned();
        projects.push(ComposeProject { name, status });
    }
    Some(projects)
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct RawComposeProject {
    name: String,
    status: String,
}

#[cfg(target_os = "linux")]
impl<'de> serde::Deserialize<'de> for RawComposeProject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, IgnoredAny, MapAccess, Visitor};
        use std::fmt;

        struct ProjectVisitor;

        impl<'de> Visitor<'de> for ProjectVisitor {
            type Value = RawComposeProject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Docker Compose project object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut name = None;
                let mut status = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "Name" => {
                            if name.is_some() {
                                return Err(de::Error::duplicate_field("Name"));
                            }
                            name = Some(map.next_value::<String>()?);
                        }
                        "Status" => {
                            if status.is_some() {
                                return Err(de::Error::duplicate_field("Status"));
                            }
                            status = Some(map.next_value::<String>()?);
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(RawComposeProject {
                    name: name.ok_or_else(|| de::Error::missing_field("Name"))?,
                    status: status.ok_or_else(|| de::Error::missing_field("Status"))?,
                })
            }
        }

        deserializer.deserialize_map(ProjectVisitor)
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum CommandFailure {
    Launch,
    Spawn,
    Rejected,
    TimedOut,
    OutputLimit,
    Io,
}

#[cfg(target_os = "linux")]
struct ProcessGroupGuard {
    id: libc::pid_t,
    active: bool,
}

#[cfg(target_os = "linux")]
impl ProcessGroupGuard {
    fn terminate(&mut self) {
        if self.active {
            // SAFETY: `run_command` assigns this child a fresh process group,
            // so the negative ID targets only the Docker CLI and its children.
            let _ = unsafe { libc::kill(-self.id, libc::SIGKILL) };
            self.active = false;
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

#[cfg(target_os = "linux")]
impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(target_os = "linux")]
async fn run_command_with_timeout(
    command: &mut tokio::process::Command,
    deadline: Duration,
) -> Result<Vec<u8>, CommandFailure> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    #[cfg(test)]
    let _test_subprocess_guard = crate::TEST_SUBPROCESS_LOCK.lock().await;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    // SAFETY: The pre-exec closure uses only stack locals and AS-Safe
    // getrlimit/setrlimit calls. It performs no allocation, locking, logging,
    // or environment access after fork.
    unsafe {
        command.as_std_mut().pre_exec(|| {
            let mut limit: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_AS, &mut limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            limit.rlim_cur = limit.rlim_cur.min(CHILD_ADDRESS_SPACE_LIMIT);
            limit.rlim_max = limit.rlim_max.min(CHILD_ADDRESS_SPACE_LIMIT);
            limit.rlim_cur = limit.rlim_cur.min(limit.rlim_max);
            // SAFETY: setrlimit is AS-Safe and `limit` is a stack-local copy
            // of RLIMIT_AS with non-increasing soft and hard bounds.
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            if libc::getrlimit(libc::RLIMIT_DATA, &mut limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            limit.rlim_cur = limit.rlim_cur.min(CHILD_DATA_LIMIT);
            limit.rlim_max = limit.rlim_max.min(CHILD_DATA_LIMIT);
            limit.rlim_cur = limit.rlim_cur.min(limit.rlim_max);
            // SAFETY: setrlimit is AS-Safe and `limit` is a stack-local copy
            // of RLIMIT_DATA with non-increasing soft and hard bounds.
            if libc::setrlimit(libc::RLIMIT_DATA, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CommandFailure::Launch
        } else {
            CommandFailure::Spawn
        }
    })?;
    let mut process_group = child
        .id()
        .and_then(|id| libc::pid_t::try_from(id).ok())
        .map(|id| ProcessGroupGuard { id, active: true });
    let result = tokio::time::timeout(deadline, async {
        let Some(stdout) = child.stdout.take() else {
            return Err(CommandFailure::Io);
        };
        let mut output = Vec::with_capacity(4096);
        let read_result = stdout
            .take((MAX_COMMAND_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut output)
            .await;
        if read_result.is_err() {
            if let Some(group) = process_group.as_mut() {
                group.terminate();
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(CommandFailure::Io);
        }
        if output.len() > MAX_COMMAND_OUTPUT_BYTES {
            if let Some(group) = process_group.as_mut() {
                group.terminate();
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(CommandFailure::OutputLimit);
        }
        let status = child.wait().await.map_err(|_| CommandFailure::Io)?;
        if let Some(group) = process_group.as_mut() {
            group.disarm();
        }
        if status.success() {
            Ok(output)
        } else {
            Err(CommandFailure::Rejected)
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            if let Some(group) = process_group.as_mut() {
                group.terminate();
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(CommandFailure::TimedOut)
        }
    }
}

#[cfg(target_os = "linux")]
struct PrivateDockerConfig {
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    docker_config: std::path::PathBuf,
}

#[cfg(target_os = "linux")]
impl PrivateDockerConfig {
    fn new(user_compose_plugin: Option<&std::path::Path>) -> std::io::Result<Self> {
        use std::{
            fs,
            os::unix::fs::{DirBuilderExt, PermissionsExt},
            sync::atomic::{AtomicU64, Ordering},
        };

        static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir();
        let mut root = None;
        for _ in 0..32 {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let candidate =
                parent.join(format!("ferrum-compose-{}-{sequence}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&candidate) {
                Ok(()) => {
                    root = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        let root = root.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "private directory namespace is unavailable",
            )
        })?;
        let setup = (|| {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
            let home = root.join("home");
            let docker_config = root.join("docker-config");
            create_private_directory(&home)?;
            create_private_directory(&docker_config)?;
            if let Some(plugin) = user_compose_plugin {
                let plugin_directory = docker_config.join("cli-plugins");
                create_private_directory(&plugin_directory)?;
                // Link only the Compose executable; keep saved Docker config
                // files and credential/proxy settings outside the child environment.
                let _ = std::os::unix::fs::symlink(plugin, plugin_directory.join("docker-compose"));
            }
            Ok(Self {
                root: root.clone(),
                home,
                docker_config,
            })
        })();
        if setup.is_err() {
            let _ = fs::remove_dir_all(&root);
        }
        setup
    }

    fn configure(&self, command: &mut tokio::process::Command, search_path: &std::ffi::OsStr) {
        command
            .env_clear()
            .env("PATH", search_path)
            .env("HOME", &self.home)
            .env("DOCKER_CONFIG", &self.docker_config)
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("GOMEMLIMIT", CHILD_GO_MEMORY_LIMIT)
            // Remote Compose sources can make Git/registry requests during
            // config parsing; disable both loaders for every CLI invocation.
            .env("COMPOSE_EXPERIMENTAL_GIT_REMOTE", "false")
            .env("COMPOSE_EXPERIMENTAL_OCI_REMOTE", "false");
    }
}

#[cfg(target_os = "linux")]
impl Drop for PrivateDockerConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(target_os = "linux")]
fn create_private_directory(path: &std::path::Path) -> std::io::Result<()> {
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, PermissionsExt},
    };

    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(target_os = "linux")]
fn user_compose_plugin() -> Option<std::path::PathBuf> {
    use std::env;
    use std::path::PathBuf;

    let config_directory = env::var_os("DOCKER_CONFIG")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".docker"))
        })?;
    let plugin = config_directory.join("cli-plugins/docker-compose");
    plugin
        .is_file()
        .then(|| plugin.canonicalize().ok())
        .flatten()
}
#[cfg(target_os = "linux")]
fn docker_binary(search_path: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    std::env::split_paths(search_path)
        .map(|directory| directory.join("docker"))
        .find_map(|candidate| {
            candidate
                .is_file()
                .then(|| candidate.canonicalize().ok())
                .flatten()
        })
}
#[cfg(target_os = "linux")]
fn selected_compose_files_are_valid(files: &[std::path::PathBuf]) -> bool {
    !files.is_empty()
        && files.len() <= MAX_COMPOSE_FILES
        && files.iter().enumerate().all(|(index, file)| {
            file.is_absolute()
                && file.parent().is_some_and(std::path::Path::is_dir)
                && file
                    .symlink_metadata()
                    .is_ok_and(|metadata| metadata.file_type().is_file())
                && !files[..index].iter().any(|selected| selected == file)
        })
}
#[cfg(target_os = "linux")]
fn selected_compose_files_size_exceeds_limit(files: &[std::path::PathBuf]) -> bool {
    files.iter().fold(0_u64, |total, file| {
        let size = file
            .symlink_metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(MAX_COMPOSE_INPUT_BYTES.saturating_add(1));
        total.saturating_add(size)
    }) > MAX_COMPOSE_INPUT_BYTES
}

#[cfg(target_os = "linux")]
pub(super) async fn validate_compose_files(
    files: &[std::path::PathBuf],
    profiles: &[String],
) -> ComposeValidationResult {
    use std::env;
    if !valid_profile_list(profiles) {
        return ComposeValidationResult::failed(ComposeValidationError::InvalidProfiles);
    }
    if !selected_compose_files_are_valid(files) {
        return ComposeValidationResult::failed(ComposeValidationError::InvalidSelectedFile);
    }
    // Keep input-limit errors deterministic even when Docker is unavailable;
    // the report path rechecks after CLI discovery before starting a subprocess.
    if selected_compose_files_size_exceeds_limit(files) {
        return ComposeValidationResult::failed(ComposeValidationError::InputTooLarge);
    }

    let Some(search_path) = env::var_os("PATH") else {
        return ComposeValidationResult::failed(ComposeValidationError::DockerCliMissing);
    };
    let Some(binary) = docker_binary(&search_path) else {
        return ComposeValidationResult::failed(ComposeValidationError::DockerCliMissing);
    };
    let user_compose_plugin = user_compose_plugin();
    validate_with_profiles_report(
        &binary,
        files,
        profiles,
        &search_path,
        user_compose_plugin.as_deref(),
        COMMAND_TIMEOUT,
    )
    .await
}

#[cfg(all(target_os = "linux", test))]
async fn validate_with_binary(
    binary: &std::path::Path,
    file: &std::path::Path,
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
    deadline: Duration,
) -> Result<(), ComposeValidationError> {
    let files = [file.to_path_buf()];
    validate_with_files(binary, &files, search_path, user_compose_plugin, deadline).await
}

#[cfg(all(target_os = "linux", test))]
async fn validate_with_files(
    binary: &std::path::Path,
    files: &[std::path::PathBuf],
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
    deadline: Duration,
) -> Result<(), ComposeValidationError> {
    validate_with_files_report(binary, files, search_path, user_compose_plugin, deadline)
        .await
        .outcome
}

#[cfg(all(target_os = "linux", test))]
async fn validate_with_binary_report(
    binary: &std::path::Path,
    file: &std::path::Path,
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
    deadline: Duration,
) -> ComposeValidationResult {
    let files = [file.to_path_buf()];
    validate_with_files_report(binary, &files, search_path, user_compose_plugin, deadline).await
}

#[cfg(all(target_os = "linux", test))]
async fn validate_with_files_report(
    binary: &std::path::Path,
    files: &[std::path::PathBuf],
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
    deadline: Duration,
) -> ComposeValidationResult {
    validate_with_profiles_report(
        binary,
        files,
        &[],
        search_path,
        user_compose_plugin,
        deadline,
    )
    .await
}

#[cfg(target_os = "linux")]
async fn validate_with_profiles_report(
    binary: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
    deadline: Duration,
) -> ComposeValidationResult {
    use tokio::process::Command;

    if !valid_profile_list(profiles) {
        return ComposeValidationResult::failed(ComposeValidationError::InvalidProfiles);
    }
    if !selected_compose_files_are_valid(files) {
        return ComposeValidationResult::failed(ComposeValidationError::InvalidSelectedFile);
    }
    if selected_compose_files_size_exceeds_limit(files) {
        return ComposeValidationResult::failed(ComposeValidationError::InputTooLarge);
    }
    let parent = files[0].parent().expect("absolute file has a parent");
    let isolated = match PrivateDockerConfig::new(user_compose_plugin) {
        Ok(isolated) => isolated,
        Err(_) => {
            return ComposeValidationResult::failed(
                ComposeValidationError::PrivateConfigSetupFailed,
            );
        }
    };
    let version_output = match probe_compose_version(binary, &isolated, search_path, deadline).await
    {
        Ok(output) => output,
        Err(failure) => {
            let error = match failure {
                CommandFailure::Launch => ComposeValidationError::DockerCliMissing,
                CommandFailure::Spawn => ComposeValidationError::CommandStartFailed,
                CommandFailure::Rejected
                | CommandFailure::TimedOut
                | CommandFailure::OutputLimit
                | CommandFailure::Io => ComposeValidationError::ComposePluginUnavailable,
            };
            return ComposeValidationResult::failed(error);
        }
    };
    let Some(version) = parse_version(&version_output).map(str::to_owned) else {
        return ComposeValidationResult::failed(ComposeValidationError::ComposePluginUnavailable);
    };
    let mut command = Command::new(binary);
    isolated.configure(&mut command, search_path);
    command
        .env("COMPOSE_DISABLE_ENV_FILE", "1")
        .args(["compose"])
        .arg("--project-directory")
        .arg(parent);
    for file in files {
        command.arg("--file").arg(file);
    }
    for profile in profiles {
        command.arg("--profile").arg(profile);
    }
    command.args([
        "config",
        "--no-interpolate",
        "--no-env-resolution",
        "--services",
    ]);
    let (services, outcome) = match run_command_with_timeout(&mut command, deadline).await {
        Ok(output) => match parse_declared_services(&output) {
            Some(services) => (services, Ok(())),
            None => (Vec::new(), Err(ComposeValidationError::ValidationFailed)),
        },
        Err(CommandFailure::Launch) => (Vec::new(), Err(ComposeValidationError::DockerCliMissing)),
        Err(CommandFailure::Spawn) => (Vec::new(), Err(ComposeValidationError::CommandStartFailed)),
        Err(
            CommandFailure::Rejected
            | CommandFailure::TimedOut
            | CommandFailure::OutputLimit
            | CommandFailure::Io,
        ) => (Vec::new(), Err(ComposeValidationError::ValidationFailed)),
    };
    ComposeValidationResult {
        version: Some(version),
        services,
        outcome,
    }
}

#[cfg(target_os = "linux")]
pub(super) async fn discover_projects(
    socket_path: &std::path::Path,
) -> Result<ComposeDiscovery, ComposeError> {
    use std::env;

    let search_path = env::var_os("PATH").ok_or(ComposeError::DockerCliMissing)?;
    let binary = docker_binary(&search_path).ok_or(ComposeError::DockerCliMissing)?;
    let user_compose_plugin = user_compose_plugin();
    discover_with_binary(
        &binary,
        socket_path,
        &search_path,
        user_compose_plugin.as_deref(),
    )
    .await
}
#[cfg(target_os = "linux")]
async fn containers_with_binary(
    binary: &std::path::Path,
    socket_path: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
) -> Result<Vec<ComposeContainer>, ComposeStatusError> {
    use tokio::process::Command;
    if !valid_profile_list(profiles)
        || !selected_compose_files_are_valid(files)
        || selected_compose_files_size_exceeds_limit(files)
    {
        return Err(ComposeStatusError::QueryFailed);
    }
    let isolated = PrivateDockerConfig::new(user_compose_plugin)
        .map_err(|_| ComposeStatusError::PrivateConfigSetupFailed)?;
    let socket_path = socket_path
        .to_str()
        .ok_or(ComposeStatusError::QueryFailed)?;
    let host = format!("unix://{socket_path}");
    let mut command = Command::new(binary);
    isolated.configure(&mut command, search_path);
    command
        .env("COMPOSE_DISABLE_ENV_FILE", "1")
        .arg("--host")
        .arg(host)
        .args(["compose", "--project-directory"])
        .arg(files[0].parent().ok_or(ComposeStatusError::QueryFailed)?);
    for file in files {
        command.arg("--file").arg(file);
    }
    for profile in profiles {
        command.arg("--profile").arg(profile);
    }
    command.args(["ps", "--all", "--format", "json"]);
    let output = run_command_with_timeout(&mut command, COMMAND_TIMEOUT)
        .await
        .map_err(|failure| match failure {
            CommandFailure::Launch => ComposeStatusError::DockerCliMissing,
            _ => ComposeStatusError::QueryFailed,
        })?;
    parse_containers(&output).ok_or(ComposeStatusError::InvalidOutput)
}

#[cfg(target_os = "linux")]
const MAX_COMPOSE_LOG_LINE_BYTES: usize = 4096;

#[cfg(target_os = "linux")]
const MAX_COMPOSE_LOG_DISPLAY_BYTES: usize = 256 * 1024;
#[cfg(target_os = "linux")]
fn sanitize_compose_logs(output: &[u8]) -> Option<String> {
    if output.len() > MAX_COMMAND_OUTPUT_BYTES {
        return None;
    }
    let mut sanitized = String::with_capacity(output.len().min(MAX_COMPOSE_LOG_DISPLAY_BYTES));
    for (index, line) in String::from_utf8_lossy(output)
        .lines()
        .take(200)
        .enumerate()
    {
        let escaped =
            escape_untrusted_display_text_with_limit(line.to_owned(), MAX_COMPOSE_LOG_LINE_BYTES)?;
        let separator_bytes = usize::from(index > 0);
        if sanitized.len() + separator_bytes + escaped.len() > MAX_COMPOSE_LOG_DISPLAY_BYTES {
            break;
        }
        if index > 0 {
            sanitized.push('\n');
        }
        sanitized.push_str(&escaped);
    }
    Some(sanitized)
}

#[cfg(target_os = "linux")]
async fn service_logs_with_binary(
    binary: &std::path::Path,
    socket_path: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
    service: &str,
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
) -> Result<String, ComposeLogsError> {
    use tokio::process::Command;
    if !valid_profile_list(profiles) {
        return Err(ComposeLogsError::QueryFailed);
    }
    let service = validate_service_selector(service).ok_or(ComposeLogsError::QueryFailed)?;
    let current_containers = containers_with_binary(
        binary,
        socket_path,
        files,
        profiles,
        search_path,
        user_compose_plugin,
    )
    .await
    .map_err(|error| match error {
        ComposeStatusError::DockerCliMissing => ComposeLogsError::DockerCliMissing,
        ComposeStatusError::PrivateConfigSetupFailed => ComposeLogsError::PrivateConfigSetupFailed,
        ComposeStatusError::QueryFailed | ComposeStatusError::InvalidOutput => {
            ComposeLogsError::QueryFailed
        }
    })?;
    if !current_containers
        .iter()
        .any(|container| container.service == service)
    {
        return Err(ComposeLogsError::ServiceUnavailable);
    }
    if !selected_compose_files_are_valid(files) || selected_compose_files_size_exceeds_limit(files)
    {
        return Err(ComposeLogsError::QueryFailed);
    }
    let isolated = PrivateDockerConfig::new(user_compose_plugin)
        .map_err(|_| ComposeLogsError::PrivateConfigSetupFailed)?;
    let socket_path = socket_path.to_str().ok_or(ComposeLogsError::QueryFailed)?;
    let host = format!("unix://{socket_path}");
    let parent = files[0].parent().ok_or(ComposeLogsError::QueryFailed)?;
    let mut command = Command::new(binary);
    isolated.configure(&mut command, search_path);
    command
        .env("COMPOSE_DISABLE_ENV_FILE", "1")
        .arg("--host")
        .arg(host)
        .args(["compose", "--project-directory"])
        .arg(parent);
    for file in files {
        command.arg("--file").arg(file);
    }
    for profile in profiles {
        command.arg("--profile").arg(profile);
    }
    command
        .args(["logs", "--tail", "200", "--no-color"])
        .arg(service);
    let output = run_command_with_timeout(&mut command, COMMAND_TIMEOUT)
        .await
        .map_err(|failure| match failure {
            CommandFailure::Launch => ComposeLogsError::DockerCliMissing,
            _ => ComposeLogsError::QueryFailed,
        })?;
    sanitize_compose_logs(&output).ok_or(ComposeLogsError::InvalidOutput)
}

#[cfg(target_os = "linux")]
pub(super) async fn compose_service_logs(
    socket_path: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
    service: &str,
) -> ComposeLogsResult {
    let result = async {
        validate_service_selector(service).ok_or(ComposeLogsError::QueryFailed)?;
        let search_path = std::env::var_os("PATH").ok_or(ComposeLogsError::DockerCliMissing)?;
        let binary = docker_binary(&search_path).ok_or(ComposeLogsError::DockerCliMissing)?;
        let plugin = user_compose_plugin();
        service_logs_with_binary(
            &binary,
            socket_path,
            files,
            profiles,
            service,
            &search_path,
            plugin.as_deref(),
        )
        .await
    }
    .await;
    match result {
        Ok(output) => ComposeLogsResult {
            output,
            outcome: Ok(()),
        },
        Err(error) => ComposeLogsResult {
            output: String::new(),
            outcome: Err(error),
        },
    }
}

#[cfg(target_os = "linux")]
async fn lifecycle_with_binary(
    binary: &std::path::Path,
    socket_path: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
    action: ComposeLifecycleAction,
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
) -> ComposeLifecycleResult {
    use tokio::process::Command;
    let validation = validate_with_profiles_report(
        binary,
        files,
        profiles,
        search_path,
        user_compose_plugin,
        COMMAND_TIMEOUT,
    )
    .await;
    if let Err(error) = validation.outcome {
        let error = match error {
            ComposeValidationError::DockerCliMissing
            | ComposeValidationError::ComposePluginUnavailable => {
                ComposeLifecycleError::DockerCliMissing
            }
            ComposeValidationError::CommandStartFailed => ComposeLifecycleError::CommandStartFailed,
            ComposeValidationError::PrivateConfigSetupFailed => {
                ComposeLifecycleError::PrivateConfigSetupFailed
            }
            _ => ComposeLifecycleError::ValidationFailed,
        };
        return ComposeLifecycleResult {
            outcome: Err(error),
            submitted: false,
        };
    }
    let isolated = match PrivateDockerConfig::new(user_compose_plugin) {
        Ok(isolated) => isolated,
        Err(_) => {
            return ComposeLifecycleResult {
                outcome: Err(ComposeLifecycleError::PrivateConfigSetupFailed),
                submitted: false,
            };
        }
    };
    let Some(socket_path) = socket_path.to_str() else {
        return ComposeLifecycleResult {
            outcome: Err(ComposeLifecycleError::OperationFailed),
            submitted: false,
        };
    };
    let Some(parent) = files.first().and_then(|file| file.parent()) else {
        return ComposeLifecycleResult {
            outcome: Err(ComposeLifecycleError::ValidationFailed),
            submitted: false,
        };
    };
    let mut command = Command::new(binary);
    isolated.configure(&mut command, search_path);
    command
        .env("COMPOSE_DISABLE_ENV_FILE", "1")
        .arg("--host")
        .arg(format!("unix://{socket_path}"))
        .args(["compose", "--project-directory"])
        .arg(parent);
    for file in files {
        command.arg("--file").arg(file);
    }
    for profile in profiles {
        command.arg("--profile").arg(profile);
    }
    command.arg(action.label());
    run_lifecycle_command_with_timeout(&mut command, LIFECYCLE_COMMAND_TIMEOUT).await
}

#[cfg(target_os = "linux")]
async fn run_lifecycle_command_with_timeout(
    command: &mut tokio::process::Command,
    deadline: Duration,
) -> ComposeLifecycleResult {
    let (outcome, submitted) = match run_command_with_timeout(command, deadline).await {
        Ok(_) => (Ok(()), true),
        Err(CommandFailure::Launch) => (Err(ComposeLifecycleError::DockerCliMissing), false),
        Err(CommandFailure::Spawn) => (Err(ComposeLifecycleError::CommandStartFailed), false),
        Err(_) => (Err(ComposeLifecycleError::OperationFailed), true),
    };
    ComposeLifecycleResult { outcome, submitted }
}

#[cfg(target_os = "linux")]
pub(super) async fn compose_lifecycle(
    socket_path: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
    action: ComposeLifecycleAction,
) -> ComposeLifecycleResult {
    let Some(search_path) = std::env::var_os("PATH") else {
        return ComposeLifecycleResult {
            outcome: Err(ComposeLifecycleError::DockerCliMissing),
            submitted: false,
        };
    };
    let Some(binary) = docker_binary(&search_path) else {
        return ComposeLifecycleResult {
            outcome: Err(ComposeLifecycleError::DockerCliMissing),
            submitted: false,
        };
    };
    let plugin = user_compose_plugin();
    lifecycle_with_binary(
        &binary,
        socket_path,
        files,
        profiles,
        action,
        &search_path,
        plugin.as_deref(),
    )
    .await
}

pub(super) async fn existing_compose_containers(
    socket_path: &std::path::Path,
    files: &[std::path::PathBuf],
    profiles: &[String],
) -> ComposeStatusResult {
    if validate_compose_files(files, profiles)
        .await
        .outcome
        .is_err()
    {
        return ComposeStatusResult {
            containers: Vec::new(),
            outcome: Err(ComposeStatusError::QueryFailed),
        };
    }
    let result = async {
        let search_path = std::env::var_os("PATH").ok_or(ComposeStatusError::DockerCliMissing)?;
        let binary = docker_binary(&search_path).ok_or(ComposeStatusError::DockerCliMissing)?;
        let plugin = user_compose_plugin();
        containers_with_binary(
            &binary,
            socket_path,
            files,
            profiles,
            &search_path,
            plugin.as_deref(),
        )
        .await
    }
    .await;
    match result {
        Ok(containers) => ComposeStatusResult {
            containers,
            outcome: Ok(()),
        },
        Err(error) => ComposeStatusResult {
            containers: Vec::new(),
            outcome: Err(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn compose_profiles_parse_limits_duplicates_names_and_wildcard() {
        assert_eq!(parse_compose_profiles(""), Ok(Vec::new()));
        assert_eq!(
            parse_compose_profiles(" web, api-v2 "),
            Ok(vec!["web".to_owned(), "api-v2".to_owned()])
        );
        assert_eq!(parse_compose_profiles("*"), Ok(vec!["*".to_owned()]));
        assert!(parse_compose_profiles("web,,api").is_err());
        assert!(parse_compose_profiles("web, web").is_err());
        assert!(parse_compose_profiles("a").is_err());
        assert!(parse_compose_profiles("-web").is_err());
        assert!(parse_compose_profiles("web!,api").is_err());
        assert!(parse_compose_profiles("*,web").is_err());
        assert!(parse_compose_profiles("web,*").is_err());
        assert!(parse_compose_profiles(&"a".repeat(MAX_COMPOSE_PROFILE_NAME_BYTES + 1)).is_err());
        assert!(
            parse_compose_profiles(
                &(0..=MAX_COMPOSE_PROFILES)
                    .map(|index| format!("p{index}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .is_err()
        );
        assert!(parse_compose_profiles(&"x".repeat(MAX_COMPOSE_PROFILE_INPUT_BYTES + 1)).is_err());
        assert_eq!(
            parse_compose_profiles(&format!(
                "p{},",
                "x".repeat(MAX_COMPOSE_PROFILE_NAME_BYTES - 1)
            )),
            Err(())
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn declared_service_output_is_bounded_unique_valid_and_display_escaped() {
        assert_eq!(parse_declared_services(b""), Some(Vec::new()));
        assert_eq!(
            parse_declared_services(b"web\napi-v2\r\n"),
            Some(vec!["web".to_owned(), "api-v2".to_owned()])
        );
        assert!(parse_declared_services(b"\napi\n").is_none());
        let escaped = parse_declared_services("safe\u{202e}\n".as_bytes())
            .expect("display controls should be retained as escaped labels");
        assert!(escaped[0].contains(r"\u{202e}"));
        assert!(parse_declared_services(b"api\napi\n").is_none());
        assert!(parse_declared_services(b"api\n\n").is_none());
        assert!(parse_declared_services(&[0xff]).is_none());
        let at_limit = (0..MAX_COMPOSE_SERVICES)
            .map(|index| format!("service-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            parse_declared_services(at_limit.as_bytes()).unwrap().len(),
            MAX_COMPOSE_SERVICES
        );
        assert!(
            parse_declared_services(
                format!("{}\n", "a".repeat(MAX_COMPOSE_SERVICE_NAME_BYTES + 1)).as_bytes()
            )
            .is_none()
        );
        assert!(
            parse_declared_services(
                format!("{}\n", "service\n".repeat(MAX_COMPOSE_SERVICES + 1)).as_bytes()
            )
            .is_none()
        );
        assert!(
            parse_declared_services(&vec![b'a'; MAX_COMPOSE_SERVICE_OUTPUT_BYTES + 1]).is_none()
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn compose_profile_limits_accept_exact_name_and_count_bounds() {
        assert_eq!(
            parse_compose_profiles(&format!(
                "a{}",
                "x".repeat(MAX_COMPOSE_PROFILE_NAME_BYTES - 1)
            ))
            .unwrap()
            .len(),
            1
        );
        let maximum_count = (0..MAX_COMPOSE_PROFILES)
            .map(|index| format!("p{index}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_compose_profiles(&maximum_count).unwrap().len(),
            MAX_COMPOSE_PROFILES
        );
        let mut maximum_input = vec![format!("a{}", "x".repeat(48))];
        maximum_input.extend((1..MAX_COMPOSE_PROFILES).map(|index| {
            let prefix = format!("p{index}");
            format!(
                "{prefix}{}",
                "x".repeat(MAX_COMPOSE_PROFILE_NAME_BYTES - prefix.len())
            )
        }));
        let maximum_input = maximum_input.join(",");
        assert_eq!(maximum_input.len(), MAX_COMPOSE_PROFILE_INPUT_BYTES);
        assert_eq!(
            parse_compose_profiles(&maximum_input).unwrap().len(),
            MAX_COMPOSE_PROFILES
        );
    }
    #[cfg(target_os = "linux")]
    mod subprocess {
        use super::*;
        use std::{ffi::OsStr, fs, os::unix::fs::PermissionsExt, path::PathBuf};

        struct FixtureDirectory(PrivateDockerConfig);

        impl FixtureDirectory {
            fn new() -> Self {
                Self(
                    PrivateDockerConfig::new(None)
                        .expect("a private fixture directory should be created"),
                )
            }

            fn path(&self) -> &std::path::Path {
                &self.0.root
            }
        }

        fn shell_quote(value: &str) -> String {
            format!("'{}'", value.replace('\'', "'\\''"))
        }

        fn fixture_cli(
            directory: &FixtureDirectory,
            socket: &std::path::Path,
            version_output: &str,
            version_exit_code: u8,
            listing_output: &str,
            listing_exit_code: u8,
        ) -> PathBuf {
            let socket = format!("unix://{}", socket.to_str().unwrap());
            let script = format!(
                "#!/bin/sh\n\
                 if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{DOCKER_CUSTOM_HEADERS+x}}${{DOCKER_DEFAULT_PLATFORM+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{ALL_PROXY+x}}${{FTP_PROXY+x}}${{http_proxy+x}}${{https_proxy+x}}${{no_proxy+x}}${{all_proxy+x}}${{ftp_proxy+x}}\" ]; then exit 91; fi\n\
                 [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] || exit 92\n\
                 if [ \"$1\" = compose ]; then\n\
                   [ \"$#\" -eq 3 ] && [ \"$2\" = version ] && [ \"$3\" = --short ] || exit 93\n\
                   printf '%s' {}\n\
                   exit {}\n\
                 fi\n\
                 if [ \"$1\" = --host ]; then\n\
                   [ \"$#\" -eq 7 ] && [ \"$2\" = {} ] || exit 94\n\
                   [ \"$3\" = compose ] && [ \"$4\" = ls ] && [ \"$5\" = --all ] && [ \"$6\" = --format ] && [ \"$7\" = json ] || exit 95\n\
                   printf '%s' {} >&2\n\
                   printf '%s' {}\n\
                   exit {}\n\
                 fi\n\
                 exit 96\n",
                shell_quote(version_output),
                version_exit_code,
                shell_quote(&socket),
                shell_quote("fixture diagnostic: daemon refused project listing"),
                shell_quote(listing_output),
                listing_exit_code,
            );
            let path = directory.path().join("docker-fixture");
            fs::write(&path, script).expect("fixture CLI should be written");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("fixture CLI should be executable");
            path
        }
        struct ValidationFixtureOptions<'a> {
            version_exit_code: u8,
            version_output: &'a str,
            exit_code: u8,
            diagnostic: &'a str,
            stdout_chunks: usize,
            stdout_output: &'a str,
            hang: bool,
        }

        impl Default for ValidationFixtureOptions<'_> {
            fn default() -> Self {
                Self {
                    version_exit_code: 0,
                    version_output: "v5.6.0\n",
                    exit_code: 0,
                    diagnostic: "",
                    stdout_chunks: 0,
                    stdout_output: "",
                    hang: false,
                }
            }
        }

        fn validation_fixture_cli(
            directory: &FixtureDirectory,
            expected_project_directory: &std::path::Path,
            expected_file: &std::path::Path,
            options: ValidationFixtureOptions<'_>,
        ) -> PathBuf {
            let ValidationFixtureOptions {
                version_exit_code,
                version_output,
                exit_code,
                diagnostic,
                stdout_chunks,
                stdout_output,
                hang,
            } = options;
            let stdout = if stdout_chunks == 0 {
                format!("printf '%s' {}\n", shell_quote(stdout_output))
            } else {
                format!(
                    "i=0\nwhile [ \"$i\" -lt {stdout_chunks} ]; do\n  printf '%4097s' ''\n  i=$((i + 1))\ndone\n"
                )
            };
            let hang = if hang { "while :; do :; done\n" } else { "" };
            let version_config_marker = directory.path().join("validation-version-config");
            let validation_marker = directory.path().join("validation-command-invoked");
            let script = format!(
                "#!/bin/sh\n\
                 if [ \"$#\" -eq 3 ]; then\n\
                   [ \"$1\" = compose ] && [ \"$2\" = version ] && [ \"$3\" = --short ] || exit 79\n\
                   [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] || exit 86\n\
                   [ \"$HOME\" != \"$DOCKER_CONFIG\" ] && [ ! -e \"$DOCKER_CONFIG/config.json\" ] || exit 87\n\
                   [ -z \"${{COMPOSE_DISABLE_ENV_FILE+x}}\" ] || exit 90\n\
                   [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = \"false\" ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = \"false\" ] || exit 91\n\
                   env | while IFS='=' read -r name value; do\n\
                     case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|GOMEMLIMIT|PWD|SHLVL|_) : ;; *) exit 89 ;; esac\n\
                   done || exit 89\n\
                   [ \"$GOMEMLIMIT\" = 402653184 ] || exit 89\n\
                   printf '%s\\n%s' \"$DOCKER_CONFIG\" \"$HOME\" > {}\n\
                   printf '%s' {}\n\
                   exit {}\n\
                 fi\n\
                 [ \"$#\" -eq 9 ] || exit 80\n\
                 [ \"$1\" = compose ] && [ \"$2\" = --project-directory ] || exit 81\n\
                 [ \"$3\" = {} ] && [ \"$4\" = --file ] && [ \"$5\" = {} ] || exit 82\n\
                 [ \"$6\" = config ] && [ \"$7\" = --no-interpolate ] && [ \"$8\" = --no-env-resolution ] && [ \"$9\" = --services ] || exit 83\n\
                 [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] || exit 85\n\
                 [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = \"false\" ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = \"false\" ] || exit 91\n\
                 [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] || exit 86\n\
                 [ \"$HOME\" != \"$DOCKER_CONFIG\" ] && [ ! -e \"$DOCKER_CONFIG/config.json\" ] || exit 87\n\
                 [ \"$(cat {})\" = \"$(printf '%s\\n%s' \"$DOCKER_CONFIG\" \"$HOME\")\" ] || exit 90\n\
                 if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{DOCKER_CUSTOM_HEADERS+x}}${{DOCKER_DEFAULT_PLATFORM+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{ALL_PROXY+x}}${{FTP_PROXY+x}}${{http_proxy+x}}${{https_proxy+x}}${{no_proxy+x}}${{all_proxy+x}}${{ftp_proxy+x}}${{COMPOSE_FILE+x}}${{COMPOSE_ENV_FILES+x}}${{COMPOSE_PROFILES+x}}${{COMPOSE_PROJECT_NAME+x}}${{COMPOSE_PATH_SEPARATOR+x}}\" ]; then exit 88; fi\n\
                 env | while IFS='=' read -r name value; do\n\
                   case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_DISABLE_ENV_FILE|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|GOMEMLIMIT|PWD|SHLVL|_) : ;; *) exit 89 ;; esac\n\
                 done || exit 89\n\
                 [ \"$GOMEMLIMIT\" = 402653184 ] || exit 89\n\
                 : > {}\n\
                 {}\n\
                 {}\n\
                 printf '%s' {} >&2\n\
                 exit {}\n",
                shell_quote(&version_config_marker.to_string_lossy()),
                shell_quote(version_output),
                version_exit_code,
                shell_quote(&expected_project_directory.to_string_lossy()),
                shell_quote(&expected_file.to_string_lossy()),
                shell_quote(&version_config_marker.to_string_lossy()),
                shell_quote(&validation_marker.to_string_lossy()),
                stdout,
                hang,
                shell_quote(diagnostic),
                exit_code,
            );
            let path = directory.path().join("docker-validation-fixture");
            fs::write(&path, script).expect("validation CLI fixture should be written");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("validation CLI fixture should be executable");
            path
        }
        fn ordered_validation_fixture_cli(
            directory: &FixtureDirectory,
            project_directory: &std::path::Path,
            base_file: &std::path::Path,
            first_override: &std::path::Path,
            second_override: &std::path::Path,
        ) -> PathBuf {
            let script = format!(
                "#!/bin/sh\n\
                 if [ \"$#\" -eq 3 ]; then [ \"$1\" = compose ] && [ \"$2\" = version ] && [ \"$3\" = --short ] || exit 70; printf '%s' started > {}; printf 'v5.6.0\\n'; exit 0; fi\n\
                 [ \"$#\" -eq 17 ] || exit 71\n\
                 [ \"$1\" = compose ] && [ \"$2\" = --project-directory ] && [ \"$3\" = {} ] || exit 72\n\
                 [ \"$4\" = --file ] && [ \"$5\" = {} ] && [ \"$6\" = --file ] && [ \"$7\" = {} ] && [ \"$8\" = --file ] && [ \"$9\" = {} ] || exit 73\n\
                 [ \"${{10}}\" = --profile ] && [ \"${{11}}\" = frontend ] && [ \"${{12}}\" = --profile ] && [ \"${{13}}\" = qa ] || exit 74\n\
                 [ \"${{14}}\" = config ] && [ \"${{15}}\" = --no-interpolate ] && [ \"${{16}}\" = --no-env-resolution ] && [ \"${{17}}\" = --services ] || exit 75\n\
                 [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] || exit 75\n\
                 [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = false ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = false ] || exit 76\n\
                 [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] && [ \"$HOME\" != \"$DOCKER_CONFIG\" ] || exit 77\n\
                 if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{COMPOSE_FILE+x}}${{COMPOSE_ENV_FILES+x}}${{COMPOSE_PROFILES+x}}\" ]; then exit 78; fi\n\
                 env | while IFS='=' read -r name value; do case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_DISABLE_ENV_FILE|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|GOMEMLIMIT|PWD|SHLVL|_) : ;; *) exit 79 ;; esac; done || exit 79\n\
                 [ \"$GOMEMLIMIT\" = 402653184 ] || exit 79\n\
                 printf 'app\\n'\n\
                 exit 0\n",
                shell_quote(
                    &directory
                        .path()
                        .join("ordered-validation-version-marker")
                        .to_string_lossy()
                ),
                shell_quote(&project_directory.to_string_lossy()),
                shell_quote(&base_file.to_string_lossy()),
                shell_quote(&first_override.to_string_lossy()),
                shell_quote(&second_override.to_string_lossy()),
            );
            let path = directory.path().join("docker-ordered-validation-fixture");
            fs::write(&path, script).expect("ordered validation fixture should be written");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("ordered validation fixture should be executable");
            path
        }

        fn search_path() -> std::ffi::OsString {
            std::env::var_os("PATH").unwrap_or_else(|| OsStr::new("/usr/bin:/bin").to_owned())
        }
        #[tokio::test]
        async fn subprocess_limits_bound_go_memory_and_shell_resource_limits() {
            let isolated = PrivateDockerConfig::new(None)
                .expect("a private fixture directory should be created");
            let mut command = tokio::process::Command::new("/bin/sh");
            isolated.configure(&mut command, &search_path());
            command.args(["-c", "printf '%s\\n' \"$GOMEMLIMIT\"; ulimit -v; ulimit -d"]);

            let output = run_command_with_timeout(&mut command, COMMAND_TIMEOUT)
                .await
                .expect("the shell should report its configured process limits");
            let output = String::from_utf8(output).expect("shell output should be UTF-8");
            let mut values = output.lines();
            assert_eq!(values.next(), Some("402653184"));
            let address_space_kib = values
                .next()
                .expect("the address-space limit should be reported")
                .parse::<u64>()
                .expect("the address-space limit should be numeric");
            let data_kib = values
                .next()
                .expect("the data limit should be reported")
                .parse::<u64>()
                .expect("the data limit should be numeric");
            assert!(address_space_kib <= 4 * 1024 * 1024);
            assert!(data_kib <= 512 * 1024);
        }

        fn create_sparse_file(path: &std::path::Path, length: u64) {
            fs::File::create(path)
                .expect("sparse Compose fixture should be created")
                .set_len(length)
                .expect("sparse Compose fixture should have the requested length");
        }

        #[tokio::test]
        async fn validation_accepts_selected_base_and_overrides_at_16_mib_aggregate_limit() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("sparse project");
            fs::create_dir(&project_directory).expect("the project directory should exist");
            let base = project_directory.join("base.yaml");
            let override_one = directory.path().join("override-one.yaml");
            let override_two = directory.path().join("override-two.yaml");
            create_sparse_file(&base, 8 * 1024 * 1024);
            create_sparse_file(&override_one, 8 * 1024 * 1024);
            create_sparse_file(&override_two, 0);
            let marker = directory.path().join("ordered-validation-version-marker");
            let binary = ordered_validation_fixture_cli(
                &directory,
                &project_directory,
                &base,
                &override_one,
                &override_two,
            );

            let files = [base, override_one, override_two];
            let profiles = ["frontend".to_owned(), "qa".to_owned()];
            let result = validate_with_profiles_report(
                &binary,
                &files,
                &profiles,
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await
            .outcome;

            assert!(result.is_ok());
            assert!(marker.exists(), "the CLI preflight should have run");
        }

        #[tokio::test]
        async fn validation_rejects_selected_base_and_overrides_over_16_mib_before_cli() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("sparse project");
            fs::create_dir(&project_directory).expect("the project directory should exist");
            let base = project_directory.join("base.yaml");
            let override_one = directory.path().join("override-one.yaml");
            let override_two = directory.path().join("override-two.yaml");
            create_sparse_file(&base, 8 * 1024 * 1024);
            create_sparse_file(&override_one, 8 * 1024 * 1024);
            create_sparse_file(&override_two, 1);
            let marker = directory.path().join("ordered-validation-version-marker");
            let binary = ordered_validation_fixture_cli(
                &directory,
                &project_directory,
                &base,
                &override_one,
                &override_two,
            );

            let result = validate_with_files_report(
                &binary,
                &[base, override_one, override_two],
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;

            assert!(
                !marker.exists(),
                "oversized input must fail before CLI preflight"
            );
            assert!(result.version.is_none());
            assert_eq!(
                result.outcome.unwrap_err().message(),
                "Selected Compose files exceed the 16 MiB validation input limit."
            );
        }

        #[tokio::test]
        async fn validation_uses_the_selected_local_file_parent_and_isolated_environment() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("project with spaces");
            fs::create_dir(&project_directory).expect("the local project directory should exist");
            let file = project_directory.join("compose 'selected'.yaml");
            fs::write(&file, "services: {}\n").expect("the selected fixture should exist");
            let binary = validation_fixture_cli(
                &directory,
                &project_directory,
                &file,
                ValidationFixtureOptions {
                    stdout_output: "web\napi\u{001b}",
                    ..ValidationFixtureOptions::default()
                },
            );
            let result =
                validate_with_binary_report(&binary, &file, &search_path(), None, COMMAND_TIMEOUT)
                    .await;

            assert_eq!(result.version.as_deref(), Some("v5.6.0"));
            assert_eq!(result.services, ["web", "api\\x1b"]);
            assert!(result.outcome.is_ok());
        }
        #[tokio::test]
        async fn validation_passes_base_and_ordered_overrides_as_separate_arguments() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("base project with spaces");
            fs::create_dir(&project_directory).expect("the base directory should exist");
            let base = project_directory.join("base compose.yaml");
            let override_one = directory.path().join("override one.yaml");
            let override_two = directory.path().join("override two.yaml");
            fs::write(&base, "services: {}\n").expect("the base file should exist");
            fs::write(&override_one, "services: {}\n").expect("the first override should exist");
            fs::write(&override_two, "services: {}\n").expect("the second override should exist");
            let binary = ordered_validation_fixture_cli(
                &directory,
                &project_directory,
                &base,
                &override_one,
                &override_two,
            );

            let files = [base, override_one, override_two];
            let profiles = ["frontend".to_owned(), "qa".to_owned()];
            let result = validate_with_profiles_report(
                &binary,
                &files,
                &profiles,
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;
            assert_eq!(result.outcome, Ok(()));
            assert_eq!(result.services, ["app"]);
        }

        #[tokio::test]
        async fn invalid_profile_list_is_rejected_before_cli_launch() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("project");
            fs::create_dir(&project).expect("the project directory should exist");
            let file = project.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the Compose file should exist");
            let binary = directory.path().join("does-not-exist");
            let profiles = ["--help".to_owned()];

            let result = validate_with_profiles_report(
                &binary,
                &[file],
                &profiles,
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;

            assert_eq!(result.outcome, Err(ComposeValidationError::InvalidProfiles));
        }

        #[tokio::test]
        async fn validation_rejects_invalid_duplicate_and_over_limit_file_sets_before_cli() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("project");
            fs::create_dir(&project).expect("the project directory should exist");
            let base = project.join("base.yaml");
            fs::write(&base, "services: {}\n").expect("the base file should exist");
            let binary = directory.path().join("does-not-exist");
            let duplicate = validate_with_files(
                &binary,
                &[base.clone(), base.clone()],
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;
            assert_eq!(duplicate, Err(ComposeValidationError::InvalidSelectedFile));
            let non_absolute = validate_with_files(
                &binary,
                &[PathBuf::from("relative.yaml")],
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;
            assert_eq!(
                non_absolute,
                Err(ComposeValidationError::InvalidSelectedFile)
            );
            let directory_input = validate_with_files(
                &binary,
                std::slice::from_ref(&project),
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;
            assert_eq!(
                directory_input,
                Err(ComposeValidationError::InvalidSelectedFile)
            );
            let missing = validate_with_files(
                &binary,
                &[project.join("missing.yaml")],
                &search_path(),
                None,
                COMMAND_TIMEOUT,
            )
            .await;
            assert_eq!(missing, Err(ComposeValidationError::InvalidSelectedFile));
            let empty =
                validate_with_files(&binary, &[], &search_path(), None, COMMAND_TIMEOUT).await;
            assert_eq!(empty, Err(ComposeValidationError::InvalidSelectedFile));
            let too_many = vec![base; MAX_COMPOSE_FILES + 1];
            let over_limit =
                validate_with_files(&binary, &too_many, &search_path(), None, COMMAND_TIMEOUT)
                    .await;
            assert_eq!(over_limit, Err(ComposeValidationError::InvalidSelectedFile));
        }

        #[tokio::test]
        async fn validation_reports_missing_compose_plugin_before_running_config() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("private project");
            fs::create_dir(&project_directory).expect("the local project directory should exist");
            let file = project_directory.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the selected fixture should exist");
            let binary = validation_fixture_cli(
                &directory,
                &project_directory,
                &file,
                ValidationFixtureOptions {
                    version_exit_code: 1,
                    version_output: "Compose plugin missing",
                    ..ValidationFixtureOptions::default()
                },
            );

            let error = validate_with_binary(&binary, &file, &search_path(), None, COMMAND_TIMEOUT)
                .await
                .expect_err("a missing Compose plugin should fail the version preflight");

            assert_eq!(error, ComposeValidationError::ComposePluginUnavailable);
            assert_eq!(
                error.message(),
                "Docker Compose is unavailable. Install the Docker Compose plugin to validate Compose files."
            );
            assert!(!error.message().contains("private project"));
            assert!(!error.message().contains("Compose plugin missing"));
            assert!(!directory.path().join("validation-command-invoked").exists());
        }

        #[tokio::test]
        async fn validation_failure_discards_cli_diagnostics_and_selected_paths() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("private project");
            fs::create_dir(&project_directory).expect("the local project directory should exist");
            let file = project_directory.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the selected fixture should exist");
            let diagnostic = format!("{}: private CLI diagnostic marker", file.display());
            let binary = validation_fixture_cli(
                &directory,
                &project_directory,
                &file,
                ValidationFixtureOptions {
                    exit_code: 1,
                    diagnostic: &diagnostic,
                    ..ValidationFixtureOptions::default()
                },
            );

            let error = validate_with_binary(&binary, &file, &search_path(), None, COMMAND_TIMEOUT)
                .await
                .expect_err("the controlled CLI failure should be reported");

            assert_eq!(error, ComposeValidationError::ValidationFailed);
            assert_eq!(error.message(), "Compose file validation failed.");
            let selected_path = file.display().to_string();
            assert!(!error.message().contains(selected_path.as_str()));
            assert!(!error.message().contains("private CLI diagnostic marker"));
        }

        #[tokio::test]
        async fn validation_rejects_malformed_service_rows_without_exposing_output() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("project");
            fs::create_dir(&project).expect("the project directory should exist");
            let file = project.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the Compose file should exist");
            let binary = validation_fixture_cli(
                &directory,
                &project,
                &file,
                ValidationFixtureOptions {
                    stdout_output: "\nprivate-diagnostic",
                    ..ValidationFixtureOptions::default()
                },
            );

            let result =
                validate_with_binary_report(&binary, &file, &search_path(), None, COMMAND_TIMEOUT)
                    .await;

            assert_eq!(
                result.outcome,
                Err(ComposeValidationError::ValidationFailed)
            );
            assert!(result.services.is_empty());
            assert!(!format!("{result:?}").contains("private-diagnostic"));
        }

        #[tokio::test]
        async fn validation_rejects_output_over_the_shared_subprocess_limit() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("project");
            fs::create_dir(&project_directory).expect("the local project directory should exist");
            let file = project_directory.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the selected fixture should exist");
            let binary = validation_fixture_cli(
                &directory,
                &project_directory,
                &file,
                ValidationFixtureOptions {
                    stdout_chunks: 257,
                    ..ValidationFixtureOptions::default()
                },
            );

            let error = validate_with_binary(&binary, &file, &search_path(), None, COMMAND_TIMEOUT)
                .await
                .expect_err("oversized validation output should be rejected");

            assert_eq!(error, ComposeValidationError::ValidationFailed);
        }

        #[tokio::test]
        async fn validation_deadline_stops_a_controlled_cli_fixture() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("project");
            fs::create_dir(&project_directory).expect("the local project directory should exist");
            let file = project_directory.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the selected fixture should exist");
            let binary = validation_fixture_cli(
                &directory,
                &project_directory,
                &file,
                ValidationFixtureOptions {
                    hang: true,
                    ..ValidationFixtureOptions::default()
                },
            );

            let error = validate_with_binary(
                &binary,
                &file,
                &search_path(),
                None,
                Duration::from_millis(500),
            )
            .await
            .expect_err("the controlled CLI should exceed the short deadline");

            assert_eq!(error, ComposeValidationError::ValidationFailed);
        }

        #[tokio::test]
        #[ignore = "requires Docker CLI and Compose; validates owned temporary base and override files and contacts no engine"]
        async fn local_compose_validation_reads_ordered_temporary_base_and_override_files() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("project with spaces");
            fs::create_dir(&project_directory)
                .expect("the temporary project directory should be created");
            fs::create_dir(project_directory.join("data"))
                .expect("the relative data directory should be created");
            let base = project_directory.join("compose.yaml");
            let override_file = project_directory.join("compose.override.yaml");
            fs::write(
                &base,
                "services:\n  always:\n    image: busybox:latest\n  app:\n    image: busybox:latest\n    profiles: [smoke]\n    volumes:\n      - ./data:/data\n  hidden:\n    image: busybox:latest\n    profiles: [other]\n",
            )
            .expect("the controlled base Compose file should be written");
            fs::write(
                &override_file,
                "services:\n  app:\n    environment:\n      CONTROLLED: value\n",
            )
            .expect("the controlled override Compose file should be written");

            let files = [base, override_file];
            let profiles = ["smoke".to_owned()];
            let result = validate_compose_files(&files, &profiles).await;
            assert_eq!(result.version.as_deref(), Some("5.6.0"));
            let mut declared_services = result.services.clone();
            declared_services.sort();
            assert_eq!(declared_services, ["always", "app", "hidden"]);
            result.outcome.expect(
                "the local Compose CLI should validate and list all declared services without contacting an engine",
            );
        }

        #[tokio::test]
        #[ignore = "requires Docker CLI and Compose; remote references target only a local loopback listener"]
        async fn local_compose_validation_rejects_remote_includes_without_network_access() {
            use std::{
                io::Write,
                net::TcpListener,
                sync::{
                    Arc,
                    atomic::{AtomicBool, AtomicUsize, Ordering},
                },
                thread,
            };

            let search_path = search_path();
            let binary = docker_binary(&search_path).expect("Docker CLI should be installed");
            let user_compose_plugin = user_compose_plugin();
            let directory = FixtureDirectory::new();
            let listener = TcpListener::bind("127.0.0.1:0")
                .expect("the controlled loopback listener should bind");
            listener
                .set_nonblocking(true)
                .expect("the loopback listener should be nonblocking");
            let address = listener
                .local_addr()
                .expect("the local listener address should be available");
            let stop_server = Arc::new(AtomicBool::new(false));
            let connection_count = Arc::new(AtomicUsize::new(0));
            let server_stop = Arc::clone(&stop_server);
            let server_connection_count = Arc::clone(&connection_count);
            let server = thread::spawn(move || {
                while !server_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            server_connection_count.fetch_add(1, Ordering::Relaxed);
                            let _ = stream.write_all(
                                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            );
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });

            let mut results = Vec::new();
            for (name, remote_reference) in [
                (
                    "git",
                    format!("https://{address}/remote.git#main:compose.yaml"),
                ),
                ("oci", format!("oci://{address}/remote:latest")),
            ] {
                let project_directory = directory.path().join(name);
                fs::create_dir(&project_directory)
                    .expect("the controlled project directory should be created");
                let file = project_directory.join("compose.yaml");
                fs::write(
                    &file,
                    format!("include:\n  - {remote_reference}\nservices: {{}}\n"),
                )
                .expect("the controlled remote-reference Compose file should be written");
                results.push(
                    validate_with_binary(
                        &binary,
                        &file,
                        &search_path,
                        user_compose_plugin.as_deref(),
                        Duration::from_secs(4),
                    )
                    .await,
                );
            }
            thread::sleep(std::time::Duration::from_millis(25));
            stop_server.store(true, Ordering::Relaxed);
            server
                .join()
                .expect("the controlled loopback listener should stop");

            for result in results {
                assert_eq!(result, Err(ComposeValidationError::ValidationFailed));
            }
            assert_eq!(
                connection_count.load(Ordering::Relaxed),
                0,
                "Compose validation must not fetch Git or OCI include references"
            );
        }

        #[test]
        fn isolated_docker_config_and_home_are_owner_only() {
            let private = PrivateDockerConfig::new(None)
                .expect("private Docker config directories should be created");
            for path in [&private.root, &private.home, &private.docker_config] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }

        #[test]
        fn private_config_exposes_only_the_compose_plugin_without_user_config() {
            let directory = FixtureDirectory::new();
            let user_config = directory.path().join("user-config");
            let user_plugins = user_config.join("cli-plugins");
            fs::create_dir_all(&user_plugins).expect("fixture plugin directory should be created");
            let user_compose_plugin = user_plugins.join("docker-compose");
            fs::write(&user_compose_plugin, "controlled plugin fixture")
                .expect("fixture plugin should be written");
            let saved_config = user_config.join("config.json");
            fs::write(&saved_config, "credential fixture")
                .expect("fixture config should be written");

            let private = PrivateDockerConfig::new(Some(&user_compose_plugin))
                .expect("isolated config should be created");
            let private_plugin = private.docker_config.join("cli-plugins/docker-compose");
            assert!(
                fs::symlink_metadata(&private_plugin)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(fs::read_link(&private_plugin).unwrap(), user_compose_plugin);
            assert!(!private.docker_config.join("config.json").exists());

            drop(private);
            assert!(saved_config.exists());
            assert!(user_compose_plugin.exists());
        }

        #[tokio::test]
        async fn listing_uses_the_selected_socket_and_retains_only_escaped_name_and_status() {
            let directory = FixtureDirectory::new();
            let socket = directory.path().join("selected engine.sock");
            let listing = r#"[
                {"Name":"alpha\u202e","Status":"running(1)\u001b[31m","ConfigFiles":"/secret/compose.yml"},
                {"Name":"stopped","Status":"exited(0)","ConfigFiles":"/another/secret.yml"}
            ]"#;
            let binary = fixture_cli(&directory, &socket, "v5.6.0\n", 0, listing, 0);

            let result = discover_with_binary(&binary, &socket, &search_path(), None)
                .await
                .expect("the fixture CLI should return a valid discovery result");

            assert_eq!(result.version, "v5.6.0");
            assert_eq!(
                result.projects,
                vec![
                    ComposeProject {
                        name: "alpha\\u{202e}".to_owned(),
                        status: "running(1)\\x1b[31m".to_owned(),
                    },
                    ComposeProject {
                        name: "stopped".to_owned(),
                        status: "exited(0)".to_owned(),
                    },
                ]
            );
        }

        #[tokio::test]
        async fn unavailable_compose_and_engine_failures_are_static_and_distinct() {
            let directory = FixtureDirectory::new();
            let socket = directory.path().join("selected.sock");
            let compose_missing =
                fixture_cli(&directory, &socket, "Compose plugin missing", 1, "[]", 0);
            let error = discover_with_binary(&compose_missing, &socket, &search_path(), None)
                .await
                .expect_err("a failing version command means Compose is unavailable");
            assert_eq!(error, ComposeError::ComposePluginUnavailable);
            assert!(!error.message().contains("fixture diagnostic"));
            assert!(!error.message().contains("selected.sock"));

            let engine_unavailable = fixture_cli(&directory, &socket, "v5.6.0\n", 0, "[]", 1);
            let error = discover_with_binary(&engine_unavailable, &socket, &search_path(), None)
                .await
                .expect_err("a failed project list means the selected engine is unavailable");
            assert_eq!(
                error,
                ComposeError::ProjectListFailed {
                    version: "v5.6.0".to_owned(),
                }
            );
            assert_eq!(
                error.message(),
                "Compose projects could not be read from the selected local Docker Engine."
            );
            assert!(!error.message().contains("fixture diagnostic"));
            assert!(!error.message().contains("selected.sock"));
        }

        #[tokio::test]
        async fn a_missing_docker_executable_has_a_distinct_safe_error() {
            let directory = FixtureDirectory::new();
            let missing = directory.path().join("not-installed");
            let socket = directory.path().join("selected.sock");
            let error = discover_with_binary(&missing, &socket, &search_path(), None)
                .await
                .expect_err("the absent Docker executable should be reported");
            assert_eq!(error, ComposeError::DockerCliMissing);
            assert_eq!(
                error.message(),
                "Docker CLI is unavailable. Install Docker CLI to discover Compose projects."
            );
        }

        #[tokio::test]
        async fn oversized_project_output_fails_with_a_static_engine_error() {
            let directory = FixtureDirectory::new();
            let socket = directory.path().join("selected.sock");
            let output = "x".repeat(MAX_COMMAND_OUTPUT_BYTES + 1);
            let binary = fixture_cli(&directory, &socket, "v5.6.0\n", 0, &output, 0);
            let error = discover_with_binary(&binary, &socket, &search_path(), None)
                .await
                .expect_err("oversized project output should be rejected");

            assert_eq!(
                error,
                ComposeError::ProjectListFailed {
                    version: "v5.6.0".to_owned(),
                }
            );
            assert_eq!(
                error.message(),
                "Compose projects could not be read from the selected local Docker Engine."
            );
        }

        #[tokio::test]
        async fn subprocess_deadline_stops_a_controlled_fixture() {
            let directory = FixtureDirectory::new();
            let path = directory.path().join("hang");
            fs::write(&path, "#!/bin/sh\nwhile :; do :; done\n")
                .expect("the controlled hanging fixture should be written");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("the controlled fixture should be executable");

            let mut command = tokio::process::Command::new(path);
            command.env_clear();
            let failure =
                run_command_with_timeout(&mut command, std::time::Duration::from_millis(50))
                    .await
                    .expect_err("the fixture should exceed the short test deadline");

            assert!(
                matches!(&failure, CommandFailure::TimedOut),
                "expected a timeout, got {failure:?}"
            );
        }

        #[test]
        fn json_model_rejects_duplicate_identity_and_ambiguous_fields() {
            assert!(parse_projects(
                br#"[{"Name":"same","Status":"running(1)"},{"Name":"same","Status":"exited(0)"}]"#
            )
            .is_none());
            assert!(
                parse_projects(br#"[{"Name":"same","Name":"other","Status":"running(1)"}]"#)
                    .is_none()
            );
            assert!(
                parse_projects(br#"[{"Name":"same","Status":"running(1)","Status":"exited(0)"}]"#)
                    .is_none()
            );
            assert!(parse_projects(
                br#"[{"Name":"alpha\u202e","Status":"running"},{"Name":"alpha\\u{202e}","Status":"exited"}]"#
            )
            .is_none());
        }
        #[test]
        fn compose_container_json_lines_allowlist_and_limits() {
            let valid = br#"{"Name":"api-1","Service":"api","State":"running","Health":"healthy","ExitCode":0,"Command":"secret","ID":"sensitive","Publishers":[{"PublishedPort":1234}],"Project":"private"}"#;
            let rows = parse_containers(valid).expect("a valid Compose JSON line should parse");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].name, "api-1");
            assert_eq!(rows[0].service, "api");
            assert_eq!(rows[0].state, "running");
            assert_eq!(rows[0].project.as_deref(), Some("private"));
            assert_eq!(rows[0].health.as_deref(), Some("healthy"));
            assert!(!format!("{rows:?}").contains("secret"));
            assert!(!format!("{rows:?}").contains("sensitive"));
            let omitted =
                parse_containers(br#"{"Name":"worker-1","Service":"worker","State":"exited"}"#)
                    .expect("missing Project remains compatible");
            assert_eq!(omitted[0].project, None);
            let mixed = parse_containers(
                b"{\"Name\":\"api-1\",\"Service\":\"api\",\"State\":\"running\",\"Project\":\"alpha\"}\n\
                  {\"Name\":\"worker-1\",\"Service\":\"worker\",\"State\":\"exited\"}",
            )
            .expect("each row may independently report Project");
            assert_eq!(mixed[0].project.as_deref(), Some("alpha"));
            assert_eq!(mixed[1].project, None);
            let no_health_reported = parse_containers(
                br#"{"Name":"api-1","Service":"api","State":"running","Health":""}"#,
            )
            .expect("empty health is a valid unreported value");
            assert_eq!(no_health_reported[0].health, None);
            assert!(parse_containers(b"").unwrap().is_empty());
            assert!(parse_containers(b"\n").unwrap().is_empty());
            assert_eq!(
                parse_containers(b"{\"Name\":\"api\",\"Service\":\"api\",\"State\":\"running\"}\n")
                    .unwrap()
                    .len(),
                1
            );
            assert!(parse_containers(br#"{"Name":"api","Service":"api","State":"running","Health":"bad","Health":"good"}"#).is_none());
            assert!(parse_containers(br#"{"Name":"api","Service":"api","State":"running","Command":"x","Command":"y"}"#).is_none());
            assert!(parse_containers(br#"{"Name":"api","Service":"api","State":"running","Project":"one","Project":"two"}"#).is_none());
            assert!(
                parse_containers(
                    br#"{"Name":"api","Service":"api","State":"running","Project":null}"#
                )
                .is_none()
            );
            assert!(
                parse_containers(
                    br#"{"Name":"api","Service":"api","State":"running","Project":7}"#
                )
                .is_none()
            );
            assert!(
                parse_containers(
                    br#"{"Name":"api","Service":"api","State":"running","Project":""}"#
                )
                .is_none()
            );
            assert!(parse_containers(
                b"{\"Name\":\"first\",\"Service\":\"api\",\"State\":\"running\",\"Project\":\"ok\"}\n\
                  {\"Name\":\"second\",\"Service\":\"api\",\"State\":\"running\",\"Project\":null}"
            )
            .is_none());
            assert!(parse_containers(br#"{"Name":"api","State":"running"}"#).is_none());
            assert!(
                parse_containers(
                    format!(
                        r#"{{"Name":"{}","Service":"api","State":"running"}}"#,
                        "n".repeat(MAX_CONTAINER_FIELD_BYTES + 1)
                    )
                    .as_bytes()
                )
                .is_none()
            );
            assert!(
                parse_containers(
                    format!(
                        r#"{{"Name":"api","Service":"api","State":"running","Project":"{}"}}"#,
                        "p".repeat(MAX_CONTAINER_FIELD_BYTES + 1)
                    )
                    .as_bytes()
                )
                .is_none()
            );
            let escaped = parse_containers(
                br#"{"Name":"api","Service":"api","State":"running","Project":"team\u202e"}"#,
            )
            .expect("display controls should be escaped");
            assert!(escaped[0].project.as_deref().unwrap().contains(r"\u{202e}"));
            let escaped_name =
                parse_containers(br#"{"Name":"api\u202e","Service":"api","State":"running"}"#)
                    .expect("display controls should be escaped");
            assert!(escaped_name[0].name.contains(r"\u{202e}"));
        }
        #[test]
        fn compose_container_status_rejects_duplicate_rows_and_excessive_counts() {
            assert!(
                parse_containers(
                    b"{\"Name\":\"api-1\",\"Service\":\"api\",\"State\":\"running\"}\n\
                      {\"Name\":\"api-1\",\"Service\":\"api\",\"State\":\"exited\"}"
                )
                .is_none(),
                "duplicate container identity must not create ambiguous rows"
            );

            let rows = |count: usize| {
                (0..count)
                    .map(|index| {
                        format!(
                            r#"{{"Name":"container-{index}","Service":"api","State":"running"}}"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let at_limit = rows(MAX_COMPOSE_CONTAINERS);
            assert_eq!(
                parse_containers(at_limit.as_bytes())
                    .expect("the exact row limit should be accepted")
                    .len(),
                MAX_COMPOSE_CONTAINERS
            );
            let over_limit = rows(MAX_COMPOSE_CONTAINERS + 1);
            assert!(
                parse_containers(over_limit.as_bytes()).is_none(),
                "one row over the limit must be rejected"
            );
        }
        #[tokio::test]
        async fn container_status_uses_pinned_socket_ordered_files_and_private_environment() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("project with spaces");
            fs::create_dir(&project).unwrap();
            let base = project.join("base compose.yaml");
            let override_file = project.join("override compose.yaml");
            fs::write(&base, "services: {}\n").unwrap();
            fs::write(&override_file, "services: {}\n").unwrap();
            let socket = directory.path().join("selected socket.sock");
            let trace = directory.path().join("argv");
            let binary = directory.path().join("docker");
            let output = "{\"Name\":\"web-1\",\"Service\":\"web\",\"State\":\"exited\",\"Health\":null,\"Command\":\"discard\",\"ID\":\"discard\"}\n";
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n\
                 if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{DOCKER_CUSTOM_HEADERS+x}}${{DOCKER_DEFAULT_PLATFORM+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{ALL_PROXY+x}}${{FTP_PROXY+x}}${{COMPOSE_FILE+x}}${{COMPOSE_ENV_FILES+x}}${{COMPOSE_PROFILES+x}}\" ]; then exit 91; fi\n\
                 env | while IFS='=' read -r name value; do case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|GOMEMLIMIT|COMPOSE_DISABLE_ENV_FILE|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|PWD|SHLVL|_) : ;; *) exit 92 ;; esac; done || exit 92\n\
                 [ \"$GOMEMLIMIT\" = 402653184 ] && [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] || exit 93\n\
                 printf '%s' {}\n",
                shell_quote(trace.to_str().unwrap()),
                shell_quote(output),
            );
            fs::write(&binary, script).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            let files = [base.clone(), override_file.clone()];
            let profiles = ["frontend".to_owned(), "qa".to_owned()];
            let rows = containers_with_binary(
                &binary,
                &socket,
                &files,
                &profiles,
                OsStr::new("/usr/bin"),
                None,
            )
            .await
            .expect("fake CLI status output should parse");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].state, "exited");
            assert!(!format!("{rows:?}").contains("discard"));
            let args = fs::read_to_string(trace).unwrap();
            let host = format!("unix://{}", socket.display());
            let expected = [
                "--host".to_owned(),
                host,
                "compose".to_owned(),
                "--project-directory".to_owned(),
                project.display().to_string(),
                "--file".to_owned(),
                base.display().to_string(),
                "--file".to_owned(),
                override_file.display().to_string(),
                "--profile".to_owned(),
                "frontend".to_owned(),
                "--profile".to_owned(),
                "qa".to_owned(),
                "ps".to_owned(),
                "--all".to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ]
            .join("\n")
                + "\n";
            assert_eq!(args, expected);
        }
        #[tokio::test]
        async fn logs_are_not_requested_when_service_is_absent_from_fresh_status() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("temporary project");
            fs::create_dir(&project).unwrap();
            let file = project.join("compose.yaml");
            fs::write(&file, "services: {}\n").unwrap();
            let marker = directory.path().join("logs-invoked");
            let binary = directory.path().join("docker-fixture");
            let script = format!(
                "#!/bin/sh\n\
                 for argument do\n\
                   if [ \"$argument\" = logs ]; then : > {}; printf 'unexpected-log-output\\n'; exit 0; fi\n\
                 done\n\
                 exit 0\n",
                shell_quote(marker.to_str().unwrap()),
            );
            fs::write(&binary, script).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();

            let result = service_logs_with_binary(
                &binary,
                &directory.path().join("socket.sock"),
                &[file],
                &[],
                "web",
                OsStr::new("/usr/bin"),
                None,
            )
            .await;

            assert_eq!(result, Err(ComposeLogsError::ServiceUnavailable));
            assert!(
                !marker.exists(),
                "the logs command must not run if fresh status lacks the service"
            );
        }
        #[tokio::test]
        async fn service_logs_use_exact_safe_arguments_private_environment_and_sanitized_output() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("project with spaces");
            fs::create_dir(&project).unwrap();
            let base = project.join("compose.yaml");
            let overlay = project.join("override.yaml");
            fs::write(&base, "services: {}\n").unwrap();
            fs::write(&overlay, "services: {}\n").unwrap();
            let socket = directory.path().join("selected socket.sock");
            let trace_status = directory.path().join("status-argv");
            let trace_logs = directory.path().join("logs-argv");
            let binary = directory.path().join("docker-logs");
            let status_output = "{\"Name\":\"web-1\",\"Service\":\"web\",\"State\":\"running\"}\n";
            let script = format!(
                "#!/bin/sh\n\
                 [ -n \"$PATH\" ] && [ -n \"$HOME\" ] && [ -n \"$DOCKER_CONFIG\" ] || exit 91\n\
                 [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] && [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = false ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = false ] || exit 92\n\
                 [ -z \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{HTTP_PROXY+x}}${{COMPOSE_FILE+x}}${{COMPOSE_PROFILES+x}}\" ] || exit 93\n\
                 for argument do\n\
                   if [ \"$argument\" = ps ]; then printf '%s\\n' \"$@\" > {}; printf '%s' {}; exit 0; fi\n\
                   if [ \"$argument\" = logs ]; then printf '%s\\n' \"$@\" > {}; printf 'first\\033[2J\\nsecond\\n'; exit 0; fi\n\
                 done\n\
                 exit 94\n",
                shell_quote(trace_status.to_str().unwrap()),
                shell_quote(status_output),
                shell_quote(trace_logs.to_str().unwrap()),
            );
            fs::write(&binary, script).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            let files = [base.clone(), overlay.clone()];
            let profiles = ["frontend".to_owned(), "qa".to_owned()];
            let output = service_logs_with_binary(
                &binary,
                &socket,
                &files,
                &profiles,
                "web",
                OsStr::new("/usr/bin"),
                None,
            )
            .await
            .expect("fake Compose logs should succeed");
            assert!(output.contains(r"first\x1b[2J"));
            assert!(output.contains("second"));
            let status_args = fs::read_to_string(trace_status).unwrap();
            let logs_args = fs::read_to_string(trace_logs).unwrap();
            let common = [
                "--host".to_owned(),
                format!("unix://{}", socket.display()),
                "compose".to_owned(),
                "--project-directory".to_owned(),
                project.display().to_string(),
                "--file".to_owned(),
                base.display().to_string(),
                "--file".to_owned(),
                overlay.display().to_string(),
                "--profile".to_owned(),
                "frontend".to_owned(),
                "--profile".to_owned(),
                "qa".to_owned(),
            ];
            let expected_status = [
                common.clone().to_vec(),
                vec![
                    "ps".to_owned(),
                    "--all".to_owned(),
                    "--format".to_owned(),
                    "json".to_owned(),
                ],
            ]
            .concat()
            .join("\n")
                + "\n";
            let expected_logs = [
                common.to_vec(),
                vec![
                    "logs".to_owned(),
                    "--tail".to_owned(),
                    "200".to_owned(),
                    "--no-color".to_owned(),
                    "web".to_owned(),
                ],
            ]
            .concat()
            .join("\n")
                + "\n";
            assert_eq!(status_args, expected_status);
            assert_eq!(logs_args, expected_logs);
        }

        #[tokio::test]
        async fn unsafe_service_is_rejected_before_launch() {
            let error = service_logs_with_binary(
                std::path::Path::new("/definitely/not/a/docker-cli"),
                std::path::Path::new("/tmp/socket"),
                &[],
                &[],
                "--help",
                OsStr::new("/usr/bin"),
                None,
            )
            .await
            .expect_err("unsafe service must be rejected before CLI launch");
            assert_eq!(error, ComposeLogsError::QueryFailed);
        }
        #[tokio::test]
        async fn service_logs_report_cli_failure_without_exposing_diagnostics() {
            let directory = FixtureDirectory::new();
            let project = directory.path().join("compose.yaml");
            fs::write(&project, "services: {}\n").unwrap();
            let binary = directory.path().join("docker-failure");
            let status_output = "{\"Name\":\"web-1\",\"Service\":\"web\",\"State\":\"running\"}\n";
            let script = format!(
                "#!/bin/sh\n\
                 for argument do\n\
                   if [ \"$argument\" = ps ]; then printf '%s' {}; exit 0; fi\n\
                   if [ \"$argument\" = logs ]; then echo secret-diagnostic >&2; exit 7; fi\n\
                 done\n\
                 exit 94\n",
                shell_quote(status_output),
            );
            fs::write(&binary, script).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            let result = service_logs_with_binary(
                &binary,
                &directory.path().join("socket.sock"),
                &[project],
                &[],
                "web",
                OsStr::new("/usr/bin"),
                None,
            )
            .await;
            assert_eq!(result, Err(ComposeLogsError::QueryFailed));
            assert!(!format!("{result:?}").contains("secret-diagnostic"));
        }
        #[tokio::test]
        async fn lifecycle_uses_only_explicit_ordered_files_profiles_and_start_stop_commands() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("project with spaces");
            fs::create_dir(&project_directory).expect("the project directory should exist");
            let file = project_directory.join("compose.yaml");
            let override_file = project_directory.join("override.yaml");
            fs::write(&file, "services:\n  web:\n    image: busybox\n")
                .expect("the Compose fixture should be written");
            fs::write(&override_file, "services:\n  worker:\n    image: busybox\n")
                .expect("the Compose override fixture should be written");
            let files = [file.clone(), override_file.clone()];
            let profiles = ["web".to_owned(), "qa".to_owned()];
            let socket = directory.path().join("selected.sock");
            let lifecycle_marker = directory.path().join("lifecycle-actions");
            let binary = directory.path().join("docker-lifecycle-fixture");
            let script = format!(
                "#!/bin/sh\n\
                 check_environment() {{\n\
                   [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] && [ \"$GOMEMLIMIT\" = 402653184 ] || exit 90\n\
                   [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = false ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = false ] || exit 91\n\
                   [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] && [ \"$HOME\" != \"$DOCKER_CONFIG\" ] || exit 92\n\
                   [ ! -e \"$DOCKER_CONFIG/config.json\" ] || exit 93\n\
                   [ \"$(ulimit -v)\" -le 4194304 ] && [ \"$(ulimit -d)\" -le 524288 ] || exit 96\n\
                   if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{COMPOSE_FILE+x}}${{COMPOSE_ENV_FILES+x}}${{COMPOSE_PROFILES+x}}\" ]; then exit 94; fi\n\
                   env | while IFS='=' read -r name value; do case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_DISABLE_ENV_FILE|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|GOMEMLIMIT|PWD|SHLVL|_) : ;; *) exit 95 ;; esac; done || exit 95\n\
                 }}\n\
                 if [ \"$1\" = compose ]; then\n\
                   if [ \"$#\" -eq 3 ]; then [ \"$2\" = version ] && [ \"$3\" = --short ] || exit 71; printf 'v5.6.0\\n'; exit 0; fi\n\
                   [ \"$#\" -eq 15 ] || exit 72\n\
                   [ \"$1\" = compose ] && [ \"$2\" = --project-directory ] && [ \"$3\" = {} ] || exit 73\n\
                   [ \"$4\" = --file ] && [ \"$5\" = {} ] && [ \"$6\" = --file ] && [ \"$7\" = {} ] || exit 74\n\
                   [ \"$8\" = --profile ] && [ \"$9\" = web ] && [ \"${{10}}\" = --profile ] && [ \"${{11}}\" = qa ] || exit 75\n\
                   [ \"${{12}}\" = config ] && [ \"${{13}}\" = --no-interpolate ] && [ \"${{14}}\" = --no-env-resolution ] && [ \"${{15}}\" = --services ] || exit 76\n\
                   check_environment\n\
                   printf 'web\\n'; exit 0\n\
                 fi\n\
                 [ \"$#\" -eq 14 ] || exit 77\n\
                 [ \"$1\" = --host ] && [ \"$2\" = {} ] && [ \"$3\" = compose ] || exit 78\n\
                 [ \"$4\" = --project-directory ] && [ \"$5\" = {} ] && [ \"$6\" = --file ] && [ \"$7\" = {} ] && [ \"$8\" = --file ] && [ \"$9\" = {} ] || exit 79\n\
                 [ \"${{10}}\" = --profile ] && [ \"${{11}}\" = web ] && [ \"${{12}}\" = --profile ] && [ \"${{13}}\" = qa ] || exit 80\n\
                 [ \"${{14}}\" = start ] || [ \"${{14}}\" = stop ] || exit 81\n\
                 check_environment\n\
                 printf '%s\\n' \"${{14}}\" >> {}\n",
                shell_quote(&project_directory.to_string_lossy()),
                shell_quote(&file.to_string_lossy()),
                shell_quote(&override_file.to_string_lossy()),
                shell_quote(&format!("unix://{}", socket.to_string_lossy())),
                shell_quote(&project_directory.to_string_lossy()),
                shell_quote(&file.to_string_lossy()),
                shell_quote(&override_file.to_string_lossy()),
                shell_quote(&lifecycle_marker.to_string_lossy()),
            );
            fs::write(&binary, script).expect("the fake Docker CLI should be written");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                .expect("the fake Docker CLI should be executable");

            for (action, expected) in [
                (ComposeLifecycleAction::Start, "start"),
                (ComposeLifecycleAction::Stop, "stop"),
            ] {
                let result = lifecycle_with_binary(
                    &binary,
                    &socket,
                    &files,
                    &profiles,
                    action,
                    OsStr::new("/usr/bin"),
                    None,
                )
                .await;
                assert_eq!(result.outcome, Ok(()));
                assert!(result.submitted, "the lifecycle process started");
                assert_eq!(
                    fs::read_to_string(&lifecycle_marker)
                        .expect("the fixture should record only accepted lifecycle commands")
                        .lines()
                        .last(),
                    Some(expected)
                );
            }
        }
        #[tokio::test]
        async fn invalid_configuration_prevents_lifecycle_command_and_hides_diagnostics() {
            let directory = FixtureDirectory::new();
            let project_directory = directory.path().join("trusted fixture");
            fs::create_dir(&project_directory).expect("the fixture project should exist");
            let file = project_directory.join("compose.yaml");
            fs::write(&file, "services: {}\n").expect("the Compose fixture should be written");
            let marker = directory.path().join("lifecycle-command-ran");
            let binary = directory.path().join("docker-validation-failure");
            let script = format!(
                "#!/bin/sh\n\
                 if [ \"$#\" -eq 3 ]; then [ \"$1\" = compose ] && [ \"$2\" = version ] && [ \"$3\" = --short ] || exit 71; printf 'v5.6.0\\n'; exit 0; fi\n\
                 if [ \"$1\" = compose ]; then [ \"$#\" -eq 9 ] && [ \"$6\" = config ] || exit 72; echo private-diagnostic >&2; exit 1; fi\n\
                 : > {}\n\
                 exit 0\n",
                shell_quote(&marker.to_string_lossy()),
            );
            fs::write(&binary, script).expect("the fake Docker CLI should be written");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                .expect("the fake Docker CLI should be executable");

            let result = lifecycle_with_binary(
                &binary,
                &directory.path().join("selected.sock"),
                std::slice::from_ref(&file),
                &[],
                ComposeLifecycleAction::Start,
                OsStr::new("/usr/bin"),
                None,
            )
            .await;

            assert_eq!(result.outcome, Err(ComposeLifecycleError::ValidationFailed));
            assert!(!result.submitted, "validation failure is not submitted");
            assert!(
                !marker.exists(),
                "validation failure must block lifecycle commands"
            );
            assert!(!format!("{result:?}").contains("private-diagnostic"));
            assert!(!format!("{result:?}").contains(&file.display().to_string()));
        }
        #[tokio::test]
        async fn lifecycle_failure_and_timeout_are_classified_after_submission() {
            let directory = FixtureDirectory::new();

            for (mode, deadline) in [
                ("failure", COMMAND_TIMEOUT),
                ("timeout", Duration::from_millis(50)),
            ] {
                let binary = directory.path().join(format!("docker-lifecycle-{mode}"));
                let script = "#!/bin/sh\n\
                     case \"$0\" in *failure) exit 7 ;; *timeout) sleep 1 ;; esac\n\
                     exit 0\n";
                fs::write(&binary, script).expect("the fake Docker CLI should be written");
                fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
                    .expect("the fake Docker CLI should be executable");
                let mut command = tokio::process::Command::new(&binary);
                let result = run_lifecycle_command_with_timeout(&mut command, deadline).await;
                assert_eq!(result.outcome, Err(ComposeLifecycleError::OperationFailed));
                assert!(
                    result.submitted,
                    "the lifecycle process started before {mode}"
                );
            }

            let unlaunchable = directory.path().join("docker-lifecycle-unlaunchable");
            fs::create_dir(&unlaunchable).expect("the non-executable test path should exist");
            let mut command = tokio::process::Command::new(&unlaunchable);
            let result = run_lifecycle_command_with_timeout(&mut command, COMMAND_TIMEOUT).await;
            assert_eq!(
                result.outcome,
                Err(ComposeLifecycleError::CommandStartFailed)
            );
            assert!(!result.submitted, "the process could not be started");
            let project_file = directory.path().join("compose.yaml");
            fs::write(&project_file, "services: {}\n")
                .expect("the validation fixture should be written");
            let validation = validate_with_profiles_report(
                &unlaunchable,
                std::slice::from_ref(&project_file),
                &[],
                OsStr::new("/usr/bin"),
                None,
                COMMAND_TIMEOUT,
            )
            .await;
            assert_eq!(
                validation.outcome,
                Err(ComposeValidationError::CommandStartFailed)
            );

            let missing = directory.path().join("missing-docker-command");
            let mut command = tokio::process::Command::new(missing);
            let result = run_lifecycle_command_with_timeout(&mut command, COMMAND_TIMEOUT).await;
            assert_eq!(result.outcome, Err(ComposeLifecycleError::DockerCliMissing));
            assert!(!result.submitted, "a missing CLI did not submit a process");
        }
        #[tokio::test]
        #[ignore = "requires a local Docker-compatible Unix socket and Compose; queries only a unique owned temporary project and creates no resources"]
        async fn local_compose_profile_status_reads_temporary_project_without_mutation() {
            use std::time::{SystemTime, UNIX_EPOCH};

            let engine = ferrum_engine::EngineClient::connect_default()
                .await
                .expect("the selected local engine should be accessible");
            let directory = FixtureDirectory::new();
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the system clock should be after the Unix epoch")
                .as_nanos();
            let project_name = format!("ferrum-status-{}-{suffix}", std::process::id());
            let project = directory.path().join(&project_name);
            fs::create_dir(&project)
                .expect("the owned Compose project directory should be created");
            let file = project.join("compose.yaml");
            fs::write(
                &file,
                format!(
                    "name: {project_name}\nservices:\n  always-probe:\n    image: scratch\n  status-probe:\n    image: scratch\n    profiles: [status-smoke]\n  hidden-probe:\n    image: scratch\n    profiles: [inactive-smoke]\n"
                ),
            )
            .expect("the temporary Compose file should be written");

            let files = [file];
            let profiles = ["status-smoke".to_owned()];
            let status =
                existing_compose_containers(engine.local_socket_path(), &files, &profiles).await;
            status
                .outcome
                .expect("read-only profile status for the temporary project should succeed");
            assert!(
                status.containers.is_empty(),
                "status queries must not create Compose containers"
            );
        }
    }
    #[cfg(test)]
    mod page {
        use super::*;
        use egui::accesskit::Role;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };

        struct ComposeUi {
            view: ComposeView,
            available: bool,
            message: &'static str,
            refresh_enabled: bool,
            validation_enabled: bool,
            status_enabled: bool,
            actions: ComposeActions,
        }

        fn harness_for(view: ComposeView) -> Harness<'static, ComposeUi> {
            harness_for_availability(view, true)
        }

        fn harness_for_availability(
            view: ComposeView,
            available: bool,
        ) -> Harness<'static, ComposeUi> {
            Harness::new_ui_state(
                |ui, state: &mut ComposeUi| {
                    state.actions = state.view.show(
                        ui,
                        state.available,
                        state.message,
                        state.refresh_enabled,
                        state.validation_enabled,
                        state.status_enabled,
                    );
                },
                ComposeUi {
                    view,
                    available,
                    message: if available {
                        ""
                    } else {
                        "The local Docker Engine is unavailable."
                    },
                    refresh_enabled: true,
                    validation_enabled: true,
                    status_enabled: available,
                    actions: ComposeActions::default(),
                },
            )
        }
        fn validation_result(
            outcome: Result<(), ComposeValidationError>,
        ) -> ComposeValidationResult {
            ComposeValidationResult {
                version: None,
                services: Vec::new(),
                outcome,
            }
        }

        #[test]
        fn override_files_append_in_order_can_be_removed_and_cancelled_without_path_display() {
            let mut view = ComposeView::default();
            let base = std::path::Path::new("/private/base/base-compose.yaml");
            let first = std::path::Path::new("/private/override/one.yaml");
            let second = std::path::Path::new("/private/override/two.yaml");

            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(base)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            let generation = view.begin_override_selection();
            assert!(view.finish_file_selection(generation, Some(first)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            let generation = view.begin_override_selection();
            assert!(view.finish_file_selection(generation, Some(second)));
            assert_eq!(
                view.selected_files(),
                &[
                    base.to_path_buf(),
                    first.to_path_buf(),
                    second.to_path_buf()
                ]
            );
            assert!(!format!("{:?}", view.validation).contains("/private/"));

            let cancelled_generation = view.begin_override_selection();
            assert!(view.finish_file_selection(cancelled_generation, None));
            assert_eq!(
                view.selected_files(),
                &[
                    base.to_path_buf(),
                    first.to_path_buf(),
                    second.to_path_buf()
                ]
            );
            view.remove_override(1);
            assert_eq!(
                view.selected_files(),
                &[base.to_path_buf(), second.to_path_buf()]
            );
            let (generation, files, profiles) = view
                .begin_validation()
                .expect("the remaining ordered file set should be revalidatable");
            assert_eq!(files, [base.to_path_buf(), second.to_path_buf()]);
            assert!(profiles.is_empty());
            assert!(view.is_validation_running());
            assert!(view.finish_validation(generation, validation_result(Ok(()))));

            let mut harness = harness_for(view);
            harness.run();
            let _order = harness.get_by_role_and_label(
                Role::Label,
                "Compose file order (base first, then overrides):",
            );
            let _base_name = harness.get_by_role_and_label(Role::Label, "base-compose.yaml");
            let _remaining_override = harness.get_by_role_and_label(Role::Label, "two.yaml");
            let _validate = harness.get_by_role_and_label(Role::Button, "Validate selected files");
        }
        #[test]
        fn changing_base_replaces_previous_base_and_clears_overrides() {
            let mut view = ComposeView::default();
            let original = std::path::Path::new("/private/old/base.yaml");
            let override_file = std::path::Path::new("/private/old/override.yaml");
            let replacement = std::path::Path::new("/private/new/base.yaml");

            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(original)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            let generation = view.begin_override_selection();
            assert!(view.finish_file_selection(generation, Some(override_file)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));

            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(replacement)));
            assert_eq!(view.selected_files(), &[replacement.to_path_buf()]);
            assert!(view.finish_validation(generation, validation_result(Ok(()))));

            let mut harness = harness_for(view);
            harness.run();
            let _change_base =
                harness.get_by_role_and_label(Role::Button, "Change base file (clears overrides)");
        }

        #[test]
        fn compose_selection_enforces_sixteen_file_limit() {
            let mut view = ComposeView::default();
            let base = std::path::PathBuf::from("/private/base.yaml");
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(&base)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            for index in 0..(MAX_COMPOSE_FILES - 1) {
                let path = std::path::PathBuf::from(format!("/private/override-{index}.yaml"));
                let generation = view.begin_override_selection();
                assert!(view.finish_file_selection(generation, Some(&path)));
                assert!(view.finish_validation(generation, validation_result(Ok(()))));
            }
            assert_eq!(view.selected_files().len(), MAX_COMPOSE_FILES);
            let extra = std::path::PathBuf::from("/private/extra.yaml");
            let generation = view.begin_override_selection();
            assert!(view.finish_file_selection(generation, Some(&extra)));
            assert_eq!(view.selected_files().len(), MAX_COMPOSE_FILES);
            assert!(matches!(
                &view.validation,
                ComposeValidationState::Failed {
                    error: ComposeValidationError::InvalidSelectedFile,
                    ..
                }
            ));
            let mut harness = harness_for(view);
            harness.run();
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Add override file")
                    .accesskit_node()
                    .is_disabled()
            );
        }
        #[test]
        fn compose_version_is_visible_without_an_available_engine() {
            let mut view = ComposeView::default();
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(
                generation,
                Some(std::path::Path::new("/tmp/compose.yaml")),
            ));
            assert!(view.finish_validation(
                generation,
                ComposeValidationResult {
                    version: Some("v5.6.0".to_owned()),
                    services: vec!["web".to_owned()],
                    outcome: Ok(()),
                },
            ));
            let mut harness = harness_for_availability(view, false);

            harness.run();

            let _version_label =
                harness.get_by_role_and_label(Role::Label, "Docker Compose version:");
            let _version = harness.get_by_role_and_label(Role::Label, "v5.6.0");
        }

        #[test]
        fn compose_project_page_exposes_accessible_refresh_and_read_only_inventory() {
            let mut view = ComposeView::default();
            let generation = view.begin_load();
            view.finish_load(
                generation,
                Ok(ComposeDiscovery {
                    version: "v5.6.0".to_owned(),
                    projects: vec![ComposeProject {
                        name: "frontend".to_owned(),
                        status: "running(2)".to_owned(),
                    }],
                }),
            );
            let mut harness = harness_for(view);
            harness.run();

            assert!(
                !harness
                    .get_by_role_and_label(Role::Button, "Refresh Compose projects")
                    .accesskit_node()
                    .is_disabled()
            );
            {
                let _version_label =
                    harness.get_by_role_and_label(Role::Label, "Docker Compose version:");
                let _version = harness.get_by_role_and_label(Role::Label, "v5.6.0");
                let _project = harness.get_by_role_and_label(Role::Label, "frontend");
                let _status = harness.get_by_role_and_label(Role::Label, "running(2)");
            }
            harness
                .get_by_role_and_label(Role::Button, "Refresh Compose projects")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.refresh);
        }
        #[test]
        fn compose_file_selection_is_accessible_without_an_engine_connection() {
            let mut harness = harness_for_availability(ComposeView::default(), false);
            harness.run();

            let _security_disclosure = harness.get_by_role_and_label(
                Role::Label,
"Security notice: validate only trusted Compose files. Compose may recursively read local files referenced by include or service extends.file, including each included project's default .env and explicit include.env_file, even outside the selected directory under your permissions. Disabling the root .env and using --no-interpolate/--no-env-resolution do not confine these reads. Remote Git/OCI loaders are disabled. The selection is not a path boundary, and validation is not sandboxed.",
            );
            assert!(
                !harness
                    .get_by_role_and_label(Role::Button, "Choose base file")
                    .accesskit_node()
                    .is_disabled()
            );
            harness
                .get_by_role_and_label(Role::Button, "Choose base file")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.select_base_file);
        }

        #[test]
        fn superseded_file_selection_results_are_rejected() {
            let mut view = ComposeView::default();
            let stale_generation = view.begin_file_selection();
            let current_generation = view.begin_file_selection();
            let stale_file = std::path::Path::new("/private/stale/compose.yaml");
            let current_file = std::path::Path::new("/private/current/compose.yaml");

            assert!(
                !view.finish_file_selection(stale_generation, Some(stale_file)),
                "an older chooser result must not replace a newer selection"
            );
            assert!(view.finish_file_selection(current_generation, Some(current_file)));
            assert_eq!(view.selected_files(), &[current_file.to_path_buf()]);
        }

        #[test]
        fn pending_file_selection_is_accessible_and_rejects_stale_or_cancelled_results() {
            let mut waiting = ComposeView::default();
            waiting.begin_file_selection();
            assert!(waiting.is_file_selection_running());
            let mut harness = harness_for(waiting);
            harness.step();
            let _waiting = harness.get_by_role_and_label(Role::Label, "Choosing a base file…");
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Choose base file")
                    .accesskit_node()
                    .is_disabled()
            );
            assert!(
                harness
                    .get_by_role_and_label(Role::TextInput, "Profiles:")
                    .accesskit_node()
                    .is_disabled()
            );
            drop(harness);

            let selected_file = std::path::Path::new("/private/project/compose\u{202e}.yaml");
            let mut selected = ComposeView::default();
            let generation = selected.begin_file_selection();
            assert!(selected.finish_file_selection(generation, Some(selected_file)));
            assert!(selected.is_validation_running());
            assert!(matches!(
                &selected.validation,
                ComposeValidationState::Validating { file_name }
                    if file_name == "compose\\u{202e}.yaml"
            ));

            let mut stale = ComposeView::default();
            let stale_generation = stale.begin_file_selection();
            stale.invalidate();
            assert!(!stale.finish_file_selection(stale_generation, Some(selected_file)));
            assert!(!stale.is_validation_running());

            let mut cancelled = ComposeView::default();
            let cancel_generation = cancelled.begin_file_selection();
            assert!(cancelled.finish_file_selection(cancel_generation, None));
            assert!(matches!(
                &cancelled.validation,
                ComposeValidationState::SelectionCancelled
            ));
            assert!(!cancelled.is_file_selection_running());
        }

        #[test]
        fn validation_states_are_accessible_and_show_only_safe_file_names() {
            let selected_file = std::path::Path::new("/private/project/compose\u{202e}.yaml");
            let safe_name = "compose\\u{202e}.yaml";

            let mut validating = ComposeView::default();
            let generation = validating.begin_file_selection();
            assert!(validating.finish_file_selection(generation, Some(selected_file)));
            assert!(
                !format!("{:?}", validating.validation).contains("/private/project"),
                "the view must retain only a bounded basename"
            );
            let mut harness = harness_for(validating);
            harness.step();
            {
                let _validating = harness.get_by_role_and_label(
                    Role::Label,
                    "Validating Compose files: compose\\u{202e}.yaml",
                );
            }
            drop(harness);
            let overlong_file_name = format!(
                "/private/project/{}.yaml",
                "x".repeat(MAX_COMPOSE_FILENAME_DISPLAY_BYTES),
            );
            let mut bounded = ComposeView::default();
            let generation = bounded.begin_file_selection();
            assert!(bounded.finish_file_selection(
                generation,
                Some(std::path::Path::new(&overlong_file_name)),
            ));
            assert!(matches!(
                &bounded.validation,
                ComposeValidationState::Validating { file_name } if file_name == "[name omitted]"
            ));
            let mut harness = harness_for(bounded);
            harness.step();
            let _bounded_name = harness.get_by_role_and_label(Role::Label, "[name omitted]");
            drop(harness);

            let mut succeeded = ComposeView::default();
            let generation = succeeded.begin_file_selection();
            assert!(succeeded.finish_file_selection(generation, Some(selected_file)));
            assert!(succeeded.finish_validation(generation, validation_result(Ok(()))));
            let mut harness = harness_for(succeeded);
            harness.run();
            {
                let _name = harness.get_by_role_and_label(Role::Label, safe_name);
                let _success = harness.get_by_role_and_label(
                    Role::Label,
                    "Structural validation succeeded. Environment interpolation and service env_file resolution were skipped.",
                );
            }
            drop(harness);

            let mut failed = ComposeView::default();
            let generation = failed.begin_file_selection();
            assert!(failed.finish_file_selection(generation, Some(selected_file)));
            assert!(failed.finish_validation(
                generation,
                validation_result(Err(ComposeValidationError::ValidationFailed)),
            ));
            let mut harness = harness_for(failed);
            harness.run();
            {
                let _name = harness.get_by_role_and_label(Role::Label, safe_name);
                let _static_error =
                    harness.get_by_role_and_label(Role::Label, "Compose file validation failed.");
            }
            drop(harness);

            let mut unavailable = ComposeView::default();
            let generation = unavailable.begin_file_selection();
            assert!(unavailable.finish_file_selection(generation, Some(selected_file)));
            assert!(unavailable.finish_validation(
                generation,
                validation_result(Err(ComposeValidationError::ComposePluginUnavailable)),
            ));
            let mut harness = harness_for(unavailable);
            harness.run();
            let _plugin_error = harness.get_by_role_and_label(
                Role::Label,
                "Docker Compose is unavailable. Install the Docker Compose plugin to validate Compose files.",
            );
        }
        #[test]
        fn declared_service_inventory_renders_success_empty_error_loading_and_ignores_stale_results()
         {
            let file = std::path::Path::new("/private/project/compose.yaml");
            let mut successful = ComposeView::default();
            let generation = successful.begin_file_selection();
            assert!(successful.finish_file_selection(generation, Some(file)));
            assert!(successful.finish_validation(
                generation,
                ComposeValidationResult {
                    version: None,
                    services: vec!["web".to_owned(), "api\\u{202e}".to_owned()],
                    outcome: Ok(()),
                },
            ));
            let mut harness = harness_for(successful);
            harness.run();
            let _web = harness.get_by_role_and_label(Role::Label, "web");
            let _escaped = harness.get_by_role_and_label(Role::Label, "api\\u{202e}");
            let _declared_not_running = harness.get_by_role_and_label(
                Role::Label,
                "Local Compose CLI/file-derived inventory includes every declared service, including profile-gated services even when their profiles are inactive. These are not running containers. Listing does not query Engine status or logs.",
            );
            drop(harness);

            let mut empty = ComposeView::default();
            let generation = empty.begin_file_selection();
            assert!(empty.finish_file_selection(generation, Some(file)));
            assert!(empty.finish_validation(generation, validation_result(Ok(()))));
            let mut harness = harness_for(empty);
            harness.run();
            let _empty = harness.get_by_role_and_label(
                Role::Label,
                "No declared services were returned for the selected files.",
            );
            drop(harness);

            let mut failed = ComposeView::default();
            let generation = failed.begin_file_selection();
            assert!(failed.finish_file_selection(generation, Some(file)));
            assert!(failed.finish_validation(
                generation,
                validation_result(Err(ComposeValidationError::ValidationFailed)),
            ));
            let mut harness = harness_for(failed);
            harness.run();
            let _error = harness.get_by_role_and_label(
                Role::Label,
                "Declared services are unavailable because validation failed.",
            );
            drop(harness);

            let mut loading = ComposeView::default();
            let generation = loading.begin_file_selection();
            assert!(loading.finish_file_selection(generation, Some(file)));
            let mut harness = harness_for(loading);
            harness.step();
            let _loading = harness.get_by_role_and_label(Role::Label, "Loading declared services…");
            drop(harness);

            let mut stale = ComposeView::default();
            let generation = stale.begin_file_selection();
            assert!(stale.finish_file_selection(generation, Some(file)));
            assert!(stale.finish_validation(
                generation,
                ComposeValidationResult {
                    version: None,
                    services: vec!["current".to_owned()],
                    outcome: Ok(()),
                },
            ));
            let (stale_generation, _, _) = stale.begin_validation().unwrap();
            stale.profiles_changed();
            assert!(!stale.finish_validation(
                stale_generation,
                ComposeValidationResult {
                    version: None,
                    services: vec!["stale".to_owned()],
                    outcome: Ok(()),
                },
            ));
            assert!(stale.validated_services.is_empty());
        }

        #[test]
        fn validation_cancel_and_stale_results_do_not_replace_current_state() {
            let stale_file = std::path::Path::new("/private/stale/old.yaml");
            let current_file = std::path::Path::new("/private/current/compose.yaml");
            let mut view = ComposeView::default();
            let stale_generation = view.begin_file_selection();
            assert!(view.finish_file_selection(stale_generation, Some(stale_file)));
            view.invalidate();
            let current_generation = view.begin_file_selection();
            assert!(view.finish_file_selection(current_generation, Some(current_file)));
            assert!(!view.finish_validation(stale_generation, validation_result(Ok(()))));
            assert!(view.finish_validation(current_generation, validation_result(Ok(()))));
            assert!(matches!(
                &view.validation,
                ComposeValidationState::Succeeded { file_name } if file_name == "compose.yaml"
            ));
            let mut harness = harness_for(view);
            harness.run();
            {
                let _name = harness.get_by_role_and_label(Role::Label, "compose.yaml");
                let _success = harness.get_by_role_and_label(
                    Role::Label,
                    "Structural validation succeeded. Environment interpolation and service env_file resolution were skipped.",
                );
            }
            drop(harness);

            let mut cancellable = ComposeView::default();
            let generation = cancellable.begin_file_selection();
            assert!(cancellable.finish_file_selection(generation, Some(current_file)));
            let mut harness = harness_for(cancellable);
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Cancel validation")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.cancel_validation);
            drop(harness);

            let mut cancelled = ComposeView::default();
            let generation = cancelled.begin_file_selection();
            assert!(cancelled.finish_file_selection(generation, Some(current_file)));
            cancelled.cancel_validation();
            let mut harness = harness_for(cancelled);
            harness.run();
            {
                let _name = harness.get_by_role_and_label(Role::Label, "compose.yaml");
                let _cancelled = harness.get_by_role_and_label(
                    Role::Label,
                    "Validation cancelled for Compose files: compose.yaml",
                );
            }
            drop(harness);

            let mut selection_cancelled = ComposeView::default();
            let generation = selection_cancelled.begin_file_selection();
            assert!(selection_cancelled.finish_file_selection(generation, None));
            let mut harness = harness_for(selection_cancelled);
            harness.run();
            let _cancelled =
                harness.get_by_role_and_label(Role::Label, "File selection cancelled.");
        }

        #[test]
        fn stale_list_generations_cannot_replace_the_current_compose_inventory() {
            let mut view = ComposeView::default();
            let stale_generation = view.begin_load();
            let current_generation = view.begin_load();
            assert!(!view.finish_load(
                stale_generation,
                Ok(ComposeDiscovery {
                    version: "v5.5.0".to_owned(),
                    projects: vec![ComposeProject {
                        name: "stale".to_owned(),
                        status: "exited(0)".to_owned(),
                    }],
                }),
            ));
            assert!(view.is_loading());
            assert!(view.finish_load(
                current_generation,
                Ok(ComposeDiscovery {
                    version: "v5.6.0".to_owned(),
                    projects: vec![ComposeProject {
                        name: "current".to_owned(),
                        status: "running(1)".to_owned(),
                    }],
                }),
            ));
            assert_eq!(
                view.projects,
                vec![ComposeProject {
                    name: "current".to_owned(),
                    status: "running(1)".to_owned(),
                }]
            );
        }

        #[test]
        fn validated_compose_files_enable_container_status_refresh() {
            let mut view = ComposeView::default();
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(
                generation,
                Some(std::path::Path::new("/private/compose.yaml")),
            ));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));

            let mut harness = harness_for(view);
            harness.run();
            assert!(
                !harness
                    .get_by_role_and_label(Role::Button, "Refresh existing containers")
                    .accesskit_node()
                    .is_disabled()
            );
            let _environment_note = harness.get_by_role_and_label(
                Role::Label,
                "Status uses Ferrum's isolated CLI environment and skips the selected project's default .env; environment-dependent project matching may differ from a normal shell.",
            );
        }
        fn validated_compose_view() -> ComposeView {
            let mut view = ComposeView::default();
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(
                generation,
                Some(std::path::Path::new("/private/compose.yaml")),
            ));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            view
        }
        #[test]
        fn profile_changes_invalidate_status_and_logs_and_reject_stale_results() {
            let mut view = validated_compose_view();
            let (status_generation, _, _) = view.begin_status_refresh().unwrap();
            assert!(view.finish_status_refresh(
                status_generation,
                ComposeStatusResult {
                    containers: vec![ComposeContainer {
                        name: "api-1".to_owned(),
                        service: "api".to_owned(),
                        state: "running".to_owned(),
                        project: Some("profile-project".to_owned()),
                        health: None,
                    }],
                    outcome: Ok(()),
                },
            ));
            let (logs_generation, _, _, _) = view.begin_service_logs("api").unwrap();
            view.profiles_changed();

            assert!(matches!(&view.validation, ComposeValidationState::Idle));
            assert!(matches!(&view.status, ComposeStatusState::Idle));
            assert!(matches!(&view.logs, ComposeLogsState::Idle));
            assert!(view.containers.is_empty());
            assert!(view.logs_output.is_empty());
            assert!(!view.finish_status_refresh(
                status_generation,
                ComposeStatusResult {
                    containers: Vec::new(),
                    outcome: Ok(()),
                },
            ));
            assert!(!view.finish_service_logs(
                logs_generation,
                ComposeLogsResult {
                    output: "stale".to_owned(),
                    outcome: Ok(()),
                },
            ));
            assert!(view.logs_output.is_empty());
        }

        #[test]
        fn profile_text_input_emits_change_action() {
            let mut harness = harness_for(ComposeView::default());
            harness.run();
            harness
                .get_by_role_and_label(Role::TextInput, "Profiles:")
                .focus();
            harness.step();
            assert!(
                harness
                    .get_by_role_and_label(Role::TextInput, "Profiles:")
                    .is_focused()
            );
            harness
                .get_by_role_and_label(Role::TextInput, "Profiles:")
                .type_text("api");
            harness.step();

            assert_eq!(harness.state().view.profile_input, "api");
            assert!(harness.state().actions.profiles_changed);
        }

        #[test]
        fn invalid_profiles_are_visible_and_gate_validation() {
            let mut view = validated_compose_view();
            view.profile_input = "api,,worker".to_owned();
            let mut harness = harness_for(view);
            harness.run();
            let _invalid = harness.get_by_role_and_label(
                Role::Label,
                "Invalid profiles. Use up to 16 unique names (64 bytes each), or * alone to enable all profiles; input is limited to 1024 bytes.",
            );
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Validate selected files")
                    .accesskit_node()
                    .is_disabled()
            );
            assert!(harness.state_mut().view.begin_validation().is_none());
            assert!(matches!(
                &harness.state().view.validation,
                ComposeValidationState::Failed {
                    error: ComposeValidationError::InvalidProfiles,
                    ..
                }
            ));
        }
        #[test]
        fn file_selection_with_invalid_profiles_skips_automatic_validation() {
            let mut view = ComposeView {
                profile_input: "api,,worker".to_owned(),
                ..ComposeView::default()
            };
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(
                generation,
                Some(std::path::Path::new("/private/compose.yaml")),
            ));
            assert!(view.is_validation_running());
            assert!(view.profiles_for_automatic_validation().is_none());
            assert!(matches!(
                &view.validation,
                ComposeValidationState::Failed {
                    error: ComposeValidationError::InvalidProfiles,
                    ..
                }
            ));
        }
        #[test]
        fn container_status_refresh_waits_for_project_inventory() {
            let mut view = validated_compose_view();
            view.begin_load();
            let mut harness = harness_for(view);
            harness.step();
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Refresh existing containers")
                    .accesskit_node()
                    .is_disabled()
            );
        }
        #[test]
        fn container_status_cancellation_and_selection_reject_stale_results() {
            let mut stale = validated_compose_view();
            let (stale_generation, _, _) = stale
                .begin_status_refresh()
                .expect("validated selected files should allow a status refresh");
            stale.begin_file_selection();
            assert!(!stale.finish_status_refresh(
                stale_generation,
                ComposeStatusResult {
                    containers: vec![ComposeContainer {
                        name: "stale-1".to_owned(),
                        service: "stale".to_owned(),
                        state: "running".to_owned(),
                        project: Some("stale-project".to_owned()),
                        health: None,
                    }],
                    outcome: Ok(()),
                },
            ));
            assert!(stale.is_file_selection_running());
            assert!(stale.containers.is_empty());

            let mut cancelled = validated_compose_view();
            let (generation, _, _) = cancelled
                .begin_status_refresh()
                .expect("validated selected files should allow a status refresh");
            cancelled.cancel_status_refresh();
            assert!(!cancelled.finish_status_refresh(
                generation,
                ComposeStatusResult {
                    containers: Vec::new(),
                    outcome: Ok(()),
                },
            ));
            assert!(matches!(
                &cancelled.status,
                ComposeStatusState::Cancelled { .. }
            ));
            let mut harness = harness_for(cancelled);
            harness.run();
            let _cancelled =
                harness.get_by_role_and_label(Role::Label, "Container refresh cancelled.");
        }

        #[test]
        fn pending_container_status_exposes_cancel_action() {
            let mut view = validated_compose_view();
            view.begin_status_refresh()
                .expect("validated selected files should allow a status refresh");
            let mut harness = harness_for(view);
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Cancel container refresh")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.cancel_status);
        }

        #[test]
        fn successful_container_status_renders_existing_service_fields() {
            let mut view = validated_compose_view();
            let (generation, _, _) = view
                .begin_status_refresh()
                .expect("validated selected files should allow a status refresh");
            assert!(view.finish_status_refresh(
                generation,
                ComposeStatusResult {
                    containers: vec![
                        ComposeContainer {
                            name: "project-web-1".to_owned(),
                            service: "web".to_owned(),
                            project: Some("alpha".to_owned()),
                            state: "running".to_owned(),
                            health: Some("healthy".to_owned()),
                        },
                        ComposeContainer {
                            name: "project-worker-1".to_owned(),
                            service: "worker".to_owned(),
                            project: None,
                            state: "exited".to_owned(),
                            health: None,
                        },
                    ],
                    outcome: Ok(()),
                },
            ));

            let mut harness = harness_for(view);
            harness.run();
            let _container = harness.get_by_role_and_label(Role::Label, "project-web-1");
            let _service = harness.get_by_role_and_label(Role::Label, "web");
            let _reported_project = harness.get_by_role_and_label(Role::Label, "alpha");
            let _unreported_project = harness.get_by_role_and_label(Role::Label, "Not reported");
            let _project_trust_note = harness.get_by_role_and_label(
                Role::Label,
                "Project is untrusted per-container metadata reported by Compose; it is not a canonical identity, does not establish file ownership or authorization, and does not determine lifecycle scope.",
            );
            let _state = harness.get_by_role_and_label(Role::Label, "running");
            let _stopped_state = harness.get_by_role_and_label(Role::Label, "exited");
            let _health = harness.get_by_role_and_label(Role::Label, "healthy");
            let _unreported_health = harness.get_by_role_and_label(Role::Label, "—");
        }

        #[test]
        fn empty_container_status_is_distinct_from_a_query_error() {
            let mut empty = validated_compose_view();
            let (generation, _, _) = empty
                .begin_status_refresh()
                .expect("validated selected files should allow a status refresh");
            assert!(empty.finish_status_refresh(
                generation,
                ComposeStatusResult {
                    containers: Vec::new(),
                    outcome: Ok(()),
                },
            ));
            let mut empty_harness = harness_for(empty);
            empty_harness.run();
            let _empty = empty_harness.get_by_role_and_label(
                Role::Label,
                "No existing containers for the selected Compose files.",
            );

            let mut failed = validated_compose_view();
            let (generation, _, _) = failed
                .begin_status_refresh()
                .expect("validated selected files should allow a status refresh");
            assert!(failed.finish_status_refresh(
                generation,
                ComposeStatusResult {
                    containers: Vec::new(),
                    outcome: Err(ComposeStatusError::QueryFailed),
                },
            ));
            let mut failed_harness = harness_for(failed);
            failed_harness.run();
            let _error = failed_harness
                .get_by_role_and_label(Role::Label, "Could not read existing Compose containers.");
        }

        #[test]
        fn loading_empty_and_unavailable_states_are_accessible() {
            let mut loading = ComposeView::default();
            loading.begin_load();
            let mut harness = harness_for(loading);
            harness.step();
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Refresh Compose projects")
                    .accesskit_node()
                    .is_disabled()
            );
            {
                let _loading =
                    harness.get_by_role_and_label(Role::Label, "Loading Compose projects…");
            }
            drop(harness);

            let mut empty = ComposeView::default();
            let generation = empty.begin_load();
            empty.finish_load(
                generation,
                Ok(ComposeDiscovery {
                    version: "v5.6.0".to_owned(),
                    projects: Vec::new(),
                }),
            );
            let mut harness = harness_for(empty);
            harness.run();
            {
                let _empty =
                    harness.get_by_role_and_label(Role::Label, "No Compose projects found.");
            }
            drop(harness);

            let mut unavailable = ComposeView::default();
            let generation = unavailable.begin_load();
            unavailable.finish_load(
                generation,
                Err(ComposeError::ProjectListFailed {
                    version: "v5.6.0".to_owned(),
                }),
            );
            let mut harness = harness_for(unavailable);
            harness.run();
            let _error = harness.get_by_role_and_label(Role::Label, "Compose projects unavailable");
            let _version_label =
                harness.get_by_role_and_label(Role::Label, "Docker Compose version:");
            let _version = harness.get_by_role_and_label(Role::Label, "v5.6.0");
            let _message = harness.get_by_role_and_label(
                Role::Label,
                "Compose projects could not be read from the selected local Docker Engine.",
            );
        }
        #[test]
        fn service_log_action_requires_current_status_and_cancel_rejects_stale_output() {
            let mut view = validated_compose_view();
            assert!(view.begin_service_logs("api").is_none());
            let (status_generation, _, _) = view.begin_status_refresh().unwrap();
            assert!(view.finish_status_refresh(
                status_generation,
                ComposeStatusResult {
                    containers: vec![
                        ComposeContainer {
                            name: "api-1".to_owned(),
                            service: "api".to_owned(),
                            state: "running".to_owned(),
                            project: None,
                            health: None,
                        },
                        ComposeContainer {
                            name: "worker-1".to_owned(),
                            service: "worker".to_owned(),
                            state: "running".to_owned(),
                            project: None,
                            health: None,
                        },
                    ],
                    outcome: Ok(()),
                }
            ));
            assert!(view.begin_service_logs("--help").is_none());
            let mut harness = harness_for(view);
            harness.run();
            let _api_logs = harness.get_by_role_and_label(Role::Button, "Snapshot logs for api");
            let _worker_logs =
                harness.get_by_role_and_label(Role::Button, "Snapshot logs for worker");
            harness
                .get_by_role_and_label(Role::Button, "Snapshot logs for api")
                .click_accesskit();
            harness.step();
            assert_eq!(harness.state().actions.logs_service.as_deref(), Some("api"));
            let (generation, _, _, service) =
                harness.state_mut().view.begin_service_logs("api").unwrap();
            assert_eq!(service, "api");
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Cancel log snapshot")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.cancel_logs);
            harness.state_mut().view.cancel_service_logs();
            assert!(!harness.state_mut().view.finish_service_logs(
                generation,
                ComposeLogsResult {
                    output: "stale".to_owned(),
                    outcome: Ok(()),
                }
            ));
            assert!(harness.state().view.logs_output.is_empty());
            harness.run();
            let _cancelled = harness.get_by_role_and_label(Role::Label, "Log snapshot cancelled.");
        }
        #[test]
        fn service_log_results_render_content_empty_and_static_error_states() {
            fn view_with_service_status() -> ComposeView {
                let mut view = validated_compose_view();
                let (generation, _, _) = view.begin_status_refresh().unwrap();
                assert!(view.finish_status_refresh(
                    generation,
                    ComposeStatusResult {
                        containers: vec![ComposeContainer {
                            name: "api-1".to_owned(),
                            service: "api".to_owned(),
                            state: "running".to_owned(),
                            project: None,
                            health: None,
                        }],
                        outcome: Ok(()),
                    },
                ));
                view
            }

            let mut success = view_with_service_status();
            let (generation, _, _, _) = success.begin_service_logs("api").unwrap();
            assert!(success.finish_service_logs(
                generation,
                ComposeLogsResult {
                    output: "safe log line".to_owned(),
                    outcome: Ok(()),
                },
            ));
            let mut success_harness = harness_for(success);
            success_harness.run();
            let _output = success_harness.get_by_role_and_label(Role::Label, "safe log line");
            drop(success_harness);

            let mut empty = view_with_service_status();
            let (generation, _, _, _) = empty.begin_service_logs("api").unwrap();
            assert!(empty.finish_service_logs(
                generation,
                ComposeLogsResult {
                    output: String::new(),
                    outcome: Ok(()),
                },
            ));
            let mut empty_harness = harness_for(empty);
            empty_harness.run();
            let _empty = empty_harness.get_by_role_and_label(Role::Label, "No log output.");
            drop(empty_harness);

            let mut failed = view_with_service_status();
            let (generation, _, _, _) = failed.begin_service_logs("api").unwrap();
            assert!(failed.finish_service_logs(
                generation,
                ComposeLogsResult {
                    output: String::new(),
                    outcome: Err(ComposeLogsError::QueryFailed),
                },
            ));
            let mut failed_harness = harness_for(failed);
            failed_harness.run();
            let _error = failed_harness.get_by_role_and_label(
                Role::Label,
                "Could not read the selected Compose service logs.",
            );
        }
        #[test]
        fn compose_import_requires_current_successful_validation() {
            let file = std::path::Path::new("/private/project/compose.yaml");
            let mut view = ComposeView::default();
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(file)));
            assert!(!view.is_current_selection_validated());
            assert!(view.finish_validation(
                generation,
                validation_result(Err(ComposeValidationError::ValidationFailed)),
            ));
            assert!(!view.import_current_project("project"));
            let mut changed_profiles = validated_compose_view();
            changed_profiles.profile_input = "different".to_owned();
            changed_profiles.profiles_changed();
            assert!(!changed_profiles.is_current_selection_validated());
            assert!(!changed_profiles.import_current_project("stale-profile"));

            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(file)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            assert!(view.is_current_selection_validated());
            let mut eligible_harness = harness_for(view);
            eligible_harness.run();
            assert!(
                !eligible_harness
                    .get_by_role_and_label(Role::Button, "Import validated selection")
                    .accesskit_node()
                    .is_disabled()
            );
            drop(eligible_harness);

            let mut view = ComposeView::default();
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(file)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            view.begin_override_selection();
            let mut pending_harness = harness_for(view);
            pending_harness.step();
            assert!(
                pending_harness
                    .get_by_role_and_label(Role::Button, "Import validated selection")
                    .accesskit_node()
                    .is_disabled()
            );
        }

        #[test]
        fn compose_import_rejects_duplicate_names_and_enforces_bounds() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Development"));
            assert!(!view.import_current_project("Development"));
            assert!(!view.import_current_project(""));
            assert!(!view.import_current_project(&"é".repeat(129)));
            for index in 1..MAX_IMPORTED_COMPOSE_PROJECTS {
                assert!(view.import_current_project(&format!("project-{index}")));
            }
            assert_eq!(view.imported_projects.len(), MAX_IMPORTED_COMPOSE_PROJECTS);
            assert!(!view.import_current_project("overflow"));
            let mut harness = harness_for(view);
            harness.run();
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Import validated selection")
                    .accesskit_node()
                    .is_disabled()
            );
            let _limit_message = harness.get_by_role_and_label(
                Role::Label,
                "This session has reached the 32-import limit.",
            );
        }

        #[test]
        fn selecting_import_restores_ordered_files_and_profiles_and_clears_old_results() {
            let base = std::path::PathBuf::from("/private/base/compose.yaml");
            let override_file = std::path::PathBuf::from("/private/overrides/dev.yaml");
            let mut view = ComposeView {
                profile_input: "dev".to_owned(),
                ..ComposeView::default()
            };
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(&base)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            let generation = view.begin_override_selection();
            assert!(view.finish_file_selection(generation, Some(&override_file)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            view.profile_input = "dev,qa".to_owned();
            view.profiles_changed();
            let (generation, _, _) = view.begin_validation().unwrap();
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            assert!(view.import_current_project("Imported"));

            let (status_generation, _, _) = view.begin_status_refresh().unwrap();
            assert!(view.finish_status_refresh(
                status_generation,
                ComposeStatusResult {
                    containers: vec![ComposeContainer {
                        name: "api-1".to_owned(),
                        service: "api".to_owned(),
                        project: Some("untrusted-label".to_owned()),
                        state: "running".to_owned(),
                        health: None,
                    }],
                    outcome: Ok(()),
                },
            ));
            let (logs_generation, _, _, _) = view.begin_service_logs("api").unwrap();
            let picker_generation = view.begin_override_selection();
            let stale_file = std::path::Path::new("/private/stale/override.yaml");
            let validation_generation = view.validation_generation;
            assert!(view.select_imported_project(0));
            assert!(!view.finish_file_selection(picker_generation, Some(stale_file)));
            assert_eq!(view.selected_files, [base.clone(), override_file.clone()]);
            assert_eq!(
                view.imported_projects[0].files,
                [base.clone(), override_file.clone()]
            );
            assert_eq!(
                view.imported_projects[0].profiles,
                vec!["dev".to_owned(), "qa".to_owned()],
            );
            assert_eq!(view.profile_input, "dev,qa");
            assert!(!view.can_refresh_containers());
            assert!(view.containers.is_empty());
            assert!(view.logs_output.is_empty());
            assert!(!view.can_request_service_logs("api"));
            assert!(!view.finish_validation(validation_generation, validation_result(Ok(())),));
            assert!(!view.finish_status_refresh(
                status_generation,
                ComposeStatusResult {
                    containers: Vec::new(),
                    outcome: Ok(()),
                },
            ));
            assert!(!view.finish_service_logs(
                logs_generation,
                ComposeLogsResult {
                    output: "stale".to_owned(),
                    outcome: Ok(()),
                },
            ));
            assert!(view.begin_validation().is_some());
        }

        #[test]
        fn imported_configuration_button_restores_ordered_selection_and_requires_validation() {
            let base = std::path::PathBuf::from("/private/base/compose.yaml");
            let override_file = std::path::PathBuf::from("/private/overrides/dev.yaml");
            let current_file = std::path::PathBuf::from("/private/current.yaml");
            let mut view = ComposeView {
                profile_input: "dev".to_owned(),
                ..ComposeView::default()
            };
            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(&base)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            let generation = view.begin_override_selection();
            assert!(view.finish_file_selection(generation, Some(&override_file)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            view.profile_input = "dev,qa".to_owned();
            view.profiles_changed();
            let (generation, _, _) = view.begin_validation().unwrap();
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            assert!(view.import_current_project("Imported"));

            let generation = view.begin_file_selection();
            assert!(view.finish_file_selection(generation, Some(&current_file)));
            assert!(view.finish_validation(generation, validation_result(Ok(()))));
            let mut harness = harness_for(view);
            harness.run();
            harness
                .get_by_role_and_label(Role::Button, "Use imported configuration")
                .click_accesskit();
            harness.step();

            assert_eq!(harness.state().view.selected_files, [base, override_file]);
            assert_eq!(harness.state().view.profile_input, "dev,qa");
            assert_eq!(
                harness.state().view.selected_import_label.as_deref(),
                Some("Imported")
            );
            assert!(!harness.state().view.is_current_selection_validated());
        }

        #[test]
        fn imported_projects_render_separately_from_engine_discovery() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Local label"));
            view.projects = vec![ComposeProject {
                name: "engine-project".to_owned(),
                status: "running(1)".to_owned(),
            }];
            view.loaded = true;
            let mut harness = harness_for(view);
            harness.run();
            let _import_heading = harness
                .get_by_role_and_label(Role::Label, "Imported local projects (this session only)");
            let _local_name = harness.get_by_role_and_label(Role::Label, "Local label");
            let _discovery_name = harness.get_by_role_and_label(Role::Label, "engine-project");
        }
        #[test]
        fn selected_imported_configuration_exposes_start_and_stop_actions() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Local label"));
            let mut harness = harness_for(view);
            harness.run();

            let _start = harness.get_by_role_and_label(Role::Button, "Start imported project");
            let _stop = harness.get_by_role_and_label(Role::Button, "Stop imported project");
        }
        #[test]
        fn lifecycle_confirmation_is_accessible_and_cancel_does_not_create_a_task() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Local label"));
            let mut harness = harness_for(view);
            harness.run();
            harness
                .get_by_role_and_label(Role::Button, "Start imported project")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.start_imported);
            assert!(
                harness
                    .state_mut()
                    .view
                    .request_lifecycle_confirmation(ComposeLifecycleAction::Start)
            );
            harness.step();
            let _label =
                harness.get_by_role_and_label(Role::Label, "Imported local label: Local label");
            let _files = harness.get_by_role_and_label(Role::Label, "Ordered files: compose.yaml");
            let _profiles = harness.get_by_role_and_label(Role::Label, "Profiles: default");
            let _confirm = harness.get_by_role_and_label(Role::Button, "Confirm start");
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Start imported project")
                    .accesskit_node()
                    .is_disabled()
            );
            harness
                .get_by_role_and_label(Role::Button, "Cancel lifecycle action")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.cancel_lifecycle);
            assert!(
                harness
                    .state_mut()
                    .view
                    .cancel_lifecycle_confirmation()
                    .is_some()
            );
            assert!(harness.state_mut().view.confirm_lifecycle().is_none());
        }

        #[test]
        fn unavailable_engine_disables_lifecycle_confirmation() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Local label"));
            assert!(view.request_lifecycle_confirmation(ComposeLifecycleAction::Start));
            let mut harness = harness_for_availability(view, false);
            harness.run();

            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Confirm start")
                    .accesskit_node()
                    .is_disabled()
            );
            let _notice = harness.get_by_role_and_label(
                Role::Label,
                "The engine or Compose configuration is not ready; reconnect or wait before confirming.",
            );
        }

        #[test]
        fn status_refresh_is_disabled_during_running_lifecycle_operation() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Local label"));
            assert!(view.request_lifecycle_confirmation(ComposeLifecycleAction::Start));
            assert!(view.confirm_lifecycle().is_some());
            let mut harness = harness_for(view);
            harness.run();

            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Refresh existing containers")
                    .accesskit_node()
                    .is_disabled()
            );
        }
        #[test]
        fn lifecycle_confirmation_requires_imported_revalidated_selection_and_survives_page_invalidation()
         {
            let mut view = validated_compose_view();
            assert!(!view.request_lifecycle_confirmation(ComposeLifecycleAction::Start));
            assert!(view.import_current_project("Local label"));
            assert!(view.request_lifecycle_confirmation(ComposeLifecycleAction::Start));
            assert!(view.lifecycle_confirmation_open());
            assert_eq!(
                view.lifecycle_confirmation()
                    .map(|confirmation| confirmation.selection.imported_label.as_str()),
                Some("Local label")
            );
            assert!(view.cancel_lifecycle_confirmation().is_some());
            assert!(view.cancel_lifecycle_confirmation().is_none());
            assert!(view.request_lifecycle_confirmation(ComposeLifecycleAction::Start));
            let task = view
                .confirm_lifecycle()
                .expect("the imported, validated selection should be actionable");
            assert!(!view.request_lifecycle_confirmation(ComposeLifecycleAction::Stop));
            assert!(
                view.begin_status_refresh().is_none(),
                "status reconciliation must wait until lifecycle completion"
            );

            view.invalidate();
            assert!(view.finish_lifecycle(
                task.generation,
                ComposeLifecycleResult {
                    outcome: Ok(()),
                    submitted: true,
                },
            ));
            assert!(matches!(
                &view.lifecycle,
                ComposeLifecycleState::Succeeded {
                    action: ComposeLifecycleAction::Start,
                    ..
                }
            ));
        }

        #[test]
        fn page_invalidation_cancels_unconfirmed_lifecycle_action() {
            let mut view = validated_compose_view();
            assert!(view.import_current_project("Local label"));
            assert!(view.request_lifecycle_confirmation(ComposeLifecycleAction::Stop));

            view.invalidate();

            assert!(!view.lifecycle_confirmation_open());
            assert!(view.confirm_lifecycle().is_none());
        }
        #[test]
        fn lifecycle_confirmation_rejects_changed_files_profiles_or_validation() {
            for change in 0..4 {
                let mut view = validated_compose_view();
                assert!(view.import_current_project("Local label"));
                assert!(view.request_lifecycle_confirmation(ComposeLifecycleAction::Stop));
                match change {
                    0 => view.selected_files[0] = std::path::PathBuf::from("/private/changed.yaml"),
                    1 => view.profile_input = "qa".to_owned(),
                    2 => view.selected_import_label = Some("Different label".to_owned()),
                    _ => view.profiles_changed(),
                }
                assert!(view.confirm_lifecycle().is_none());
                assert!(matches!(view.lifecycle, ComposeLifecycleState::Idle));
            }
        }
        #[test]
        fn import_and_selection_buttons_restore_session_configuration_without_running_commands() {
            let mut harness = harness_for(validated_compose_view());
            harness.run();
            harness
                .get_by_role_and_label(Role::TextInput, "Import name:")
                .focus();
            harness.step();
            harness
                .get_by_role_and_label(Role::TextInput, "Import name:")
                .type_text("Imported from UI");
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Import validated selection")
                .click_accesskit();
            harness.step();
            assert_eq!(harness.state().view.imported_projects.len(), 1);
            assert_eq!(
                harness.state().view.imported_projects[0].name,
                "Imported from UI"
            );

            harness
                .get_by_role_and_label(Role::Button, "Use imported configuration")
                .click_accesskit();
            harness.step();
            assert!(matches!(
                &harness.state().view.validation,
                ComposeValidationState::Idle
            ));
            assert!(!harness.state().actions.validate_files);
            assert!(!harness.state().actions.refresh_containers);
        }
        #[test]
        fn selecting_import_requests_cancellation_for_in_flight_compose_work() {
            let mut validation = validated_compose_view();
            assert!(validation.import_current_project("Saved"));
            validation.begin_override_selection();
            let mut harness = harness_for(validation);
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Use imported configuration")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.cancel_validation);

            let mut status = validated_compose_view();
            assert!(status.import_current_project("Saved status"));
            status.begin_status_refresh().expect("status can start");
            let mut harness = harness_for(status);
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Use imported configuration")
                .click_accesskit();
            harness.step();
            assert!(harness.state().actions.cancel_status);
            assert!(!harness.state().actions.cancel_logs);

            let mut logs = validated_compose_view();
            assert!(logs.import_current_project("Saved logs"));
            let (status_generation, _, _) = logs.begin_status_refresh().expect("status can start");
            assert!(logs.finish_status_refresh(
                status_generation,
                ComposeStatusResult {
                    containers: vec![ComposeContainer {
                        name: "api-1".to_owned(),
                        service: "api".to_owned(),
                        project: None,
                        state: "running".to_owned(),
                        health: None,
                    }],
                    outcome: Ok(()),
                },
            ));
            logs.begin_service_logs("api").expect("logs can start");
            let mut harness = harness_for(logs);
            harness.step();
            harness
                .get_by_role_and_label(Role::Button, "Use imported configuration")
                .click_accesskit();
            harness.step();
            assert!(!harness.state().actions.cancel_status);
            assert!(harness.state().actions.cancel_logs);
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "requires a local Docker-compatible Unix socket and Docker Compose; reads only"]
    async fn local_compose_discovery_smoke_is_read_only() {
        let engine = ferrum_engine::EngineClient::connect_default()
            .await
            .expect("the selected local engine should be accessible");
        let discovery = discover_projects(engine.local_socket_path())
            .await
            .expect("read-only Compose discovery should succeed");

        assert!(
            !discovery.version.is_empty()
                && discovery.version.len() <= MAX_VERSION_BYTES
                && discovery.version.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
                }),
            "the detected version should satisfy the bounded safe version grammar"
        );
        let mut names = std::collections::HashSet::new();
        for project in &discovery.projects {
            assert!(
                !project.name.is_empty(),
                "project identities must be present"
            );
            assert!(
                names.insert(project.name.as_str()),
                "project identities must be unique"
            );
            assert!(
                project
                    .name
                    .chars()
                    .chain(project.status.chars())
                    .all(|character| !ferrum_core::is_untrusted_display_character(character)),
                "displayed project text must be escaped"
            );
        }
    }
    #[test]
    fn compose_logs_result_debug_redacts_log_content() {
        let result = ComposeLogsResult {
            output: "compose-log-SECRET-SENTINEL".to_owned(),
            outcome: Ok(()),
        };
        let debug = format!("{result:?}");

        assert!(!debug.contains("compose-log-SECRET-SENTINEL"));
        assert!(debug.contains("output_bytes"));
    }

    #[test]
    fn service_selector_accepts_safe_compose_names_and_rejects_option_injection() {
        assert_eq!(validate_service_selector("api"), Some("api"));
        assert_eq!(
            validate_service_selector("api.v2_worker-1"),
            Some("api.v2_worker-1")
        );
        assert_eq!(validate_service_selector("--help"), None);
        assert_eq!(validate_service_selector("-dash"), None);
        assert_eq!(validate_service_selector("bad/name"), None);
        assert_eq!(validate_service_selector(&"a".repeat(129)), None);
        assert_eq!(validate_service_selector(""), None);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn compose_log_sanitization_bounds_lines_and_rendered_bytes() {
        let output = (0..201)
            .map(|line| format!("line-{line}\n"))
            .collect::<String>();
        let sanitized = sanitize_compose_logs(output.as_bytes()).unwrap();
        assert_eq!(sanitized.lines().count(), 200);
        assert_eq!(
            sanitize_compose_logs("x".repeat(MAX_COMPOSE_LOG_LINE_BYTES + 1).as_bytes()),
            None,
        );
        let controls = "\u{0001}".repeat(1000);
        let large = std::iter::repeat_n(controls, 200)
            .collect::<Vec<_>>()
            .join("\n");
        let sanitized = sanitize_compose_logs(large.as_bytes()).unwrap();
        assert!(sanitized.len() <= MAX_COMPOSE_LOG_DISPLAY_BYTES);
    }
}
