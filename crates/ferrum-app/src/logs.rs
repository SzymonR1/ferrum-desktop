use crate::search::CaseInsensitiveQuery;
use ferrum_core::{ContainerLogLine, ContainerLogSource, is_untrusted_display_character};
use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::{io, path::Path};

const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;
const MAX_RETAINED_LINES: usize = 10_000;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
fn escaped_log_text_length(text: &str) -> Option<usize> {
    if text.len() > MAX_RETAINED_BYTES {
        return None;
    }
    let escaped_len = text.chars().try_fold(text.len(), |length, character| {
        if !is_untrusted_display_character(character) {
            return Some(length);
        }
        let replacement_len = if character.is_control() {
            4
        } else {
            4 + unicode_hex_digit_count(character as u32)
        };
        length.checked_add(replacement_len.checked_sub(character.len_utf8())?)
    })?;
    (escaped_len <= MAX_RETAINED_BYTES).then_some(escaped_len)
}

fn unicode_hex_digit_count(mut value: u32) -> usize {
    let mut digits = 1;
    while value >= 16 {
        value >>= 4;
        digits += 1;
    }
    digits
}

fn push_untrusted_log_character(output: &mut String, character: char) {
    let value = character as u32;
    if character.is_control() {
        output.push_str("\\x");
        output.push(char::from(HEX_DIGITS[((value >> 4) & 0xf) as usize]));
        output.push(char::from(HEX_DIGITS[(value & 0xf) as usize]));
        return;
    }

    output.push_str("\\u{");
    let mut digits = [0; 6];
    let mut start = digits.len();
    let mut remaining = value;
    loop {
        start -= 1;
        digits[start] = HEX_DIGITS[(remaining & 0xf) as usize];
        remaining >>= 4;
        if remaining == 0 {
            break;
        }
    }
    for digit in &digits[start..] {
        output.push(char::from(*digit));
    }
    output.push('}');
}

fn escape_untrusted_log_text(text: String, escaped_len: usize) -> String {
    if escaped_len == text.len() {
        return text;
    }
    let mut escaped = String::with_capacity(escaped_len);
    for character in text.chars() {
        if is_untrusted_display_character(character) {
            push_untrusted_log_character(&mut escaped, character);
        } else {
            escaped.push(character);
        }
    }
    escaped
}
#[cfg(target_os = "linux")]
/// Writes visible log output to the selected regular file by atomic replacement.
///
/// New files are private (`0600`). Existing regular files must be writable, and
/// their ordinary permission bits are preserved. Symlinks and non-regular
/// targets are rejected. Replacement affects only the selected directory entry,
/// so other hard links retain the previous contents.
pub(super) fn export_visible_logs(path: &Path, text: &str) -> io::Result<()> {
    export_visible_logs_using(path, text, rename_log_export_temp_at)
}

#[cfg(target_os = "linux")]
fn export_visible_logs_using(
    path: &Path,
    text: &str,
    replace: impl FnOnce(&std::fs::File, &std::ffi::CStr, &std::ffi::CStr) -> io::Result<()>,
) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    use std::{
        ffi::CString,
        io::Write as _,
        os::{fd::FromRawFd as _, unix::ffi::OsStrExt as _},
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "export target has no file name",
        )
    })?;
    if file_name.as_bytes() == b"." || file_name.as_bytes() == b".." {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export target has an invalid file name",
        ));
    }
    let target_name = CString::new(file_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid export target"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_name = CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid export directory"))?;
    // O_PATH avoids requiring directory read permission; entry changes still
    // require write and search access through the directory descriptor.
    // SAFETY: `parent_name` is a valid NUL-terminated path used only for this call.
    let descriptor = unsafe {
        libc::open(
            parent_name.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `open` returned a newly owned descriptor.
    let directory = unsafe { std::fs::File::from_raw_fd(descriptor) };
    let mode = open_existing_log_target_mode(&directory, &target_name)?;

    let (temp_name, mut temp_file) = create_log_export_temp(&directory, &target_name, || {
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    })?;
    let result = (|| {
        temp_file.write_all(text.as_bytes())?;
        temp_file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        temp_file.sync_all()?;
        replace(&directory, &temp_name, &target_name)
    })();
    drop(temp_file);
    if result.is_err() {
        let _ = unlink_log_export_temp(&directory, &temp_name);
    }
    result
}

#[cfg(target_os = "linux")]
fn open_existing_log_target_mode(
    directory: &std::fs::File,
    target_name: &std::ffi::CStr,
) -> io::Result<u32> {
    use std::os::{
        fd::{AsRawFd as _, FromRawFd as _},
        unix::fs::PermissionsExt as _,
    };

    // SAFETY: `directory` is an open directory and `target_name` is one
    // NUL-terminated filename interpreted relative to that descriptor.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            target_name.as_ptr(),
            libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(0o600);
        }
        return Err(error);
    }

    // SAFETY: a successful `openat` call returned a newly owned descriptor.
    let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export target is not a regular file",
        ));
    }
    Ok(metadata.permissions().mode() & 0o777)
}

