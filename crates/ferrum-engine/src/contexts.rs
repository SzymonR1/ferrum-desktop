//! Safe discovery and resolution of Docker CLI contexts.

use std::{fmt, path::PathBuf, process::Stdio, time::Duration};

use serde::Deserialize;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

const MAX_CONTEXT_OUTPUT: usize = 1024 * 1024;
const CONTEXT_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// Maximum number of ASCII bytes shown for a context name in the UI.
const MAX_CONTEXT_LABEL_BYTES: usize = 64;

/// The transport class supported for a Docker CLI context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextTransport {
    /// A local Unix-domain socket.
    LocalUnix,
    /// A transport requiring remote/TLS/SSH behavior not implemented by Ferrum.
    Unsupported,
    /// An absent, malformed, or otherwise invalid endpoint.
    Invalid,
}

/// A Docker CLI context safe to present in the application.
///
/// The configured endpoint is intentionally private and excluded from debug
/// output. Ferrum supports connecting only to contexts classified `LocalUnix`.
#[derive(Clone)]
pub struct DockerContext {
    name: String,
    display_label: Option<String>,
    current: bool,
    transport: ContextTransport,
    socket_path: Option<PathBuf>,
}

impl fmt::Debug for DockerContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DockerContext")
            .field("name", &self.display_name())
            .field("current", &self.current)
            .field("transport", &self.transport)
            .finish()
    }
}

impl DockerContext {
    /// The validated context name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A bounded label suitable for display without altering the context name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.display_label.as_deref().unwrap_or(&self.name)
    }

    /// Whether the Docker CLI marks this context current.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.current
    }

    /// The endpoint transport classification, without exposing endpoint data.
    #[must_use]
    pub fn transport(&self) -> ContextTransport {
        self.transport
    }

    pub(crate) fn socket_path(&self) -> Option<&std::path::Path> {
        self.socket_path.as_deref()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContextRecord {
    name: String,
    #[serde(default)]
    current: bool,
    #[serde(default)]
    docker_endpoint: Option<String>,
}

/// Parses Docker CLI's newline-delimited `context ls --format '{{json .}}'` output.
///
/// Malformed records and invalid names fail closed. Endpoint contents never
/// appear in returned errors or debug output.
pub(super) fn parse_context_records(
    output: &str,
) -> Result<Vec<DockerContext>, ContextCommandError> {
    let mut contexts = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let record: ContextRecord =
            serde_json::from_str(line).map_err(|_| ContextCommandError::InvalidOutput)?;
        if !valid_context_name(&record.name) {
            return Err(ContextCommandError::InvalidOutput);
        }

        let endpoint = record.docker_endpoint.filter(|value| !value.is_empty());
        let (transport, socket_path) = match endpoint.as_deref() {
            Some(endpoint) if endpoint.starts_with("unix://") => {
                let path = endpoint.strip_prefix("unix://").unwrap_or_default();
                if path.starts_with('/') && !path.contains('\0') {
                    (ContextTransport::LocalUnix, Some(PathBuf::from(path)))
                } else {
                    (ContextTransport::Invalid, None)
                }
            }
            Some(_) => (ContextTransport::Unsupported, None),
            None => (ContextTransport::Invalid, None),
        };
        let display_label = display_context_name(&record.name);
        contexts.push(DockerContext {
            name: record.name,
            display_label,
            current: record.current,
            transport,
            socket_path,
        });
    }

    if contexts.is_empty() {
        return Err(ContextCommandError::InvalidOutput);
    }
    Ok(contexts)
}

/// Startup selection policy matching Docker CLI environment precedence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupSource {
    /// A named context selected explicitly through `DOCKER_CONTEXT`.
    ExplicitContext,
    /// The existing Ferrum `DOCKER_HOST` local-socket path.
    DockerHost,
    /// The CLI's current non-default context.
    CurrentContext,
    /// Existing automatic local socket discovery.
    DefaultDiscovery,
}

/// Resolves startup precedence from injected environment and CLI context data.
#[must_use]
pub(crate) fn resolve_startup_source(
    docker_context: Option<&str>,
    docker_host: Option<&str>,
    current_context: Option<&DockerContext>,
) -> StartupSource {
    if docker_context.is_some_and(|value| !value.is_empty()) {
        StartupSource::ExplicitContext
    } else if docker_host.is_some_and(|value| !value.is_empty()) {
        StartupSource::DockerHost
    } else if current_context.is_some_and(|context| context.current && context.name != "default") {
        StartupSource::CurrentContext
    } else {
        StartupSource::DefaultDiscovery
    }
}

pub(crate) fn current_cli_context(
    contexts: &[DockerContext],
) -> Result<&DockerContext, ContextCommandError> {
    let mut current = contexts.iter().filter(|context| context.is_current());
    let selected = current.next().ok_or(ContextCommandError::InvalidOutput)?;
    if current.next().is_some() {
        return Err(ContextCommandError::InvalidOutput);
    }
    Ok(selected)
}

pub(crate) async fn discover_contexts() -> Result<Vec<DockerContext>, ContextCommandError> {
    timeout(CONTEXT_COMMAND_TIMEOUT, async {
        let mut child = Command::new("docker")
            .args(["context", "ls", "--format", "{{json .}}"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ContextCommandError::CliUnavailable
                } else {
                    ContextCommandError::Unavailable
                }
            })?;
        let Some(stdout) = child.stdout.take() else {
            return Err(ContextCommandError::Unavailable);
        };
        let mut bytes = Vec::with_capacity(4096);
        stdout
            .take((MAX_CONTEXT_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| ContextCommandError::Unavailable)?;
        if bytes.len() > MAX_CONTEXT_OUTPUT {
            return Err(ContextCommandError::OutputTooLarge);
        }
        let status = child
            .wait()
            .await
            .map_err(|_| ContextCommandError::Unavailable)?;
        if !status.success() {
            return Err(ContextCommandError::Unavailable);
        }
        let output = String::from_utf8(bytes).map_err(|_| ContextCommandError::InvalidOutput)?;
        parse_context_records(&output)
    })
    .await
    .map_err(|_| ContextCommandError::TimedOut)?
}

