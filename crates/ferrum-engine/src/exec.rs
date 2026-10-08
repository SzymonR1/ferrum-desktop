use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    Method, Request, StatusCode,
    body::Bytes,
    header::{CONNECTION, CONTENT_TYPE, UPGRADE},
};
use hyper_util::{
    client::legacy::Client,
    rt::{TokioExecutor, TokioIo},
};
use hyperlocal::{UnixConnector, Uri};
use serde::Deserialize;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};

use crate::{EngineClient, EngineError};
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CREATE_RESPONSE_BYTES: usize = 64 * 1024;
#[cfg(target_os = "linux")]
pub(crate) async fn open_exec_session(
    client: &EngineClient,
    container_id: &str,
    command: &[String],
    initial_size: TerminalSize,
) -> Result<ExecSession, EngineError> {
    if !valid_id(container_id)
        || command.is_empty()
        || command.iter().any(|part| part.contains('\0'))
    {
        return Err(EngineError::InvalidExecRequest);
    }
    if initial_size.rows == 0 || initial_size.columns == 0 {
        return Err(EngineError::InvalidTerminalSize);
    }
    let container = encode_segment(container_id);
    let endpoint_label = client.info.endpoint_label;
    let create_path = format!("/v{}/containers/{container}/exec", client.api_version);
    let body = serde_json::json!({"AttachStdin":true,"AttachStdout":true,"AttachStderr":true,"Tty":true,"Cmd":command,"ConsoleSize":[initial_size.rows, initial_size.columns]});
    let operation = timeout(HANDSHAKE_TIMEOUT, async {
        let http = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let create = Request::builder().method(Method::POST).uri(Uri::new(&client.socket_path, &create_path)).header(CONTENT_TYPE, "application/json").body(Full::new(Bytes::from(body.to_string()))).map_err(|_| OpenError::Unavailable)?;
        let created = http.request(create).await.map_err(|_| OpenError::Unavailable)?;
        if !created.status().is_success() { return Err(OpenError::Rejected(created.status())); }
        let decoded = Limited::new(created.into_body(), MAX_CREATE_RESPONSE_BYTES).collect().await.map_err(|_| OpenError::Unavailable)?;
        let exec: CreateExecResponse = serde_json::from_slice(&decoded.to_bytes()).map_err(|_| OpenError::Unavailable)?;
        if !valid_id(&exec.id) { return Err(OpenError::Unavailable); }
        let id = encode_segment(&exec.id);
        let start_path = format!("/v{}/exec/{id}/start", client.api_version);
        let start_body = serde_json::json!({"Detach":false,"Tty":true,"ConsoleSize":[initial_size.rows, initial_size.columns]});
        let start = Request::builder().method(Method::POST).uri(Uri::new(&client.socket_path, &start_path)).header(CONTENT_TYPE, "application/json").header(CONNECTION, "Upgrade").header(UPGRADE, "tcp").body(Full::new(Bytes::from(start_body.to_string()))).map_err(|_| OpenError::Unavailable)?;
        let started = http.request(start).await.map_err(|_| OpenError::Unavailable)?;
        if started.status() != StatusCode::SWITCHING_PROTOCOLS { return Err(OpenError::Rejected(started.status())); }
        if started.headers().get(UPGRADE).is_none_or(|value| !value.as_bytes().eq_ignore_ascii_case(b"tcp"))
            || started.headers().get(CONNECTION).is_none_or(|value| !value.as_bytes().eq_ignore_ascii_case(b"upgrade")) {
            return Err(OpenError::Unavailable);
        }
        let upgraded = hyper::upgrade::on(started).await.map_err(|_| OpenError::Unavailable)?;
        Ok((upgraded, exec.id))
    }).await;
    let (io, exec_id) = match operation {
        Err(_) => return Err(EngineError::ExecSessionTimedOut { endpoint_label }),
        Ok(Err(OpenError::Rejected(status))) => {
            return Err(EngineError::ExecSessionRejected {
                endpoint_label,
                status: status.as_u16(),
            });
        }
        Ok(Err(OpenError::Unavailable)) => {
            return Err(EngineError::ExecSessionUnavailable { endpoint_label });
        }
        Ok(Ok(session)) => session,
    };
    let session = ExecSession {
        io: TokioIo::new(io),
        resize: ExecResizeHandle {
            socket_path: client.socket_path.clone(),
            api_version: client.api_version.clone(),
            exec_id,
            endpoint_label,
        },
    };
    Ok(session)
}

/// Initial or updated terminal dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    /// Terminal rows.
    pub rows: u16,
    /// Terminal columns.
    pub columns: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateExecResponse {
    id: String,
}

/// A byte-transparent interactive terminal stream. Dropping it closes only the
/// exec connection; the associated container is not stopped or removed.
pub struct ExecSession {
    io: TokioIo<hyper::upgrade::Upgraded>,
    resize: ExecResizeHandle,
}

