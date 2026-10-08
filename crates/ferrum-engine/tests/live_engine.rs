use std::collections::HashSet;

#[cfg(target_os = "linux")]
use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "linux")]
use ferrum_core::{ContainerAction, ContainerState};
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

#[cfg(target_os = "linux")]
static NEXT_CONTAINER_NAME: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires a local Docker-compatible engine and FERRUM_TEST_IMAGE already present locally with /bin/sleep"]
async fn local_engine_lifecycle_actions_emit_events_and_update_state() {
    let image = std::env::var("FERRUM_TEST_IMAGE")
        .expect("set FERRUM_TEST_IMAGE to a locally available image containing /bin/sleep");
    let image = image.trim();
    assert!(
        valid_test_image_reference(image),
        "FERRUM_TEST_IMAGE must be a valid image reference, not a Docker CLI option"
    );

    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker CLI context discovery should succeed");
    let context = contexts
        .iter()
        .find(|context| context.is_current())
        .expect("Docker CLI should report its current context");
    assert!(
        context.transport() == ContextTransport::LocalUnix,
        "the lifecycle test requires a local Unix Docker context"
    );
    let context_name = context.name().to_owned();
    let engine = EngineClient::connect_context(&context_name)
        .await
        .expect("the current local Docker context should connect");

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let name_sequence = NEXT_CONTAINER_NAME.fetch_add(1, Ordering::Relaxed);
    let container_name = format!(
        "ferrum-live-lifecycle-{}-{timestamp}-{name_sequence}",
        std::process::id()
    );
    let cid_directory = DisposableTempDirectory::new(&container_name);
    let create = Command::new("docker")
        .args([
            "--context",
            context_name.as_str(),
            "create",
            "--name",
            container_name.as_str(),
            "--network",
            "none",
            "--pull",
            "never",
            "--cidfile",
        ])
        .arg(cid_directory.cid_file())
        .args(["--entrypoint", "/bin/sleep", "--"])
        .arg(image)
        .arg("3600")
        .output()
        .expect("Docker CLI should be available to create the disposable container");
    assert!(
        create.status.success(),
        "Docker CLI should create the disposable container from the local image"
    );
    let created_id = std::str::from_utf8(&create.stdout)
        .ok()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let mut container = DisposableContainer::new(created_id, context_name, cid_directory);
    assert!(
        container.ensure_id(),
        "Docker CLI should provide the full ID of the disposable container"
    );

    assert_owned_container_state(&engine, container.id(), ContainerState::Created).await;

    // The create command provisions the disposable resource; subscribe before
    // the external lifecycle change that the event stream must observe.
    let mut changes = engine
        .subscribe_container_changes()
        .await
        .expect("the engine should provide a container change stream");
    let start = Command::new("docker")
        .args([
            "--context",
            container.context_name(),
            "start",
            container.id(),
        ])
        .output()
        .expect("Docker CLI should be available to start the disposable container");
    assert!(
        start.status.success(),
        "Docker CLI should start the disposable container"
    );
    let matching_change = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match changes.next_change().await {
                Ok(Some(change)) if change.container_id == container.id() => return true,
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => return false,
            }
        }
    })
    .await;
    assert!(
        matches!(matching_change, Ok(true)),
        "the event stream should report a change for the externally started container"
    );
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;

    perform_action(&engine, container.id(), ContainerAction::Stop).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Exited).await;

    perform_action(&engine, container.id(), ContainerAction::Start).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;

    perform_action(&engine, container.id(), ContainerAction::Restart).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;

    perform_action(&engine, container.id(), ContainerAction::Pause).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Paused).await;

    perform_action(&engine, container.id(), ContainerAction::Unpause).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;

    perform_action(&engine, container.id(), ContainerAction::Stop).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Exited).await;
    perform_action(&engine, container.id(), ContainerAction::Remove).await;

    let inventory = engine
        .list_containers()
        .await
        .expect("the engine should return its inventory after removal");
    assert!(
        !inventory.iter().any(|summary| summary.id == container.id()),
        "the removed container should be absent from the inventory"
    );
    assert!(
        engine.inspect_container(container.id()).await.is_err(),
        "the removed container should no longer be inspectable"
    );
    container.disarm();
}