#[cfg(target_os = "linux")]
fn create_log_export_temp(
    directory: &std::fs::File,
    target_name: &std::ffi::CStr,
    mut next_sequence: impl FnMut() -> u64,
) -> io::Result<(std::ffi::CString, std::fs::File)> {
    use std::os::unix::fs::PermissionsExt as _;
    use std::{
        ffi::CString,
        fs::File,
        os::fd::{AsRawFd as _, FromRawFd as _},
    };

    for _ in 0..32 {
        let sequence = next_sequence();
        let name = CString::new(format!(
            ".ferrum-log-export-{}-{sequence}.tmp",
            std::process::id()
        ))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid temporary name"))?;
        if name.as_c_str() == target_name {
            continue;
        }

        // SAFETY: `directory` is an open directory and `name` is one
        // NUL-terminated filename interpreted relative to that descriptor.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::mode_t,
            )
        };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(error);
        }

        // SAFETY: a successful `openat` call returned a newly owned descriptor.
        let file = unsafe { File::from_raw_fd(descriptor) };
        if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
            drop(file);
            let _ = unlink_log_export_temp(directory, &name);
            return Err(error);
        }
        return Ok((name, file));
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique log-export file",
    ))
}

#[cfg(target_os = "linux")]
fn rename_log_export_temp_at(
    directory: &std::fs::File,
    temp_name: &std::ffi::CStr,
    target_name: &std::ffi::CStr,
) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: `directory` is open and both C strings are single filename
    // components interpreted relative to that descriptor.
    let result = unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            temp_name.as_ptr(),
            directory.as_raw_fd(),
            target_name.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn unlink_log_export_temp(directory: &std::fs::File, temp_name: &std::ffi::CStr) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: `directory` is open and `temp_name` is one NUL-terminated
    // filename interpreted relative to that descriptor.
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), temp_name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Default)]
pub(super) struct LogBuffer {
    entries: VecDeque<ContainerLogLine>,
    visible_sequences: VecDeque<u64>,
    retained_bytes: usize,
    dropped_lines: usize,
    first_sequence: u64,
    next_sequence: u64,
    query: String,
    matcher: CaseInsensitiveQuery,
    stdout: bool,
    stderr: bool,
}

impl LogBuffer {
    pub(super) fn new() -> Self {
        Self {
            stdout: true,
            stderr: true,
            ..Self::default()
        }
    }

