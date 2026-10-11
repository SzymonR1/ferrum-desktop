use std::time::Duration;

use ferrum_core::{ImagePushProgress, ImagePushTarget};
use http_body_util::Full;
use hyper::{Method, Request, body::Bytes};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use hyperlocal::{UnixConnector, Uri};
use tokio::time::timeout;

use crate::{
    EngineClient, EngineError,
    image_pull::{ImageProgressStream, ImageProgressStreamError},
};

const OPEN_TIMEOUT: Duration = Duration::from_secs(8);

/// Incremental progress for an anonymous push of one existing local image tag.
///
/// Data frames are limited to 1 MiB, JSON records to 64 KiB, and progress
/// status to 256 bytes. Dropping the stream closes Ferrum's response stream,
/// but does not guarantee that the Docker daemon stopped the registry transfer.
pub struct ImagePushStream {
    inner: ImageProgressStream,
    endpoint_label: &'static str,
    daemon_failure_seen: bool,
}

impl ImagePushStream {
    /// Returns the next sanitized progress update, or `None` after stream completion.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the daemon rejects the push, the response
    /// is malformed, oversized, interrupted, or idle for 60 seconds.
    pub async fn next_progress(&mut self) -> Result<Option<ImagePushProgress>, EngineError> {
        loop {
            match self.inner.next_record().await {
                Ok(Some(progress)) if !self.daemon_failure_seen => {
                    return Ok(Some(ImagePushProgress {
                        status: progress.status,
                        current_bytes: progress.current_bytes,
                        total_bytes: progress.total_bytes,
                    }));
                }
                Ok(Some(_)) => {}
                Ok(None) if self.daemon_failure_seen => {
                    return Err(EngineError::ImagePushFailed);
                }
                Ok(None) => return Ok(None),
                Err(ImageProgressStreamError::DaemonFailure) => {
                    self.daemon_failure_seen = true;
                }
                Err(
                    ImageProgressStreamError::Malformed | ImageProgressStreamError::Interrupted,
                ) => {
                    return Err(EngineError::ImagePushStreamInterrupted {
                        endpoint_label: self.endpoint_label,
                    });
                }
            }
        }
    }
}

