use std::{
    env,
    ffi::{CStr, CString, OsStr, OsString},
    fs::{self, File},
    io::{Read, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::compose_selection::{
    MAX_COMPOSE_FILES, MAX_COMPOSE_REGISTRATION_LABEL_BYTES, MAX_COMPOSE_REGISTRATIONS,
    valid_profile_list,
};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 2;
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;
pub(super) const MAX_SAVED_COMPOSE_PROJECTS: usize = MAX_COMPOSE_REGISTRATIONS;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RegistryCredentialHelper {
    #[default]
    None,
    SecretService,
    Pass,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedComposeProject {
    pub(super) label: String,
    pub(super) files: Vec<String>,
    #[serde(default)]
    pub(super) profiles: Vec<String>,
}

pub(super) fn valid_saved_compose_project(project: &SavedComposeProject) -> bool {
    let label = project.label.trim();
    !label.is_empty()
        && label == project.label
        && label.len() <= MAX_COMPOSE_REGISTRATION_LABEL_BYTES
        && !project.files.is_empty()
        && project.files.len() <= MAX_COMPOSE_FILES
        && project.files.iter().enumerate().all(|(index, file)| {
            Path::new(file).is_absolute()
                && !file.as_bytes().contains(&0)
                && !project.files[..index]
                    .iter()
                    .any(|previous| previous == file)
        })
        && valid_profile_list(&project.profiles)
}

fn valid_saved_compose_projects(projects: &[SavedComposeProject]) -> bool {
    projects.len() <= MAX_SAVED_COMPOSE_PROJECTS
        && projects.iter().enumerate().all(|(index, project)| {
            valid_saved_compose_project(project)
                && !projects[..index]
                    .iter()
                    .any(|previous| previous.label == project.label)
        })
}

// Keep records untyped until each can be checked independently; one malformed
// registration must not discard unrelated settings or valid saved entries.
fn parse_saved_compose_projects(value: Option<toml::Value>) -> (Vec<SavedComposeProject>, bool) {
    let Some(value) = value else {
        return (Vec::new(), false);
    };
    let Some(records) = value.as_array() else {
        return (Vec::new(), true);
    };
    let mut projects: Vec<SavedComposeProject> =
        Vec::with_capacity(records.len().min(MAX_SAVED_COMPOSE_PROJECTS));
    let mut warning = false;
    for record in records {
        if projects.len() >= MAX_SAVED_COMPOSE_PROJECTS {
            warning = true;
            break;
        }
        let Ok(project) = record.clone().try_into::<SavedComposeProject>() else {
            warning = true;
            continue;
        };
        if valid_saved_compose_project(&project)
            && !projects
                .iter()
                .any(|existing| existing.label == project.label)
        {
            projects.push(project);
        } else {
            warning = true;
        }
    }
    (projects, warning)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    #[serde(default = "current_schema_version")]
    schema_version: u32,
    #[serde(default = "default_auto_connect")]
    pub(super) auto_connect_on_startup: bool,
    #[serde(default)]
    pub(super) registry_credential_helper: RegistryCredentialHelper,
    #[serde(default)]
    pub(super) saved_compose_projects: Vec<SavedComposeProject>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            auto_connect_on_startup: true,
            registry_credential_helper: RegistryCredentialHelper::None,
            saved_compose_projects: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocument {
    #[serde(default = "current_schema_version")]
    schema_version: u32,
    #[serde(default = "default_auto_connect")]
    auto_connect_on_startup: bool,
    #[serde(default)]
    registry_credential_helper: RegistryCredentialHelper,
    #[serde(default)]
    saved_compose_projects: Option<toml::Value>,
}

fn current_schema_version() -> u32 {
    SCHEMA_VERSION
}
fn default_auto_connect() -> bool {
    true
}

pub(super) struct SettingsStore {
    location: Option<Result<SettingsLocation, PathResolutionError>>,
}

impl SettingsStore {
    pub(super) fn new() -> Self {
        Self {
            location: config_path().map(|path| SettingsLocation::resolve(&path)),
        }
    }

    #[cfg(test)]
    pub(super) fn at(path: PathBuf) -> Self {
        Self {
            location: Some(SettingsLocation::resolve(&path)),
        }
    }

    pub(super) fn load(&self) -> (Settings, Option<SettingsError>) {
        let Some(location) = self.location.as_ref() else {
            return (Settings::default(), Some(SettingsError::Unavailable));
        };
        let location = match location {
            Ok(location) => location,
            Err(error) => return (Settings::default(), Some(error.load_error())),
        };
        let directory = match open_settings_directory(location, false) {
            Ok(Some(directory)) => directory,
            Ok(None) => return (Settings::default(), None),
            Err(DirectoryError::Invalid) => {
                return (Settings::default(), Some(SettingsError::Invalid));
            }
            Err(DirectoryError::Io) => {
                return (Settings::default(), Some(SettingsError::Read));
            }
        };
        if directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .is_err()
        {
            return (Settings::default(), Some(SettingsError::Read));
        }
        let file = match open_settings_file(&directory, &location.file_name) {
            Ok(Some(file)) => file,
            Ok(None) => return (Settings::default(), None),
            Err(error) => return (Settings::default(), Some(error)),
        };
        let mut content = String::new();
        let bytes_read = match file
            .take(MAX_SETTINGS_BYTES + 1)
            .read_to_string(&mut content)
        {
            Ok(bytes_read) => bytes_read,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                return (Settings::default(), Some(SettingsError::Invalid));
            }
            Err(_) => return (Settings::default(), Some(SettingsError::Read)),
        };
        if bytes_read as u64 > MAX_SETTINGS_BYTES {
            return (Settings::default(), Some(SettingsError::Invalid));
        }
        match toml::from_str::<SettingsDocument>(&content) {
            Ok(document) if document.schema_version <= SCHEMA_VERSION => {
                let SettingsDocument {
                    schema_version,
                    auto_connect_on_startup,
                    registry_credential_helper,
                    saved_compose_projects,
                } = document;
                let mut warning = false;
                let saved_compose_projects = if schema_version < SCHEMA_VERSION {
                    warning = saved_compose_projects.is_some();
                    None
                } else {
                    saved_compose_projects
                };
                let (saved_compose_projects, invalid_records) =
                    parse_saved_compose_projects(saved_compose_projects);
                warning |= invalid_records;
                (
                    Settings {
                        schema_version: SCHEMA_VERSION,
                        auto_connect_on_startup,
                        registry_credential_helper,
                        saved_compose_projects,
                    },
                    warning.then_some(SettingsError::InvalidComposeRegistration),
                )
            }
            _ => (Settings::default(), Some(SettingsError::Invalid)),
        }
    }

    pub(super) fn open_audit_file(&self) -> std::io::Result<File> {
        let location = self
            .location
            .as_ref()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?
            .as_ref()
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::Other))?;
        let directory = open_settings_directory(location, true)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
        let name = c"audit.jsonl";
        open_audit_file(&directory, name)
    }

    pub(super) fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        self.save_using(settings, rename_temp_at)
    }

    fn save_using(
        &self,
        settings: &Settings,
        replace: impl FnOnce(&File, &CStr, &CStr) -> std::io::Result<()>,
    ) -> Result<(), SettingsError> {
        if !valid_saved_compose_projects(&settings.saved_compose_projects) {
            return Err(SettingsError::InvalidComposeRegistration);
        }
        let location = self
            .location
            .as_ref()
            .ok_or(SettingsError::Unavailable)?
            .as_ref()
            .map_err(|error| error.save_error())?;
        let directory = open_settings_directory(location, true)
            .map_err(|_| SettingsError::Write)?
            .ok_or(SettingsError::Write)?;
        validate_settings_save_target(&directory, &location.file_name)?;
        directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| SettingsError::Write)?;
        let mut persisted = settings.clone();
        persisted.schema_version = SCHEMA_VERSION;
        let serialized = toml::to_string_pretty(&persisted).map_err(|_| SettingsError::Write)?;
        if serialized.len() as u64 > MAX_SETTINGS_BYTES {
            return Err(SettingsError::TooLarge);
        }
        let (temp_name, mut file) = create_temp(&directory)?;
        let result = (|| {
            file.write_all(serialized.as_bytes())
                .map_err(|_| SettingsError::Write)?;
            file.flush().map_err(|_| SettingsError::Write)?;
            file.sync_all().map_err(|_| SettingsError::Write)?;
            validate_settings_save_target(&directory, &location.file_name)?;
            replace(&directory, &temp_name, &location.file_name)
                .map_err(|_| SettingsError::Write)?;
            let directory_sync = directory.sync_all();
            if directory_sync.is_err() {
                tracing::warn!(
                    error_kind = "settings_directory_sync",
                    "Settings were saved but directory sync failed"
                );
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = unlink_temp_at(&directory, &temp_name);
        }
        result
    }
}

struct SettingsLocation {
    anchor: File,
    directories: Vec<OsString>,
    file_name: CString,
}

impl SettingsLocation {
    fn resolve(path: &Path) -> Result<Self, PathResolutionError> {
        if !path.is_absolute() {
            return Err(PathResolutionError::Unavailable);
        }
        let file_name = CString::new(
            path.file_name()
                .ok_or(PathResolutionError::Unavailable)?
                .as_bytes(),
        )
        .map_err(|_| PathResolutionError::Unavailable)?;
        let settings_directory = path.parent().ok_or(PathResolutionError::Unavailable)?;
        let final_directory = settings_directory
            .file_name()
            .ok_or(PathResolutionError::Unavailable)?
            .to_os_string();
        let ancestor_path = settings_directory
            .parent()
            .ok_or(PathResolutionError::Unavailable)?;

        let mut anchor = File::open("/").map_err(|_| PathResolutionError::Io)?;
        let mut directories = Vec::new();
        let mut missing_component_seen = false;
        for component in ancestor_path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Prefix(_) => return Err(PathResolutionError::Unavailable),
                Component::ParentDir => {
                    if missing_component_seen {
                        return Err(PathResolutionError::Unavailable);
                    }
                    anchor = open_directory_at(&anchor, OsStr::new(".."), false)
                        .map_err(|_| PathResolutionError::Io)?;
                }
                Component::Normal(name) if missing_component_seen => {
                    directories.push(name.to_os_string());
                }
                Component::Normal(name) => match open_directory_at(&anchor, name, false) {
                    Ok(directory) => anchor = directory,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        directories.push(name.to_os_string());
                        missing_component_seen = true;
                    }
                    Err(_) => return Err(PathResolutionError::Io),
                },
            }
        }
        directories.push(final_directory);

        Ok(Self {
            anchor,
            directories,
            file_name,
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum PathResolutionError {
    Unavailable,
    Io,
}

impl PathResolutionError {
    fn load_error(self) -> SettingsError {
        match self {
            Self::Unavailable => SettingsError::Unavailable,
            Self::Io => SettingsError::Read,
        }
    }

    fn save_error(self) -> SettingsError {
        match self {
            Self::Unavailable => SettingsError::Unavailable,
            Self::Io => SettingsError::Write,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum DirectoryError {
    Invalid,
    Io,
}

fn open_settings_directory(
    location: &SettingsLocation,
    create_missing: bool,
) -> Result<Option<File>, DirectoryError> {
    let mut directory = location
        .anchor
        .try_clone()
        .map_err(|_| DirectoryError::Io)?;
    for component in &location.directories {
        let child = match open_directory_at(&directory, component, true) {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create_missing => {
                let created = create_directory_at(&directory, component, 0o700)?;
                let child = open_directory_at(&directory, component, true)
                    .map_err(classify_directory_error)?;
                if created {
                    // Correct umask-filtered permissions through the opened descriptor.
                    child
                        .set_permissions(fs::Permissions::from_mode(0o700))
                        .map_err(classify_directory_error)?;
                }
                child
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(classify_directory_error(error)),
        };
        directory = child;
    }
    Ok(Some(directory))
}

fn open_directory_at(parent: &File, name: &OsStr, nofollow: bool) -> std::io::Result<File> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;
    let mut flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    if nofollow {
        flags |= libc::O_NOFOLLOW;
    }
    // SAFETY: `parent` is an open directory and `name` is one NUL-terminated
    // path component interpreted relative to that descriptor.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned a newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn create_directory_at(
    parent: &File,
    name: &OsStr,
    mode: libc::mode_t,
) -> Result<bool, DirectoryError> {
    let name = CString::new(name.as_bytes()).map_err(|_| DirectoryError::Invalid)?;
    // SAFETY: `parent` is an open directory and `name` is one NUL-terminated
    // path component interpreted relative to that descriptor.
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), mode) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        Ok(false)
    } else {
        Err(classify_directory_error(error))
    }
}

fn classify_directory_error(error: std::io::Error) -> DirectoryError {
    if error.raw_os_error() == Some(libc::ELOOP)
        || error.kind() == std::io::ErrorKind::NotADirectory
    {
        DirectoryError::Invalid
    } else {
        DirectoryError::Io
    }
}

fn open_settings_file(directory: &File, name: &CStr) -> Result<Option<File>, SettingsError> {
    // SAFETY: `directory` is an open directory and `name` is a single
    // NUL-terminated filename interpreted relative to that descriptor.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        if error.raw_os_error() == Some(libc::ELOOP) {
            return Err(SettingsError::Invalid);
        }
        return Err(SettingsError::Read);
    }
    // SAFETY: successful `openat` returned a newly owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(|_| SettingsError::Read)?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(SettingsError::Invalid);
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| SettingsError::Read)?;
    Ok(Some(file))
}

