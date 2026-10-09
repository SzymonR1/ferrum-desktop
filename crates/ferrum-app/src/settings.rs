use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
const MAX_SETTINGS_BYTES: u64 = 16 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    #[serde(default = "current_schema_version")]
    schema_version: u32,
    #[serde(default = "default_auto_connect")]
    pub(super) auto_connect_on_startup: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            auto_connect_on_startup: true,
        }
    }
}

fn current_schema_version() -> u32 {
    SCHEMA_VERSION
}
fn default_auto_connect() -> bool {
    true
}

#[derive(Debug)]
pub(super) struct SettingsStore {
    path: Option<PathBuf>,
}

impl SettingsStore {
    pub(super) fn new() -> Self {
        Self {
            path: config_path(),
        }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub(super) fn load(&self) -> (Settings, Option<SettingsError>) {
        let Some(path) = self.path.as_ref() else {
            return (Settings::default(), Some(SettingsError::Unavailable));
        };
        let file = match open_settings_file(path) {
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
        match toml::from_str::<Settings>(&content) {
            Ok(settings) if settings.schema_version <= SCHEMA_VERSION => {
                let mut settings = settings;
                settings.schema_version = SCHEMA_VERSION;
                (settings, None)
            }
            _ => (Settings::default(), Some(SettingsError::Invalid)),
        }
    }

    pub(super) fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        self.save_using(settings, |from, to| fs::rename(from, to))
    }

    fn save_using(
        &self,
        settings: &Settings,
        replace: impl FnOnce(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
    ) -> Result<(), SettingsError> {
        let path = self.path.as_ref().ok_or(SettingsError::Unavailable)?;
        let parent = path.parent().ok_or(SettingsError::Unavailable)?;
        fs::create_dir_all(parent).map_err(|_| SettingsError::Write)?;
        let directory = open_private_settings_directory(parent)?;
        let mut persisted = settings.clone();
        persisted.schema_version = SCHEMA_VERSION;
        let serialized = toml::to_string_pretty(&persisted).map_err(|_| SettingsError::Write)?;
        let (temp_path, mut file) = create_temp(parent)?;
        let result = (|| {
            file.write_all(serialized.as_bytes())
                .map_err(|_| SettingsError::Write)?;
            file.flush().map_err(|_| SettingsError::Write)?;
            file.sync_all().map_err(|_| SettingsError::Write)?;
            drop(file);
            replace(&temp_path, path).map_err(|_| SettingsError::Write)?;
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
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
}
fn open_settings_file(path: &std::path::Path) -> Result<Option<File>, SettingsError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            return Err(SettingsError::Invalid);
        }
        Err(_) => return Err(SettingsError::Read),
    };
    let metadata = file.metadata().map_err(|_| SettingsError::Read)?;
    if !metadata.file_type().is_file() {
        return Err(SettingsError::Invalid);
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| SettingsError::Read)?;
    Ok(Some(file))
}
fn open_private_settings_directory(parent: &std::path::Path) -> Result<File, SettingsError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let directory = options.open(parent).map_err(|_| SettingsError::Write)?;
    if !directory
        .metadata()
        .map_err(|_| SettingsError::Write)?
        .is_dir()
    {
        return Err(SettingsError::Write);
    }
    directory
        .set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(|_| SettingsError::Write)?;
    Ok(directory)
}

fn create_temp(parent: &std::path::Path) -> Result<(PathBuf, File), SettingsError> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".settings-{}-{sequence}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(SettingsError::Write),
        }
    }
    Err(SettingsError::Write)
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
            Self::Write => "Settings could not be saved.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let (_dir, store) = store();
        assert_eq!(store.load(), (Settings::default(), None));
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

        assert!(matches!(
            open_settings_file(&path),
            Err(SettingsError::Invalid)
        ));
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );

        fs::remove_file(&path).unwrap();
        fs::create_dir(&directory).unwrap();
        assert!(matches!(
            open_settings_file(&directory),
            Err(SettingsError::Invalid)
        ));
        let _ = fs::remove_dir_all(dir);
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

        let result = store.save_using(&replacement, |_, _| {
            Err(std::io::Error::other("injected replacement failure"))
        });

        assert_eq!(result, Err(SettingsError::Write));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(store.load(), (Settings::default(), None));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
