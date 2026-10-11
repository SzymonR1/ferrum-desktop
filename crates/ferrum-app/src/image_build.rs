use std::{
    ffi::{OsStr, OsString},
    fmt, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::{mpsc, oneshot},
};

const MAX_OUTPUT_LINE_BYTES: usize = 16 * 1024;
static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ImageBuildError {
    InvalidTarget,
    InvalidContext,
    DockerUnavailable,
    Failed,
    Cancelled,
}

impl fmt::Display for ImageBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTarget => "Enter a valid repository and Docker tag.",
            Self::InvalidContext => {
                "Choose a directory containing a regular root-level Dockerfile."
            }
            Self::DockerUnavailable => "Docker CLI with the Buildx plugin is unavailable.",
            Self::Failed => "Docker build failed; image state may have changed.",
            Self::Cancelled => "Docker build cancelled; image state may have changed.",
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct ImageBuildRequest {
    context: PathBuf,
    repository: String,
    tag: String,
}

impl ImageBuildRequest {
    pub(super) fn new(
        context: PathBuf,
        repository: String,
        tag: String,
    ) -> Result<Self, ImageBuildError> {
        if !valid_target(&repository, &tag) {
            return Err(ImageBuildError::InvalidTarget);
        }
        let context = validate_context(&context)?;
        Ok(Self::unchecked(context, repository, tag))
    }

    pub(super) fn unchecked(context: PathBuf, repository: String, tag: String) -> Self {
        Self {
            context,
            repository,
            tag,
        }
    }

    fn target(&self) -> String {
        format!("{}:{}", self.repository, self.tag)
    }
    pub(super) fn context(&self) -> &Path {
        &self.context
    }

    pub(super) fn repository(&self) -> &str {
        &self.repository
    }

    pub(super) fn tag(&self) -> &str {
        &self.tag
    }
}

pub(super) fn valid_target(repository: &str, tag: &str) -> bool {
    repository.len().saturating_add(1).saturating_add(tag.len()) <= 512
        && valid_repository(repository)
        && valid_tag(tag)
}
fn valid_repository(repository: &str) -> bool {
    !repository.is_empty()
        && repository.len() <= 510
        && repository.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
        && !repository.starts_with('/')
        && !repository.ends_with('/')
        && !repository.contains("//")
}

fn valid_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

pub(super) fn validate_context(context: &Path) -> Result<PathBuf, ImageBuildError> {
    let canonical = fs::canonicalize(context).map_err(|_| ImageBuildError::InvalidContext)?;
    if !canonical.is_dir() {
        return Err(ImageBuildError::InvalidContext);
    }
    let dockerfile = canonical.join("Dockerfile");
    let metadata = fs::symlink_metadata(dockerfile).map_err(|_| ImageBuildError::InvalidContext)?;
    if !metadata.file_type().is_file() {
        return Err(ImageBuildError::InvalidContext);
    }
    Ok(canonical)
}

pub(super) fn build_command(
    docker: &OsStr,
    request: &ImageBuildRequest,
    socket: &Path,
    config: &Path,
) -> Result<Command, ImageBuildError> {
    if socket.as_os_str().is_empty() || config.as_os_str().is_empty() {
        return Err(ImageBuildError::DockerUnavailable);
    }
    let mut command = Command::new(docker);
    let mut host = OsString::from("unix://");
    host.push(socket.as_os_str());
    command
        .arg("--config")
        .arg(config)
        .arg("--host")
        .arg(host)
        .args(["buildx", "build", "--progress=plain", "--load"])
        .arg(format!("--tag={}", request.target()))
        .args(["--file", "Dockerfile"])
        .arg(&request.context)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false)
        .process_group(0);
    let path = std::env::var_os("PATH").filter(|path| !path.is_empty());
    command.env_clear();
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command
        .env("HOME", config)
        .env("DOCKER_CONFIG", config)
        .env("XDG_CONFIG_HOME", config)
        .env("BUILDX_CONFIG", config);
    Ok(command)
}

pub(super) fn version_command(docker: &OsStr, config: &Path) -> Result<Command, ImageBuildError> {
    if config.as_os_str().is_empty() {
        return Err(ImageBuildError::DockerUnavailable);
    }
    let mut command = Command::new(docker);
    command
        .arg("--config")
        .arg(config)
        .args(["buildx", "version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let path = std::env::var_os("PATH").filter(|path| !path.is_empty());
    command.env_clear();
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command
        .env("HOME", config)
        .env("DOCKER_CONFIG", config)
        .env("XDG_CONFIG_HOME", config)
        .env("BUILDX_CONFIG", config);
    Ok(command)
}

pub(super) fn create_private_config() -> Result<PrivateConfig, ImageBuildError> {
    for _ in 0..32 {
        let id = NEXT_CONFIG.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ferrum-build-{}-{id}", std::process::id()));
        #[cfg(unix)]
        let created = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700).create(&path)
        };
        #[cfg(not(unix))]
        let created = fs::create_dir(&path);
        match created {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).is_err() {
                        let _ = fs::remove_dir(&path);
                        return Err(ImageBuildError::DockerUnavailable);
                    }
                }
                return Ok(PrivateConfig(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ImageBuildError::DockerUnavailable),
        }
    }
    Err(ImageBuildError::DockerUnavailable)
}