fn open_audit_file(directory: &File, name: &CStr) -> std::io::Result<File> {
    // SAFETY: `directory` is open and `name` is one NUL-terminated component.
    let create = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR
                | libc::O_APPEND
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC,
            0o600 as libc::mode_t,
        )
    };
    let (file, created) = if create >= 0 {
        // SAFETY: successful openat returned a newly owned descriptor.
        (unsafe { File::from_raw_fd(create) }, true)
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error);
        }
        // SAFETY: `directory` is open and `name` is one NUL-terminated component.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_APPEND
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: successful openat returned a newly owned descriptor.
        (unsafe { File::from_raw_fd(descriptor) }, false)
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    if created {
        directory.sync_all()?;
    }
    Ok(file)
}
fn validate_settings_save_target(directory: &File, name: &CStr) -> Result<(), SettingsError> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `directory` is an open directory, `name` is a single
    // NUL-terminated filename, and `fstatat` writes the status on success.
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(SettingsError::Write)
        };
    }
    // SAFETY: a successful `fstatat` initialized the metadata structure.
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT as libc::mode_t == libc::S_IFREG as libc::mode_t {
        Ok(())
    } else {
        Err(SettingsError::Write)
    }
}

fn create_temp(directory: &File) -> Result<(CString, File), SettingsError> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = CString::new(format!(".settings-{}-{sequence}.tmp", std::process::id()))
            .map_err(|_| SettingsError::Write)?;
        // SAFETY: `directory` is an open directory, `name` is NUL-terminated, and
        // the flags/mode are valid for creating a new private regular file.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::mode_t,
            )
        };
        if descriptor >= 0 {
            // SAFETY: a successful `openat` call returned a newly owned descriptor.
            let file = unsafe { File::from_raw_fd(descriptor) };
            if file
                .set_permissions(fs::Permissions::from_mode(0o600))
                .is_err()
            {
                let _ = unlink_temp_at(directory, &name);
                return Err(SettingsError::Write);
            }
            return Ok((name, file));
        }
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
            continue;
        }
        return Err(SettingsError::Write);
    }
    Err(SettingsError::Write)
}

