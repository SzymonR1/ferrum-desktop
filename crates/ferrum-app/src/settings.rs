use std::{
    env,
    ffi::{CStr, CString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{OpenOptionsExt, PermissionsExt},
        },
    },
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
        self.save_using(settings, rename_temp_at)
    }

    fn save_using(
        &self,
        settings: &Settings,
        replace: impl FnOnce(&File, &CStr, &CStr) -> std::io::Result<()>,
    ) -> Result<(), SettingsError> {
        let path = self.path.as_ref().ok_or(SettingsError::Unavailable)?;
        let file_name = settings_file_name(path)?;
        let parent = path.parent().ok_or(SettingsError::Unavailable)?;
        fs::create_dir_all(parent).map_err(|_| SettingsError::Write)?;
        let directory = open_private_settings_directory(parent)?;
        let mut persisted = settings.clone();
        persisted.schema_version = SCHEMA_VERSION;
        let serialized = toml::to_string_pretty(&persisted).map_err(|_| SettingsError::Write)?;
        let (temp_name, mut file) = create_temp(&directory)?;
        let result = (|| {
            file.write_all(serialized.as_bytes())
                .map_err(|_| SettingsError::Write)?;
            file.flush().map_err(|_| SettingsError::Write)?;
            file.sync_all().map_err(|_| SettingsError::Write)?;
            drop(file);
            replace(&directory, &temp_name, &file_name).map_err(|_| SettingsError::Write)?;
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

fn settings_file_name(path: &std::path::Path) -> Result<CString, SettingsError> {
    let name = path.file_name().ok_or(SettingsError::Unavailable)?;
    CString::new(name.as_bytes()).map_err(|_| SettingsError::Unavailable)
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
        let directory = open_private_settings_directory(&settings_directory).unwrap();

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
}