pub(crate) fn valid_context_name(name: &str) -> bool {
    if name == "default" {
        return true;
    }

    let bytes = name.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+'))
}

fn display_context_name(name: &str) -> Option<String> {
    (name.len() > MAX_CONTEXT_LABEL_BYTES).then(|| format!("{}…", &name[..MAX_CONTEXT_LABEL_BYTES]))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContextCommandError {
    CliUnavailable,
    Unavailable,
    TimedOut,
    OutputTooLarge,
    InvalidOutput,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_and_remote_contexts_without_endpoint_in_debug() {
        let contexts = parse_context_records(concat!(
            "{\"Name\":\"default\",\"Current\":true,\"DockerEndpoint\":\"unix:///run/docker.sock\"}\n",
            "{\"Name\":\"remote\",\"Current\":false,\"DockerEndpoint\":\"ssh://user@host\"}\n",
        ))
        .unwrap();
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].transport(), ContextTransport::LocalUnix);
        assert_eq!(contexts[1].transport(), ContextTransport::Unsupported);
        assert!(!format!("{:?}", contexts[0]).contains("/run/docker.sock"));
        assert!(!format!("{:?}", contexts[1]).contains("user@host"));
    }

    #[test]
    fn accepts_documented_plus_sign_context_names() {
        let contexts = parse_context_records(
            "{\"Name\":\"local+aks\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/docker.sock\"}",
        )
        .unwrap();
        assert_eq!(contexts[0].name(), "local+aks");
        assert!(valid_context_name("default"));
        assert!(!valid_context_name("x"));
        assert!(valid_context_name(&format!("ctx{}", "a".repeat(300))));
    }

    #[test]
    fn bounds_display_label_without_truncating_context_identity() {
        let name = format!("ctx{}", "a".repeat(300));
        let record = format!(
            r#"{{"Name":"{name}","Current":false,"DockerEndpoint":"unix:///tmp/docker.sock"}}"#
        );
        let contexts = parse_context_records(&record).unwrap();

        assert_eq!(contexts[0].name(), name);
        assert_eq!(
            contexts[0].display_name().len(),
            MAX_CONTEXT_LABEL_BYTES + "…".len()
        );
        assert!(contexts[0].display_name().ends_with('…'));
    }

    #[test]
    fn rejects_invalid_names_and_malformed_records() {
        assert!(matches!(
            parse_context_records(
                "{\"Name\":\"../secret\",\"Current\":false,\"DockerEndpoint\":\"unix:///tmp/x\"}"
            ),
            Err(ContextCommandError::InvalidOutput)
        ));
        assert!(matches!(
            parse_context_records("not JSON"),
            Err(ContextCommandError::InvalidOutput)
        ));
    }

    #[test]
    fn classifies_missing_and_malformed_endpoints() {
        let contexts = parse_context_records(concat!(
            "{\"Name\":\"missing\",\"Current\":false}\n",
            "{\"Name\":\"bad-path\",\"Current\":false,\"DockerEndpoint\":\"unix://relative\"}",
        ))
        .unwrap();
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].transport(), ContextTransport::Invalid);
        assert_eq!(contexts[1].transport(), ContextTransport::Invalid);
    }

    #[test]
    fn startup_precedence_matches_docker_context_rules() {
        let current = parse_context_records(
            "{\"Name\":\"work\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/docker.sock\"}",
        )
        .unwrap();
        let default = parse_context_records(
            "{\"Name\":\"default\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/docker.sock\"}",
        )
        .unwrap();
        assert_eq!(
            resolve_startup_source(Some("work"), Some("unix:///tmp/host.sock"), current.first()),
            StartupSource::ExplicitContext
        );
        assert_eq!(
            resolve_startup_source(None, Some("unix:///tmp/host.sock"), current.first()),
            StartupSource::DockerHost
        );
        assert_eq!(
            resolve_startup_source(None, None, current.first()),
            StartupSource::CurrentContext
        );
        assert_eq!(
            resolve_startup_source(None, None, default.first()),
            StartupSource::DefaultDiscovery
        );
    }

    #[test]
    fn current_context_requires_exactly_one_active_record() {
        let current = parse_context_records(
            "{\"Name\":\"default\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/docker.sock\"}",
        )
        .unwrap();
        assert_eq!(current_cli_context(&current).unwrap().name(), "default");

        let inactive = parse_context_records(
            "{\"Name\":\"work\",\"Current\":false,\"DockerEndpoint\":\"unix:///tmp/docker.sock\"}",
        )
        .unwrap();
        assert!(matches!(
            current_cli_context(&inactive),
            Err(ContextCommandError::InvalidOutput)
        ));

        let multiple = parse_context_records(concat!(
            "{\"Name\":\"first\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/a.sock\"}\n",
            "{\"Name\":\"second\",\"Current\":true,\"DockerEndpoint\":\"unix:///tmp/b.sock\"}",
        ))
        .unwrap();
        assert!(matches!(
            current_cli_context(&multiple),
            Err(ContextCommandError::InvalidOutput)
        ));
    }
}
