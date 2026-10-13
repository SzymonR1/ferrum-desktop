//! Shared test-only local OCI registry fixture for Ferrum Engine and app integration smokes.
//!
//! The registry is loopback-only and serves a generated OCI image, an intentionally
//! missing manifest tag, and optional exact synthetic Basic authorization evidence.

#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(target_os = "linux")]
const LIVE_PULL_LAYER_DATA_BYTES: usize = 64 * 1024;
#[cfg(target_os = "linux")]
const LIVE_PULL_LAYER_BYTES: usize = 512 + LIVE_PULL_LAYER_DATA_BYTES + 1024;
#[cfg(target_os = "linux")]
const LOOPBACK_REGISTRY_NOT_FOUND: &[u8] =
    br#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"fixture reference is intentionally absent"}]}"#;

/// Generated, non-empty OCI config, layer and manifest data for a test registry.
#[cfg(target_os = "linux")]
pub struct LivePullImageFixture {
    config_digest: String,
    config: Vec<u8>,
    layer_digest: String,
    layer: Vec<u8>,
    manifest_digest: String,
    manifest: Vec<u8>,
}

#[cfg(target_os = "linux")]
impl LivePullImageFixture {
    /// Creates deterministic OCI image data containing the supplied unique nonce.
    ///
    /// # Panics
    ///
    /// Panics if the generated archive filename is 100 bytes or longer.
    pub fn new(nonce: &str) -> Self {
        let layer = live_pull_layer(nonce);
        let layer_digest = sha256_digest(&layer);
        let config = serde_json::to_vec(&serde_json::json!({
            "architecture": "amd64",
            "os": "linux",
            "config": {},
            "rootfs": {
                "type": "layers",
                "diff_ids": [layer_digest.clone()],
            },
            "history": [{
                "created_by": format!("Ferrum loopback pull progress fixture {nonce}"),
            }],
        }))
        .expect("the deterministic fixture image config should serialize");
        let config_digest = sha256_digest(&config);
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": config_digest,
                "size": config.len(),
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar",
                "digest": layer_digest.clone(),
                "size": layer.len(),
            }],
        }))
        .expect("the deterministic fixture image manifest should serialize");
        let manifest_digest = sha256_digest(&manifest);

        Self {
            config_digest,
            config,
            layer_digest,
            layer,
            manifest_digest,
            manifest,
        }
    }
}

#[cfg(target_os = "linux")]
fn sha256_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest.iter() {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

#[cfg(target_os = "linux")]
fn write_tar_octal_field(field: &mut [u8], value: u64) {
    let encoded = format!("{:0width$o}\0", value, width = field.len() - 1);
    assert_eq!(encoded.len(), field.len());
    field.copy_from_slice(encoded.as_bytes());
}

#[cfg(target_os = "linux")]
fn live_pull_layer(nonce: &str) -> Vec<u8> {
    let file_name = format!("ferrum-progress-{nonce}.txt");
    assert!(file_name.len() < 100);
    let mut header = [0; 512];
    header[..file_name.len()].copy_from_slice(file_name.as_bytes());
    write_tar_octal_field(&mut header[100..108], 0o644);
    write_tar_octal_field(&mut header[108..116], 0);
    write_tar_octal_field(&mut header[116..124], 0);
    write_tar_octal_field(&mut header[124..136], LIVE_PULL_LAYER_DATA_BYTES as u64);
    write_tar_octal_field(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let checksum_field = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_field.as_bytes());

    let mut data = vec![b'F'; LIVE_PULL_LAYER_DATA_BYTES];
    for (target, source) in data.iter_mut().zip(nonce.bytes()) {
        *target = source;
    }
    let padding = (512 - data.len() % 512) % 512;
    let mut layer = Vec::with_capacity(LIVE_PULL_LAYER_BYTES);
    layer.extend_from_slice(&header);
    layer.extend_from_slice(&data);
    layer.resize(layer.len() + padding, 0);
    layer.extend_from_slice(&[0; 1024]);
    assert_eq!(layer.len(), LIVE_PULL_LAYER_BYTES);
    layer
}

/// Test-only loopback OCI registry for a generated repository and missing tag.
#[cfg(target_os = "linux")]
pub struct LoopbackRegistry {
    port: u16,
    task: tokio::task::JoinHandle<()>,
    missing_manifest_requested: Arc<AtomicBool>,
    valid_authorization_observed: Arc<AtomicBool>,
}

#[cfg(target_os = "linux")]
impl LoopbackRegistry {
    /// Starts a registry on `127.0.0.1` at an ephemeral port.
    ///
    /// It serves the generated image at `image_tag` under `image_repository`
    /// and rejects requests for `missing_manifest_tag`. When
    /// `expected_authorization` is supplied, it must be the exact synthetic
    /// Basic authorization value expected from the client; the registry
    /// records whether that value was observed. No non-loopback address is
    /// bound.
    ///
    /// `fixture` supplies the OCI config, layer, and manifest served for the
    /// image tag. All values and credentials are test-only.
    ///
    /// # Arguments
    ///
    /// * `image_repository` — OCI repository path without a registry host.
    /// * `image_tag` — tag served with the generated image.
    /// * `missing_manifest_tag` — tag that receives a manifest-not-found response.
    /// * `fixture` — generated OCI image contents.
    /// * `expected_authorization` — optional exact synthetic Basic value to require.
    ///
    /// # Panics
    ///
    /// Panics if no Tokio runtime is active or binding/reading the ephemeral
    /// loopback address fails.
    pub async fn start(
        image_repository: String,
        image_tag: String,
        missing_manifest_tag: String,
        fixture: LivePullImageFixture,
        expected_authorization: Option<zeroize::Zeroizing<String>>,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the loopback registry should bind an ephemeral local port");
        let port = listener
            .local_addr()
            .expect("bound loopback address")
            .port();
        let missing_manifest_requested = Arc::new(AtomicBool::new(false));
        let valid_authorization_observed = Arc::new(AtomicBool::new(false));
        let registry_state = LoopbackRegistryState {
            image_repository,
            image_tag,
            missing_manifest_tag,
            fixture,
            missing_manifest_requested: Arc::clone(&missing_manifest_requested),
            valid_authorization_observed: Arc::clone(&valid_authorization_observed),
            expected_authorization,
        };
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                serve_loopback_registry_request(&mut stream, &registry_state).await;
            }
        });
        Self {
            port,
            task,
            missing_manifest_requested,
            valid_authorization_observed,
        }
    }

    /// Returns the ephemeral loopback TCP port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Reports whether the intentionally missing manifest tag was requested.
    pub fn missing_manifest_was_requested(&self) -> bool {
        self.missing_manifest_requested.load(Ordering::Relaxed)
    }

    /// Reports whether exact synthetic Basic authorization was observed.
    pub fn valid_authorization_was_observed(&self) -> bool {
        self.valid_authorization_observed.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "linux")]