#[cfg(target_os = "linux")]
struct DisposableTempDirectory {
    path: std::path::PathBuf,
}

#[cfg(target_os = "linux")]
impl DisposableTempDirectory {
    fn new(container_name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{container_name}-cid"));
        std::fs::create_dir(&path).expect("the private cidfile directory should be created");
        let directory = Self { path };
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory.path, std::fs::Permissions::from_mode(0o700))
            .expect("the cidfile directory should be private");
        directory
    }

    fn cid_file(&self) -> std::path::PathBuf {
        self.path.join("container.id")
    }
}

#[cfg(target_os = "linux")]
impl Drop for DisposableTempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.cid_file());
        let _ = std::fs::remove_dir(&self.path);
    }
}

#[cfg(target_os = "linux")]
struct DisposableContainer {
    id: String,
    context_name: String,
    cid_directory: DisposableTempDirectory,
    cleanup_required: bool,
}

#[cfg(target_os = "linux")]
impl DisposableContainer {
    fn new(id: String, context_name: String, cid_directory: DisposableTempDirectory) -> Self {
        Self {
            id,
            context_name,
            cid_directory,
            cleanup_required: true,
        }
    }

    fn ensure_id(&mut self) -> bool {
        let Some(id) = self.id_for_cleanup() else {
            return false;
        };
        self.id = id;
        true
    }

    fn id_for_cleanup(&self) -> Option<String> {
        if is_full_container_id(&self.id) {
            return Some(self.id.clone());
        }
        let id = std::fs::read_to_string(self.cid_directory.cid_file()).ok()?;
        let id = id.trim();
        is_full_container_id(id).then(|| id.to_owned())
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn context_name(&self) -> &str {
        &self.context_name
    }

    fn disarm(&mut self) {
        self.cleanup_required = false;
    }
}

#[cfg(target_os = "linux")]
impl Drop for DisposableContainer {
    fn drop(&mut self) {
        if !self.cleanup_required {
            return;
        }
        let Some(id) = self.id_for_cleanup() else {
            return;
        };
        let _ = Command::new("docker")
            .args([
                "--context",
                self.context_name.as_str(),
                "rm",
                "--force",
                id.as_str(),
            ])
            .output();
    }
}

#[cfg(target_os = "linux")]
fn is_full_container_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(target_os = "linux")]
async fn assert_owned_container_state(engine: &EngineClient, id: &str, expected: ContainerState) {
    let details = engine
        .inspect_container(id)
        .await
        .expect("the disposable container should be inspectable");
    assert!(
        details.id == id,
        "inspection should identify the exact disposable container"
    );
    assert!(
        details.state.as_ref() == Some(&expected),
        "inspection should report the expected lifecycle state"
    );

    let inventory = engine
        .list_containers()
        .await
        .expect("the engine should return its container inventory");
    let summary = inventory
        .iter()
        .find(|summary| summary.id == id)
        .expect("the disposable container should appear in the inventory");
    assert!(
        summary.state.as_ref() == Some(&expected),
        "the inventory should report the expected lifecycle state"
    );
}

#[cfg(target_os = "linux")]
async fn perform_action(engine: &EngineClient, id: &str, action: ContainerAction) {
    if let Err(error) = engine.perform_container_action(id, action).await {
        panic!("the {action} action should succeed against the disposable container: {error}");
    }
}
#[cfg(target_os = "linux")]
#[test]
fn test_image_reference_rejects_docker_options() {
    assert!(valid_test_image_reference("busybox:1.37.0"));
    assert!(valid_test_image_reference(
        "registry.example:5000/team/image:tag"
    ));
    assert!(!valid_test_image_reference("--privileged"));
    assert!(!valid_test_image_reference("busybox --mount=/host:/host"));
    assert!(!valid_test_image_reference(""));
}

#[cfg(target_os = "linux")]
fn valid_test_image_reference(image: &str) -> bool {
    !image.is_empty()
        && image.len() <= 512
        && !image.starts_with('-')
        && image
            .bytes()
            .all(|byte| !byte.is_ascii_whitespace() && !byte.is_ascii_control())
}
