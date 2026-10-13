use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize)]
pub(super) struct AuditRecord {
    schema_version: u32,
    timestamp_unix_ms: u64,
    event: &'static str,
    action: &'static str,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    container_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network_id: Option<String>,
}

impl AuditRecord {
    fn new(
        event: &'static str,
        action: &'static str,
        outcome: &'static str,
        container_id: Option<&str>,
        network_id: Option<&str>,
    ) -> Self {
        Self {
            schema_version: 1,
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            event,
            action,
            outcome,
            container_id: container_id.map(str::to_owned),
            network_id: network_id.map(str::to_owned),
        }
    }
}

#[cfg(target_os = "linux")]
use crate::settings::SettingsStore;
#[cfg(target_os = "linux")]
use std::sync::Arc;

pub(super) const MAX_AUDIT_LOG_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(target_os = "linux")]
const AUDIT_FILE_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(target_os = "linux")]
const AUDIT_FILE_LOCK_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

#[derive(Clone)]
pub(super) struct AuditStore {
    #[cfg(target_os = "linux")]
    settings: Option<Arc<SettingsStore>>,
}

#[derive(Debug)]
pub(super) enum AuditStoreError {
    Full,
    Unavailable,
    Write,
    Serialization,
    Worker,
}

impl AuditStore {
    #[cfg(target_os = "linux")]
    pub(super) fn new(settings: Arc<SettingsStore>) -> Self {
        Self {
            settings: Some(settings),
        }
    }

    #[cfg(any(test, not(target_os = "linux")))]
    pub(super) fn disabled() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            settings: None,
        }
    }

    pub(super) async fn persist(&self, record: AuditRecord) -> Result<(), AuditStoreError> {
        #[cfg(target_os = "linux")]
        {
            let Some(settings) = self.settings.clone() else {
                return Ok(());
            };
            tokio::task::spawn_blocking(move || persist_record(settings, record))
                .await
                .map_err(|_| AuditStoreError::Worker)?
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = record;
            Err(AuditStoreError::Unavailable)
        }
    }
}

#[cfg(target_os = "linux")]
fn persist_record(
    settings: Arc<SettingsStore>,
    record: AuditRecord,
) -> Result<(), AuditStoreError> {
    use std::io::Write;
    let mut line = serde_json::to_vec(&record).map_err(|_| AuditStoreError::Serialization)?;
    line.push(b'\n');
    if line.len() as u64 > MAX_AUDIT_LOG_BYTES {
        return Err(AuditStoreError::Full);
    }
    let mut file = settings
        .open_audit_file()
        .map_err(|_| AuditStoreError::Unavailable)?;
    lock_audit_file(&file)?;
    struct Unlock(std::os::fd::RawFd);
    impl Drop for Unlock {
        fn drop(&mut self) {
            // SAFETY: the audit file remains open until this guard is dropped.
            let _ = unsafe { libc::flock(self.0, libc::LOCK_UN) };
        }
    }
    let _unlock = Unlock(std::os::fd::AsRawFd::as_raw_fd(&file));
    let current = file.metadata().map_err(|_| AuditStoreError::Write)?.len();
    if current.saturating_add(line.len() as u64) > MAX_AUDIT_LOG_BYTES {
        return Err(AuditStoreError::Full);
    }
    if file.write_all(&line).is_err() {
        let _ = file.set_len(current);
        let _ = file.sync_data();
        return Err(AuditStoreError::Write);
    }
    if file.sync_data().is_err() {
        let _ = file.set_len(current);
        let _ = file.sync_data();
        return Err(AuditStoreError::Write);
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn lock_audit_file(file: &std::fs::File) -> Result<(), AuditStoreError> {
    let descriptor = std::os::fd::AsRawFd::as_raw_fd(file);
    let started = std::time::Instant::now();
    loop {
        // SAFETY: `file` owns an open descriptor; `flock` does not take ownership.
        let result = unsafe { libc::flock(descriptor, libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            return Ok(());
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
            return Err(AuditStoreError::Write);
        }
        let remaining = AUDIT_FILE_LOCK_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AuditStoreError::Write);
        }
        std::thread::sleep(remaining.min(AUDIT_FILE_LOCK_RETRY_INTERVAL));
    }
}
use ferrum_core::ContainerAction;
#[cfg(target_os = "linux")]
use ferrum_core::VolumeDetails;
#[cfg(target_os = "linux")]
use ferrum_engine::EngineError;

/// Emits an ephemeral trace event and returns the redacted lifecycle outcome
/// record for durable local persistence.
pub(super) fn record_container_action_result(
    container_id: &str,
    action: ContainerAction,
    engine_confirmed: bool,
) -> AuditRecord {
    let container_id = valid_resource_id(container_id);
    if engine_confirmed {
        tracing::info!(
            target: "ferrum::audit",
            audit_event = "container_action",
            action = action.label(),
            container_id = container_id.unwrap_or("unavailable"),
            outcome = "success",
            "Container lifecycle action completed"
        );
    } else {
        tracing::warn!(
            target: "ferrum::audit",
            audit_event = "container_action",
            action = action.label(),
            container_id = container_id.unwrap_or("unavailable"),
            outcome = "request_error",
            "Container lifecycle request returned an error"
        );
    }
    AuditRecord::new(
        "container_action",
        action.label(),
        if engine_confirmed {
            "success"
        } else {
            "request_error"
        },
        container_id,
        None,
    )
}
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ComposeMutationAction {
    Up,
    Start,
    Stop,
    Restart,
}

#[cfg(target_os = "linux")]
impl ComposeMutationAction {
    fn label(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ComposeMutationOutcome {
    Started,
    Success,
    Uncertain,
    NotSubmitted,
    RequestError,
}

#[cfg(target_os = "linux")]
impl ComposeMutationOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Success => "success",
            Self::NotSubmitted => "not_submitted",
            Self::Uncertain => "uncertain",
            Self::RequestError => "request_error",
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn record_compose_mutation_result(
    action: ComposeMutationAction,
    outcome: ComposeMutationOutcome,
) -> AuditRecord {
    let outcome_label = outcome.label();
    match outcome {
        ComposeMutationOutcome::Started => tracing::info!(
            target: "ferrum::audit",
            audit_event = "compose_mutation",
            action = action.label(),
            outcome = outcome_label,
            "Compose lifecycle attempt started"
        ),
        ComposeMutationOutcome::Success => tracing::info!(
            target: "ferrum::audit",
            audit_event = "compose_mutation",
            action = action.label(),
            outcome = outcome_label,
            "Compose lifecycle command completed"
        ),
        ComposeMutationOutcome::NotSubmitted | ComposeMutationOutcome::RequestError => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "compose_mutation",
                action = action.label(),
                outcome = outcome_label,
                "Compose lifecycle command did not complete successfully"
            );
        }
        ComposeMutationOutcome::Uncertain => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_mutation",
            action = action.label(),
            outcome = outcome_label,
            "Compose lifecycle command outcome is uncertain"
        ),
    }
    AuditRecord::new(
        "compose_mutation",
        action.label(),
        outcome_label,
        None,
        None,
    )
}
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ComposePullOutcome {
    Started,
    Success,
    NotSubmitted,
    RequestError,
    Uncertain,
}