pub(crate) async fn open_image_push(
    engine: &EngineClient,
    image_id: &str,
    target: &ImagePushTarget,
) -> Result<ImagePushStream, EngineError> {
    if image_id.is_empty() || image_id.len() > 4096 {
        return Err(EngineError::InvalidImageId);
    }
    let endpoint_label = engine.info.endpoint_label;
    let source = engine
        .inspect_image(image_id)
        .await
        .map_err(|_| EngineError::ImagePushSourceUnavailable { endpoint_label })?;
    if source.id != image_id || !source.repo_tags.iter().any(|tag| tag == target.as_str()) {
        return Err(EngineError::ImagePushSourceChanged { endpoint_label });
    }

    let repository = target.repository();
    let mut path = String::with_capacity(
        20 + engine.api_version.len() + repository.len().saturating_mul(3) + target.tag().len(),
    );
    path.push_str("/v");
    path.push_str(&engine.api_version);
    path.push_str("/images/");
    for byte in repository.bytes() {
        match byte {
            b'[' => path.push_str("%5B"),
            b']' => path.push_str("%5D"),
            _ => path.push(char::from(byte)),
        }
    }
    path.push_str("/push?tag=");
    path.push_str(target.tag());
    let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
    // Match Moby's anonymous ImagePush client path: no auth header, but
    // an empty JSON auth object prevents older daemon EOF fallback errors.
    let request = Request::builder()
        .method(Method::POST)
        .uri(Uri::new(&engine.socket_path, &path))
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from_static(b"{}")))
        .map_err(|_| EngineError::ImagePushUnavailable { endpoint_label })?;
    let response = timeout(OPEN_TIMEOUT, client.request(request))
        .await
        .map_err(|_| EngineError::ImagePushTimedOut { endpoint_label })?
        .map_err(|_| EngineError::ImagePushOutcomeUnknown { endpoint_label })?;

    if !response.status().is_success() {
        return Err(EngineError::ImagePushRejected {
            endpoint_label,
            status: response.status().as_u16(),
        });
    }

    Ok(ImagePushStream {
        inner: ImageProgressStream::new(response.into_body()),
        endpoint_label,
        daemon_failure_seen: false,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::SystemTime,
    };

    use ferrum_core::{ImagePushProgress, ImagePushTarget};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    use crate::{EngineClient, EngineError, EngineInfo};

    static NEXT_TEMP_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

    fn temporary_socket() -> (PathBuf, PathBuf) {
        let timestamp_nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after UNIX epoch")
            .as_nanos();
        loop {
            let id = NEXT_TEMP_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "ferrum-image-push-{}-{timestamp_nanos}-{id}",
                std::process::id(),
            ));
            match fs::create_dir(&directory) {
                Ok(()) => return (directory.join("engine.sock"), directory),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("failed to create image-push test directory: {error}"),
            }
        }
    }

    fn engine(socket_path: PathBuf) -> EngineClient {
        EngineClient {
            info: EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.56".to_owned(),
                endpoint_label: "local Docker socket",
            },
            api_version: "1.56".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn read_request(stream: &mut tokio::net::UnixStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream
                .read(&mut buffer)
                .await
                .expect("request headers should be readable");
            assert_ne!(count, 0, "request should contain HTTP headers");
            request.extend_from_slice(&buffer[..count]);
        }
        request
    }

    async fn read_request_with_body(stream: &mut tokio::net::UnixStream) -> Vec<u8> {
        let mut request = read_request(stream).await;
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("request header terminator should exist");
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("valid content length"))
            })
            .unwrap_or_default();
        let body_end = header_end + 4 + content_length;
        let mut buffer = [0; 1024];
        while request.len() < body_end {
            let count = stream
                .read(&mut buffer)
                .await
                .expect("request body should be readable");
            assert_ne!(count, 0, "request body should not end early");
            request.extend_from_slice(&buffer[..count]);
        }
        request
    }

    async fn write_response(stream: &mut tokio::net::UnixStream, status: &str, body: &[u8]) {
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .expect("response headers should be writable");
        stream
            .write_all(body)
            .await
            .expect("response body should be writable");
    }

    async fn write_chunked(stream: &mut tokio::net::UnixStream, body: &[u8]) {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("stream response headers should be writable");
        stream
            .write_all(format!("{:X}\r\n", body.len()).as_bytes())
            .await
            .expect("chunk size should be writable");
        stream
            .write_all(body)
            .await
            .expect("progress body should be writable");
        stream
            .write_all(b"\r\n0\r\n\r\n")
            .await
            .expect("chunk terminator should be writable");
    }

    fn cleanup(socket: &Path, directory: &Path) {
        if socket.exists() {
            fs::remove_file(socket).expect("socket should be removed");
        }
        fs::remove_dir(directory).expect("temporary directory should be removed");
    }

    #[tokio::test]
    async fn pushes_one_existing_tag_without_credentials_and_streams_progress() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).expect("fake engine socket should bind");
        let server = tokio::spawn(async move {
            let (mut inspect, _) = listener
                .accept()
                .await
                .expect("image preflight request should arrive");
            let inspect_request = read_request(&mut inspect).await;
            assert!(
                String::from_utf8_lossy(&inspect_request)
                    .starts_with("GET /v1.56/images/sha256%3Asource-id/json HTTP/1.1\r\n")
            );
            write_response(
                &mut inspect,
                "200 OK",
                br#"{"Id":"sha256:source-id","RepoTags":["registry.example/team/app:stable"],"Size":12}"#,
            )
            .await;

            let (mut push, _) = listener
                .accept()
                .await
                .expect("push request should follow a successful preflight");
            let request = read_request_with_body(&mut push).await;
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.starts_with(
                "POST /v1.56/images/registry.example/team/app/push?tag=stable HTTP/1.1\r\n"
            ));
            assert!(
                request_text.contains("content-type: application/json\r\n")
                    || request_text.contains("Content-Type: application/json\r\n")
            );
            assert!(
                !request_text
                    .to_ascii_lowercase()
                    .contains("x-registry-auth:")
            );
            let body_start = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("push request headers should end")
                + 4;
            assert_eq!(&request[body_start..], b"{}");

            write_chunked(
                &mut push,
                b"{\"status\":\"Pushing\",\"progressDetail\":{\"current\":2,\"total\":9}}\n{\"status\":\"Pushed\"}\n",
            )
            .await;
        });

        let target = ImagePushTarget::parse("registry.example/team/app:stable")
            .expect("local repository tag should parse");
        let mut push = engine(socket.clone())
            .open_image_push("sha256:source-id", &target)
            .await
            .expect("selected local tag should open an anonymous push");
        assert_eq!(
            push.next_progress().await.unwrap(),
            Some(ImagePushProgress {
                status: "Pushing".to_owned(),
                current_bytes: Some(2),
                total_bytes: Some(9),
            })
        );
        assert_eq!(
            push.next_progress().await.unwrap(),
            Some(ImagePushProgress {
                status: "Pushed".to_owned(),
                current_bytes: None,
                total_bytes: None,
            })
        );
        assert_eq!(push.next_progress().await.unwrap(), None);

        server.await.expect("fake engine server should finish");
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn refuses_to_push_when_preflight_tag_no_longer_belongs_to_image() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).expect("fake engine socket should bind");
        let server = tokio::spawn(async move {
            let (mut inspect, _) = listener
                .accept()
                .await
                .expect("image preflight request should arrive");
            read_request(&mut inspect).await;
            write_response(
                &mut inspect,
                "200 OK",
                br#"{"Id":"sha256:source-id","RepoTags":["registry.example/team/app:other"]}"#,
            )
            .await;
        });

        let target = ImagePushTarget::parse("registry.example/team/app:stable")
            .expect("local repository tag should parse");
        let error = engine(socket.clone())
            .open_image_push("sha256:source-id", &target)
            .await
            .err()
            .expect("a reassigned tag must not be pushed");
        assert!(matches!(error, EngineError::ImagePushSourceChanged { .. }));

        server.await.expect("preflight server should finish");
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn reports_http_rejection_without_response_body() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).expect("fake engine socket should bind");
        let server = tokio::spawn(async move {
            let (mut inspect, _) = listener
                .accept()
                .await
                .expect("image preflight request should arrive");
            read_request(&mut inspect).await;
            write_response(
                &mut inspect,
                "200 OK",
                br#"{"Id":"sha256:source-id","RepoTags":["registry.example/team/app:stable"]}"#,
            )
            .await;

            let (mut push, _) = listener
                .accept()
                .await
                .expect("push request should follow a successful preflight");
            let request = read_request_with_body(&mut push).await;
            assert!(
                !String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("x-registry-auth:")
            );
            write_response(
                &mut push,
                "401 Unauthorized",
                b"SECRET registry credentials",
            )
            .await;
        });

        let target = ImagePushTarget::parse("registry.example/team/app:stable")
            .expect("local repository tag should parse");
        let error = engine(socket.clone())
            .open_image_push("sha256:source-id", &target)
            .await
            .err()
            .expect("HTTP rejection should be safe");
        assert!(matches!(
            error,
            EngineError::ImagePushRejected { status: 401, .. }
        ));
        assert!(!error.to_string().contains("SECRET"));

        server.await.expect("fake engine server should finish");
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn redacts_secret_text_in_daemon_push_error_records() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).expect("fake engine socket should bind");
        let server = tokio::spawn(async move {
            let (mut inspect, _) = listener
                .accept()
                .await
                .expect("image preflight request should arrive");
            read_request(&mut inspect).await;
            write_response(
                &mut inspect,
                "200 OK",
                br#"{"Id":"sha256:source-id","RepoTags":["registry.example/team/app:stable"]}"#,
            )
            .await;

            let (mut push, _) = listener
                .accept()
                .await
                .expect("push request should follow a successful preflight");
            read_request_with_body(&mut push).await;
            write_chunked(
                &mut push,
                b"{\"errorDetail\":{\"message\":\"SECRET registry credentials\"}}\n{\"status\":\"still draining\"}\n",
            )
            .await;
        });

        let target = ImagePushTarget::parse("registry.example/team/app:stable")
            .expect("local repository tag should parse");
        let mut push = engine(socket.clone())
            .open_image_push("sha256:source-id", &target)
            .await
            .expect("push stream should open before daemon progress error");
        let error = push
            .next_progress()
            .await
            .expect_err("daemon error record should fail the push");
        assert!(matches!(error, EngineError::ImagePushFailed));
        assert!(!error.to_string().contains("SECRET"));

        server.await.expect("fake engine server should finish");
        cleanup(&socket, &directory);
    }
    #[tokio::test]
    async fn classifies_connection_loss_after_push_submission_as_unknown() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).expect("fake engine socket should bind");
        let server = tokio::spawn(async move {
            let (mut inspect, _) = listener
                .accept()
                .await
                .expect("image preflight request should arrive");
            read_request(&mut inspect).await;
            write_response(
                &mut inspect,
                "200 OK",
                br#"{"Id":"sha256:source-id","RepoTags":["registry.example/team/app:stable"]}"#,
            )
            .await;

            let (mut push, _) = listener
                .accept()
                .await
                .expect("push request should follow a successful preflight");
            read_request_with_body(&mut push).await;
            drop(push);
        });

        let target = ImagePushTarget::parse("registry.example/team/app:stable")
            .expect("local repository tag should parse");
        let error = engine(socket.clone())
            .open_image_push("sha256:source-id", &target)
            .await
            .err()
            .expect("lost response should return an error");
        assert!(matches!(error, EngineError::ImagePushOutcomeUnknown { .. }));

        server.await.expect("fake engine server should finish");
        cleanup(&socket, &directory);
    }
}
