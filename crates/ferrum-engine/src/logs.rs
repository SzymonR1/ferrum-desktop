use std::{path::Path, time::Duration};

use ferrum_core::{ContainerLogLine, ContainerLogSource};
use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request, body::Bytes};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use hyperlocal::{UnixConnector, Uri};
use tokio::time::timeout;

use crate::{EngineError, container_request_path};

const LOG_OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_HTTP_DATA_FRAME_BYTES: usize = 1024 * 1024;
const MAX_DOCKER_FRAME_BYTES: usize = 1024 * 1024;
const MAX_LOG_LINE_BYTES: usize = 64 * 1024;

/// Options for a historical or follow container log request. The default
/// requests both output streams with timestamps, without following, and the
/// most recent 500 lines. At least one stream must be selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerLogsOptions {
    /// Include standard output.
    pub stdout: bool,
    /// Include standard error.
    pub stderr: bool,
    /// Ask the engine to prefix lines with timestamps.
    pub timestamps: bool,
    /// Keep the request open for future log output.
    pub follow: bool,
    /// Maximum number of historical lines to return (1..=5000).
    pub tail: u32,
}

impl Default for ContainerLogsOptions {
    fn default() -> Self {
        Self {
            stdout: true,
            stderr: true,
            timestamps: true,
            follow: false,
            tail: 500,
        }
    }
}

/// Incremental stream of decoded container log lines. Docker and HTTP data
/// frames are limited to 1 MiB; decoded UTF-8 lines are limited to 64 KiB.
/// Dropping the stream cancels the response body.
pub struct ContainerLogStream {
    body: hyper::body::Incoming,
    endpoint_label: &'static str,
    tty: bool,
    data: Option<Bytes>,
    data_offset: usize,
    header: [u8; 8],
    header_len: usize,
    payload_remaining: usize,
    payload_source: ContainerLogSource,
    pending_stdout: Vec<u8>,
    pending_stderr: Vec<u8>,
    pending_combined: Vec<u8>,
    pending_order: Vec<ContainerLogSource>,
    finished: bool,
}

pub(crate) async fn open_container_logs(
    socket_path: &Path,
    api_version: &str,
    endpoint_label: &'static str,
    id: &str,
    tty: bool,
    options: ContainerLogsOptions,
) -> Result<ContainerLogStream, EngineError> {
    if (!options.stdout && !options.stderr) || !(1..=5000).contains(&options.tail) {
        return Err(EngineError::InvalidContainerLogsOptions);
    }
    let mut path = container_request_path(api_version, id, "/logs")
        .map_err(|_| EngineError::InvalidContainerId)?;
    use std::fmt::Write as _;
    write!(
        path,
        "?stdout={}&stderr={}&timestamps={}&follow={}&tail={}",
        options.stdout, options.stderr, options.timestamps, options.follow, options.tail
    )
    .map_err(|_| EngineError::ContainerLogsRequestFailed { endpoint_label })?;

    let response = timeout(LOG_OPEN_TIMEOUT, async {
        let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        let request = Request::builder()
            .method(Method::GET)
            .uri(Uri::new(socket_path, &path))
            .body(Empty::<Bytes>::new())
            .map_err(|_| EngineError::ContainerLogsRequestFailed { endpoint_label })?;
        client
            .request(request)
            .await
            .map_err(|_| EngineError::Unavailable { endpoint_label })
    })
    .await
    .map_err(|_| EngineError::ContainerLogsTimedOut { endpoint_label })??;

    if !response.status().is_success() {
        return Err(EngineError::ContainerLogsUnavailable {
            endpoint_label,
            status: response.status().as_u16(),
        });
    }
    Ok(ContainerLogStream {
        body: response.into_body(),
        endpoint_label,
        tty,
        data: None,
        data_offset: 0,
        header: [0; 8],
        header_len: 0,
        payload_remaining: 0,
        payload_source: ContainerLogSource::Combined,
        pending_stdout: Vec::with_capacity(256),
        pending_stderr: Vec::with_capacity(256),
        pending_combined: Vec::with_capacity(256),
        pending_order: Vec::with_capacity(2),
        finished: false,
    })
}