#[cfg(target_os = "linux")]
impl ComposePullOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Success => "success",
            Self::NotSubmitted => "not_submitted",
            Self::RequestError => "request_error",
            Self::Uncertain => "uncertain",
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn record_compose_pull_result(outcome: ComposePullOutcome) -> AuditRecord {
    let outcome_label = outcome.label();
    match outcome {
        ComposePullOutcome::Started => tracing::info!(
            target: "ferrum::audit",
            audit_event = "compose_pull",
            action = "pull",
            outcome = outcome_label,
            "Compose pull attempt started"
        ),
        ComposePullOutcome::Success => tracing::info!(
            target: "ferrum::audit",
            audit_event = "compose_pull",
            action = "pull",
            outcome = outcome_label,
            "Compose pull completed"
        ),
        ComposePullOutcome::NotSubmitted => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_pull",
            action = "pull",
            outcome = outcome_label,
            "Compose pull was not submitted"
        ),
        ComposePullOutcome::RequestError => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_pull",
            action = "pull",
            outcome = outcome_label,
            "Compose pull returned an error"
        ),
        ComposePullOutcome::Uncertain => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_pull",
            action = "pull",
            outcome = outcome_label,
            "Compose pull outcome is uncertain"
        ),
    }
    AuditRecord::new("compose_pull", "pull", outcome_label, None, None)
}
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ComposeBuildOutcome {
    Started,
    Success,
    NotSubmitted,
    RequestError,
    Uncertain,
}

#[cfg(target_os = "linux")]
impl ComposeBuildOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Success => "success",
            Self::NotSubmitted => "not_submitted",
            Self::RequestError => "request_error",
            Self::Uncertain => "uncertain",
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn record_compose_build_result(outcome: ComposeBuildOutcome) -> AuditRecord {
    let outcome_label = outcome.label();
    match outcome {
        ComposeBuildOutcome::Started => tracing::info!(
            target: "ferrum::audit",
            audit_event = "compose_build",
            action = "build",
            outcome = outcome_label,
            "Compose build attempt started"
        ),
        ComposeBuildOutcome::Success => tracing::info!(
            target: "ferrum::audit",
            audit_event = "compose_build",
            action = "build",
            outcome = outcome_label,
            "Compose build completed"
        ),
        ComposeBuildOutcome::NotSubmitted => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_build",
            action = "build",
            outcome = outcome_label,
            "Compose build was not submitted"
        ),
        ComposeBuildOutcome::RequestError => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_build",
            action = "build",
            outcome = outcome_label,
            "Compose build returned an error"
        ),
        ComposeBuildOutcome::Uncertain => tracing::warn!(
            target: "ferrum::audit",
            audit_event = "compose_build",
            action = "build",
            outcome = outcome_label,
            "Compose build outcome is uncertain"
        ),
    }
    AuditRecord::new("compose_build", "build", outcome_label, None, None)
}

#[derive(Clone, Copy)]
pub(super) enum NetworkMutation {
    Create,
    Remove,
    Connect,
    Disconnect,
}

impl NetworkMutation {
    fn label(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Remove => "remove",
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
        }
    }
}

/// Emits an ephemeral trace event and returns the redacted network outcome
/// record for durable local persistence.
pub(super) fn record_network_mutation_result(
    network_id: Option<&str>,
    action: NetworkMutation,
    request_succeeded: bool,
) -> AuditRecord {
    let validated_network_id = network_id.and_then(valid_resource_id);
    let logged_network_id = validated_network_id.unwrap_or("unavailable");
    if request_succeeded {
        tracing::info!(
            target: "ferrum::audit",
            audit_event = "network_mutation",
            action = action.label(),
            network_id = logged_network_id,
            outcome = "success",
            "Network mutation request succeeded"
        );
    } else {
        tracing::warn!(
            target: "ferrum::audit",
            audit_event = "network_mutation",
            action = action.label(),
            network_id = logged_network_id,
            outcome = "request_error",
            "Network mutation request returned an error"
        );
    }
    AuditRecord::new(
        "network_mutation",
        action.label(),
        if request_succeeded {
            "success"
        } else {
            "request_error"
        },
        None,
        validated_network_id,
    )
}