impl Drop for LoopbackRegistry {
    fn drop(&mut self) {
        self.task.abort();
    }
}
/// Best-effort cleanup for a verified, test-owned image.
///
/// This guard starts unarmed. Call [`DisposableImageTag::arm_if_verified`]
/// only after finding the generated reference in the engine inventory; it arms
/// cleanup only when the image has a full SHA-256 ID and that reference is its
/// sole repository tag. Cleanup is pinned to the same validated local Unix
/// socket and removes by the retained full image ID without force or prune.
/// If another tag is added, Docker refuses non-forced removal and the image may
/// remain for manual cleanup. Cleanup is bounded to five seconds.
#[cfg(target_os = "linux")]
pub struct DisposableImageTag {
    socket_path: std::path::PathBuf,
    reference: String,
    image_id: Option<String>,
}

#[cfg(target_os = "linux")]
impl DisposableImageTag {
    /// Creates an unarmed cleanup guard for an exact image tag.
    ///
    /// `socket_path` must be the validated local Unix socket used by the
    /// `EngineClient` that created and inspected the image.
    pub fn new(socket_path: std::path::PathBuf, reference: String) -> Self {
        Self {
            socket_path,
            reference,
            image_id: None,
        }
    }

    /// Arms cleanup only if the image ID and tag inventory establish sole ownership.
    ///
    /// Returns `true` when `image_id` is a full SHA-256 Docker image ID and
    /// `repo_tags` contains exactly the guarded reference. Invalid IDs, missing
    /// tags, or additional tags leave cleanup unarmed.
    pub fn arm_if_verified(&mut self, image_id: &str, repo_tags: &[String]) -> bool {
        let verified = is_full_sha256_image_id(image_id)
            && repo_tags.len() == 1
            && repo_tags[0] == self.reference;
        self.image_id = verified.then(|| image_id.to_owned());
        self.image_id.is_some()
    }

    /// Removes the retained verified image ID, waiting at most five seconds.
    ///
    /// Returns `PermissionDenied` without spawning Docker unless ownership has
    /// been verified with [`arm_if_verified`](Self::arm_if_verified).
    ///
    /// # Errors
    ///
    /// Returns `PermissionDenied` if the guard is unarmed. Otherwise, returns
    /// an I/O error if Docker cannot be spawned or waited on, including a
    /// timeout if the cleanup child exceeds five seconds.
    pub fn remove(&self) -> std::io::Result<std::process::ExitStatus> {
        let Some(image_id) = self.image_id.as_deref() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "image cleanup ownership has not been verified",
            ));
        };
        docker_remove_image(&self.socket_path, image_id)
    }

    /// Disarms best-effort cleanup after explicit removal has been confirmed.
    pub fn disarm(&mut self) {
        self.image_id = None;
    }
}