    pub(super) fn append(&mut self, mut line: ContainerLogLine) {
        let Some(escaped_len) = escaped_log_text_length(&line.text) else {
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            return;
        };
        line.text = escape_untrusted_log_text(std::mem::take(&mut line.text), escaped_len);
        let bytes = line.text.len();
        while self.entries.len() >= MAX_RETAINED_LINES
            || self.retained_bytes.saturating_add(bytes) > MAX_RETAINED_BYTES
        {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            self.retained_bytes -= oldest.text.len();
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            self.first_sequence = self.first_sequence.saturating_add(1);
            while self
                .visible_sequences
                .front()
                .is_some_and(|sequence| *sequence < self.first_sequence)
            {
                self.visible_sequences.pop_front();
            }
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let visible = self.is_visible(&line);
        self.retained_bytes += bytes;
        self.entries.push_back(line);
        if visible {
            self.visible_sequences.push_back(sequence);
        }
    }

    pub(super) fn set_filters(&mut self, query: &str, stdout: bool, stderr: bool) {
        if self.query == query && self.stdout == stdout && self.stderr == stderr {
            return;
        }
        self.query.clear();
        self.query.push_str(query);
        self.stdout = stdout;
        self.stderr = stderr;
        self.matcher.set(query);
        self.rebuild_visibility();
    }

    #[cfg(test)]
    pub(super) fn visible_indices(&self) -> Vec<usize> {
        self.visible_sequences
            .iter()
            .map(|sequence| (*sequence - self.first_sequence) as usize)
            .collect()
    }
    pub(super) fn visible_count(&self) -> usize {
        self.visible_sequences.len()
    }

    pub(super) fn visible_entry_at(&self, row: usize) -> Option<&ContainerLogLine> {
        let sequence = *self.visible_sequences.get(row)?;
        let index = usize::try_from(sequence.checked_sub(self.first_sequence)?).ok()?;
        self.entries.get(index)
    }

    pub(super) fn visible_text(&self) -> String {
        let visible_count = self.visible_count();
        let capacity = (0..visible_count)
            .filter_map(|row| self.visible_entry_at(row))
            .fold(visible_count.saturating_sub(1), |capacity, entry| {
                capacity
                    .saturating_add(source_label(&entry.source).len())
                    .saturating_add(2)
                    .saturating_add(entry.text.len())
            });
        let mut text = String::with_capacity(capacity);
        for row in 0..visible_count {
            if row > 0 {
                text.push('\n');
            }
            if let Some(entry) = self.visible_entry_at(row) {
                text.push_str(source_label(&entry.source));
                text.push_str(": ");
                text.push_str(&entry.text);
            }
        }
        text
    }

    pub(super) fn entries(&self) -> &VecDeque<ContainerLogLine> {
        &self.entries
    }

    #[cfg(test)]
    pub(super) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(super) fn dropped_lines(&self) -> usize {
        self.dropped_lines
    }

    pub(super) fn reset(&mut self) {
        self.entries.clear();
        self.visible_sequences.clear();
        self.retained_bytes = 0;
        self.dropped_lines = 0;
        self.first_sequence = self.next_sequence;
    }

    fn source_enabled(&self, source: &ContainerLogSource) -> bool {
        match source {
            ContainerLogSource::Stdout => self.stdout,
            ContainerLogSource::Stderr => self.stderr,
            ContainerLogSource::Combined => self.stdout || self.stderr,
        }
    }
    fn is_visible(&self, line: &ContainerLogLine) -> bool {
        self.source_enabled(&line.source) && self.matcher.matches(&line.text)
    }

    fn rebuild_visibility(&mut self) {
        self.visible_sequences.clear();
        for (offset, line) in self.entries.iter().enumerate() {
            if self.is_visible(line) {
                self.visible_sequences
                    .push_back(self.first_sequence + offset as u64);
            }
        }
    }
}

pub(super) fn source_label(source: &ContainerLogSource) -> &'static str {
    match source {
        ContainerLogSource::Stdout => "stdout",
        ContainerLogSource::Stderr => "stderr",
        ContainerLogSource::Combined => "combined",
    }
}
#[cfg(all(test, target_os = "linux"))]
mod export_tests {
    use super::{create_log_export_temp, export_visible_logs, export_visible_logs_using};
    use std::{
        fs,
        os::unix::fs::{MetadataExt as _, PermissionsExt as _},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "ferrum-log-export-{}-{}",
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("could not create export test directory: {error}"),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_private_file_with_requested_content() {
        let directory = TestDirectory::new();
        let path = directory.0.join("logs.txt");

        export_visible_logs(&path, "visible logs\n").expect("export should succeed");

        let metadata = fs::metadata(path).expect("exported file should exist");
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(
            fs::read(directory.0.join("logs.txt")).unwrap(),
            b"visible logs\n"
        );
    }

    #[test]
    fn preserves_permissions_of_existing_file() {
        let directory = TestDirectory::new();
        let path = directory.0.join("logs.txt");
        fs::write(&path, "old contents").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        export_visible_logs(&path, "replacement").expect("export should succeed");

        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o640);
        assert_eq!(fs::read(path).unwrap(), b"replacement");
    }