pub(super) fn record_network_attachment_result(
    network_id: &str,
    container_id: &str,
    action: NetworkMutation,
    request_succeeded: bool,
) -> AuditRecord {
    let validated_network_id = valid_resource_id(network_id);
    let validated_container_id = valid_resource_id(container_id);
    let logged_network_id = validated_network_id.unwrap_or("unavailable");
    let logged_container_id = validated_container_id.unwrap_or("unavailable");
    let outcome = if request_succeeded {
        "success"
    } else {
        "request_error"
    };
    if request_succeeded {
        tracing::info!(
            target: "ferrum::audit",
            audit_event = "network_mutation",
            action = action.label(),
            network_id = logged_network_id,
            container_id = logged_container_id,
            outcome,
            "Network attachment request returned"
        );
    } else {
        tracing::warn!(
            target: "ferrum::audit",
            audit_event = "network_mutation",
            action = action.label(),
            network_id = logged_network_id,
            container_id = logged_container_id,
            outcome,
            "Network attachment request returned"
        );
    }
    AuditRecord::new(
        "network_mutation",
        action.label(),
        outcome,
        validated_container_id,
        validated_network_id,
    )
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ImageMutation {
    Pull,
    Push,
    Tag,
    Remove,
}

#[cfg(target_os = "linux")]
impl ImageMutation {
    fn label(self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Tag => "tag",
            Self::Remove => "remove",
        }
    }
}

/// Emits an ephemeral trace event and returns the redacted image outcome
/// record for durable local persistence. Image identifiers, references, and
/// repository tags are never logged.
#[cfg(target_os = "linux")]
pub(super) fn record_image_mutation_result(
    action: ImageMutation,
    result: &Result<(), EngineError>,
) -> AuditRecord {
    let outcome = match result {
        Ok(()) => {
            tracing::info!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = action.label(),
                outcome = "success",
                "Image mutation request succeeded"
            );
            "success"
        }
        Err(
            EngineError::InvalidImageId
            | EngineError::InvalidImageReference
            | EngineError::InvalidImageTagTarget
            | EngineError::ImagePullCredentialUnavailable
            | EngineError::ImagePushCredentialUnavailable
            | EngineError::ImagePushSourceUnavailable { .. }
            | EngineError::ImagePushSourceChanged { .. },
        ) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = action.label(),
                outcome = "not_submitted",
                "Image mutation input was rejected before request submission"
            );
            "not_submitted"
        }
        Err(_) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = action.label(),
                outcome = "request_error",
                "Image mutation request returned an error"
            );
            "request_error"
        }
    };
    AuditRecord::new("image_mutation", action.label(), outcome, None, None)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ImageBuildOutcome {
    Success,
    NotSubmitted,
    RequestError,
}

#[cfg(target_os = "linux")]
pub(super) fn record_image_build_result(outcome: ImageBuildOutcome) -> AuditRecord {
    let outcome = match outcome {
        ImageBuildOutcome::Success => {
            tracing::info!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = "build",
                outcome = "success",
                "Image build completed"
            );
            "success"
        }
        ImageBuildOutcome::NotSubmitted => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = "build",
                outcome = "not_submitted",
                "Image build was not submitted"
            );
            "not_submitted"
        }
        ImageBuildOutcome::RequestError => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = "build",
                outcome = "request_error",
                "Image build request failed"
            );
            "request_error"
        }
    };
    AuditRecord::new("image_mutation", "build", outcome, None, None)
}
#[cfg(target_os = "linux")]
/// Returns the redacted volume-create outcome after emitting its trace event.
pub(super) fn record_volume_create_result(
    result: &Result<VolumeDetails, EngineError>,
) -> AuditRecord {
    record_volume_mutation_result("create", result)
}

#[cfg(target_os = "linux")]
/// Returns the redacted volume-removal outcome after emitting its trace event.
pub(super) fn record_volume_remove_result(result: &Result<(), EngineError>) -> AuditRecord {
    record_volume_mutation_result("remove", result)
}

#[cfg(target_os = "linux")]
fn record_volume_mutation_result<T>(
    action: &'static str,
    result: &Result<T, EngineError>,
) -> AuditRecord {
    let outcome = match result {
        Ok(_) => {
            tracing::info!(
                target: "ferrum::audit",
                audit_event = "volume_mutation",
                action,
                outcome = "success",
                "Volume mutation request succeeded"
            );
            "success"
        }
        Err(EngineError::InvalidVolumeName) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "volume_mutation",
                action,
                outcome = "not_submitted",
                "Volume mutation input was rejected before request submission"
            );
            "not_submitted"
        }
        Err(_) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "volume_mutation",
                action,
                outcome = "request_error",
                "Volume mutation request returned an error"
            );
            "request_error"
        }
    };
    AuditRecord::new("volume_mutation", action, outcome, None, None)
}