#[cfg(target_os = "linux")]
impl Drop for DisposableImageTag {
    fn drop(&mut self) {
        if let Some(image_id) = self.image_id.as_deref() {
            let _ = docker_remove_image(&self.socket_path, image_id);
        }
    }
}

#[cfg(target_os = "linux")]
fn is_full_sha256_image_id(id: &str) -> bool {
    id.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

#[cfg(target_os = "linux")]
fn docker_remove_image(
    socket_path: &std::path::Path,
    image_id: &str,
) -> std::io::Result<std::process::ExitStatus> {
    use std::{
        ffi::OsString,
        process::{Command, Stdio},
    };

    let mut host = OsString::from("unix://");
    host.push(socket_path.as_os_str());
    let mut child = Command::new("docker")
        .arg("--host")
        .arg(host)
        .args(["image", "rm", "--no-prune"])
        .arg(image_id)
        .env_remove("DOCKER_HOST")
        .env_remove("DOCKER_CONTEXT")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_for_child_with_timeout(&mut child, Duration::from_secs(5))
}

#[cfg(target_os = "linux")]
fn wait_for_child_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        if std::time::Instant::now() >= deadline {
            let timeout_error = std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "cleanup child exceeded its deadline",
            );
            return match child.kill() {
                Ok(()) => {
                    let _ = child.wait();
                    Err(timeout_error)
                }
                Err(kill_error) => match child.try_wait() {
                    Ok(Some(status)) => Ok(status),
                    Ok(None) | Err(_) => Err(kill_error),
                },
            };
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .min(Duration::from_millis(25)),
        );
    }
}

#[cfg(target_os = "linux")]
struct LoopbackRegistryState {
    image_repository: String,
    image_tag: String,
    missing_manifest_tag: String,
    fixture: LivePullImageFixture,
    missing_manifest_requested: Arc<AtomicBool>,
    valid_authorization_observed: Arc<AtomicBool>,
    expected_authorization: Option<zeroize::Zeroizing<String>>,
}

#[cfg(target_os = "linux")]
async fn serve_loopback_registry_request(
    stream: &mut tokio::net::TcpStream,
    state: &LoopbackRegistryState,
) {
    let image_repository = state.image_repository.as_str();
    let image_tag = state.image_tag.as_str();
    let missing_manifest_tag = state.missing_manifest_tag.as_str();
    let fixture = &state.fixture;
    let missing_manifest_requested = &state.missing_manifest_requested;
    let valid_authorization_observed = &state.valid_authorization_observed;
    let expected_authorization = state
        .expected_authorization
        .as_ref()
        .map(|value| value.as_str());
    let Some((method, raw_path, authorization)) = read_loopback_registry_request(stream).await
    else {
        return;
    };
    if let Some(expected) = expected_authorization {
        if authorization.as_deref().map(String::as_str) != Some(expected) {
            write_loopback_registry_unauthorized(stream).await;
            return;
        }
        valid_authorization_observed.store(true, Ordering::Relaxed);
    }
    let path = raw_path
        .split('?')
        .next()
        .unwrap_or(&raw_path)
        .replace("%3A", ":")
        .replace("%3a", ":");
    if matches!(path.as_str(), "/v2" | "/v2/") {
        write_loopback_registry_response(stream, &method, "200 OK", b"", "application/json", None)
            .await;
        return;
    }

    let manifest_prefix = format!("/v2/{image_repository}/manifests/");
    if let Some(requested_manifest) = path.strip_prefix(&manifest_prefix) {
        if requested_manifest == missing_manifest_tag {
            missing_manifest_requested.store(true, Ordering::Relaxed);
        }
        if requested_manifest != image_tag && requested_manifest != fixture.manifest_digest.as_str()
        {
            write_loopback_registry_response(
                stream,
                &method,
                "404 Not Found",
                LOOPBACK_REGISTRY_NOT_FOUND,
                "application/json",
                None,
            )
            .await;
            return;
        }
        write_loopback_registry_response(
            stream,
            &method,
            "200 OK",
            &fixture.manifest,
            "application/vnd.oci.image.manifest.v1+json",
            Some(&fixture.manifest_digest),
        )
        .await;
        return;
    }

    let config_blob_path = format!("/v2/{image_repository}/blobs/{}", fixture.config_digest);
    if path == config_blob_path {
        write_loopback_registry_response(
            stream,
            &method,
            "200 OK",
            &fixture.config,
            "application/vnd.oci.image.config.v1+json",
            Some(&fixture.config_digest),
        )
        .await;
        return;
    }

    let layer_blob_path = format!("/v2/{image_repository}/blobs/{}", fixture.layer_digest);
    if path == layer_blob_path {
        write_loopback_registry_response(
            stream,
            &method,
            "200 OK",
            &fixture.layer,
            "application/vnd.oci.image.layer.v1.tar",
            Some(&fixture.layer_digest),
        )
        .await;
        return;
    }

    write_loopback_registry_response(
        stream,
        &method,
        "404 Not Found",
        LOOPBACK_REGISTRY_NOT_FOUND,
        "application/json",
        None,
    )
    .await;
}

