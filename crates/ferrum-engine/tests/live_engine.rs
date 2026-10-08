use std::collections::HashSet;

use ferrum_engine::{ContextTransport, DockerContext, EngineClient};

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
    let details = engine
        .inspect_container(&first.id)
        .await
        .expect("a listed container should be inspectable");
    assert_eq!(details.id, first.id);
}

#[tokio::test]
#[ignore = "requires an accessible current local Docker CLI context; reads inventory only"]
async fn current_local_docker_context_connects_and_returns_a_valid_inventory() {
    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker CLI context discovery should succeed");
    let current: &DockerContext = contexts
        .iter()
        .find(|context| context.is_current())
        .expect("Docker CLI should report its current context");
    assert_context_connects_and_returns_a_valid_inventory(current).await;
}

#[tokio::test]
#[ignore = "requires FERRUM_TEST_DOCKER_CONTEXT to name an accessible non-current local Docker context"]
async fn named_noncurrent_local_docker_context_connects_and_returns_a_valid_inventory() {
    let name = std::env::var("FERRUM_TEST_DOCKER_CONTEXT")
        .expect("set FERRUM_TEST_DOCKER_CONTEXT to a non-current local context name");
    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker CLI context discovery should succeed");
    let context = contexts
        .iter()
        .find(|context| context.name() == name.as_str())
        .expect("Docker CLI should report the requested context");
    assert!(
        !context.is_current(),
        "FERRUM_TEST_DOCKER_CONTEXT must identify a non-current context"
    );
    assert_context_connects_and_returns_a_valid_inventory(context).await;
}

async fn assert_context_connects_and_returns_a_valid_inventory(context: &DockerContext) {
    assert_eq!(context.transport(), ContextTransport::LocalUnix);
    let engine = EngineClient::connect_context(context.name())
        .await
        .expect("the selected local context should connect");
    assert_eq!(engine.selected_context_name(), Some(context.name()));
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
