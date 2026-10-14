use std::{ffi::OsStr, io, path::Path, process::Stdio, time::Duration};

use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use zeroize::Zeroizing;

use crate::{EngineError, settings::RegistryCredentialHelper};
use ferrum_engine::RegistryAuth;

const HELPER_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HELPER_OUTPUT: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperResponse {
    #[serde(rename = "ServerURL")]
    server_url: String,
    #[serde(rename = "Username", deserialize_with = "deserialize_secret")]
    username: Zeroizing<String>,
    #[serde(rename = "Secret", deserialize_with = "deserialize_secret")]
    secret: Zeroizing<String>,
}

fn deserialize_secret<'de, D>(deserializer: D) -> Result<Zeroizing<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Zeroizing::new)
}

fn parse_helper_response(
    output: &[u8],
    expected_server_url: &str,
) -> Result<HelperResponse, EngineError> {
    let response: HelperResponse =
        serde_json::from_slice(output).map_err(|_| EngineError::ImagePullCredentialUnavailable)?;
    if response.server_url != expected_server_url {
        return Err(EngineError::ImagePullCredentialUnavailable);
    }
    Ok(response)
}

struct RegistryServer {
    key: String,
}

/// Resolves a configured helper only after a one-operation opt-in is consumed.
pub(super) async fn authorization(
    helper: RegistryCredentialHelper,
    reference: &str,
) -> Result<RegistryAuth, EngineError> {
    let binary = match helper {
        RegistryCredentialHelper::None => {
            return Err(EngineError::ImagePullCredentialUnavailable);
        }
        RegistryCredentialHelper::SecretService => "docker-credential-secretservice",
        RegistryCredentialHelper::Pass => "docker-credential-pass",
    };
    authorization_with_binary(reference, Path::new(binary).as_os_str()).await
}
/// Resolves credentials for a push and maps helper failures to a push-specific,
/// pre-submission error.
pub(super) async fn authorization_for_push(
    helper: RegistryCredentialHelper,
    reference: &str,
) -> Result<RegistryAuth, EngineError> {
    match authorization(helper, reference).await {
        Err(EngineError::ImagePullCredentialUnavailable) => {
            Err(EngineError::ImagePushCredentialUnavailable)
        }
        result => result,
    }
}

async fn authorization_with_binary(
    reference: &str,
    binary: &OsStr,
) -> Result<RegistryAuth, EngineError> {
    let server = registry_server(reference).ok_or(EngineError::InvalidImageReference)?;
    let output = run_helper(binary, &server.key)
        .await
        .map_err(|_| EngineError::ImagePullCredentialUnavailable)?;
    let mut response = parse_helper_response(output.as_slice(), &server.key)?;
    if response.username.is_empty() || response.secret.is_empty() {
        return Err(EngineError::ImagePullCredentialUnavailable);
    }
    let username = std::mem::take(&mut *response.username);
    let secret = std::mem::take(&mut *response.secret);
    RegistryAuth::from_helper_credentials(username, secret, &server.key)
}

fn registry_server(reference: &str) -> Option<RegistryServer> {
    if reference.is_empty()
        || reference.len() > 512
        || reference
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return None;
    }
    let Some((first, repository)) = reference.split_once('/') else {
        return Some(RegistryServer {
            key: "https://index.docker.io/v1/".to_owned(),
        });
    };
    let registry_marker = first.contains('.')
        || first.contains(':')
        || first.starts_with('[')
        || first == "localhost";
    if !registry_marker {
        return Some(RegistryServer {
            key: "https://index.docker.io/v1/".to_owned(),
        });
    }
    if repository.is_empty() || repository.starts_with('/') {
        return None;
    }

    let authority = if first.starts_with('[') {
        let close = first.find(']')?;
        if first[1..close].parse::<std::net::Ipv6Addr>().is_err() {
            return None;
        }
        let suffix = &first[close + 1..];
        if !suffix.is_empty() && (!suffix.starts_with(':') || !valid_port(&suffix[1..])) {
            return None;
        }
        if first[close + 1..].contains(']') {
            return None;
        }
        first
    } else {
        if first.matches(':').count() > 1 {
            return None;
        }
        let (host, port) = first
            .rsplit_once(':')
            .map_or((first, None), |(host, port)| (host, Some(port)));
        if let Some(port) = port
            && !valid_port(port)
        {
            return None;
        }
        if !valid_dns_or_ipv4_host(host) {
            return None;
        }
        first
    };
    let is_hub = authority.eq_ignore_ascii_case("docker.io")
        || authority.eq_ignore_ascii_case("index.docker.io");
    let normalized = if is_hub {
        "https://index.docker.io/v1/".to_owned()
    } else {
        authority.to_ascii_lowercase()
    };
    Some(RegistryServer { key: normalized })
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|number| number != 0)
}