fn valid_resource_id(resource_id: &str) -> Option<&str> {
    (resource_id.len() == 64 && resource_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(resource_id)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::record_volume_create_result;
    #[cfg(target_os = "linux")]
    use super::record_volume_remove_result;
    #[cfg(target_os = "linux")]
    use super::{ComposeMutationAction, ComposeMutationOutcome, record_compose_mutation_result};
    #[cfg(target_os = "linux")]
    use super::{
        ImageBuildOutcome, ImageMutation, record_image_build_result, record_image_mutation_result,
    };
    use super::{
        NetworkMutation, record_container_action_result, record_network_attachment_result,
        record_network_mutation_result,
    };
    use ferrum_core::ContainerAction;
    #[cfg(target_os = "linux")]
    use ferrum_engine::EngineError;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl std::io::Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let mut output = match self.0.lock() {
                Ok(output) => output,
                Err(poisoned) => poisoned.into_inner(),
            };
            output.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn records_outcomes_with_validated_ids_and_redacts_untrusted_ids() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let valid_id = "a".repeat(64);
        let malformed_id = "container\nTOKEN=sentinel";

        tracing::dispatcher::with_default(&dispatcher, || {
            record_container_action_result(&valid_id, ContainerAction::Remove, true);
            record_container_action_result(malformed_id, ContainerAction::Stop, false);
        });

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let output = String::from_utf8(captured).expect("audit events should be UTF-8");
        assert_eq!(output.lines().count(), 2);
        assert!(output.contains("audit_event=\"container_action\""));
        assert!(output.contains("action=\"Remove\""));
        assert!(output.contains("outcome=\"success\""));
        assert!(output.contains(&format!("container_id=\"{valid_id}\"")));
        assert!(output.contains("action=\"Stop\""));
        assert!(output.contains("outcome=\"request_error\""));
        assert!(output.contains("container_id=\"unavailable\""));
        assert!(!output.contains("TOKEN=sentinel"));
    }
    #[test]
    fn records_network_mutation_outcomes_with_validated_ids() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let valid_id = "b".repeat(64);
        let malformed_id = "network\nTOKEN=sentinel";
        let short_id = "c".repeat(63);
        let non_hex_id = "g".repeat(64);
        let long_id = "d".repeat(65);

        tracing::dispatcher::with_default(&dispatcher, || {
            record_network_mutation_result(Some(&valid_id), NetworkMutation::Create, true);
            record_network_mutation_result(Some(&valid_id), NetworkMutation::Remove, false);
            record_network_mutation_result(Some(malformed_id), NetworkMutation::Remove, false);
            record_network_mutation_result(Some(&short_id), NetworkMutation::Remove, false);
            record_network_mutation_result(Some(&non_hex_id), NetworkMutation::Create, false);
            record_network_mutation_result(Some(&long_id), NetworkMutation::Remove, false);
            record_network_mutation_result(None, NetworkMutation::Create, false);
            record_network_attachment_result(&valid_id, &valid_id, NetworkMutation::Connect, true);
            record_network_attachment_result(
                &valid_id,
                &valid_id,
                NetworkMutation::Disconnect,
                false,
            );
        });

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let output = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = output.lines().collect::<Vec<_>>();
        let expected = [
            ("create", valid_id.as_str(), "success"),
            ("remove", valid_id.as_str(), "request_error"),
            ("remove", "unavailable", "request_error"),
            ("remove", "unavailable", "request_error"),
            ("create", "unavailable", "request_error"),
            ("remove", "unavailable", "request_error"),
            ("create", "unavailable", "request_error"),
            ("connect", valid_id.as_str(), "success"),
            ("disconnect", valid_id.as_str(), "request_error"),
        ];
        assert_eq!(events.len(), expected.len());
        for (line, (action, network_id, outcome)) in events.iter().zip(expected) {
            assert!(line.contains("audit_event=\"network_mutation\""));
            assert!(line.contains(&format!("action=\"{action}\"")));
            assert!(line.contains(&format!("network_id=\"{network_id}\"")));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
        }
        assert!(!output.contains("TOKEN=sentinel"));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn compose_mutation_audit_is_static_and_contains_no_project_metadata() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let attempts = [
            (
                ComposeMutationAction::Start,
                ComposeMutationOutcome::Started,
                "start",
                "started",
            ),
            (
                ComposeMutationAction::Start,
                ComposeMutationOutcome::Success,
                "start",
                "success",
            ),
            (
                ComposeMutationAction::Restart,
                ComposeMutationOutcome::Started,
                "restart",
                "started",
            ),
            (
                ComposeMutationAction::Restart,
                ComposeMutationOutcome::Success,
                "restart",
                "success",
            ),
            (
                ComposeMutationAction::Stop,
                ComposeMutationOutcome::NotSubmitted,
                "stop",
                "not_submitted",
            ),
            (
                ComposeMutationAction::Start,
                ComposeMutationOutcome::RequestError,
                "start",
                "request_error",
            ),
            (
                ComposeMutationAction::Up,
                ComposeMutationOutcome::Started,
                "up",
                "started",
            ),
            (
                ComposeMutationAction::Up,
                ComposeMutationOutcome::Success,
                "up",
                "success",
            ),
            (
                ComposeMutationAction::Up,
                ComposeMutationOutcome::Uncertain,
                "up",
                "uncertain",
            ),
        ];
        let records = tracing::dispatcher::with_default(&dispatcher, || {
            attempts.map(|(action, outcome, _, _)| {
                serde_json::to_string(&record_compose_mutation_result(action, outcome))
                    .expect("the static Compose audit record should serialize")
            })
        });

        for ((_, _, action, outcome), record) in attempts.iter().zip(&records) {
            assert!(record.contains("\"event\":\"compose_mutation\""));
            assert!(record.contains(&format!("\"action\":\"{action}\"")));
            assert!(record.contains(&format!("\"outcome\":\"{outcome}\"")));
            assert!(!record.contains("container_id"));
            assert!(!record.contains("network_id"));
            assert!(!record.contains("project"));
            assert!(!record.contains("path"));
            assert!(!record.contains("service"));
            assert!(!record.contains("diagnostic"));
        }
        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let events = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = events.lines().collect::<Vec<_>>();
        assert_eq!(events.len(), attempts.len());
        for (line, (_, _, action, outcome)) in events.iter().zip(attempts) {
            assert!(line.contains("audit_event=\"compose_mutation\""));
            assert!(line.contains(&format!("action=\"{action}\"")));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
            assert!(!line.contains("label="));
            assert!(!line.contains("path="));
            assert!(!line.contains("project="));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn compose_pull_audit_is_static_and_records_uncertain_outcome() {
        use super::{ComposePullOutcome, record_compose_pull_result};

        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let attempts = [
            (ComposePullOutcome::Started, "started"),
            (ComposePullOutcome::Success, "success"),
            (ComposePullOutcome::NotSubmitted, "not_submitted"),
            (ComposePullOutcome::RequestError, "request_error"),
            (ComposePullOutcome::Uncertain, "uncertain"),
        ];
        let records = tracing::dispatcher::with_default(&dispatcher, || {
            attempts.map(|(outcome, _)| {
                serde_json::to_string(&record_compose_pull_result(outcome))
                    .expect("the static Compose pull audit record should serialize")
            })
        });

        for ((_, outcome), record) in attempts.iter().zip(&records) {
            assert!(record.contains("\"event\":\"compose_pull\""));
            assert!(record.contains("\"action\":\"pull\""));
            assert!(record.contains(&format!("\"outcome\":\"{outcome}\"")));
            for private_field in [
                "label",
                "path",
                "file",
                "service",
                "image",
                "credential",
                "output",
            ] {
                assert!(!record.contains(private_field));
            }
        }

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let events = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = events.lines().collect::<Vec<_>>();
        assert_eq!(events.len(), attempts.len());
        for (line, (_, outcome)) in events.iter().zip(attempts) {
            assert!(line.contains("audit_event=\"compose_pull\""));
            assert!(line.contains("action=\"pull\""));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
            for private_field in [
                "label=",
                "path=",
                "file=",
                "service=",
                "image=",
                "credential=",
                "output=",
            ] {
                assert!(!line.contains(private_field));
            }
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn compose_build_audit_is_static_and_redacted() {
        use super::{ComposeBuildOutcome, record_compose_build_result};

        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let attempts = [
            (ComposeBuildOutcome::Started, "started"),
            (ComposeBuildOutcome::Success, "success"),
            (ComposeBuildOutcome::NotSubmitted, "not_submitted"),
            (ComposeBuildOutcome::RequestError, "request_error"),
            (ComposeBuildOutcome::Uncertain, "uncertain"),
        ];
        let records = tracing::dispatcher::with_default(&dispatcher, || {
            attempts.map(|(outcome, _)| {
                serde_json::to_string(&record_compose_build_result(outcome))
                    .expect("the static Compose build audit record should serialize")
            })
        });
        for ((_, outcome), record) in attempts.iter().zip(&records) {
            assert!(record.contains("\"event\":\"compose_build\""));
            assert!(record.contains("\"action\":\"build\""));
            assert!(record.contains(&format!("\"outcome\":\"{outcome}\"")));
            for private_field in [
                "project",
                "file",
                "service",
                "context",
                "image",
                "credential",
                "output",
            ] {
                assert!(!record.contains(private_field));
            }
        }
        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let events = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = events.lines().collect::<Vec<_>>();
        assert_eq!(events.len(), attempts.len());
        for (line, (_, outcome)) in events.iter().zip(attempts) {
            assert!(line.contains("audit_event=\"compose_build\""));
            assert!(line.contains("action=\"build\""));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
            assert!(!line.contains("project="));
            assert!(!line.contains("context="));
            assert!(!line.contains("image="));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn records_image_mutation_action_outcomes_without_resource_metadata() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);

        let succeeded = Ok::<(), EngineError>(());
        let invalid_pull = Err(EngineError::InvalidImageReference);
        let credential_helper_failed = Err(EngineError::ImagePullCredentialUnavailable);
        let rejected_pull = Err(EngineError::ImagePullRejected {
            endpoint_label: "local Docker Unix socket",
            status: 500,
        });
        let invalid_tag = Err(EngineError::InvalidImageTagTarget);
        let rejected_tag = Err(EngineError::ImageTagRejected {
            endpoint_label: "local Docker Unix socket",
            status: 409,
        });
        let invalid_remove = Err(EngineError::InvalidImageId);
        let rejected_remove = Err(EngineError::ImageRemovalRejected {
            endpoint_label: "local Docker Unix socket",
            status: 409,
        });
        let rejected_push = Err(EngineError::ImagePushRejected {
            endpoint_label: "local Docker Unix socket",
            status: 401,
        });
        let unverified_push = Err(EngineError::ImagePushSourceChanged {
            endpoint_label: "local Docker Unix socket",
        });
        let push_credential_helper_failed = Err(EngineError::ImagePushCredentialUnavailable);

        tracing::dispatcher::with_default(&dispatcher, || {
            record_image_mutation_result(ImageMutation::Pull, &succeeded);
            record_image_mutation_result(ImageMutation::Pull, &invalid_pull);
            record_image_mutation_result(ImageMutation::Pull, &credential_helper_failed);
            record_image_mutation_result(ImageMutation::Pull, &rejected_pull);
            record_image_mutation_result(ImageMutation::Tag, &succeeded);
            record_image_mutation_result(ImageMutation::Tag, &invalid_tag);
            record_image_mutation_result(ImageMutation::Tag, &rejected_tag);
            record_image_mutation_result(ImageMutation::Remove, &succeeded);
            record_image_mutation_result(ImageMutation::Remove, &invalid_remove);
            record_image_mutation_result(ImageMutation::Remove, &rejected_remove);
            record_image_mutation_result(ImageMutation::Push, &succeeded);
            record_image_mutation_result(ImageMutation::Push, &rejected_push);
            record_image_mutation_result(ImageMutation::Push, &unverified_push);
            record_image_mutation_result(ImageMutation::Push, &push_credential_helper_failed);
        });

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let output = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = output.lines().collect::<Vec<_>>();
        let expected = [
            ("pull", "success"),
            ("pull", "not_submitted"),
            ("pull", "not_submitted"),
            ("pull", "request_error"),
            ("tag", "success"),
            ("tag", "not_submitted"),
            ("tag", "request_error"),
            ("remove", "success"),
            ("remove", "not_submitted"),
            ("remove", "request_error"),
            ("push", "success"),
            ("push", "request_error"),
            ("push", "not_submitted"),
            ("push", "not_submitted"),
        ];
        assert_eq!(events.len(), expected.len());
        for (line, (action, outcome)) in events.iter().zip(expected) {
            assert!(line.contains("audit_event=\"image_mutation\""));
            assert!(line.contains(&format!("action=\"{action}\"")));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
            assert!(!line.contains("image_id="));
            assert!(!line.contains("reference="));
            assert!(!line.contains("repository="));
            assert!(!line.contains("tag="));
            assert!(!line.contains("endpoint_label="));
            assert!(!line.contains("local Docker Unix socket"));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn records_static_image_build_outcomes() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatcher, || {
            record_image_build_result(ImageBuildOutcome::Success);
            record_image_build_result(ImageBuildOutcome::NotSubmitted);
            record_image_build_result(ImageBuildOutcome::RequestError);
        });

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let output = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = output.lines().collect::<Vec<_>>();
        let expected = ["success", "not_submitted", "request_error"];
        assert_eq!(events.len(), expected.len());
        for (line, outcome) in events.iter().zip(expected) {
            assert!(line.contains("audit_event=\"image_mutation\""));
            assert!(line.contains("action=\"build\""));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn records_volume_create_outcomes_without_resource_metadata() {
        use ferrum_core::VolumeDetails;

        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let success = Ok(VolumeDetails {
            name: "private-volume-name".to_owned(),
            driver: Some("local".to_owned()),
            scope: Some("local".to_owned()),
            created_at: Some("private-timestamp".to_owned()),
            mountpoint: Some("/host/private/path".to_owned()),
        });
        let invalid = Err(EngineError::InvalidVolumeName);
        let rejected = Err(EngineError::VolumeCreationRejected {
            endpoint_label: "local Docker Unix socket",
            status: 409,
        });

        tracing::dispatcher::with_default(&dispatcher, || {
            record_volume_create_result(&success);
            record_volume_create_result(&invalid);
            record_volume_create_result(&rejected);
        });

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let output = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = output.lines().collect::<Vec<_>>();
        let expected = ["success", "not_submitted", "request_error"];
        assert_eq!(events.len(), expected.len());
        for (line, outcome) in events.iter().zip(expected) {
            assert!(line.contains("audit_event=\"volume_mutation\""));
            assert!(line.contains("action=\"create\""));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
        }
        for sensitive_value in [
            "private-volume-name",
            "private-timestamp",
            "/host/private/path",
            "local Docker Unix socket",
            "endpoint_label",
            "mountpoint",
        ] {
            assert!(!output.contains(sensitive_value));
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn records_volume_removal_outcomes_without_resource_metadata() {
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);
        let success = Ok::<(), EngineError>(());
        let invalid = Err(EngineError::InvalidVolumeName);
        let rejected = Err(EngineError::VolumeRemovalRejected {
            endpoint_label: "local Docker Unix socket",
            status: 409,
        });

        tracing::dispatcher::with_default(&dispatcher, || {
            record_volume_remove_result(&success);
            record_volume_remove_result(&invalid);
            record_volume_remove_result(&rejected);
        });

        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let output = String::from_utf8(captured).expect("audit events should be UTF-8");
        let events = output.lines().collect::<Vec<_>>();
        let expected = ["success", "not_submitted", "request_error"];
        assert_eq!(events.len(), expected.len());
        for (line, outcome) in events.iter().zip(expected) {
            assert!(line.contains("audit_event=\"volume_mutation\""));
            assert!(line.contains("action=\"remove\""));
            assert!(line.contains(&format!("outcome=\"{outcome}\"")));
        }
        for sensitive_value in [
            "local Docker Unix socket",
            "endpoint_label",
            "volume-secret",
        ] {
            assert!(!output.contains(sensitive_value));
        }
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn persists_network_attachment_outcomes_without_invalid_ids() {
        use super::{AuditStore, NetworkMutation, record_network_attachment_result};
        use crate::settings::SettingsStore;
        use std::fs;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ferrum-audit-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = AuditStore::new(Arc::new(SettingsStore::at(root.join("settings.toml"))));
        let valid_network_id = "a".repeat(64);
        let valid_container_id = "b".repeat(64);
        let invalid_network_id = "invalid-network-ID-SECRET";
        let invalid_container_id = "invalid-container-ID-SECRET";
        let writer = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer.clone())
            .finish();
        let dispatcher = tracing::Dispatch::new(subscriber);

        for (network_id, container_id, action, succeeded) in [
            (
                invalid_network_id,
                valid_container_id.as_str(),
                NetworkMutation::Connect,
                true,
            ),
            (
                valid_network_id.as_str(),
                invalid_container_id,
                NetworkMutation::Disconnect,
                false,
            ),
        ] {
            let record = tracing::dispatcher::with_default(&dispatcher, || {
                record_network_attachment_result(network_id, container_id, action, succeeded)
            });
            store.persist(record).await.unwrap();
        }

        let log_path = root.join("audit.jsonl");
        let contents = fs::read_to_string(&log_path).unwrap();
        let records = contents
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let captured = match writer.0.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let trace = String::from_utf8(captured).expect("audit events should be UTF-8");
        let _ = fs::remove_dir_all(&root);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["event"], "network_mutation");
        assert_eq!(records[0]["action"], "connect");
        assert_eq!(records[0]["outcome"], "success");
        assert_eq!(records[0]["container_id"], valid_container_id);
        assert!(records[0].get("network_id").is_none());
        assert_eq!(records[1]["event"], "network_mutation");
        assert_eq!(records[1]["action"], "disconnect");
        assert_eq!(records[1]["outcome"], "request_error");
        assert_eq!(records[1]["network_id"], valid_network_id);
        assert!(records[1].get("container_id").is_none());
        assert!(!contents.contains(invalid_network_id));
        assert!(!contents.contains(invalid_container_id));
        assert_eq!(trace.lines().count(), 2);
        assert!(trace.contains("network_id=\"unavailable\""));
        assert!(trace.contains(&format!("container_id=\"{valid_container_id}\"")));
        assert!(trace.contains(&format!("network_id=\"{valid_network_id}\"")));
        assert!(trace.contains("container_id=\"unavailable\""));
        assert!(!trace.contains(invalid_network_id));
        assert!(!trace.contains(invalid_container_id));
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn persists_redacted_audit_records_for_mutation_outcomes() {
        use super::{
            AuditStore, ImageBuildOutcome, ImageMutation, record_image_build_result,
            record_image_mutation_result, record_network_mutation_result,
            record_volume_create_result,
        };
        use crate::settings::SettingsStore;
        use ferrum_core::VolumeDetails;
        use std::{fs, os::unix::fs::PermissionsExt};

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ferrum-audit-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = AuditStore::new(Arc::new(SettingsStore::at(root.join("settings.toml"))));
        let container_id = "a".repeat(64);
        let network_id = "b".repeat(64);
        let invalid_id = "invalid-container-ID-SECRET";

        store
            .persist(record_container_action_result(
                &container_id,
                ContainerAction::Remove,
                true,
            ))
            .await
            .unwrap();
        store
            .persist(record_container_action_result(
                invalid_id,
                ContainerAction::Stop,
                false,
            ))
            .await
            .unwrap();
        store
            .persist(record_image_mutation_result(
                ImageMutation::Pull,
                &Err(EngineError::ImagePullRejected {
                    endpoint_label: "endpoint-SENTINEL",
                    status: 500,
                }),
            ))
            .await
            .unwrap();
        store
            .persist(record_volume_create_result(&Ok(VolumeDetails {
                name: "volume-name-SENTINEL".to_owned(),
                driver: Some("volume-driver-SENTINEL".to_owned()),
                scope: Some("volume-scope-SENTINEL".to_owned()),
                created_at: Some("volume-time-SENTINEL".to_owned()),
                mountpoint: Some("/volume/path-SENTINEL".to_owned()),
            })))
            .await
            .unwrap();
        store
            .persist(record_network_mutation_result(
                Some(&network_id),
                NetworkMutation::Create,
                true,
            ))
            .await
            .unwrap();
        store
            .persist(record_network_attachment_result(
                &network_id,
                &container_id,
                NetworkMutation::Disconnect,
                false,
            ))
            .await
            .unwrap();
        store
            .persist(record_image_mutation_result(
                ImageMutation::Push,
                &Err(EngineError::ImagePushRejected {
                    endpoint_label: "push-endpoint-SENTINEL",
                    status: 401,
                }),
            ))
            .await
            .unwrap();
        for outcome in [
            ImageBuildOutcome::Success,
            ImageBuildOutcome::NotSubmitted,
            ImageBuildOutcome::RequestError,
        ] {
            store
                .persist(record_image_build_result(outcome))
                .await
                .unwrap();
        }
        store
            .persist(record_image_mutation_result(
                ImageMutation::Pull,
                &Err(EngineError::ImagePullCredentialUnavailable),
            ))
            .await
            .unwrap();
        store
            .persist(record_image_mutation_result(
                ImageMutation::Push,
                &Err(EngineError::ImagePushCredentialUnavailable),
            ))
            .await
            .unwrap();

        let log_path = root.join("audit.jsonl");
        let contents = fs::read_to_string(&log_path).unwrap();
        let records = contents
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let directory_mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(&log_path).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_dir_all(&root);

        assert_eq!(records.len(), 12);
        assert!(contents.ends_with('\n'));
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        for record in &records {
            assert_eq!(record["schema_version"], 1);
            assert!(record["timestamp_unix_ms"].as_u64().unwrap() > 0);
            assert!(record["event"].as_str().is_some());
            assert!(record["action"].as_str().is_some());
            assert!(matches!(
                record["outcome"].as_str(),
                Some("success" | "not_submitted" | "request_error")
            ));
            let object = record.as_object().unwrap();
            assert!(object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "schema_version"
                        | "timestamp_unix_ms"
                        | "event"
                        | "action"
                        | "outcome"
                        | "container_id"
                        | "network_id"
                )
            }));
            if let Some(id) = record["container_id"].as_str() {
                assert!(id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
            }
            if let Some(id) = record["network_id"].as_str() {
                assert!(id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
            }
        }
        assert_eq!(records[0]["event"], "container_action");
        assert_eq!(records[0]["container_id"], container_id);
        assert_eq!(records[0]["action"], "Remove");
        assert_eq!(records[0]["outcome"], "success");
        assert_eq!(records[1]["event"], "container_action");
        assert!(records[1].get("container_id").is_none());
        assert_eq!(records[1]["action"], "Stop");
        assert_eq!(records[1]["outcome"], "request_error");
        assert_eq!(records[2]["event"], "image_mutation");
        assert_eq!(records[2]["action"], "pull");
        assert_eq!(records[2]["outcome"], "request_error");
        assert_eq!(records[3]["event"], "volume_mutation");
        assert_eq!(records[3]["action"], "create");
        assert_eq!(records[3]["outcome"], "success");
        assert!(records[3].get("container_id").is_none());
        assert!(records[3].get("network_id").is_none());
        assert_eq!(records[4]["event"], "network_mutation");
        assert_eq!(records[4]["network_id"], network_id);
        assert_eq!(records[4]["action"], "create");
        assert_eq!(records[4]["outcome"], "success");
        assert_eq!(records[5]["event"], "network_mutation");
        assert_eq!(records[5]["container_id"], container_id);
        assert_eq!(records[5]["network_id"], network_id);
        assert_eq!(records[5]["action"], "disconnect");
        assert_eq!(records[5]["outcome"], "request_error");
        assert_eq!(records[6]["event"], "image_mutation");
        assert_eq!(records[6]["action"], "push");
        assert_eq!(records[6]["outcome"], "request_error");
        assert_eq!(records[10]["event"], "image_mutation");
        assert_eq!(records[10]["action"], "pull");
        assert_eq!(records[10]["outcome"], "not_submitted");
        assert_eq!(records[11]["event"], "image_mutation");
        assert_eq!(records[11]["action"], "push");
        assert_eq!(records[11]["outcome"], "not_submitted");
        for (record, outcome) in
            records[7..10]
                .iter()
                .zip(["success", "not_submitted", "request_error"])
        {
            assert_eq!(record["schema_version"], 1);
            assert!(record["timestamp_unix_ms"].as_u64().unwrap() > 0);
            assert_eq!(record["event"], "image_mutation");
            assert_eq!(record["action"], "build");
            assert_eq!(record["outcome"], outcome);
            assert_eq!(
                record
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>(),
                [
                    "action",
                    "event",
                    "outcome",
                    "schema_version",
                    "timestamp_unix_ms"
                ]
                .into_iter()
                .collect()
            );
        }
        for sensitive_value in [
            "build-output-SENTINEL",
            "/build/context-SENTINEL",
            "image-tag-SENTINEL",
            "docker.sock",
            "build-context-SENTINEL",
            "credential-SENTINEL",
            "raw-error-SENTINEL",
        ] {
            assert!(!contents.contains(sensitive_value));
        }
        for sentinel in [
            invalid_id,
            "endpoint-SENTINEL",
            "volume-name-SENTINEL",
            "volume-driver-SENTINEL",
            "volume-scope-SENTINEL",
            "volume-time-SENTINEL",
            "/volume/path-SENTINEL",
            "push-endpoint-SENTINEL",
            "SECRET",
        ] {
            assert!(
                !contents.contains(sentinel),
                "persisted sentinel: {sentinel}"
            );
        }
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn audit_persistence_returns_bounded_failure_under_lock_contention() {
        use super::{AuditRecord, AuditStore, AuditStoreError};
        use crate::settings::SettingsStore;
        use std::{
            fs,
            os::fd::AsRawFd,
            time::{Duration, SystemTime, UNIX_EPOCH},
        };

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ferrum-audit-lock-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let settings = Arc::new(SettingsStore::at(root.join("settings.toml")));
        let store = AuditStore::new(settings.clone());
        let blocker = settings.open_audit_file().expect("audit file should open");
        let descriptor = blocker.as_raw_fd();
        assert_eq!(
            unsafe { libc::flock(descriptor, libc::LOCK_EX | libc::LOCK_NB) },
            0
        );

        let persist_store = store.clone();
        let mut persist_task = tokio::spawn(async move {
            persist_store
                .persist(AuditRecord::new(
                    "container_action",
                    "start",
                    "success",
                    None,
                    None,
                ))
                .await
        });
        let result = tokio::time::timeout(Duration::from_secs(1), &mut persist_task).await;
        let completed_while_locked = result.is_ok();
        // SAFETY: `blocker` owns this descriptor and this test acquired its lock.
        assert_eq!(unsafe { libc::flock(descriptor, libc::LOCK_UN) }, 0);
        drop(blocker);
        let persistence_result = match result {
            Ok(joined) => joined.expect("persistence task should finish"),
            Err(_) => persist_task
                .await
                .expect("blocked task should finish after unlock"),
        };
        fs::remove_dir_all(&root).unwrap();

        assert!(
            completed_while_locked,
            "audit persistence must not wait indefinitely while the file lock is contended"
        );
        assert!(matches!(persistence_result, Err(AuditStoreError::Write)));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn durable_audit_log_stops_at_the_configured_size_limit() {
        use super::{AuditStore, AuditStoreError, MAX_AUDIT_LOG_BYTES};
        use crate::settings::SettingsStore;
        use std::fs;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ferrum-audit-full-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = AuditStore::new(Arc::new(SettingsStore::at(root.join("settings.toml"))));
        store
            .persist(record_container_action_result(
                &"c".repeat(64),
                ContainerAction::Start,
                true,
            ))
            .await
            .unwrap();
        let path = root.join("audit.jsonl");
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_AUDIT_LOG_BYTES).unwrap();
        drop(file);
        let previous = fs::read(&path).unwrap();

        let result = store
            .persist(record_container_action_result(
                &"d".repeat(64),
                ContainerAction::Stop,
                true,
            ))
            .await;
        let after = fs::read(&path).unwrap();
        let _ = fs::remove_dir_all(&root);

        assert!(matches!(result, Err(AuditStoreError::Full)));
        assert_eq!(after.len() as u64, MAX_AUDIT_LOG_BYTES);
        assert_eq!(after, previous);
    }
}
