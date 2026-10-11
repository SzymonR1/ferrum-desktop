#[cfg(test)]
use std::path::Path;
use std::time::Duration;

use ferrum_core::{ImagePullProgress, is_untrusted_display_character};
use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request, body::Bytes};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use hyperlocal::{UnixConnector, Uri};
use serde::{Deserialize, de::IgnoredAny};
use tokio::time::timeout;

use crate::{EngineClient, EngineError};

const OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_HTTP_DATA_FRAME_BYTES: usize = 1024 * 1024;
const MAX_JSON_LINE_BYTES: usize = 64 * 1024;
const MAX_IMAGE_REFERENCE_BYTES: usize = 512;
const MAX_STATUS_BYTES: usize = 256;

/// Incremental, cancellable progress stream for an anonymous image pull.
///
/// HTTP data frames are limited to 1 MiB, JSON Lines records to 64 KiB, and
/// each wait for new progress data to 60 seconds. Dropping this stream closes
/// Ferrum's HTTP response stream; it does not guarantee that the daemon-side
/// pull is stopped.
pub struct ImagePullStream {
    inner: ImageProgressStream,
    endpoint_label: &'static str,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ImageProgressRecord {
    pub(crate) status: String,
    pub(crate) current_bytes: Option<u64>,
    pub(crate) total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImageProgressStreamError {
    Malformed,
    DaemonFailure,
    Interrupted,
}

pub(crate) struct ImageProgressStream {
    body: hyper::body::Incoming,
    pending: Vec<u8>,
    frame: Option<Bytes>,
    frame_offset: usize,
    finished: bool,
}

impl ImageProgressStream {
    pub(crate) fn new(body: hyper::body::Incoming) -> Self {
        Self {
            body,
            pending: Vec::with_capacity(256),
            frame: None,
            frame_offset: 0,
            finished: false,
        }
    }