fn valid_dns_or_ipv4_host(host: &str) -> bool {
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

struct ProcessGroupGuard {
    id: libc::pid_t,
    active: bool,
}

impl ProcessGroupGuard {
    fn terminate(&mut self) {
        if self.active {
            // SAFETY: child was started as a new process group and this group
            // guard remains alive until its process has exited successfully.
            unsafe { libc::kill(-self.id, libc::SIGKILL) };
            self.active = false;
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

async fn run_helper(binary: &OsStr, server: &str) -> io::Result<Zeroizing<Vec<u8>>> {
    run_helper_with_deadline(binary, server, HELPER_TIMEOUT).await
}

async fn run_helper_with_deadline(
    binary: &OsStr,
    server: &str,
    deadline: Duration,
) -> io::Result<Zeroizing<Vec<u8>>> {
    #[cfg(test)]
    let _test_subprocess_guard = crate::TEST_SUBPROCESS_LOCK.lock().await;

    let mut command = Command::new(binary);
    command
        .arg("get")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    for key in [
        "PATH",
        "HOME",
        "PASSWORD_STORE_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "GNUPGHOME",
        "GPG_TTY",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn()?;
    let mut process_group = child.id().and_then(|id| {
        i32::try_from(id)
            .ok()
            .map(|id| ProcessGroupGuard { id, active: true })
    });
    let result = tokio::time::timeout(deadline, async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("stdin unavailable"))?;
        stdin.write_all(server.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.shutdown().await?;
        drop(stdin);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout unavailable"))?;
        let mut output = Zeroizing::new(Vec::with_capacity(1024));
        stdout
            .take((MAX_HELPER_OUTPUT + 1) as u64)
            .read_to_end(&mut output)
            .await?;
        if output.len() > MAX_HELPER_OUTPUT {
            return Err(io::Error::other("helper output bound"));
        }
        let status = child.wait().await?;
        if !status.success() {
            return Err(io::Error::other("helper failed"));
        }
        if let Some(group) = process_group.as_mut() {
            group.disarm();
        }
        Ok(output)
    })
    .await;
    match result {
        Ok(Ok(output)) => Ok(output),
        _ => {
            if let Some(group) = process_group.as_mut() {
                group.terminate();
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(io::Error::other("credential helper failed"))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
    /// Live loopback smoke for the fake-helper execution seam using synthetic Basic credentials.
    ///
    /// This checks helper invocation and Engine pull behavior only; it does not
    /// validate UI helper selection or consent, TLS, or production credentials.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "requires current local Docker Unix context with loopback insecure-registry support; fake-helper seam and synthetic Basic credentials only; no UI selection/consent, TLS, or production credentials"]
    async fn fake_helper_authenticates_generated_loopback_pull_through_local_engine() {
        use std::{
            sync::atomic::{AtomicU64, Ordering},
            time::{Duration, SystemTime, UNIX_EPOCH},
        };

        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use ferrum_engine::{ContextTransport, EngineClient, ImagePullStream};
        use ferrum_test_support::{DisposableImageTag, LivePullImageFixture, LoopbackRegistry};
        use zeroize::Zeroizing;

        const MAX_PROGRESS_RECORDS: usize = 1_024;
        static NEXT_LIVE_PULL: AtomicU64 = AtomicU64::new(0);

        let contexts = EngineClient::available_contexts()
            .await
            .expect("discover Docker contexts");
        let context = contexts
            .iter()
            .find(|context| context.is_current())
            .expect("Docker CLI should report its current context");
        assert_eq!(context.transport(), ContextTransport::LocalUnix);
        let context_name = context.name().to_owned();
        let engine = EngineClient::connect_context(&context_name)
            .await
            .expect("connect current local Docker context");
        use std::os::unix::fs::FileTypeExt;
        let socket_metadata = std::fs::metadata(engine.local_socket_path())
            .expect("current Docker context should resolve to an existing local socket");
        assert!(socket_metadata.file_type().is_socket());

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let sequence = NEXT_LIVE_PULL.fetch_add(1, Ordering::Relaxed);
        let nonce = format!("{}-{timestamp}-{sequence}", std::process::id());
        let repository = format!("ferrum-helper-smoke-{nonce}");
        let missing_tag = format!("missing-{nonce}");
        let username = Zeroizing::new(format!("ferrum-{nonce}"));
        let secret = Zeroizing::new(format!("synthetic-{nonce}"));
        let payload = Zeroizing::new(format!("{}:{}", username.as_str(), secret.as_str()));
        let encoded = Zeroizing::new(STANDARD.encode(payload.as_bytes()));
        let expected_authorization = Zeroizing::new(format!("Basic {}", encoded.as_str()));
        let registry = LoopbackRegistry::start(
            repository.clone(),
            nonce.clone(),
            missing_tag.clone(),
            LivePullImageFixture::new(&nonce),
            Some(expected_authorization),
        )
        .await;
        let registry_host = format!("127.0.0.1:{}", registry.port());
        let target_reference = format!("{registry_host}/{repository}:{nonce}");
        let missing_reference = format!("{registry_host}/{repository}:{missing_tag}");
        let helper_response = Zeroizing::new(format!(
            "{{\"ServerURL\":\"{registry_host}\",\"Username\":\"{}\",\"Secret\":\"{}\"}}",
            username.as_str(),
            secret.as_str(),
        ));
        let fake = FakeHelper::new(&helper_response);
        let auth = authorization_with_binary(&target_reference, fake.path.as_os_str())
            .await
            .expect("the fake-helper execution seam should return Engine auth");
        assert_eq!(
            fs::read_to_string(&fake.input).expect("fake-helper input should be recorded"),
            format!("{registry_host}\n"),
            "the helper should receive the exact generated registry authority"
        );

        let before = engine
            .list_images()
            .await
            .expect("preflight image inventory");
        assert!(!tag_is_present(&before, &target_reference));
        assert!(!tag_is_present(&before, &missing_reference));
        let missing_rejected = match engine
            .open_image_pull(&missing_reference, Some(&auth))
            .await
        {
            Err(_) => true,
            Ok(mut stream) => match drain_progress(&mut stream, MAX_PROGRESS_RECORDS).await {
                Ok((_, Some(_))) => true,
                Ok((_, None)) => false,
                Err(error) => panic!("missing-manifest probe exceeded bounds: {error}"),
            },
        };
        assert!(registry.missing_manifest_was_requested());
        assert!(registry.valid_authorization_was_observed());
        assert!(
            missing_rejected,
            "the Engine should reject the missing manifest"
        );
        let after_probe = engine
            .list_images()
            .await
            .expect("post-probe image inventory");
        assert!(!tag_is_present(&after_probe, &target_reference));
        assert!(!tag_is_present(&after_probe, &missing_reference));

        let mut stream = engine
            .open_image_pull(&target_reference, Some(&auth))
            .await
            .expect("real Engine should open the generated loopback pull");
        let progress = drain_progress(&mut stream, MAX_PROGRESS_RECORDS).await;
        drop(stream);
        let (saw_progress, stream_error) = progress.unwrap_or_else(|error| (false, Some(error)));
        let images = engine
            .list_images()
            .await
            .expect("post-pull image inventory");
        let mut matching_images = images
            .iter()
            .filter(|image| tag_is_present(std::slice::from_ref(*image), &target_reference));
        let Some(image) = matching_images.next() else {
            panic!(
                "pull should create the generated image tag; stream error: {}",
                stream_error.as_deref().unwrap_or("none")
            );
        };
        assert!(
            matching_images.next().is_none(),
            "ambiguous generated tag ownership must not trigger cleanup"
        );
        let mut cleanup = DisposableImageTag::new(
            engine.local_socket_path().to_owned(),
            target_reference.clone(),
        );
        assert!(
            cleanup.arm_if_verified(&image.id, &image.repo_tags),
            "cleanup requires a full SHA-256 inventory ID and exactly the generated tag"
        );
        let status = cleanup
            .remove()
            .expect("bounded verified-image-ID cleanup command should run");
        assert!(
            status.success(),
            "verified generated image should be removed without force"
        );
        let remaining = engine.list_images().await.expect("post-cleanup inventory");
        assert!(!tag_is_present(&remaining, &target_reference));
        cleanup.disarm();
        assert!(registry.valid_authorization_was_observed());
        if let Some(error) = stream_error {
            panic!("Engine pull progress stream should complete: {error}");
        }
        assert!(saw_progress, "Engine should report non-empty pull progress");

        async fn drain_progress(
            stream: &mut ImagePullStream,
            max_records: usize,
        ) -> Result<(bool, Option<String>), String> {
            let drain = tokio::time::timeout(Duration::from_secs(30), async {
                let mut records = 0;
                let mut saw_status = false;
                loop {
                    match stream.next_progress().await {
                        Ok(Some(update)) => {
                            records += 1;
                            if records > max_records {
                                return Err(format!("Engine pull exceeded {max_records} records"));
                            }
                            saw_status |= !update.status.trim().is_empty();
                        }
                        Ok(None) => return Ok((saw_status, None)),
                        Err(error) => return Ok((saw_status, Some(error.to_string()))),
                    }
                }
            })
            .await;
            match drain {
                Ok(result) => result,
                Err(_) => Err("Engine pull progress exceeded the 30-second deadline".to_owned()),
            }
        }

        fn tag_is_present(images: &[ferrum_core::ImageSummary], reference: &str) -> bool {
            images
                .iter()
                .any(|image| image.repo_tags.iter().any(|tag| tag == reference))
        }
    }

    pub(crate) struct FakeHelper {
        pub(crate) path: PathBuf,
        pub(crate) input: PathBuf,
        _directory: Option<tempfile::TempDir>,
    }

    impl FakeHelper {
        fn new(body: &str) -> Self {
            let directory = tempfile::Builder::new()
                .prefix("ferrum-credential-helper-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let directory_path = directory.path().to_owned();
            Self::new_named_at(body, &directory_path, "helper", Some(directory))
        }

        pub(crate) fn new_named_at(
            body: &str,
            directory_path: &Path,
            executable_name: &str,
            directory: Option<tempfile::TempDir>,
        ) -> Self {
            let path = directory_path.join(executable_name);
            let input = path.with_extension("input");
            let response = path.with_extension("response");
            write_private_file(&response, body.as_bytes(), 0o600);
            let script = b"#!/bin/sh\numask 077\ntest \"$1\" = get || exit 7\nIFS= read -r authority || exit 8\nprintf '%s\\n' \"$authority\" >> \"$0.input\"\nIFS= read -r response < \"$0.response\" || [ -n \"$response\" ] || exit 9\nprintf '%s' \"$response\"\n";
            write_private_file(&path, script, 0o700);
            Self {
                path,
                input,
                _directory: directory,
            }
        }

        fn from_script(script: &str) -> Self {
            let directory = tempfile::Builder::new()
                .prefix("ferrum-credential-helper-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let path = directory.path().join("helper");
            let input = path.with_extension("input");
            write_private_file(&path, script.as_bytes(), 0o700);
            Self {
                path,
                input,
                _directory: Some(directory),
            }
        }
    }

    fn write_private_file(path: &Path, contents: &[u8], mode: u32) {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
            .unwrap();
        file.write_all(contents).unwrap();
        file.set_permissions(fs::Permissions::from_mode(mode))
            .unwrap();
    }

    #[test]
    fn registry_addresses_normalize_hub_and_validate_authorities() {
        let hub = "https://index.docker.io/v1/";
        for reference in [
            "alpine:latest",
            "localhost",
            "project.example",
            "docker.io/team/app",
            "index.docker.io/team/app",
        ] {
            let server = registry_server(reference).unwrap();
            assert_eq!(server.key, hub);
        }
        assert_eq!(
            registry_server("registry.example:5000/team/app")
                .unwrap()
                .key,
            "registry.example:5000"
        );
        assert_eq!(
            registry_server("localhost:5000/team/app").unwrap().key,
            "localhost:5000"
        );
        assert_eq!(
            registry_server("[2001:db8::1]:5000/team/app").unwrap().key,
            "[2001:db8::1]:5000"
        );
        for invalid in [
            "registry.example:bad/team/app",
            "registry..example/app",
            "[2001:db8::1:5000/team/app",
            "registry.example:65536/app",
            "registry.example:/app",
            "registry:bad/team/app",
        ] {
            assert!(registry_server(invalid).is_none());
        }
    }

    #[test]
    fn fake_helper_files_are_private_and_tempdir_is_cleaned_on_drop() {
        let fake = FakeHelper::new(
            r#"{"ServerURL":"registry.example","Username":"synthetic-user","Secret":"synthetic-secret"}"#,
        );
        let directory = fake.path.parent().unwrap().to_owned();
        let response = fake.path.with_extension("response");
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&fake.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&response).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read(&response).unwrap(),
            br#"{"ServerURL":"registry.example","Username":"synthetic-user","Secret":"synthetic-secret"}"#
        );
        drop(fake);
        assert!(!directory.exists());
    }
    #[tokio::test]
    async fn helper_get_receives_exact_server_key_and_parses_credentials() {
        let fake = FakeHelper::new(
            r#"{"ServerURL":"https://index.docker.io/v1/","Username":"alice","Secret":"synthetic-secret"}"#,
        );
        let server = "https://index.docker.io/v1/";
        let result = run_helper(fake.path.as_os_str(), server).await.unwrap();
        assert_eq!(
            fs::read_to_string(&fake.input).unwrap(),
            format!("{server}\n")
        );
        let parsed = parse_helper_response(result.as_slice(), server).unwrap();
        assert_eq!(parsed.username.as_str(), "alice");
        assert_eq!(parsed.secret.as_str(), "synthetic-secret");
    }

    #[tokio::test]
    async fn helper_failure_oversize_timeout_and_protocol_errors_are_safe() {
        let failed = FakeHelper::from_script(
            "#!/bin/sh\ncat >/dev/null\nprintf '%s' 'helper-private-diagnostic' >&2\nexit 1\n",
        );
        let error = authorization_with_binary("private.example/team/app", failed.path.as_os_str())
            .await
            .err()
            .unwrap();
        assert!(matches!(
            &error,
            EngineError::ImagePullCredentialUnavailable
        ));
        assert!(!error.to_string().contains("helper-private-diagnostic"));

        let oversized = FakeHelper::new(&"x".repeat(MAX_HELPER_OUTPUT + 1));
        assert!(
            run_helper(oversized.path.as_os_str(), "registry.example")
                .await
                .is_err()
        );

        let slow = FakeHelper::from_script("#!/bin/sh\ncat >/dev/null\nsleep 10\n");
        assert!(
            run_helper_with_deadline(
                slow.path.as_os_str(),
                "registry.example",
                Duration::from_millis(20),
            )
            .await
            .is_err()
        );

        let protocol_error = parse_helper_response(
            b"{\"ServerURL\":\"https://registry.example\",\"Username\":\"user\",\"Secret\":\"private helper error text\"}",
            "https://different.example",
        )
        .err()
        .unwrap();
        assert!(
            !protocol_error
                .to_string()
                .contains("private helper error text")
        );
        let malformed = parse_helper_response(b"private helper output", "registry.example")
            .err()
            .unwrap();
        assert!(!malformed.to_string().contains("private helper output"));
    }

    #[tokio::test]
    async fn cancelling_helper_wait_kills_its_process_group() {
        let markers = tempfile::tempdir().unwrap();
        let started = markers.path().join("started");
        let finished = markers.path().join("finished");
        let script = format!(
            "#!/bin/sh\n(sleep 0.5; printf done > '{}') &\nprintf started > '{}'\ncat >/dev/null\nwait\n",
            finished.display(),
            started.display(),
        );
        let fake = FakeHelper::from_script(&script);
        let binary = fake.path.clone();
        let task = tokio::spawn(async move {
            run_helper_with_deadline(
                binary.as_os_str(),
                "registry.example",
                Duration::from_secs(5),
            )
            .await
        });
        let helper_started = tokio::time::timeout(Duration::from_secs(1), async {
            while !started.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(700)).await;

        assert!(helper_started);
        assert!(
            !finished.exists(),
            "a helper child outlived cancellation and wrote its marker"
        );
    }
    #[tokio::test]
    async fn fake_helper_token_response_builds_redacted_engine_auth() {
        let fake = FakeHelper::new(
            r#"{"ServerURL":"registry.example","Username":"<token>","Secret":"synthetic-token"}"#,
        );
        let auth = authorization_with_binary("registry.example/team/app", fake.path.as_os_str())
            .await
            .unwrap();
        assert_eq!(format!("{auth:?}"), "RegistryAuth([REDACTED])");
        assert_eq!(
            fs::read_to_string(&fake.input).unwrap(),
            "registry.example\n"
        );
    }

    #[tokio::test]
    async fn malformed_registry_authority_is_rejected_before_helper_invocation() {
        let fake = FakeHelper::new(
            r#"{"ServerURL":"https://index.docker.io/v1/","Username":"alice","Secret":"synthetic-secret"}"#,
        );
        let error =
            authorization_with_binary("registry.example:bad/team/app", fake.path.as_os_str())
                .await
                .err()
                .unwrap();
        assert!(matches!(&error, EngineError::InvalidImageReference));
        assert!(!fake.input.exists());
    }

    #[tokio::test]
    async fn opted_in_pull_cannot_fall_back_when_helper_selection_is_none() {
        let error = authorization(RegistryCredentialHelper::None, "malformed ref")
            .await
            .err()
            .unwrap();
        assert!(matches!(
            &error,
            EngineError::ImagePullCredentialUnavailable
        ));
    }
    #[tokio::test]
    async fn opted_in_push_reports_push_specific_helper_failure() {
        let error = authorization_for_push(
            RegistryCredentialHelper::None,
            "registry.example/team/app:stable",
        )
        .await
        .expect_err("explicit push credentials require a configured helper");
        assert!(matches!(error, EngineError::ImagePushCredentialUnavailable));
        assert!(error.to_string().contains("image push"));
    }
}
