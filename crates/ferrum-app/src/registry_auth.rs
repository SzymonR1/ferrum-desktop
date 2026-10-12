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

/// Resolves a configured helper only after the per-pull opt-in was consumed.
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
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_HELPER: AtomicU64 = AtomicU64::new(0);

    struct FakeHelper {
        path: PathBuf,
        input: PathBuf,
    }

    impl FakeHelper {
        fn new(body: &str) -> Self {
            let id = NEXT_HELPER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir();
            let stem = format!("ferrum-credential-helper-{}-{id}", std::process::id());
            let path = root.join(&stem);
            let input = root.join(format!("{stem}-input"));
            let script = format!(
                "#!/bin/sh\ntest \"$1\" = get || exit 7\ncat > '{}'\nprintf '%s' '{}'\n",
                input.display(),
                body,
            );
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self { path, input }
        }

        fn from_script(script: &str) -> Self {
            let id = NEXT_HELPER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ferrum-credential-helper-{}-{id}",
                std::process::id()
            ));
            let input = path.with_extension("input");
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self { path, input }
        }
    }

    impl Drop for FakeHelper {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(&self.input);
        }
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
        let id = NEXT_HELPER.fetch_add(1, Ordering::Relaxed);
        let stem = format!("ferrum-helper-cancel-{}-{id}", std::process::id());
        let started = std::env::temp_dir().join(format!("{stem}-started"));
        let finished = std::env::temp_dir().join(format!("{stem}-finished"));
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

        let child_finished = finished.exists();
        let _ = fs::remove_file(started);
        let _ = fs::remove_file(finished);
        assert!(helper_started);
        assert!(
            !child_finished,
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
}
