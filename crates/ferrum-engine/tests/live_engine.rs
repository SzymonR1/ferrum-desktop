use std::collections::HashSet;

use ferrum_engine::EngineClient;

#[tokio::test]
#[ignore = "requires an accessible local Docker-compatible engine; reads version and inventory only"]
async fn local_engine_returns_a_well_formed_container_inventory() {
    let engine = EngineClient::connect_default()
        .await
        .expect("a local Docker-compatible engine should be reachable");

    assert!(!engine.info().version.trim().is_empty());
    assert!(!engine.info().api_version.trim().is_empty());

    let containers = engine
        .list_containers()
        .await
        .expect("the engine should return its container inventory");
    assert!(
        !containers.is_empty(),
        "the read-only smoke test requires at least one existing container"
    );
    let mut ids = HashSet::with_capacity(containers.len());

    for container in &containers {
        assert!(!container.id.is_empty());
        assert!(!container.name.is_empty());
        assert!(!container.name.starts_with('/'));
        assert!(ids.insert(&container.id), "duplicate container ID returned");
    }

    let first = &containers[0];
    assert!(
        first
            .image
            .as_deref()
            .is_some_and(|image| !image.trim().is_empty())
    );
    assert!(first.state.is_some());
    assert!(
        first
            .status
            .as_deref()
            .is_some_and(|status| !status.trim().is_empty())
    );
}

#[tokio::test]
#[ignore = "requires an accessible current local Docker CLI context; reads inventory only"]
async fn current_local_docker_context_connects_and_returns_a_valid_inventory() {
    use ferrum_engine::{ContextTransport, DockerContext};

    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker CLI context discovery should succeed");
    let current: &DockerContext = contexts
        .iter()
        .find(|context| context.is_current())
        .expect("Docker CLI should report its current context");
    assert_eq!(current.transport(), ContextTransport::LocalUnix);

    let engine = EngineClient::connect_context(current.name())
        .await
        .expect("the current local context should connect");
    assert_eq!(engine.selected_context_name(), Some(current.name()));
    assert!(!engine.info().version.trim().is_empty());
    assert!(!engine.info().api_version.trim().is_empty());
    let inventory = engine
        .list_containers()
        .await
        .expect("the context should return a container inventory");
    for container in inventory {
        assert!(!container.id.is_empty());
        assert!(!container.name.is_empty());
    }
}