pub(super) struct PrivateConfig(PathBuf);

impl PrivateConfig {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PrivateConfig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ProcessGroupGuard(Option<i32>);

impl ProcessGroupGuard {
    fn signal(&self, signal: libc::c_int) {
        if let Some(pgid) = self.0 {
            // SAFETY: `pgid` is the positive PID of a child placed in its own
            // process group immediately before spawn; the negative PID targets only that group.
            unsafe {
                libc::kill(-pgid, signal);
            }
        }
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.signal(libc::SIGKILL);
    }
}

async fn send_output_line(
    line: &mut Vec<u8>,
    too_long: bool,
    sender: &mpsc::Sender<String>,
) -> Result<bool, ImageBuildError> {
    if too_long {
        const TRUNCATION_MARKER: &[u8] = b" [line truncated]";
        line.truncate(MAX_OUTPUT_LINE_BYTES - TRUNCATION_MARKER.len());
        line.extend_from_slice(TRUNCATION_MARKER);
    }
    let text = String::from_utf8_lossy(line).into_owned();
    let escaped =
        crate::display::escape_untrusted_display_text_with_limit(text, MAX_OUTPUT_LINE_BYTES)
            .unwrap_or_else(|| "[build output omitted]".to_owned());
    let sent = sender.send(escaped).await.is_ok();
    line.clear();
    Ok(sent)
}

async fn read_lines<R: AsyncRead + Unpin>(
    mut reader: R,
    sender: mpsc::Sender<String>,
) -> Result<(), ImageBuildError> {
    let mut line = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let mut too_long = false;
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|_| ImageBuildError::Failed)?;
        if count == 0 {
            if !line.is_empty() || too_long {
                send_output_line(&mut line, too_long, &sender).await?;
            }
            return Ok(());
        }
        for byte in &chunk[..count] {
            if *byte == b'\n' {
                if !send_output_line(&mut line, too_long, &sender).await? {
                    return Ok(());
                }
                too_long = false;
            } else if line.len() < MAX_OUTPUT_LINE_BYTES {
                line.push(*byte);
            } else {
                too_long = true;
            }
        }
    }
}

async fn terminate_group(child: &mut Child, group: &mut ProcessGroupGuard) {
    group.signal(libc::SIGTERM);
    tokio::time::sleep(Duration::from_millis(100)).await;
    group.signal(libc::SIGKILL);
    if matches!(
        tokio::time::timeout(Duration::from_millis(500), child.wait()).await,
        Ok(Ok(_))
    ) {
        group.disarm();
    }
}