impl ExecSession {
    /// Handle used to update this session's terminal dimensions.
    #[must_use]
    pub fn resize_handle(&self) -> ExecResizeHandle {
        self.resize.clone()
    }
}

impl AsyncRead for ExecSession {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for ExecSession {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

/// Cloneable handle for resizing an exec terminal.
#[derive(Clone)]
pub struct ExecResizeHandle {
    socket_path: std::path::PathBuf,
    api_version: String,
    exec_id: String,
    endpoint_label: &'static str,
}

impl ExecResizeHandle {
    /// Updates terminal dimensions through the negotiated local Engine API.
    ///
    /// # Errors
    ///
    /// Returns `EngineError::InvalidTerminalSize` for zero dimensions,
    /// `EngineError::ExecResizeUnavailable` if the engine cannot complete the
    /// resize, or `EngineError::ExecResizeTimedOut` if it exceeds the bounded wait.
    pub async fn resize(&self, size: TerminalSize) -> Result<(), EngineError> {
        if size.rows == 0 || size.columns == 0 {
            return Err(EngineError::InvalidTerminalSize);
        }
        let path = format!(
            "/v{}/exec/{}/resize?h={}&w={}",
            self.api_version, self.exec_id, size.rows, size.columns
        );
        let result = timeout(HANDSHAKE_TIMEOUT, async {
            let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
            let request = Request::builder()
                .method(Method::POST)
                .uri(Uri::new(&self.socket_path, &path))
                .body(Full::<Bytes>::new(Bytes::new()))
                .map_err(|_| ())?;
            let response = client.request(request).await.map_err(|_| ())?;
            let status = response.status();
            Ok::<_, ()>(status)
        })
        .await;
        match result {
            Err(_) => Err(EngineError::ExecResizeTimedOut {
                endpoint_label: self.endpoint_label,
            }),
            Ok(Err(_)) => Err(EngineError::ExecResizeUnavailable {
                endpoint_label: self.endpoint_label,
            }),
            Ok(Ok(status)) if status.is_success() => Ok(()),
            Ok(Ok(_)) => Err(EngineError::ExecResizeUnavailable {
                endpoint_label: self.endpoint_label,
            }),
        }
    }
}

#[derive(Debug)]
enum OpenError {
    Unavailable,
    Rejected(StatusCode),
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::EngineInfo;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{UnixListener, UnixStream},
    };

