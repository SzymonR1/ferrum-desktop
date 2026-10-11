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
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);

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
            | Self::ComposePluginUnavailable
            | Self::PrivateConfigSetupFailed => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposeValidationError {
    DockerCliMissing,
    ComposePluginUnavailable,
    PrivateConfigSetupFailed,
    InvalidSelectedFile,
    ValidationFailed,
}

impl ComposeValidationError {
    fn message(self) -> &'static str {
        match self {
            Self::DockerCliMissing => {
                "Docker CLI is unavailable. Install Docker CLI to validate Compose files."
            }
            Self::ComposePluginUnavailable => {
                "Docker Compose is unavailable. Install the Docker Compose plugin to validate Compose files."
            }
            Self::PrivateConfigSetupFailed => {
                "Compose validation could not start safely. Try again later."
            }
            Self::InvalidSelectedFile => "The selected Compose file could not be validated.",
            Self::ValidationFailed => "Compose file validation failed.",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposeValidationResult {
    pub(super) version: Option<String>,
    pub(super) outcome: Result<(), ComposeValidationError>,
}

impl ComposeValidationResult {
    fn failed(error: ComposeValidationError) -> Self {
        Self {
            version: None,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ComposeActions {
    pub(super) refresh: bool,
    pub(super) select_base_file: bool,
    pub(super) add_override_file: bool,
    pub(super) remove_override: Option<usize>,
    pub(super) validate_files: bool,
    pub(super) cancel_validation: bool,
}

const MAX_COMPOSE_FILES: usize = 16;

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
    selected_files: Vec<std::path::PathBuf>,
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

    pub(super) fn begin_file_selection(&mut self) -> u64 {
        self.begin_selection(false)
    }

    pub(super) fn begin_override_selection(&mut self) -> u64 {
        self.begin_selection(true)
    }

    fn begin_selection(&mut self, override_file: bool) -> u64 {
        self.validation_generation = self.validation_generation.wrapping_add(1);
        self.validation = ComposeValidationState::SelectingFile { override_file };
        self.validation_generation
    }

    pub(super) fn selected_files(&self) -> &[std::path::PathBuf] {
        &self.selected_files
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
        let file_name = display_compose_file_list(&self.selected_files);
        self.validation = ComposeValidationState::Validating { file_name };
        true
    }

    pub(super) fn remove_override(&mut self, index: usize) {
        if index > 0 && index < self.selected_files.len() {
            self.selected_files.remove(index);
            self.validation_generation = self.validation_generation.wrapping_add(1);
            self.validation = ComposeValidationState::Idle;
        }
    }
    pub(super) fn begin_validation(&mut self) -> Option<(u64, Vec<std::path::PathBuf>)> {
        if self.selected_files.is_empty()
            || self.is_file_selection_running()
            || self.is_validation_running()
        {
            return None;
        }
        self.validation_generation = self.validation_generation.wrapping_add(1);
        let generation = self.validation_generation;
        let file_name = display_compose_file_list(&self.selected_files);
        self.validation = ComposeValidationState::Validating { file_name };
        Some((generation, self.selected_files.clone()))
    }

    pub(super) fn cancel_validation(&mut self) {
        if let ComposeValidationState::Validating { file_name } =
            std::mem::take(&mut self.validation)
        {
            self.validation_generation = self.validation_generation.wrapping_add(1);
            self.validation = ComposeValidationState::Cancelled { file_name };
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.loading = false;
        self.loaded = false;
        self.version = None;
        self.projects.clear();
        self.error = None;
        self.validation_generation = self.validation_generation.wrapping_add(1);
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
        let selection_enabled = validation_enabled
            && !self.is_file_selection_running()
            && !self.is_validation_running();
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
                        selection_enabled,
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

#[cfg(target_os = "linux")]
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
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = command.spawn().map_err(|_| CommandFailure::Launch)?;
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
pub(super) async fn validate_compose_files(
    files: &[std::path::PathBuf],
) -> ComposeValidationResult {
    use std::env;
    if !selected_compose_files_are_valid(files) {
        return ComposeValidationResult::failed(ComposeValidationError::InvalidSelectedFile);
    }

    let Some(search_path) = env::var_os("PATH") else {
        return ComposeValidationResult::failed(ComposeValidationError::DockerCliMissing);
    };
    let Some(binary) = docker_binary(&search_path) else {
        return ComposeValidationResult::failed(ComposeValidationError::DockerCliMissing);
    };
    let user_compose_plugin = user_compose_plugin();
    validate_with_files_report(
        &binary,
        files,
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

#[cfg(target_os = "linux")]
async fn validate_with_files_report(
    binary: &std::path::Path,
    files: &[std::path::PathBuf],
    search_path: &std::ffi::OsStr,
    user_compose_plugin: Option<&std::path::Path>,
    deadline: Duration,
) -> ComposeValidationResult {
    use tokio::process::Command;

    if !selected_compose_files_are_valid(files) {
        return ComposeValidationResult::failed(ComposeValidationError::InvalidSelectedFile);
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
    command.args([
        "config",
        "--quiet",
        "--no-interpolate",
        "--no-env-resolution",
    ]);
    let outcome = match run_command_with_timeout(&mut command, deadline).await {
        Ok(_) => Ok(()),
        Err(CommandFailure::Launch) => Err(ComposeValidationError::DockerCliMissing),
        Err(
            CommandFailure::Rejected
            | CommandFailure::TimedOut
            | CommandFailure::OutputLimit
            | CommandFailure::Io,
        ) => Err(ComposeValidationError::ValidationFailed),
    };
    ComposeValidationResult {
        version: Some(version),
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

#[cfg(test)]
mod tests {
    use super::*;

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
                hang,
            } = options;
            let stdout = if stdout_chunks == 0 {
                String::new()
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
                     case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|PWD|SHLVL|_) : ;; *) exit 89 ;; esac\n\
                   done || exit 89\n\
                   printf '%s\\n%s' \"$DOCKER_CONFIG\" \"$HOME\" > {}\n\
                   printf '%s' {}\n\
                   exit {}\n\
                 fi\n\
                 [ \"$#\" -eq 9 ] || exit 80\n\
                 [ \"$1\" = compose ] && [ \"$2\" = --project-directory ] || exit 81\n\
                 [ \"$3\" = {} ] && [ \"$4\" = --file ] && [ \"$5\" = {} ] || exit 82\n\
                 [ \"$6\" = config ] && [ \"$7\" = --quiet ] || exit 83\n\
                 [ \"$8\" = --no-interpolate ] && [ \"$9\" = --no-env-resolution ] || exit 84\n\
                 [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] || exit 85\n\
                 [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = \"false\" ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = \"false\" ] || exit 91\n\
                 [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] || exit 86\n\
                 [ \"$HOME\" != \"$DOCKER_CONFIG\" ] && [ ! -e \"$DOCKER_CONFIG/config.json\" ] || exit 87\n\
                 [ \"$(cat {})\" = \"$(printf '%s\\n%s' \"$DOCKER_CONFIG\" \"$HOME\")\" ] || exit 90\n\
                 if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{DOCKER_CUSTOM_HEADERS+x}}${{DOCKER_DEFAULT_PLATFORM+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{ALL_PROXY+x}}${{FTP_PROXY+x}}${{http_proxy+x}}${{https_proxy+x}}${{no_proxy+x}}${{all_proxy+x}}${{ftp_proxy+x}}${{COMPOSE_FILE+x}}${{COMPOSE_ENV_FILES+x}}${{COMPOSE_PROFILES+x}}${{COMPOSE_PROJECT_NAME+x}}${{COMPOSE_PATH_SEPARATOR+x}}\" ]; then exit 88; fi\n\
                 env | while IFS='=' read -r name value; do\n\
                   case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_DISABLE_ENV_FILE|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|PWD|SHLVL|_) : ;; *) exit 89 ;; esac\n\
                 done || exit 89\n\
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
                 if [ \"$#\" -eq 3 ]; then [ \"$1\" = compose ] && [ \"$2\" = version ] && [ \"$3\" = --short ] || exit 70; printf 'v5.6.0\\n'; exit 0; fi\n\
                 [ \"$#\" -eq 13 ] || exit 71\n\
                 [ \"$1\" = compose ] && [ \"$2\" = --project-directory ] && [ \"$3\" = {} ] || exit 72\n\
                 [ \"$4\" = --file ] && [ \"$5\" = {} ] && [ \"$6\" = --file ] && [ \"$7\" = {} ] && [ \"$8\" = --file ] && [ \"$9\" = {} ] || exit 73\n\
                 [ \"${{10}}\" = config ] && [ \"${{11}}\" = --quiet ] && [ \"${{12}}\" = --no-interpolate ] && [ \"${{13}}\" = --no-env-resolution ] || exit 74\n\
                 [ \"$COMPOSE_DISABLE_ENV_FILE\" = 1 ] || exit 75\n\
                 [ \"$COMPOSE_EXPERIMENTAL_GIT_REMOTE\" = false ] && [ \"$COMPOSE_EXPERIMENTAL_OCI_REMOTE\" = false ] || exit 76\n\
                 [ -n \"$PATH\" ] && [ -d \"$HOME\" ] && [ -d \"$DOCKER_CONFIG\" ] && [ \"$HOME\" != \"$DOCKER_CONFIG\" ] || exit 77\n\
                 if [ -n \"${{DOCKER_HOST+x}}${{DOCKER_CONTEXT+x}}${{DOCKER_API_VERSION+x}}${{DOCKER_CERT_PATH+x}}${{DOCKER_TLS+x}}${{DOCKER_TLS_VERIFY+x}}${{HTTP_PROXY+x}}${{HTTPS_PROXY+x}}${{NO_PROXY+x}}${{COMPOSE_FILE+x}}${{COMPOSE_ENV_FILES+x}}\" ]; then exit 78; fi\n\
                 env | while IFS='=' read -r name value; do case \"$name\" in PATH|HOME|DOCKER_CONFIG|LANG|LC_ALL|COMPOSE_DISABLE_ENV_FILE|COMPOSE_EXPERIMENTAL_GIT_REMOTE|COMPOSE_EXPERIMENTAL_OCI_REMOTE|PWD|SHLVL|_) : ;; *) exit 79 ;; esac; done || exit 79\n\
                 exit 0\n",
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
                ValidationFixtureOptions::default(),
            );

            let result =
                validate_with_binary_report(&binary, &file, &search_path(), None, COMMAND_TIMEOUT)
                    .await;

            assert_eq!(result.version.as_deref(), Some("v5.6.0"));
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
            let result =
                validate_with_files(&binary, &files, &search_path(), None, COMMAND_TIMEOUT).await;
            assert!(result.is_ok());
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
                "services:\n  app:\n    image: busybox:latest\n    volumes:\n      - ./data:/data\n",
            )
            .expect("the controlled base Compose file should be written");
            fs::write(
                &override_file,
                "services:\n  app:\n    environment:\n      CONTROLLED: value\n",
            )
            .expect("the controlled override Compose file should be written");

            let files = [base, override_file];
            validate_compose_files(&files)
                .await
                .outcome
                .expect("the local Compose CLI should validate the temporary project without contacting an engine");
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
                    actions: ComposeActions::default(),
                },
            )
        }
        fn validation_result(
            outcome: Result<(), ComposeValidationError>,
        ) -> ComposeValidationResult {
            ComposeValidationResult {
                version: None,
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
            let (generation, files) = view
                .begin_validation()
                .expect("the remaining ordered file set should be revalidatable");
            assert_eq!(files, [base.to_path_buf(), second.to_path_buf()]);
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
}