#[cfg(target_os = "linux")]
async fn read_loopback_registry_request(
    stream: &mut tokio::net::TcpStream,
) -> Option<(String, String, Option<zeroize::Zeroizing<String>>)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut request = zeroize::Zeroizing::new(Vec::with_capacity(1024));
    let mut chunk = zeroize::Zeroizing::new([0u8; 1024]);
    loop {
        let count = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk[..]))
            .await
            .ok()?
            .ok()?;
        if count == 0 || request.len().saturating_add(count) > 8192 {
            return None;
        }
        request.extend_from_slice(&chunk[..count]);
        if !request
            .first()
            .is_some_and(|byte| matches!(*byte, b'G' | b'H'))
        {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            return None;
        }
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let headers = std::str::from_utf8(&request[..header_end]).ok()?;
    let mut lines = headers.split("\r\n");
    let mut fields = lines.next()?.split_ascii_whitespace();
    let method = fields.next()?.to_owned();
    let path = fields.next()?.to_owned();
    let mut authorization = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("authorization")
        {
            if authorization.is_some() {
                authorization = Some(zeroize::Zeroizing::new(String::new()));
                continue;
            }
            authorization = Some(zeroize::Zeroizing::new(value.trim().to_owned()));
        }
    }
    Some((method, path, authorization))
}

#[cfg(target_os = "linux")]
async fn write_loopback_registry_unauthorized(stream: &mut tokio::net::TcpStream) {
    use tokio::io::AsyncWriteExt;
    let _ = stream.write_all(
        b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Ferrum loopback fixture\"\r\nContent-Length: 0\r\nConnection: close\r\nDocker-Distribution-API-Version: registry/2.0\r\n\r\n",
    ).await;
}

#[cfg(target_os = "linux")]
async fn write_loopback_registry_response(
    stream: &mut tokio::net::TcpStream,
    method: &str,
    status: &str,
    body: &[u8],
    content_type: &str,
    digest: Option<&str>,
) {
    use tokio::io::AsyncWriteExt;

    let mut headers = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\nDocker-Distribution-API-Version: registry/2.0\r\nContent-Type: {content_type}\r\n",
        body.len()
    );
    if let Some(digest) = digest {
        headers.push_str("Docker-Content-Digest: ");
        headers.push_str(digest);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    if stream.write_all(headers.as_bytes()).await.is_err() {
        return;
    }
    if method != "HEAD" {
        let _ = stream.write_all(body).await;
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use super::wait_for_child_with_timeout;

    #[test]
    fn wait_for_child_with_timeout_kills_a_long_running_process() {
        let mut child = Command::new("sleep")
            .arg("60")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the Linux test environment should provide sleep");
        let started = Instant::now();
        let result = wait_for_child_with_timeout(&mut child, Duration::from_millis(20));

        assert!(
            result.is_err_and(|error| error.kind() == std::io::ErrorKind::TimedOut),
            "the child should be reported as timed out"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the bounded wait should return promptly"
        );
        assert!(
            child.try_wait().is_ok_and(|status| status.is_some()),
            "the timed-out child should be reaped"
        );
    }

    #[test]
    fn image_cleanup_arming_requires_a_full_id_and_sole_exact_tag() {
        use super::DisposableImageTag;

        let reference = "127.0.0.1:12345/ferrum-generated:tag".to_owned();
        let mut cleanup = DisposableImageTag::new(
            std::path::PathBuf::from("/run/docker.sock"),
            reference.clone(),
        );
        assert_eq!(
            cleanup.remove().unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let full_id = format!("sha256:{}", "a".repeat(64));

        assert!(!cleanup.arm_if_verified("sha256:short", std::slice::from_ref(&reference)));
        assert!(cleanup.image_id.is_none());
        assert!(!cleanup.arm_if_verified(&full_id, &[reference.clone(), "other:tag".to_owned()]));
        assert!(cleanup.image_id.is_none());
        assert!(!cleanup.arm_if_verified(&full_id, &["other:tag".to_owned()]));
        assert!(cleanup.image_id.is_none());
        assert!(cleanup.arm_if_verified(&full_id, std::slice::from_ref(&reference)));
        assert_eq!(cleanup.image_id.as_deref(), Some(full_id.as_str()));
        assert!(!cleanup.arm_if_verified("sha256:short", std::slice::from_ref(&reference)));
        assert!(cleanup.image_id.is_none());
        cleanup.disarm();
    }
}