    async fn request(stream: &mut UnixStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buf[..n]);
            if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let text = String::from_utf8_lossy(&bytes);
                let length = text
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let header = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                if bytes.len() >= header + length {
                    return bytes;
                }
            }
        }
    }

    fn request_line(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes)
            .lines()
            .next()
            .unwrap()
            .to_owned()
    }

    fn socket_path(label: &str) -> std::path::PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ferrum-exec-{label}-{}-{timestamp}.sock",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn creates_upgrades_and_resizes_exec_session() {
        let path = socket_path("session");
        let listener = UnixListener::bind(&path).unwrap();
        let server_path = path.clone();
        let server = tokio::spawn(async move {
            let (mut create, _) = listener.accept().await.unwrap();
            let created = request(&mut create).await;
            assert!(request_line(&created).contains("/v1.52/containers/container-id/exec"));
            let body = String::from_utf8_lossy(&created);
            let json_body = body.split("\r\n\r\n").nth(1).unwrap();
            let config: serde_json::Value = serde_json::from_str(json_body).unwrap();
            assert_eq!(config["AttachStdin"], true);
            assert_eq!(config["AttachStdout"], true);
            assert_eq!(config["AttachStderr"], true);
            assert_eq!(config["Tty"], true);
            assert_eq!(config["Cmd"], serde_json::json!(["/bin/sh", "-l"]));
            assert_eq!(config["ConsoleSize"], serde_json::json!([24, 80]));
            create.write_all(b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 16\r\n\r\n{\"Id\":\"exec-id\"}").await.unwrap();
            drop(create);

            let (mut start, _) = listener.accept().await.unwrap();
            let started = request(&mut start).await;
            let headers = String::from_utf8_lossy(&started).to_ascii_lowercase();
            assert!(request_line(&started).contains("/v1.52/exec/exec-id/start"));
            assert!(headers.contains("connection: upgrade"));
            assert!(headers.contains("upgrade: tcp"));
            let start_body = String::from_utf8_lossy(&started)
                .split("\r\n\r\n")
                .nth(1)
                .unwrap()
                .to_owned();
            let start_config: serde_json::Value = serde_json::from_str(&start_body).unwrap();
            assert_eq!(start_config["Detach"], false);
            assert_eq!(start_config["Tty"], true);
            assert_eq!(start_config["ConsoleSize"], serde_json::json!([24, 80]));
            start.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n").await.unwrap();

            let (mut resize, _) = listener.accept().await.unwrap();
            let resized = request(&mut resize).await;
            assert_eq!(
                request_line(&resized),
                "POST /v1.52/exec/exec-id/resize?h=37&w=101 HTTP/1.1"
            );
            resize
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            drop(resize);

            let mut one = [0u8; 1];
            assert_eq!(start.read(&mut one).await.unwrap(), 0);
            drop(start);
            drop(listener);
            let _ = std::fs::remove_file(server_path);
        });

        let client = EngineClient {
            info: EngineInfo {
                version: "test".into(),
                api_version: "1.52".into(),
                endpoint_label: "test socket",
            },
            api_version: "1.52".into(),
            selected_context: None,
            socket_path: path.clone(),
        };
        let command = vec!["/bin/sh".into(), "-l".into()];
        let session = client
            .open_exec_session(
                "container-id",
                &command,
                TerminalSize {
                    rows: 24,
                    columns: 80,
                },
            )
            .await
            .unwrap();
        session
            .resize_handle()
            .resize(TerminalSize {
                rows: 37,
                columns: 101,
            })
            .await
            .unwrap();
        drop(session);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_request_is_typed_and_secret_safe() {
        let path = socket_path("invalid");
        let client = EngineClient {
            info: EngineInfo {
                version: "test".into(),
                api_version: "1.52".into(),
                endpoint_label: "test socket",
            },
            api_version: "1.52".into(),
            selected_context: None,
            socket_path: path,
        };
        let error = match client
            .open_exec_session(
                "bad/path-secret",
                &["secret-command".into()],
                TerminalSize {
                    rows: 1,
                    columns: 1,
                },
            )
            .await
        {
            Ok(_) => panic!("invalid identifier unexpectedly opened"),
            Err(error) => error,
        };
        assert!(matches!(error, EngineError::InvalidExecRequest));
        assert!(!error.to_string().contains("secret"));
    }
    #[tokio::test]
    async fn cancelling_open_closes_only_the_pending_exec_connection() {
        let path = socket_path("cancel");
        let listener = UnixListener::bind(&path).unwrap();
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = request(&mut socket).await;
            let _ = received_tx.send(());
            let mut byte = [0u8; 1];
            assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
        });
        let client = EngineClient {
            info: EngineInfo {
                version: "test".into(),
                api_version: "1.52".into(),
                endpoint_label: "test socket",
            },
            api_version: "1.52".into(),
            selected_context: None,
            socket_path: path.clone(),
        };
        let opening = tokio::spawn(async move {
            client
                .open_exec_session(
                    "container-id",
                    &["sh".into()],
                    TerminalSize {
                        rows: 24,
                        columns: 80,
                    },
                )
                .await
        });
        received_rx.await.unwrap();
        opening.abort();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn engine_rejection_does_not_expose_response_body() {
        let path = socket_path("reject");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 19\r\n\r\nsecret-daemon-error",
                )
                .await
                .unwrap();
        });
        let client = EngineClient {
            info: EngineInfo {
                version: "test".into(),
                api_version: "1.52".into(),
                endpoint_label: "test socket",
            },
            api_version: "1.52".into(),
            selected_context: None,
            socket_path: path.clone(),
        };
        let error = match client
            .open_exec_session(
                "container-id",
                &["sh".into()],
                TerminalSize {
                    rows: 24,
                    columns: 80,
                },
            )
            .await
        {
            Ok(_) => panic!("rejected request unexpectedly opened"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            EngineError::ExecSessionRejected { status: 403, .. }
        ));
        assert!(!error.to_string().contains("secret"));
        server.await.unwrap();
        let _ = std::fs::remove_file(path);
    }
    #[tokio::test]
    async fn zero_terminal_dimensions_are_rejected_for_open_and_resize() {
        let path = socket_path("size");
        let client = EngineClient {
            info: EngineInfo {
                version: "test".into(),
                api_version: "1.52".into(),
                endpoint_label: "test socket",
            },
            api_version: "1.52".into(),
            selected_context: None,
            socket_path: path.clone(),
        };
        let error = match client
            .open_exec_session(
                "container-id",
                &["sh".into()],
                TerminalSize {
                    rows: 0,
                    columns: 80,
                },
            )
            .await
        {
            Ok(_) => panic!("zero-row terminal unexpectedly opened"),
            Err(error) => error,
        };
        assert!(matches!(error, EngineError::InvalidTerminalSize));

        let resize = ExecResizeHandle {
            socket_path: path,
            api_version: "1.52".into(),
            exec_id: "exec-id".into(),
            endpoint_label: "test socket",
        };
        let error = resize
            .resize(TerminalSize {
                rows: 24,
                columns: 0,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, EngineError::InvalidTerminalSize));
    }
}