    #[test]
    fn exports_when_parent_is_write_and_search_only() {
        let directory = TestDirectory::new();
        let path = directory.0.join("logs.txt");
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o300)).unwrap();

        let result = export_visible_logs(&path, "visible logs\n");
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();

        result.expect("write and search access should be enough for atomic replacement");
        assert_eq!(fs::read(path).unwrap(), b"visible logs\n");
    }

    #[test]
    fn temporary_name_never_matches_selected_target() {
        let directory = TestDirectory::new();
        let directory_file = fs::File::open(&directory.0).unwrap();
        let sequence = [17, 18];
        let mut next_sequence = sequence.into_iter();
        let target_name =
            std::ffi::CString::new(format!(".ferrum-log-export-{}-17.tmp", std::process::id()))
                .unwrap();
        let target_path = directory
            .0
            .join(target_name.to_str().expect("generated name is UTF-8"));

        let (temp_name, temp_file) = create_log_export_temp(&directory_file, &target_name, || {
            next_sequence.next().expect("two names should be enough")
        })
        .expect("a distinct temporary name should be allocated");

        assert_ne!(temp_name, target_name);
        assert!(!target_path.exists());
        let temp_path = directory
            .0
            .join(temp_name.to_str().expect("generated name is UTF-8"));
        assert!(temp_path.exists());
        drop(temp_file);
        fs::remove_file(temp_path).unwrap();
    }

    #[test]
    fn exporting_hard_linked_path_does_not_modify_other_link() {
        let directory = TestDirectory::new();
        let path = directory.0.join("logs.txt");
        let alias = directory.0.join("other-name.txt");
        fs::write(&path, "original contents").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        fs::hard_link(&path, &alias).unwrap();

        export_visible_logs(&path, "replacement").expect("export should succeed");

        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        assert_eq!(fs::read(&alias).unwrap(), b"original contents");
        assert_eq!(
            fs::metadata(path).unwrap().mode() & 0o777,
            0o640,
            "replacement should preserve the selected file's ordinary mode bits"
        );
    }

    #[test]
    fn keeps_previous_export_and_cleans_temp_when_replacement_fails() {
        let directory = TestDirectory::new();
        let path = directory.0.join("logs.txt");
        fs::write(&path, "previous export").unwrap();

        let error = export_visible_logs_using(&path, "new export", |_, _, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "test replacement failure",
            ))
        })
        .expect_err("a failed replacement should be reported");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(&path).unwrap(), b"previous export");
        let entries = fs::read_dir(&directory.0)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1, "the temporary export should be removed");
        assert_eq!(entries[0].file_name(), "logs.txt");
    }

    #[test]
    fn refuses_to_replace_file_without_owner_write_permission() {
        let directory = TestDirectory::new();
        let path = directory.0.join("logs.txt");
        fs::write(&path, "original contents").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();

        let error = export_visible_logs(&path, "replacement")
            .expect_err("export should fail without owner write permission");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(path).unwrap(), b"original contents");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_without_modifying_target() {
        let directory = TestDirectory::new();
        let target = directory.0.join("target.txt");
        let link = directory.0.join("logs.txt");
        fs::write(&target, "do not change").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(export_visible_logs(&link, "replacement").is_err());

        assert_eq!(fs::read(target).unwrap(), b"do not change");
    }

    #[test]
    fn rejects_non_regular_file_before_writing() {
        use std::os::unix::{
            ffi::OsStrExt as _,
            fs::{FileTypeExt as _, OpenOptionsExt as _},
        };

        let directory = TestDirectory::new();
        let path = directory.0.join("pipe");
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c_path` provides a live NUL-terminated path pointer for this call,
        // and `0o600` is a plain permission mask.
        let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(
            result,
            0,
            "could not create FIFO: {}",
            std::io::Error::last_os_error()
        );
        let _reader = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .unwrap();

        assert!(export_visible_logs(&path, "replacement").is_err());
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_fifo());
    }
}