impl ContainerLogStream {
    /// Returns the next complete log line, or `None` when the stream ends.
    /// A final nonempty line without a line terminator is returned at EOF.
    ///
    /// # Errors
    ///
    /// Returns an interrupted error for malformed framing, transport errors,
    /// or data exceeding the documented frame and line limits.
    pub async fn next_line(&mut self) -> Result<Option<ContainerLogLine>, EngineError> {
        loop {
            if self.data.is_some() {
                let byte = self
                    .data
                    .as_ref()
                    .and_then(|data| data.get(self.data_offset).copied());
                if let Some(byte) = byte {
                    self.data_offset += 1;
                    if let Some(line) = self.consume_byte(byte)? {
                        return Ok(Some(line));
                    }
                } else {
                    self.data = None;
                    self.data_offset = 0;
                }
                continue;
            }
            if self.finished {
                if let Some(source) = self.pending_order.first().copied() {
                    self.pending_order.remove(0);
                    return Ok(Some(self.take_line(source)?));
                }
                return Ok(None);
            }
            match self.body.frame().await {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        if data.len() > MAX_HTTP_DATA_FRAME_BYTES {
                            return Err(self.interrupted());
                        }
                        if !data.is_empty() {
                            self.data = Some(data.clone());
                        }
                    }
                }
                Some(Err(_)) => return Err(self.interrupted()),
                None => {
                    self.finished = true;
                    if !self.tty && (self.header_len != 0 || self.payload_remaining != 0) {
                        return Err(self.interrupted());
                    }
                }
            }
        }
    }

    fn consume_byte(&mut self, byte: u8) -> Result<Option<ContainerLogLine>, EngineError> {
        if self.tty {
            return self.consume_payload_byte(byte, ContainerLogSource::Combined);
        }
        if self.payload_remaining == 0 {
            self.header[self.header_len] = byte;
            self.header_len += 1;
            if self.header_len < self.header.len() {
                return Ok(None);
            }
            self.header_len = 0;
            let source = match self.header[0] {
                1 => ContainerLogSource::Stdout,
                2 => ContainerLogSource::Stderr,
                _ => return Err(self.interrupted()),
            };
            if self.header[1..4] != [0, 0, 0] {
                return Err(self.interrupted());
            }
            let length = u32::from_be_bytes(
                self.header[4..8]
                    .try_into()
                    .map_err(|_| self.interrupted())?,
            ) as usize;
            if length > MAX_DOCKER_FRAME_BYTES {
                return Err(self.interrupted());
            }
            self.payload_source = source;
            self.payload_remaining = length;
            return Ok(None);
        }
        self.payload_remaining -= 1;
        self.consume_payload_byte(byte, self.payload_source)
    }

    fn consume_payload_byte(
        &mut self,
        byte: u8,
        source: ContainerLogSource,
    ) -> Result<Option<ContainerLogLine>, EngineError> {
        if byte == b'\n' {
            let text = {
                let pending = self.pending_mut(source);
                if pending.last() == Some(&b'\r') {
                    pending.pop();
                }
                let text = String::from_utf8_lossy(pending).into_owned();
                pending.clear();
                text
            };
            self.pending_order
                .retain(|pending_source| *pending_source != source);
            if text.len() > MAX_LOG_LINE_BYTES {
                return Err(self.interrupted());
            }
            return Ok(Some(ContainerLogLine { source, text }));
        }

        let pending_len = self.pending(source).len();
        if pending_len == 0 {
            self.pending_order.push(source);
        }
        if pending_len == MAX_LOG_LINE_BYTES {
            return Err(self.interrupted());
        }
        self.pending_mut(source).push(byte);
        Ok(None)
    }

    fn pending(&self, source: ContainerLogSource) -> &[u8] {
        match source {
            ContainerLogSource::Stdout => &self.pending_stdout,
            ContainerLogSource::Stderr => &self.pending_stderr,
            ContainerLogSource::Combined => &self.pending_combined,
        }
    }

    fn pending_mut(&mut self, source: ContainerLogSource) -> &mut Vec<u8> {
        match source {
            ContainerLogSource::Stdout => &mut self.pending_stdout,
            ContainerLogSource::Stderr => &mut self.pending_stderr,
            ContainerLogSource::Combined => &mut self.pending_combined,
        }
    }

    fn take_line(&mut self, source: ContainerLogSource) -> Result<ContainerLogLine, EngineError> {
        let text = {
            let pending = self.pending_mut(source);
            let text = String::from_utf8_lossy(pending).into_owned();
            pending.clear();
            text
        };
        if text.len() > MAX_LOG_LINE_BYTES {
            return Err(self.interrupted());
        }
        Ok(ContainerLogLine { source, text })
    }

    fn interrupted(&self) -> EngineError {
        EngineError::ContainerLogStreamInterrupted {
            endpoint_label: self.endpoint_label,
        }
    }
}
