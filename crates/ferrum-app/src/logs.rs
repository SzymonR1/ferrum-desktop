use crate::search::CaseInsensitiveQuery;
use ferrum_core::{ContainerLogLine, ContainerLogSource};
use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::{io, path::Path};

const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;
const MAX_RETAINED_LINES: usize = 10_000;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
#[cfg(target_os = "linux")]
/// Writes UTF-8 log output without following the selected final path component.
pub(super) fn export_visible_logs(path: &Path, text: &str) -> io::Result<()> {
    use std::{fs::OpenOptions, io::Write as _, os::unix::fs::OpenOptionsExt as _};

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export target is not a regular file",
        ));
    }
    file.set_len(0)?;
    file.write_all(text.as_bytes())
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

    pub(super) fn append(&mut self, line: ContainerLogLine) {
        let bytes = line.text.len();
        if bytes > MAX_RETAINED_BYTES {
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            return;
        }
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
                let escaped_bytes = entry.text.chars().fold(entry.text.len(), |size, ch| {
                    if ch.is_control() {
                        size.saturating_add(4usize.saturating_sub(ch.len_utf8()))
                    } else {
                        size
                    }
                });
                capacity
                    .saturating_add(source_label(&entry.source).len())
                    .saturating_add(2)
                    .saturating_add(escaped_bytes)
            });
        let mut text = String::with_capacity(capacity);
        for row in 0..visible_count {
            if row > 0 {
                text.push('\n');
            }
            if let Some(entry) = self.visible_entry_at(row) {
                text.push_str(source_label(&entry.source));
                text.push_str(": ");
                for ch in entry.text.chars() {
                    if ch.is_control() {
                        text.push_str("\\x");
                        let value = ch as u32;
                        text.push(char::from(HEX_DIGITS[((value >> 4) & 0xf) as usize]));
                        text.push(char::from(HEX_DIGITS[(value & 0xf) as usize]));
                    } else {
                        text.push(ch);
                    }
                }
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
    use super::export_visible_logs;
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
