use ferrum_core::ContainerAction;
#[cfg(target_os = "linux")]
use ferrum_core::VolumeDetails;
#[cfg(target_os = "linux")]
use ferrum_engine::EngineError;

/// Emits a minimal local trace event when an engine lifecycle request returns.
///
/// Only 64-character hexadecimal container IDs are attached. Engine errors,
/// endpoint details, names, and daemon response data are never logged here.
pub(super) fn record_container_action_result(
    container_id: &str,
    action: ContainerAction,
    engine_confirmed: bool,
) {
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
}

#[derive(Clone, Copy)]
pub(super) enum NetworkMutation {
    Create,
    Remove,
}

impl NetworkMutation {
    fn label(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Remove => "remove",
        }
    }
}

/// Emits a minimal local trace event when a network mutation request returns.
///
/// Only validated full network IDs are attached. Names, engine endpoints,
/// errors, and daemon response data are never logged.
pub(super) fn record_network_mutation_result(
    network_id: Option<&str>,
    action: NetworkMutation,
    request_succeeded: bool,
) {
    let network_id = network_id
        .and_then(valid_resource_id)
        .unwrap_or("unavailable");
    if request_succeeded {
        tracing::info!(
            target: "ferrum::audit",
            audit_event = "network_mutation",
            action = action.label(),
            network_id,
            outcome = "success",
            "Network mutation request succeeded"
        );
    } else {
        tracing::warn!(
            target: "ferrum::audit",
            audit_event = "network_mutation",
            action = action.label(),
            network_id,
            outcome = "request_error",
            "Network mutation request returned an error"
        );
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
pub(super) enum ImageMutation {
    Pull,
    Tag,
    Remove,
}

#[cfg(target_os = "linux")]
impl ImageMutation {
    fn label(self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Tag => "tag",
            Self::Remove => "remove",
        }
    }
}

/// Emits a local trace event when an image mutation request returns.
///
/// Local ID, reference, or tag validation failures are distinguished from
/// errors after the request path begins. Image identifiers, references, and
/// repository tags are never logged.
#[cfg(target_os = "linux")]
pub(super) fn record_image_mutation_result(
    action: ImageMutation,
    result: &Result<(), EngineError>,
) {
    match result {
        Ok(()) => {
            tracing::info!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = action.label(),
                outcome = "success",
                "Image mutation request succeeded"
            );
        }
        Err(
            EngineError::InvalidImageId
            | EngineError::InvalidImageReference
            | EngineError::InvalidImageTagTarget,
        ) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = action.label(),
                outcome = "not_submitted",
                "Image mutation input was rejected before request submission"
            );
        }
        Err(_) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "image_mutation",
                action = action.label(),
                outcome = "request_error",
                "Image mutation request returned an error"
            );
        }
    }
}
#[cfg(target_os = "linux")]
/// Emits an ephemeral volume-create outcome without resource or endpoint metadata.
pub(super) fn record_volume_create_result(result: &Result<VolumeDetails, EngineError>) {
    record_volume_mutation_result("create", result);
}

#[cfg(target_os = "linux")]
/// Emits an ephemeral volume-removal outcome without resource or endpoint metadata.
pub(super) fn record_volume_remove_result(result: &Result<(), EngineError>) {
    record_volume_mutation_result("remove", result);
}

#[cfg(target_os = "linux")]
fn record_volume_mutation_result<T>(action: &'static str, result: &Result<T, EngineError>) {
    match result {
        Ok(_) => {
            tracing::info!(
                target: "ferrum::audit",
                audit_event = "volume_mutation",
                action,
                outcome = "success",
                "Volume mutation request succeeded"
            );
        }
        Err(EngineError::InvalidVolumeName) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "volume_mutation",
                action,
                outcome = "not_submitted",
                "Volume mutation input was rejected before request submission"
            );
        }
        Err(_) => {
            tracing::warn!(
                target: "ferrum::audit",
                audit_event = "volume_mutation",
                action,
                outcome = "request_error",
                "Volume mutation request returned an error"
            );
        }
    }
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
    use super::{ImageMutation, record_image_mutation_result};
    use super::{NetworkMutation, record_container_action_result, record_network_mutation_result};
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

        tracing::dispatcher::with_default(&dispatcher, || {
            record_image_mutation_result(ImageMutation::Pull, &succeeded);
            record_image_mutation_result(ImageMutation::Pull, &invalid_pull);
            record_image_mutation_result(ImageMutation::Pull, &rejected_pull);
            record_image_mutation_result(ImageMutation::Tag, &succeeded);
            record_image_mutation_result(ImageMutation::Tag, &invalid_tag);
            record_image_mutation_result(ImageMutation::Tag, &rejected_tag);
            record_image_mutation_result(ImageMutation::Remove, &succeeded);
            record_image_mutation_result(ImageMutation::Remove, &invalid_remove);
            record_image_mutation_result(ImageMutation::Remove, &rejected_remove);
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
            ("pull", "request_error"),
            ("tag", "success"),
            ("tag", "not_submitted"),
            ("tag", "request_error"),
            ("remove", "success"),
            ("remove", "not_submitted"),
            ("remove", "request_error"),
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
}