fn rename_temp_at(directory: &File, temp_name: &CStr, file_name: &CStr) -> std::io::Result<()> {
    // SAFETY: the directory descriptor stays open and both names are valid NUL-terminated
    // single components, interpreted relative to that directory.
    let result = unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            temp_name.as_ptr(),
            directory.as_raw_fd(),
            file_name.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn unlink_temp_at(directory: &File, temp_name: &CStr) -> std::io::Result<()> {
    // SAFETY: the directory descriptor stays open and `temp_name` is a valid
    // NUL-terminated single component, interpreted relative to that directory.
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), temp_name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn config_path() -> Option<PathBuf> {
    config_path_from(
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn config_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(xdg) = xdg.filter(|path| path.is_absolute()) {
        return Some(xdg.join("ferrum/settings.toml"));
    }

    home.filter(|path| path.is_absolute())
        .map(|home| home.join(".config/ferrum/settings.toml"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsError {
    Unavailable,
    Read,
    Invalid,
    InvalidComposeRegistration,
    TooLarge,
    Write,
}

impl SettingsError {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Unavailable => {
                "Settings cannot be saved because no usable configuration directory is available."
            }
            Self::Read => "Settings could not be read; defaults are being used.",
            Self::Invalid => {
                "Settings are invalid or from a newer version; defaults are being used."
            }
            Self::InvalidComposeRegistration => {
                "Invalid saved Compose registrations were skipped on load or rejected on save."
            }
            Self::TooLarge => "Settings exceed the maximum supported file size.",
            Self::Write => "Settings could not be saved.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_credential_helper_setting_persists_only_the_selected_variant() {
        let settings = Settings {
            registry_credential_helper: RegistryCredentialHelper::Pass,
            ..Settings::default()
        };
        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("registry_credential_helper = \"pass\""));
        assert!(!serialized.contains("Secret"));
        let decoded: SettingsDocument = toml::from_str(&serialized).unwrap();
        assert_eq!(
            decoded.registry_credential_helper,
            RegistryCredentialHelper::Pass
        );
        let legacy: SettingsDocument = toml::from_str("auto_connect_on_startup = true").unwrap();
        assert_eq!(
            legacy.registry_credential_helper,
            RegistryCredentialHelper::None
        );
    }

    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn store() -> (PathBuf, SettingsStore) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("ferrum-settings-{}-{nonce}", std::process::id()));
        (dir.clone(), SettingsStore::at(dir.join("settings.toml")))
    }
    #[cfg(unix)]
    fn restrictive_umask_child_config() -> Option<PathBuf> {
        const CHILD: &str = "FERRUM_SETTINGS_UMASK_TEST_CHILD";
        const TEST_ROOT: &str = "FERRUM_SETTINGS_UMASK_TEST_ROOT";

        let marker = env::var(CHILD).ok()?;
        let (parent_pid, nonce) = marker.split_once(':')?;
        let parent_pid = parent_pid.parse::<u32>().ok()?;
        // SAFETY: `getppid` has no pointer arguments or memory-safety preconditions.
        let actual_parent_pid = unsafe { libc::getppid() as u32 };
        if parent_pid != actual_parent_pid || nonce.parse::<u128>().ok()? == 0 {
            return None;
        }

        let test_root = env::var_os(TEST_ROOT).map(PathBuf::from)?;
        if !test_root.is_absolute() || !test_root.starts_with(env::temp_dir()) {
            return None;
        }
        let test_root_metadata = fs::symlink_metadata(&test_root).ok()?;
        let existing_xdg_parent = test_root.join("existing-xdg-parent");
        let existing_xdg_parent_metadata = fs::symlink_metadata(&existing_xdg_parent).ok()?;
        if !test_root_metadata.file_type().is_dir()
            || !existing_xdg_parent_metadata.file_type().is_dir()
        {
            return None;
        }
        let root_prefix = format!("ferrum-settings-{parent_pid}-");
        if test_root
            .file_name()?
            .to_str()?
            .strip_prefix(&root_prefix)?
            != nonce
        {
            return None;
        }

        let config_home = test_root.join("existing-xdg-parent/missing-xdg-root");
        if env::var_os("XDG_CONFIG_HOME").as_deref() != Some(config_home.as_os_str())
            || env::var_os("HOME").as_deref() != Some(config_home.as_os_str())
        {
            return None;
        }
        match fs::symlink_metadata(&config_home) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return None,
        }
        Some(config_home)
    }

    #[test]
    fn config_path_uses_only_absolute_config_roots() {
        assert_eq!(
            config_path_from(
                Some(PathBuf::from("/tmp/ferrum-config")),
                Some(PathBuf::from("/home/ferrum")),
            ),
            Some(PathBuf::from("/tmp/ferrum-config/ferrum/settings.toml")),
        );
        assert_eq!(
            config_path_from(
                Some(PathBuf::from("relative")),
                Some(PathBuf::from("/home/ferrum")),
            ),
            Some(PathBuf::from("/home/ferrum/.config/ferrum/settings.toml")),
        );
        assert_eq!(
            config_path_from(None, Some(PathBuf::from("relative"))),
            None,
        );
    }

    #[test]
    fn missing_file_uses_defaults() {
        let (dir, store) = store();
        assert_eq!(store.load(), (Settings::default(), None));
        assert!(!dir.exists(), "load must not create missing directories");
    }
    #[cfg(unix)]
    #[test]
    fn loading_missing_settings_restricts_existing_directory_permissions() {
        let (dir, store) = store();
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let settings_path = dir.join("settings.toml");

        let result = store.load();
        let directory_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o7777;
        let settings_file_exists = settings_path.exists();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, (Settings::default(), None));
        assert_eq!(directory_mode, 0o700);
        assert!(
            !settings_file_exists,
            "loading must not create a settings file"
        );
    }

    #[test]
    fn save_and_reopen_persists_setting() {
        let (dir, store) = store();
        let settings = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load(), (settings, None));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_versionless_settings() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.toml"),
            "auto_connect_on_startup = false\n",
        )
        .unwrap();
        let (settings, error) = store.load();
        assert_eq!(error, None);
        assert!(!settings.auto_connect_on_startup);
        assert_eq!(settings.schema_version, SCHEMA_VERSION);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn migrates_v1_settings_with_empty_saved_compose_projects() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.toml"),
            "schema_version = 1\nauto_connect_on_startup = false\nregistry_credential_helper = 'pass'\n",
        )
        .unwrap();
        let (settings, error) = store.load();
        assert_eq!(error, None);
        assert!(!settings.auto_connect_on_startup);
        assert_eq!(settings.schema_version, SCHEMA_VERSION);
        assert_eq!(
            settings.registry_credential_helper,
            RegistryCredentialHelper::Pass
        );
        assert!(settings.saved_compose_projects.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saved_compose_registration_round_trips_through_settings_store() {
        let (dir, store) = store();
        let project = SavedComposeProject {
            label: "dev-stack".into(),
            files: vec!["/work/base.yml".into(), "/work/override.yaml".into()],
            profiles: vec!["dev".into(), "qa".into()],
        };
        let settings = Settings {
            saved_compose_projects: vec![project],
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load(), (settings, None));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_serialized_settings_do_not_replace_existing_file() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        let original = b"auto_connect_on_startup = false\n";
        fs::write(&path, original).unwrap();
        let settings = Settings {
            saved_compose_projects: vec![SavedComposeProject {
                label: "oversize".into(),
                files: vec![format!("/{}", "x".repeat(300_000))],
                profiles: Vec::new(),
            }],
            ..Settings::default()
        };
        assert_eq!(store.save(&settings), Err(SettingsError::TooLarge));
        assert_eq!(fs::read(&path).unwrap(), original);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn skips_invalid_saved_compose_records_with_static_warning() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.toml"),
            "schema_version = 2\n\
             auto_connect_on_startup = false\n\
             [[saved_compose_projects]]\n\
             label = 'valid'\n\
             files = ['/private/base.yaml', '/private/override.yml']\n\
             profiles = ['dev', 'qa']\n\
             [[saved_compose_projects]]\n\
             label = 'invalid-path'\n\
             files = ['relative.yaml']\n\
             profiles = []\n\
             [[saved_compose_projects]]\n\
             label = 'malformed-fields'\n\
             files = ['/private/malformed.yaml']\n\
             profiles = 7\n",
        )
        .unwrap();
        let (settings, warning) = store.load();
        assert!(!settings.auto_connect_on_startup);
        assert_eq!(warning, Some(SettingsError::InvalidComposeRegistration));
        assert_eq!(
            settings.saved_compose_projects,
            [SavedComposeProject {
                label: "valid".into(),
                files: vec!["/private/base.yaml".into(), "/private/override.yml".into()],
                profiles: vec!["dev".into(), "qa".into()],
            }]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_saved_compose_collection_does_not_discard_other_settings() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.toml"),
            "schema_version = 2\n\
             auto_connect_on_startup = false\n\
             registry_credential_helper = 'pass'\n\
             saved_compose_projects = 7\n",
        )
        .unwrap();

        let (settings, warning) = store.load();

        assert!(!settings.auto_connect_on_startup);
        assert_eq!(
            settings.registry_credential_helper,
            RegistryCredentialHelper::Pass
        );
        assert!(settings.saved_compose_projects.is_empty());
        assert_eq!(warning, Some(SettingsError::InvalidComposeRegistration));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saved_compose_records_enforce_registry_and_compose_bounds() {
        let valid = SavedComposeProject {
            label: "local stack".into(),
            files: vec!["/private/base.yaml".into(), "/private/override.yml".into()],
            profiles: vec!["dev".into(), "*".into()],
        };
        assert!(!valid_saved_compose_project(&valid));
        let valid = SavedComposeProject {
            profiles: vec!["dev".into(), "qa".into()],
            ..valid
        };
        assert!(valid_saved_compose_project(&valid));
        assert!(valid_saved_compose_projects(std::slice::from_ref(&valid)));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            label: " padded ".into(),
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            files: vec!["/private/base.yaml".into(), "/private/base.yaml".into()],
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            files: vec!["relative.yaml".into()],
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            profiles: vec!["bad profile".into()],
            ..valid.clone()
        }));
        let repeated = SavedComposeProject {
            label: "same".into(),
            files: vec!["/private/base.yaml".into()],
            profiles: vec![],
        };
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            label: "x".repeat(257),
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            files: (0..17)
                .map(|index| format!("/private/file-{index}.yaml"))
                .collect(),
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            profiles: vec!["dev".into(); 17],
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_project(&SavedComposeProject {
            profiles: vec!["x".repeat(65)],
            ..valid.clone()
        }));
        assert!(!valid_saved_compose_projects(&vec![
            repeated;
            MAX_SAVED_COMPOSE_PROJECTS
                + 1
        ]));
    }
    #[test]
    fn loading_existing_settings_restricts_file_permissions() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "auto_connect_on_startup = false\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let (settings, error) = store.load();

        assert_eq!(error, None);
        assert!(!settings.auto_connect_on_startup);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn loading_rejects_hardlinked_settings_without_changing_sibling_permissions() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        let sentinel = dir.join("sentinel");
        let sentinel_contents = b"auto_connect_on_startup = false\n";
        fs::create_dir_all(&dir).unwrap();
        fs::write(&sentinel, sentinel_contents).unwrap();
        fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o644)).unwrap();
        fs::hard_link(&sentinel, &path).unwrap();

        let result = store.load();
        let sentinel_after = fs::read(&sentinel).unwrap();
        let sentinel_mode = fs::metadata(&sentinel).unwrap().permissions().mode() & 0o777;
        let directory_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, (Settings::default(), Some(SettingsError::Invalid)));
        assert_eq!(sentinel_after, sentinel_contents);
        assert_eq!(sentinel_mode, 0o644);
        assert_eq!(directory_mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_settings_without_changing_target_permissions() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        let target = dir.join("other-file");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&target, "not Ferrum settings").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        assert_eq!(
            store.load(),
            (Settings::default(), Some(SettingsError::Invalid))
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_file_open_rejects_symlinks_and_nonregular_files() {
        let (dir, _store) = store();
        let path = dir.join("settings.toml");
        let target = dir.join("other-file");
        let directory = dir.join("settings-directory");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&target, "not Ferrum settings").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let directory_fd = File::open(&dir).unwrap();
        let settings_name = CString::new("settings.toml").unwrap();
        assert!(matches!(
            open_settings_file(&directory_fd, &settings_name),
            Err(SettingsError::Invalid)
        ));
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );

        fs::remove_file(&path).unwrap();
        fs::create_dir(&directory).unwrap();
        assert!(matches!(
            open_settings_file(&directory_fd, &CString::new("settings-directory").unwrap()),
            Err(SettingsError::Invalid)
        ));
        let _ = fs::remove_dir_all(dir);
    }
    #[cfg(unix)]
    #[test]
    fn loading_rejects_a_symlinked_settings_directory_without_changing_target_permissions() {
        let (dir, _) = store();
        let target_directory = dir.join("target");
        let settings_directory = dir.join("ferrum");
        let target_settings = target_directory.join("settings.toml");
        fs::create_dir_all(&target_directory).unwrap();
        fs::write(&target_settings, "auto_connect_on_startup = false\n").unwrap();
        fs::set_permissions(&target_settings, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&target_directory, &settings_directory).unwrap();
        let store = SettingsStore::at(settings_directory.join("settings.toml"));

        let result = store.load();
        let target_mode = fs::metadata(&target_settings).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, (Settings::default(), Some(SettingsError::Invalid)));
        assert_eq!(target_mode, 0o644);
    }
    #[cfg(unix)]
    #[test]
    fn saving_rejects_a_settings_file_symlink_without_changing_its_target() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        let target = dir.join("other-settings.toml");
        fs::create_dir_all(&dir).unwrap();
        let original = b"auto_connect_on_startup = true\n";
        fs::write(&target, original).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let settings = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        let result = store.save(&settings);
        let target_after = fs::read(&target).unwrap();
        let target_mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        let settings_entry_is_symlink = fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, Err(SettingsError::Write));
        assert_eq!(target_after, original);
        assert_eq!(target_mode, 0o644);
        assert!(settings_entry_is_symlink);
    }
    #[cfg(unix)]
    #[test]
    fn saving_rejects_a_non_regular_settings_entry_without_changing_it() {
        use std::os::unix::{fs::FileTypeExt, net::UnixListener};

        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.join("settings.toml");
        let listener = UnixListener::bind(&path).unwrap();

        let result = store.save(&Settings::default());
        let entry_is_socket = fs::symlink_metadata(&path).unwrap().file_type().is_socket();
        let directory_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        drop(listener);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, Err(SettingsError::Write));
        assert!(entry_is_socket);
        assert_eq!(directory_mode, 0o755);
    }
    #[cfg(unix)]
    #[test]
    fn atomic_save_does_not_follow_a_symlink_substituted_before_rename() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        let target = dir.join("other-settings.toml");
        fs::create_dir_all(&dir).unwrap();
        let original = b"auto_connect_on_startup = true\n";
        fs::write(&target, original).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let settings = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        let result = store.save_using(&settings, |directory, temp_name, settings_name| {
            std::os::unix::fs::symlink(&target, &path)?;
            rename_temp_at(directory, temp_name, settings_name)
        });
        let target_after = fs::read(&target).unwrap();
        let target_mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        let saved_metadata = fs::symlink_metadata(&path).unwrap();
        let saved_contents = fs::read(&path).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, Ok(()));
        assert_eq!(target_after, original);
        assert_eq!(target_mode, 0o644);
        assert!(saved_metadata.file_type().is_file());
        assert_eq!(
            saved_contents,
            toml::to_string_pretty(&settings).unwrap().as_bytes()
        );
    }

    #[cfg(unix)]
    #[test]
    fn configured_symlink_root_is_resolved_and_pinned_at_initialization() {
        let (root, _) = store();
        fs::create_dir_all(&root).unwrap();
        let real_root = root.join("real-config");
        let decoy_root = root.join("decoy-config");
        let configured_root = root.join("xdg-config");
        fs::create_dir(&real_root).unwrap();
        fs::create_dir(&decoy_root).unwrap();
        std::os::unix::fs::symlink(&real_root, &configured_root).unwrap();
        let store = SettingsStore::at(configured_root.join("ferrum/settings.toml"));
        let expected = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        fs::remove_file(&configured_root).unwrap();
        std::os::unix::fs::symlink(&decoy_root, &configured_root).unwrap();
        store.save(&expected).unwrap();

        assert_eq!(store.load(), (expected, None));
        assert!(real_root.join("ferrum/settings.toml").exists());
        assert!(!decoy_root.join("ferrum/settings.toml").exists());
        let _ = fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[test]
    fn save_rejects_a_symlink_added_to_a_missing_suffix_after_resolution() {
        let (root, _) = store();
        let configured_root = root.join("config");
        let decoy_root = root.join("decoy");
        let decoy_settings_directory = decoy_root.join("ferrum");
        let decoy_settings = decoy_settings_directory.join("settings.toml");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&decoy_settings_directory).unwrap();
        fs::set_permissions(&decoy_settings_directory, fs::Permissions::from_mode(0o755)).unwrap();
        let decoy_contents = b"preserve the replacement target";
        fs::write(&decoy_settings, decoy_contents).unwrap();
        fs::set_permissions(&decoy_settings, fs::Permissions::from_mode(0o644)).unwrap();

        let store = SettingsStore::at(configured_root.join("ferrum/settings.toml"));
        std::os::unix::fs::symlink(&decoy_root, &configured_root).unwrap();
        let result = store.save(&Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        });
        let decoy_mode = fs::metadata(&decoy_settings_directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let decoy_after = fs::read(&decoy_settings).unwrap();
        let _ = fs::remove_dir_all(&root);

        assert_eq!(result, Err(SettingsError::Write));
        assert_eq!(decoy_mode, 0o755);
        assert_eq!(decoy_after, decoy_contents);
    }

    #[cfg(unix)]
    #[test]
    fn replacing_an_anchored_parent_path_does_not_redirect_load_or_save() {
        let (anchor_path, _) = store();
        let anchor_name = anchor_path.file_name().unwrap().to_string_lossy();
        let moved_anchor = anchor_path.with_file_name(format!("{anchor_name}-moved"));
        let decoy_anchor = anchor_path.with_file_name(format!("{anchor_name}-decoy"));
        let settings_directory = anchor_path.join("ferrum");
        let settings_path = settings_directory.join("settings.toml");
        fs::create_dir_all(&settings_directory).unwrap();
        fs::create_dir_all(decoy_anchor.join("ferrum")).unwrap();
        fs::write(&settings_path, "auto_connect_on_startup = true\n").unwrap();
        let decoy_settings = decoy_anchor.join("ferrum/settings.toml");
        let decoy_contents = b"leave this settings file alone";
        fs::write(&decoy_settings, decoy_contents).unwrap();
        let store = SettingsStore::at(settings_path);
        let replacement = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        fs::rename(&anchor_path, &moved_anchor).unwrap();
        std::os::unix::fs::symlink(&decoy_anchor, &anchor_path).unwrap();
        assert_eq!(
            store.load(),
            (
                Settings {
                    auto_connect_on_startup: true,
                    ..Settings::default()
                },
                None
            )
        );
        store.save(&replacement).unwrap();

        assert_eq!(
            SettingsStore::at(moved_anchor.join("ferrum/settings.toml")).load(),
            (replacement, None)
        );
        assert_eq!(fs::read(decoy_settings).unwrap().as_slice(), decoy_contents);
        fs::remove_file(&anchor_path).unwrap();
        let _ = fs::remove_dir_all(moved_anchor);
        let _ = fs::remove_dir_all(decoy_anchor);
    }

    #[cfg(unix)]
    #[test]
    fn unresolved_parent_components_are_rejected_without_creating_directories() {
        let (root, _) = store();
        let path = root.join("missing/../ferrum/settings.toml");
        let store = SettingsStore::at(path);

        assert_eq!(
            store.load(),
            (Settings::default(), Some(SettingsError::Unavailable))
        );
        assert_eq!(
            store.save(&Settings::default()),
            Err(SettingsError::Unavailable)
        );
        assert!(!root.exists());
    }
    #[cfg(unix)]
    #[test]
    fn settings_location_rejects_dot_components_at_the_file_and_directory_leaves() {
        let (root, _) = store();
        fs::create_dir_all(&root).unwrap();
        let paths = [root.join("../settings.toml"), root.join("ferrum/..")];

        let results = paths.map(|path| SettingsLocation::resolve(&path));
        let _ = fs::remove_dir_all(&root);

        assert!(
            results
                .iter()
                .all(|result| matches!(result, Err(PathResolutionError::Unavailable)))
        );
    }

    #[test]
    fn rejects_future_schema_versions() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.toml"),
            "schema_version = 99\nauto_connect_on_startup = false\n",
        )
        .unwrap();
        assert_eq!(
            store.load(),
            (Settings::default(), Some(SettingsError::Invalid))
        );
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn corrupt_file_falls_back_without_overwriting() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        let original = b"not = [ valid";
        fs::write(dir.join("settings.toml"), original).unwrap();
        assert_eq!(
            store.load(),
            (Settings::default(), Some(SettingsError::Invalid))
        );
        assert_eq!(fs::read(dir.join("settings.toml")).unwrap(), original);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unknown_secret_like_fields() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.toml"), "api_token = 'secret'\n").unwrap();
        assert_eq!(
            store.load(),
            (Settings::default(), Some(SettingsError::Invalid))
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saves_with_private_permissions_and_replaces_atomically() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "auto_connect_on_startup = true\n").unwrap();
        let settings = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load(), (settings, None));
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
    }
    #[cfg(unix)]
    #[test]
    fn saving_under_restrictive_umask_normalizes_only_new_directories() {
        const CHILD: &str = "FERRUM_SETTINGS_UMASK_TEST_CHILD";
        const TEST_ROOT: &str = "FERRUM_SETTINGS_UMASK_TEST_ROOT";
        if restrictive_umask_child_config().is_some() {
            // SAFETY: the marker and both config paths were validated for this child.
            unsafe {
                libc::umask(0o200 as libc::mode_t);
            }
            SettingsStore::new().save(&Settings::default()).unwrap();
            return;
        }

        let pid = std::process::id();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("ferrum-settings-{pid}-{nonce}"));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o750)).unwrap();
        let existing_ancestor = dir.join("existing-xdg-parent");
        fs::create_dir(&existing_ancestor).unwrap();
        fs::set_permissions(&existing_ancestor, fs::Permissions::from_mode(0o751)).unwrap();
        let config_home = existing_ancestor.join("missing-xdg-root");
        let settings_directory = config_home.join("ferrum");
        let settings_file = settings_directory.join("settings.toml");
        let home_fallback_file = config_home.join(".config/ferrum/settings.toml");
        let child_marker = format!("{pid}:{nonce}");

        let _subprocess_guard = crate::TEST_SUBPROCESS_LOCK.blocking_lock();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "settings::tests::saving_under_restrictive_umask_normalizes_only_new_directories",
            ])
            .env(CHILD, &child_marker)
            .env(TEST_ROOT, &dir)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &config_home)
            .status()
            .unwrap();

        let mode = |path: &Path| {
            fs::metadata(path)
                .ok()
                .map(|metadata| metadata.permissions().mode() & 0o7777)
        };
        let child_succeeded = status.success();
        let created_directory_modes = [mode(&config_home), mode(&settings_directory)];
        let settings_file_mode = mode(&settings_file);
        let pre_existing_ancestor_modes = [mode(&dir), mode(&existing_ancestor)];
        let home_fallback_file_exists = home_fallback_file.exists();
        let umask_probe = dir.join("umask-probe");
        fs::create_dir(&umask_probe).unwrap();
        let umask_probe_mode = fs::metadata(&umask_probe).unwrap().permissions().mode() & 0o7777;
        let _ = fs::remove_dir_all(&dir);

        assert!(child_succeeded, "settings save failed under umask 0200");
        assert_eq!(
            created_directory_modes,
            [Some(0o700), Some(0o700)],
            "each directory created by Ferrum must be exactly 0700"
        );
        assert_eq!(
            settings_file_mode,
            Some(0o600),
            "settings file at XDG_CONFIG_HOME/ferrum/settings.toml must be exactly 0600"
        );
        assert!(
            !home_fallback_file_exists,
            "the HOME/.config fallback must not be used when XDG_CONFIG_HOME is set"
        );
        assert_eq!(
            pre_existing_ancestor_modes,
            [Some(0o750), Some(0o751)],
            "pre-existing ancestors must retain their modes"
        );
        println!("FERRUM_SETTINGS_UMASK_TEST_PROBE={umask_probe_mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn inherited_umask_child_marker_cannot_touch_ambient_settings_or_umask() {
        const CHILD: &str = "FERRUM_SETTINGS_UMASK_TEST_CHILD";
        const TEST_ROOT: &str = "FERRUM_SETTINGS_UMASK_TEST_ROOT";

        let pid = std::process::id();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("ferrum-settings-{pid}-{nonce}"));
        fs::create_dir(&dir).unwrap();
        fs::create_dir(dir.join("existing-xdg-parent")).unwrap();
        let expected_config_home = dir.join("existing-xdg-parent/missing-xdg-root");
        fs::create_dir_all(&expected_config_home).unwrap();
        let malformed_config_home = dir.join("inherited-marker-config");
        fs::create_dir(&malformed_config_home).unwrap();
        let cases = [
            ("1".to_owned(), malformed_config_home),
            (format!("{pid}:{nonce}"), expected_config_home),
        ];
        use std::os::unix::process::CommandExt;

        for (marker, config_home) in cases {
            let _subprocess_guard = crate::TEST_SUBPROCESS_LOCK.blocking_lock();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "settings::tests::saving_under_restrictive_umask_normalizes_only_new_directories",
                    "--nocapture",
                ])
                .env(CHILD, marker)
                .env(TEST_ROOT, &dir)
                .env("XDG_CONFIG_HOME", &config_home)
                .env("HOME", &config_home);
            // SAFETY: the pre-exec closure only calls async-signal-safe `umask`.
            unsafe {
                command.pre_exec(|| {
                    libc::umask(0o027);
                    Ok(())
                });
            }

            let output = command.output().unwrap();
            let output_text = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "the inherited-marker test process failed: {output_text}"
            );
            assert!(
                !config_home.join("ferrum/settings.toml").exists(),
                "an inherited child marker must not save at ambient XDG/HOME"
            );
            assert!(
                output_text
                    .lines()
                    .any(|line| line == "FERRUM_SETTINGS_UMASK_TEST_PROBE=750"),
                "the inherited marker changed the test process umask: {output_text}"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn inherited_valid_marker_cannot_write_through_symlinked_test_roots() {
        const CHILD: &str = "FERRUM_SETTINGS_UMASK_TEST_CHILD";
        const TEST_ROOT: &str = "FERRUM_SETTINGS_UMASK_TEST_ROOT";

        let pid = std::process::id();
        use std::os::unix::process::CommandExt;

        for (index, symlink_test_root) in [true, false].into_iter().enumerate() {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                + index as u128;
            let test_root = env::temp_dir().join(format!("ferrum-settings-{pid}-{nonce}"));
            let target_root = env::temp_dir().join(format!("ferrum-settings-target-{pid}-{nonce}"));
            let target_parent = target_root.join("existing-xdg-parent");
            fs::create_dir_all(&target_parent).unwrap();
            let target_sentinel = target_root.join("sentinel");
            let sentinel_contents = b"preserve this sentinel";
            fs::write(&target_sentinel, sentinel_contents).unwrap();
            let target_sentinel_mode =
                fs::metadata(&target_sentinel).unwrap().permissions().mode() & 0o7777;
            if symlink_test_root {
                std::os::unix::fs::symlink(&target_root, &test_root).unwrap();
            } else {
                fs::create_dir(&test_root).unwrap();
                std::os::unix::fs::symlink(&target_parent, test_root.join("existing-xdg-parent"))
                    .unwrap();
            }

            let target_config_root = target_parent.join("missing-xdg-root");
            let config_home = test_root.join("existing-xdg-parent/missing-xdg-root");
            let target_settings = target_config_root.join("ferrum/settings.toml");
            let _subprocess_guard = crate::TEST_SUBPROCESS_LOCK.blocking_lock();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "settings::tests::saving_under_restrictive_umask_normalizes_only_new_directories",
                    "--nocapture",
                ])
                .env(CHILD, format!("{pid}:{nonce}"))
                .env(TEST_ROOT, &test_root)
                .env("XDG_CONFIG_HOME", &config_home)
                .env("HOME", &config_home);
            // SAFETY: the pre-exec closure only calls async-signal-safe `umask`.
            unsafe {
                command.pre_exec(|| {
                    libc::umask(0o027);
                    Ok(())
                });
            }

            let output = command.output().unwrap();
            let output_text = String::from_utf8_lossy(&output.stdout);
            let child_succeeded = output.status.success();
            let settings_were_written = target_settings.exists();
            let target_config_root_was_created = target_config_root.exists();
            let target_sentinel_unchanged = fs::read(&target_sentinel)
                .map(|contents| {
                    contents.as_slice() == sentinel_contents
                        && fs::metadata(&target_sentinel)
                            .map(|metadata| {
                                metadata.permissions().mode() & 0o7777 == target_sentinel_mode
                            })
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            let umask_unchanged = output_text
                .lines()
                .any(|line| line == "FERRUM_SETTINGS_UMASK_TEST_PROBE=750");
            if symlink_test_root {
                fs::remove_file(&test_root).unwrap();
            } else {
                fs::remove_dir_all(&test_root).unwrap();
            }
            let _ = fs::remove_dir_all(&target_root);

            assert!(
                child_succeeded,
                "the inherited-marker helper failed: {output_text}"
            );
            assert!(
                !settings_were_written,
                "an inherited marker must not save through a symlinked test root"
            );
            assert!(
                !target_config_root_was_created,
                "the symlink target's missing config root must remain missing"
            );
            assert!(
                target_sentinel_unchanged,
                "the symlink target sentinel must remain unchanged"
            );
            assert!(
                umask_unchanged,
                "the child must not enter umask mode and must preserve its 027 umask: {output_text}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn save_only_restricts_permissions_on_the_final_settings_directory() {
        let (config_root, _) = store();
        let settings_directory = config_root.join("ferrum");
        fs::create_dir_all(&settings_directory).unwrap();
        fs::set_permissions(&config_root, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&settings_directory, fs::Permissions::from_mode(0o755)).unwrap();
        let store = SettingsStore::at(settings_directory.join("settings.toml"));

        store.save(&Settings::default()).unwrap();

        assert_eq!(
            fs::metadata(&config_root).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(&settings_directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(settings_directory.join("settings.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(config_root);
    }

    #[test]
    fn rejecting_symlinked_settings_directory_preserves_target_and_writes_nothing() {
        let (dir, _) = store();
        let target_directory = dir.join("target");
        let settings_directory = dir.join("ferrum");
        let settings_path = settings_directory.join("settings.toml");
        fs::create_dir_all(&target_directory).unwrap();
        fs::set_permissions(&target_directory, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target_directory, &settings_directory).unwrap();
        let store = SettingsStore::at(settings_path);
        let settings = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        let result = store.save(&settings);
        let target_mode = fs::metadata(&target_directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let settings_file_exists = target_directory.join("settings.toml").exists();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, Err(SettingsError::Write));
        assert_eq!(target_mode, 0o755);
        assert!(!settings_file_exists);
    }

    #[cfg(unix)]
    #[test]
    fn replacing_settings_directory_path_does_not_redirect_save() {
        let (settings_directory, store) = store();
        let directory_name = settings_directory.file_name().unwrap().to_string_lossy();
        let moved_directory = settings_directory.with_file_name(format!("{directory_name}-moved"));
        let decoy_directory = settings_directory.with_file_name(format!("{directory_name}-decoy"));
        fs::create_dir_all(&settings_directory).unwrap();
        fs::create_dir_all(&decoy_directory).unwrap();
        let decoy_settings = decoy_directory.join("settings.toml");
        let decoy_contents = b"leave the replacement target untouched";
        fs::write(&decoy_settings, decoy_contents).unwrap();
        let replacement = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        let result = store.save_using(&replacement, |directory, temp_name, settings_name| {
            fs::rename(&settings_directory, &moved_directory)?;
            std::os::unix::fs::symlink(&decoy_directory, &settings_directory)?;
            rename_temp_at(directory, temp_name, settings_name)
        });
        let decoy_after = fs::read(&decoy_settings).ok();
        let relocated_settings = SettingsStore::at(moved_directory.join("settings.toml")).load();
        let moved_entries = fs::read_dir(&moved_directory)
            .map(|entries| entries.count())
            .ok();
        fs::remove_file(&settings_directory).unwrap();
        fs::remove_dir_all(&moved_directory).unwrap();
        fs::remove_dir_all(&decoy_directory).unwrap();

        assert_eq!(result, Ok(()));
        assert_eq!(decoy_after, Some(decoy_contents.to_vec()));
        assert_eq!(relocated_settings, (replacement, None));
        assert_eq!(moved_entries, Some(1));
    }

    #[cfg(unix)]
    #[test]
    fn temporary_file_stays_in_opened_settings_directory_after_path_replacement() {
        let (settings_directory, _) = store();
        let directory_name = settings_directory.file_name().unwrap().to_string_lossy();
        let moved_directory = settings_directory.with_file_name(format!("{directory_name}-moved"));
        let decoy_directory = settings_directory.with_file_name(format!("{directory_name}-decoy"));
        fs::create_dir_all(&settings_directory).unwrap();
        fs::create_dir_all(&decoy_directory).unwrap();
        let location =
            SettingsLocation::resolve(&settings_directory.join("settings.toml")).unwrap();
        let directory = open_settings_directory(&location, true).unwrap().unwrap();

        fs::rename(&settings_directory, &moved_directory).unwrap();
        std::os::unix::fs::symlink(&decoy_directory, &settings_directory).unwrap();
        let (temp_name, file) = create_temp(&directory).unwrap();
        let temp_name_for_paths = temp_name.to_str().unwrap();
        let created_in_moved = moved_directory.join(temp_name_for_paths).exists();
        let created_in_decoy = decoy_directory.join(temp_name_for_paths).exists();
        drop(file);
        drop(directory);
        let _ = fs::remove_file(moved_directory.join(temp_name_for_paths));
        let _ = fs::remove_file(decoy_directory.join(temp_name_for_paths));
        fs::remove_file(&settings_directory).unwrap();
        fs::remove_dir_all(&moved_directory).unwrap();
        fs::remove_dir_all(&decoy_directory).unwrap();

        assert!(created_in_moved);
        assert!(!created_in_decoy);
    }

    #[cfg(unix)]
    #[test]
    fn failed_save_cleans_temp_in_opened_directory_after_path_replacement() {
        let (settings_directory, store) = store();
        let directory_name = settings_directory.file_name().unwrap().to_string_lossy();
        let moved_directory = settings_directory.with_file_name(format!("{directory_name}-moved"));
        let decoy_directory = settings_directory.with_file_name(format!("{directory_name}-decoy"));
        fs::create_dir_all(&settings_directory).unwrap();
        fs::create_dir_all(&decoy_directory).unwrap();
        let settings_path = settings_directory.join("settings.toml");
        let original = b"auto_connect_on_startup = true\n";
        fs::write(&settings_path, original).unwrap();
        let replacement = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        let result = store.save_using(&replacement, |_, _, _| {
            fs::rename(&settings_directory, &moved_directory)?;
            std::os::unix::fs::symlink(&decoy_directory, &settings_directory)?;
            Err(std::io::Error::other("injected replacement failure"))
        });
        let original_after = fs::read(moved_directory.join("settings.toml")).ok();
        let moved_entries = fs::read_dir(&moved_directory)
            .map(|entries| entries.count())
            .ok();
        let decoy_entries = fs::read_dir(&decoy_directory)
            .map(|entries| entries.count())
            .ok();
        fs::remove_file(&settings_directory).unwrap();
        fs::remove_dir_all(&moved_directory).unwrap();
        fs::remove_dir_all(&decoy_directory).unwrap();

        assert_eq!(result, Err(SettingsError::Write));
        assert_eq!(original_after, Some(original.to_vec()));
        assert_eq!(moved_entries, Some(1));
        assert_eq!(decoy_entries, Some(0));
    }
    #[test]
    fn failed_atomic_replacement_preserves_previous_settings() {
        let (dir, store) = store();
        let path = dir.join("settings.toml");
        let original = b"auto_connect_on_startup = true\n";
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, original).unwrap();
        let replacement = Settings {
            auto_connect_on_startup: false,
            ..Settings::default()
        };

        let result = store.save_using(&replacement, |_, _, _| {
            Err(std::io::Error::other("injected replacement failure"))
        });

        assert_eq!(result, Err(SettingsError::Write));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(store.load(), (Settings::default(), None));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
    }
    #[cfg(unix)]
    #[test]
    fn audit_file_is_created_with_private_permissions() {
        let (dir, store) = store();

        let file = store.open_audit_file().expect("audit file should open");
        let metadata = file.metadata().unwrap();
        let file_mode = metadata.permissions().mode() & 0o777;
        let directory_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        drop(file);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(file_mode, 0o600);
        assert_eq!(directory_mode, 0o700);
    }
    #[cfg(unix)]
    #[test]
    fn audit_file_rejects_symlink_and_hardlink_targets() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        let sibling = dir.join("sibling");
        let contents = b"preserve this file";
        fs::write(&sibling, contents).unwrap();
        fs::set_permissions(&sibling, fs::Permissions::from_mode(0o644)).unwrap();

        let audit_path = dir.join("audit.jsonl");
        std::os::unix::fs::symlink(&sibling, &audit_path).unwrap();
        let symlink_result = store.open_audit_file();
        let symlink_target_contents = fs::read(&sibling).unwrap();
        let symlink_target_mode = fs::metadata(&sibling).unwrap().permissions().mode() & 0o777;
        fs::remove_file(&audit_path).unwrap();
        fs::hard_link(&sibling, &audit_path).unwrap();
        let hardlink_result = store.open_audit_file();
        let hardlink_sibling_contents = fs::read(&sibling).unwrap();
        let hardlink_sibling_mode = fs::metadata(&sibling).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_dir_all(&dir);

        assert!(symlink_result.is_err());
        assert_eq!(symlink_target_contents, contents);
        assert_eq!(symlink_target_mode, 0o644);
        assert!(hardlink_result.is_err());
        assert_eq!(hardlink_sibling_contents, contents);
        assert_eq!(hardlink_sibling_mode, 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn audit_file_rejects_non_regular_targets() {
        let (dir, store) = store();
        fs::create_dir_all(&dir).unwrap();
        let audit_path = dir.join("audit.jsonl");
        fs::create_dir(&audit_path).unwrap();

        let result = store.open_audit_file();
        let remains_directory = fs::symlink_metadata(&audit_path)
            .unwrap()
            .file_type()
            .is_dir();
        let _ = fs::remove_dir_all(&dir);

        assert!(result.is_err());
        assert!(remains_directory);
    }
}