    pub(crate) async fn next_record(
        &mut self,
    ) -> Result<Option<ImageProgressRecord>, ImageProgressStreamError> {
        loop {
            if let Some(frame) = self.frame.as_ref() {
                let remaining = &frame[self.frame_offset..];
                if let Some(newline) = remaining.iter().position(|byte| *byte == b'\n') {
                    let line_len = self.pending.len().saturating_add(newline);
                    if line_len > MAX_JSON_LINE_BYTES {
                        return Err(ImageProgressStreamError::Malformed);
                    }
                    self.pending.extend_from_slice(&remaining[..newline]);
                    let result = parse_line(&self.pending);
                    self.frame_offset += newline + 1;
                    self.pending.clear();
                    let result = result.map_err(Self::parse_error)?;
                    if let Some(progress) = result {
                        return Ok(Some(progress));
                    }
                    continue;
                }

                let remaining_len = remaining.len();
                if self.pending.len().saturating_add(remaining_len) > MAX_JSON_LINE_BYTES {
                    return Err(ImageProgressStreamError::Malformed);
                }
                self.pending.extend_from_slice(remaining);
                self.frame = None;
                self.frame_offset = 0;
                continue;
            }

            if self.finished {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.pending);
                return parse_line(&line).map_err(Self::parse_error);
            }

            let frame = timeout(IDLE_TIMEOUT, self.body.frame())
                .await
                .map_err(|_| ImageProgressStreamError::Interrupted)?;
            match frame {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        validate_http_data_frame(data.len())
                            .map_err(|_| ImageProgressStreamError::Malformed)?;
                        if !data.is_empty() {
                            self.frame = Some(data.clone());
                        }
                    }
                }
                Some(Err(_)) => return Err(ImageProgressStreamError::Interrupted),
                None => self.finished = true,
            }
        }
    }

    fn parse_error(error: ImageProgressParseError) -> ImageProgressStreamError {
        match error {
            ImageProgressParseError::Malformed => ImageProgressStreamError::Malformed,
            ImageProgressParseError::DaemonFailure => ImageProgressStreamError::DaemonFailure,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRecord {
    status: Option<String>,
    progress_detail: Option<ProgressDetail>,
    error: Option<IgnoredAny>,
    error_detail: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressDetail {
    current: Option<u64>,
    total: Option<u64>,
}

pub(crate) async fn open_image_pull(
    engine: &EngineClient,
    reference: &str,
) -> Result<ImagePullStream, EngineError> {
    if !valid_reference(reference) {
        return Err(EngineError::InvalidImageReference);
    }
    let endpoint_label = engine.info.endpoint_label;
    let path = format!(
        "/v{}/images/create?fromImage={}",
        engine.api_version,
        crate::encode_query_component(reference),
    );
    let response = timeout(OPEN_TIMEOUT, async {
        let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let request = Request::builder()
            .method(Method::POST)
            .uri(Uri::new(&engine.socket_path, &path))
            .body(Empty::<Bytes>::new())
            .map_err(|_| EngineError::ImagePullUnavailable { endpoint_label })?;
        client
            .request(request)
            .await
            .map_err(|_| EngineError::ImagePullUnavailable { endpoint_label })
    })
    .await
    .map_err(|_| EngineError::ImagePullTimedOut { endpoint_label })??;

    if !response.status().is_success() {
        return Err(EngineError::ImagePullRejected {
            endpoint_label,
            status: response.status().as_u16(),
        });
    }
    Ok(ImagePullStream {
        inner: ImageProgressStream::new(response.into_body()),
        endpoint_label,
    })
}
impl ImagePullStream {
    /// Returns the next safe progress update, or `None` when the stream ends.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if the daemon reports a failed pull or the
    /// response is malformed, oversized, interrupted, or idle for 60 seconds.
    pub async fn next_progress(&mut self) -> Result<Option<ImagePullProgress>, EngineError> {
        match self.inner.next_record().await {
            Ok(Some(progress)) => Ok(Some(ImagePullProgress {
                status: progress.status,
                current_bytes: progress.current_bytes,
                total_bytes: progress.total_bytes,
            })),
            Ok(None) => Ok(None),
            Err(ImageProgressStreamError::DaemonFailure) => Err(EngineError::ImagePullFailed),
            Err(ImageProgressStreamError::Malformed | ImageProgressStreamError::Interrupted) => {
                Err(EngineError::ImagePullStreamInterrupted {
                    endpoint_label: self.endpoint_label,
                })
            }
        }
    }
}
fn valid_reference(reference: &str) -> bool {
    !reference.is_empty()
        && reference.len() <= MAX_IMAGE_REFERENCE_BYTES
        && !reference
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn parse_line(bytes: &[u8]) -> Result<Option<ImageProgressRecord>, ImageProgressParseError> {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > MAX_JSON_LINE_BYTES {
        return Err(ImageProgressParseError::Malformed);
    }
    let record: PullRecord =
        serde_json::from_slice(bytes).map_err(|_| ImageProgressParseError::Malformed)?;
    if record.error.is_some() || record.error_detail.is_some() {
        return Err(ImageProgressParseError::DaemonFailure);
    }
    let status = sanitize_status(record.status.unwrap_or_default());
    let (current_bytes, total_bytes) = record
        .progress_detail
        .map_or((None, None), |progress| (progress.current, progress.total));
    Ok(Some(ImageProgressRecord {
        status,
        current_bytes,
        total_bytes,
    }))
}
fn validate_http_data_frame(frame_len: usize) -> Result<(), ImageProgressParseError> {
    if frame_len > MAX_HTTP_DATA_FRAME_BYTES {
        Err(ImageProgressParseError::Malformed)
    } else {
        Ok(())
    }
}

// Public errors intentionally don't carry daemon text or response metadata.
#[derive(Clone, Copy)]
enum ImageProgressParseError {
    Malformed,
    DaemonFailure,
}

fn sanitize_status(status: String) -> String {
    let mut safe = String::with_capacity(status.len().min(MAX_STATUS_BYTES));
    for character in status.chars() {
        let replacement = if is_untrusted_display_character(character) {
            ' '
        } else {
            character
        };
        if safe.len() + replacement.len_utf8() > MAX_STATUS_BYTES {
            break;
        }
        safe.push(replacement);
    }
    safe
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::SystemTime,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    static NEXT_TEMP_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

    fn temporary_socket() -> (std::path::PathBuf, std::path::PathBuf) {
        let timestamp_nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        temporary_socket_at(timestamp_nanos)
    }

    fn temporary_socket_at(timestamp_nanos: u128) -> (std::path::PathBuf, std::path::PathBuf) {
        loop {
            let id = NEXT_TEMP_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "ferrum-image-pull-{}-{timestamp_nanos}-{id}",
                std::process::id(),
            ));
            match fs::create_dir(&directory) {
                Ok(()) => return (directory.join("engine.sock"), directory),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("failed to create image-pull test directory: {error}"),
            }
        }
    }

    #[test]
    fn temporary_socket_directories_are_unique_for_equal_clock_values() {
        let timestamp_nanos = 42;
        let (_, first_directory) = temporary_socket_at(timestamp_nanos);
        let (_, second_directory) = temporary_socket_at(timestamp_nanos);

        assert_ne!(first_directory, second_directory);
        fs::remove_dir(first_directory).unwrap();
        fs::remove_dir(second_directory).unwrap();
    }

    fn engine(socket_path: std::path::PathBuf) -> EngineClient {
        EngineClient {
            info: crate::EngineInfo {
                version: "fixture".to_owned(),
                api_version: "1.45".to_owned(),
                endpoint_label: "test endpoint",
            },
            api_version: "1.45".to_owned(),
            selected_context: None,
            socket_path,
        }
    }

    async fn read_request(stream: &mut tokio::net::UnixStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&buffer[..count]);
        }
        request
    }

    fn cleanup(socket: &Path, directory: &Path) {
        fs::remove_file(socket).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn tags_image_with_encoded_request_and_accepts_created() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = String::from_utf8(read_request(&mut stream).await).unwrap();
            assert!(request.starts_with(
                "POST /v1.45/images/sha256%3Aabc%2Fdef%3Ftag/tag?repo=registry.example%3A5000%2Fteam%2Fapp&tag=v1_blue HTTP/1.1\r\n"
            ));
            assert!(!request.contains("force="));
            assert!(!request.to_ascii_lowercase().contains("x-registry-auth"));
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            request
        });

        engine(socket.clone())
            .tag_image(
                "sha256:abc/def?tag",
                "registry.example:5000/team/app",
                "v1_blue",
            )
            .await
            .unwrap();
        server.await.unwrap();
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_tag_inputs_without_contacting_engine() {
        let (socket, directory) = temporary_socket();
        let client = engine(socket.clone());
        assert!(matches!(
            client.tag_image("", "repo", "tag").await,
            Err(EngineError::InvalidImageId)
        ));
        assert!(matches!(
            client.tag_image(&"x".repeat(4097), "repo", "tag").await,
            Err(EngineError::InvalidImageId)
        ));

        let too_long_target = "r".repeat(508);
        let too_long_tag = "t".repeat(129);
        for (repository, tag) in [
            ("", "tag"),
            ("repo", ""),
            ("repo with-space", "tag"),
            ("repo", "bad/tag"),
            ("repo", ".bad"),
            ("repo", &too_long_target),
            ("repo", too_long_tag.as_str()),
        ] {
            assert!(matches!(
                client.tag_image("image-id", repository, tag).await,
                Err(EngineError::InvalidImageTagTarget)
            ));
        }
        assert!(!socket.exists());
        fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn rejects_http_tag_error_without_exposing_response_body() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 27\r\nConnection: close\r\n\r\nSECRET registry credentials")
                .await
                .unwrap();
        });

        let error = engine(socket.clone())
            .tag_image("image-id", "registry.example/team/app", "latest")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::ImageTagRejected { status: 500, .. }
        ));
        assert!(!error.to_string().contains("SECRET"));
        server.await.unwrap();
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn posts_encoded_reference_without_registry_auth_and_parses_chunks() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let headers = String::from_utf8_lossy(&request);
            assert!(headers.starts_with(
                "POST /v1.45/images/create?fromImage=registry.example%3A5000%2Fteam%2Fapp%3Alatest"
            ));
            assert!(!headers.to_ascii_lowercase().contains("x-registry-auth"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            for part in [br#"{"status":"Downloading","progressDetail":{"current":2,"total":9}}"#.as_slice(), b"\n{\"status\":\"Extracting\",\"progressDetail\":{\"current\":0,\"total\":0}}\n{\"status\":\"Done\"}\n"] {
                stream.write_all(format!("{:X}\r\n", part.len()).as_bytes()).await.unwrap();
                stream.write_all(part).await.unwrap();
                stream.write_all(b"\r\n").await.unwrap();
            }
            stream.write_all(b"0\r\n\r\n").await.unwrap();
        });
        let mut pull = engine(socket.clone())
            .open_image_pull("registry.example:5000/team/app:latest")
            .await
            .unwrap();
        assert_eq!(
            pull.next_progress().await.unwrap(),
            Some(ImagePullProgress {
                status: "Downloading".into(),
                current_bytes: Some(2),
                total_bytes: Some(9)
            })
        );
        assert_eq!(
            pull.next_progress().await.unwrap(),
            Some(ImagePullProgress {
                status: "Extracting".into(),
                current_bytes: Some(0),
                total_bytes: Some(0)
            })
        );
        assert_eq!(
            pull.next_progress().await.unwrap(),
            Some(ImagePullProgress {
                status: "Done".into(),
                current_bytes: None,
                total_bytes: None
            })
        );
        assert_eq!(pull.next_progress().await.unwrap(), None);
        server.await.unwrap();
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_invalid_references_without_contacting_engine() {
        let (socket, directory) = temporary_socket();
        let too_long = "x".repeat(513);
        for reference in [
            "",
            "bad ref",
            "line\nbreak",
            "\u{7f}control",
            too_long.as_str(),
        ] {
            assert!(matches!(
                engine(socket.clone()).open_image_pull(reference).await,
                Err(EngineError::InvalidImageReference)
            ));
        }
        assert!(!socket.exists());
        fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn redacts_daemon_error_event() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let body = b"{\"errorDetail\":{\"message\":\"SECRET registry credentials\"}}\n";
            stream
                .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
            stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
        });
        let mut pull = engine(socket.clone())
            .open_image_pull("public/image:latest")
            .await
            .unwrap();
        let error = pull.next_progress().await.unwrap_err();
        assert!(matches!(error, EngineError::ImagePullFailed));
        assert!(!error.to_string().contains("SECRET"));
        server.await.unwrap();
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn redacts_http_api_error_body() {
        let (socket, directory) = temporary_socket();
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
            let body = b"SECRET registry credentials response";
            stream
                .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
            stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
        });
        let error = engine(socket.clone())
            .open_image_pull("public/image:latest")
            .await
            .err()
            .expect("HTTP error status should reject the pull");
        assert!(matches!(
            error,
            EngineError::ImagePullRejected { status: 500, .. }
        ));
        assert!(!error.to_string().contains("SECRET"));
        server.await.unwrap();
        cleanup(&socket, &directory);
    }

    #[tokio::test]
    async fn rejects_malformed_json_and_oversized_lines() {
        for body in [
            b"not json\n".to_vec(),
            [vec![b' '; MAX_JSON_LINE_BYTES + 1], b"\n".to_vec()].concat(),
        ] {
            let (socket, directory) = temporary_socket();
            let listener = UnixListener::bind(&socket).unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                stream
                    .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                    .await
                    .unwrap();
                if stream.write_all(&body).await.is_ok() {
                    let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
                }
            });
            let mut pull = engine(socket.clone())
                .open_image_pull("public/image:latest")
                .await
                .unwrap();
            assert!(matches!(
                pull.next_progress().await,
                Err(EngineError::ImagePullStreamInterrupted { .. })
            ));
            drop(pull);
            server.await.unwrap();
            cleanup(&socket, &directory);
        }
    }

    #[test]
    fn http_data_frame_limit_accepts_boundary_and_rejects_oversize() {
        assert!(validate_http_data_frame(MAX_HTTP_DATA_FRAME_BYTES).is_ok());
        assert!(matches!(
            validate_http_data_frame(MAX_HTTP_DATA_FRAME_BYTES + 1),
            Err(ImageProgressParseError::Malformed)
        ));
    }
    #[test]
    fn status_text_is_sanitized_and_capped() {
        assert_eq!(sanitize_status("a\nsecret".into()), "a secret");
        assert_eq!(sanitize_status("a\u{202e}b\u{2066}c".into()), "a b c");
        assert_eq!(sanitize_status("a\u{200c}b\u{200d}c".into()), "a b c");
        assert_eq!(
            sanitize_status("a\u{00ad}b\u{034f}c\u{180e}d".into()),
            "a b c d"
        );
        assert_eq!(
            sanitize_status("x".repeat(MAX_STATUS_BYTES + 1)).len(),
            MAX_STATUS_BYTES
        );
    }
}