pub(super) async fn run_command(
    mut command: Command,
    cancel: oneshot::Receiver<()>,
    output: mpsc::Sender<String>,
) -> Result<(), ImageBuildError> {
    command.process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ImageBuildError::DockerUnavailable
        } else {
            ImageBuildError::Failed
        }
    })?;
    let pid = child.id().ok_or(ImageBuildError::Failed)? as i32;
    let mut group = ProcessGroupGuard(Some(pid));
    let stdout = child.stdout.take().ok_or(ImageBuildError::Failed)?;
    let stderr = child.stderr.take().ok_or(ImageBuildError::Failed)?;
    let (reader_tx, mut reader_rx) = mpsc::channel::<Result<(), ImageBuildError>>(2);
    let stderr_reader_tx = reader_tx.clone();
    let stdout_tx = output.clone();
    let stdout_task = tokio::spawn(async move {
        let result = read_lines(stdout, stdout_tx).await;
        let _ = reader_tx.send(result).await;
    });
    let stderr_task = tokio::spawn(async move {
        let result = read_lines(stderr, output).await;
        let _ = stderr_reader_tx.send(result).await;
    });
    tokio::pin!(cancel);
    let status = tokio::select! {
        result = child.wait() => result.map_err(|_| ImageBuildError::Failed)?,
        _ = &mut cancel => {
            terminate_group(&mut child, &mut group).await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(ImageBuildError::Cancelled);
        }
    };
    group.disarm();
    let first = reader_rx
        .recv()
        .await
        .unwrap_or(Err(ImageBuildError::Failed));
    let second = reader_rx
        .recv()
        .await
        .unwrap_or(Err(ImageBuildError::Failed));
    stdout_task.await.map_err(|_| ImageBuildError::Failed)?;
    stderr_task.await.map_err(|_| ImageBuildError::Failed)?;
    first?;
    second?;
    if status.success() {
        Ok(())
    } else {
        Err(ImageBuildError::Failed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::{OsStr, OsString},
        fs,
        path::{Path, PathBuf},
        process::Stdio,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };
    use tokio::sync::{mpsc, oneshot};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("ferrum-image-build-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("create isolated temporary test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn private_build_config_is_owner_only_and_empty() {
        let config = create_private_config().expect("create private Docker config directory");
        let path = config.path().to_path_buf();
        assert!(path.is_dir());
        assert!(
            fs::read_dir(&path)
                .expect("read isolated config")
                .next()
                .is_none()
        );
        assert!(!path.join("config.json").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                0o700
            );
        }
        drop(config);
        assert!(!path.exists());
    }

    fn request(context: &Path) -> ImageBuildRequest {
        ImageBuildRequest::new(
            context.to_path_buf(),
            "example/service".to_owned(),
            "release_1".to_owned(),
        )
        .expect("valid synthetic image target")
    }

    #[test]
    fn build_request_rejects_invalid_context_and_target() {
        let root = TestDir::new();
        fs::write(root.path().join("Dockerfile"), "FROM scratch\n")
            .expect("write synthetic Dockerfile");
        assert!(
            ImageBuildRequest::new(
                root.path().to_path_buf(),
                "example/service".to_owned(),
                "release_1".to_owned(),
            )
            .is_ok()
        );
        assert!(
            ImageBuildRequest::new(
                root.path().to_path_buf(),
                "example/service".to_owned(),
                "x".repeat(129),
            )
            .is_err()
        );
        for repository in ["", "bad repository", "line\nbreak", "ctrl\u{7f}"] {
            assert!(
                ImageBuildRequest::new(
                    root.path().to_path_buf(),
                    repository.to_owned(),
                    "latest".to_owned(),
                )
                .is_err()
            );
        }
        assert!(
            ImageBuildRequest::new(root.path().to_path_buf(), "r".repeat(509), "tag".to_owned(),)
                .is_err()
        );
    }
    #[test]
    fn build_target_rejects_uppercase_repository_names() {
        assert!(!valid_target("Example/service", "latest"));
        assert!(!valid_target("example/Service", "latest"));
        assert!(valid_target("example/service", "latest"));
    }

    #[cfg(unix)]
    #[test]
    fn validate_context_requires_regular_root_dockerfile_and_rejects_symlink_or_missing_file() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new();
        assert!(matches!(
            validate_context(root.path()),
            Err(ImageBuildError::InvalidContext)
        ));
        let outside = root.path().join("outside");
        fs::write(&outside, "FROM scratch\n").expect("write synthetic Dockerfile");
        let context = root.path().join("context");
        fs::create_dir(&context).expect("create synthetic context");
        symlink(&outside, context.join("Dockerfile")).expect("create Dockerfile symlink");
        assert!(matches!(
            validate_context(&context),
            Err(ImageBuildError::InvalidContext)
        ));
        fs::remove_file(context.join("Dockerfile")).expect("remove symlink");
        fs::write(context.join("Dockerfile"), "FROM scratch\n").expect("write regular Dockerfile");
        assert_eq!(
            validate_context(&context).unwrap(),
            fs::canonicalize(&context).unwrap()
        );
        fs::remove_file(context.join("Dockerfile")).expect("remove Dockerfile");
        fs::create_dir(context.join("Dockerfile")).expect("create directory named Dockerfile");
        assert!(matches!(
            validate_context(&context),
            Err(ImageBuildError::InvalidContext)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn build_command_uses_local_socket_and_private_config() {
        let root = TestDir::new();
        let context = root.path().join("context");
        fs::create_dir(&context).unwrap();
        fs::write(context.join("Dockerfile"), "FROM scratch\n").unwrap();
        let config = root.path().join("empty-docker-config");
        fs::create_dir(&config).unwrap();
        let socket = Path::new("/run/user/1000/docker.sock");
        let command = build_command(OsStr::new("docker"), &request(&context), socket, &config)
            .expect("build command from explicit local inputs");
        let args: Vec<OsString> = command.as_std().get_args().map(OsStr::to_owned).collect();
        let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
        assert!(args.iter().any(|arg| *arg == OsStr::new("--host")));
        assert!(
            args.iter()
                .any(|arg| *arg == OsStr::new("unix:///run/user/1000/docker.sock"))
        );
        assert!(args.iter().any(|arg| *arg == OsStr::new("--config")));
        assert!(args.iter().any(|arg| *arg == config.as_os_str()));
        for required in [
            "buildx",
            "build",
            "--progress=plain",
            "--load",
            "--tag=example/service:release_1",
            "--file",
            "Dockerfile",
        ] {
            assert!(
                args.iter().any(|arg| *arg == OsStr::new(required)),
                "missing {required}"
            );
        }
        assert!(args.iter().any(|arg| *arg == context.as_os_str()));
        for forbidden in [
            "--push",
            "--secret",
            "--ssh",
            "--build-arg",
            "--privileged",
            "--authfile",
            "--builder",
            "--allow",
            "--cache-to",
            "--output",
            "--platform",
        ] {
            assert!(
                !args
                    .iter()
                    .any(|arg| arg.to_string_lossy().contains(forbidden))
            );
        }
        assert!(
            !args
                .iter()
                .any(|arg| arg.to_string_lossy().contains("password"))
        );
        let env: Vec<(OsString, Option<OsString>)> = command
            .as_std()
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
            .collect();
        assert!(
            env.iter()
                .any(|(key, value)| key == OsStr::new("DOCKER_CONFIG")
                    && value.as_deref() == Some(config.as_os_str()))
        );
        assert!(env.iter().any(|(key, value)| key == OsStr::new("HOME")
            && value.as_deref() == Some(config.as_os_str())));
        for forbidden in [
            "DOCKER_CONTEXT",
            "DOCKER_HOST",
            "DOCKER_TLS_VERIFY",
            "DOCKER_CERT_PATH",
            "DOCKER_API_VERSION",
            "DOCKER_CUSTOM_HEADERS",
            "DOCKER_DEFAULT_PLATFORM",
            "DOCKER_AUTH_CONFIG",
            "REGISTRY_AUTH_FILE",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "FTP_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "ftp_proxy",
            "no_proxy",
        ] {
            assert!(
                !env.iter()
                    .any(|(key, value)| key == OsStr::new(forbidden) && value.is_some()),
                "unexpected inherited {forbidden}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn build_process_streams_safe_bounded_output_and_reports_failure() {
        let (_cancel_tx, cancel_rx) = oneshot::channel();
        let (output_tx, mut output_rx) = mpsc::channel(32);
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("printf 'stdout\\033[31m\u{202e}\\n'; printf 'stderr\\007\\n' >&2; printf '%020000d\\n' 1")
            .stdin(Stdio::null());
        let result = run_command(command, cancel_rx, output_tx).await;
        assert!(result.is_ok());
        let mut lines = Vec::new();
        while let Some(line) = output_rx.recv().await {
            lines.push(line);
        }
        assert!(lines.iter().any(|line| line.contains("stdout")));
        assert!(lines.iter().any(|line| line.contains("stderr")));
        assert!(lines.iter().all(|line| line.len() <= 16 * 1024));
        assert!(lines.iter().all(|line| !line.chars().any(char::is_control)));
        assert!(lines.iter().all(|line| !line.contains('\u{202e}')));
        let combined = lines.join("\n");
        assert!(combined.contains("\\x1b"));
        assert!(combined.contains("\\u{202e}"));
        let (_cancel_tx, cancel_rx) = oneshot::channel();
        let (output_tx, _output_rx) = mpsc::channel(8);
        let mut command = tokio::process::Command::new("/bin/sh");
        command.arg("-c").arg("exit 7").stdin(Stdio::null());
        assert!(matches!(
            run_command(command, cancel_rx, output_tx).await,
            Err(ImageBuildError::Failed)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn build_cancellation_stops_the_process_group() {
        let root = TestDir::new();
        let marker = root.path().join("child-finished");
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("(sleep 1; printf child > \"$1\") & printf 'ready\\n'; wait")
            .arg("helper")
            .arg(&marker)
            .stdin(Stdio::null());
        let task = tokio::spawn(run_command(command, cancel_rx, output_tx));
        let ready_line = match tokio::time::timeout(Duration::from_secs(2), output_rx.recv()).await
        {
            Ok(Some(line)) => line,
            _ => {
                let _ = cancel_tx.send(());
                let _ = task.await;
                panic!("helper process did not report readiness");
            }
        };
        if !ready_line.contains("ready") {
            let _ = cancel_tx.send(());
            let _ = task.await;
            panic!("unexpected helper output before cancellation: {ready_line}");
        }
        cancel_tx
            .send(())
            .expect("runner still waiting for cancellation");
        assert!(matches!(
            task.await.unwrap(),
            Err(ImageBuildError::Cancelled)
        ));
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert!(
            !marker.exists(),
            "descendant process survived group cancellation"
        );
    }
}
