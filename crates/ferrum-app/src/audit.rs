use ferrum_core::ContainerAction;

/// Emits a minimal local trace event when an engine lifecycle request returns.
///
/// Only 64-character hexadecimal container IDs are attached. Engine errors,
/// endpoint details, names, and daemon response data are never logged here.
pub(super) fn record_container_action_result(
    container_id: &str,
    action: ContainerAction,
    engine_confirmed: bool,
) {
    let container_id = valid_container_id(container_id);
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

fn valid_container_id(container_id: &str) -> Option<&str> {
    (container_id.len() == 64 && container_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(container_id)
}

#[cfg(test)]
mod tests {
    use super::record_container_action_result;
    use ferrum_core::ContainerAction;
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
}
