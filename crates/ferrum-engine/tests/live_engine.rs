use std::collections::HashSet;

#[cfg(target_os = "linux")]
use ferrum_core::ContainerLogSource;
#[cfg(target_os = "linux")]
use ferrum_engine::ContainerLogsOptions;

#[cfg(target_os = "linux")]
use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "linux")]
use ferrum_core::{ContainerAction, ContainerState, ContainerSummary, NetworkSummary};
use ferrum_engine::{ContextTransport, DockerContext, EngineClient};

/// Read-only opt-in check: inspects only a caller-specified existing volume.
///
/// Requires `FERRUM_TEST_VOLUME` to name an existing volume. This test only
/// lists and inspects; it does not create, remove, modify, or clean up volumes.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires a local Docker-compatible engine and existing FERRUM_TEST_VOLUME; reads only"]
async fn local_engine_lists_and_inspects_existing_volume_read_only() {
    let name = std::env::var("FERRUM_TEST_VOLUME")
        .expect("set FERRUM_TEST_VOLUME to the name of an existing volume");
    assert!(!name.is_empty(), "FERRUM_TEST_VOLUME must not be empty");
    let engine = EngineClient::connect_default()
        .await
        .expect("local engine should be accessible");
    let volumes = engine
        .list_volumes()
        .await
        .expect("volume inventory should be readable");
    assert!(
        volumes.iter().any(|volume| volume.name == name),
        "configured existing volume must appear in inventory"
    );
    let details = engine
        .inspect_volume(&name)
        .await
        .expect("configured existing volume should be inspectable");
    assert_eq!(details.name, name);
    let usage = engine
        .volume_usage(&name)
        .await
        .expect("configured existing volume usage should be readable");
    assert!(
        usage.is_some(),
        "configured existing volume should appear in the verbose usage snapshot"
    );
}

/// Read-only opt-in check: inspects only a caller-specified existing network.
///
/// Requires `FERRUM_TEST_NETWORK` to name an existing network. This test only
/// lists and inspects; it never creates, removes, connects, disconnects, or
/// cleans up networks.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires a local Docker-compatible engine and existing FERRUM_TEST_NETWORK; reads only"]
async fn local_engine_lists_and_inspects_existing_network_read_only() {
    let name = std::env::var("FERRUM_TEST_NETWORK")
        .expect("set FERRUM_TEST_NETWORK to the name of an existing network");
    assert!(!name.is_empty(), "FERRUM_TEST_NETWORK must not be empty");
    let engine = EngineClient::connect_default()
        .await
        .expect("local engine should be accessible");
    let networks = engine
        .list_networks()
        .await
        .expect("network inventory should be readable");
    let listed = networks
        .iter()
        .find(|network| network.name == name)
        .expect("configured existing network must appear in inventory");
    let details = engine
        .inspect_network(&listed.id)
        .await
        .expect("configured existing network should be inspectable");
    assert_eq!(details.id, listed.id);
    assert_eq!(details.name, name);
    let _: Option<usize> = details.attached_container_count;
}
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires an existing local Docker network with verbose IPAM counts; read-only"]
async fn local_engine_reads_verbose_ipam_allocation_for_existing_network_read_only() {
    let name = std::env::var("FERRUM_TEST_NETWORK")
        .expect("set FERRUM_TEST_NETWORK to an existing network with IPAM status");
    assert!(!name.is_empty(), "FERRUM_TEST_NETWORK must not be empty");
    let engine = EngineClient::connect_default()
        .await
        .expect("local engine should be accessible");
    let networks = engine
        .list_networks()
        .await
        .expect("network inventory should be readable");
    let listed = networks
        .iter()
        .find(|network| network.name == name)
        .expect("configured existing network must appear in inventory");
    let details = engine
        .inspect_network(&listed.id)
        .await
        .expect("configured existing network should be inspectable");
    assert!(
        details.ipam_usage.ips_in_use.is_some()
            || details.ipam_usage.dynamic_ips_available.is_some(),
        "configured network should report at least one verbose IPAM count"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "creates and removes one unique test-owned network with EngineClient; exact-ID Docker CLI cleanup is only the Drop fallback"]
async fn local_engine_creates_default_bridge_network_and_cleans_up_exact_id() {
    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker CLI context discovery should succeed");
    let context = contexts
        .iter()
        .find(|context| context.is_current())
        .expect("Docker CLI should report its current context");
    assert_eq!(
        context.transport(),
        ContextTransport::LocalUnix,
        "network creation requires the current local Unix Docker context"
    );
    let context_name = context.name().to_owned();
    let engine = EngineClient::connect_context(&context_name)
        .await
        .expect("the current local Docker context should connect");

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let sequence = NEXT_NETWORK_NAME.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        "ferrum-live-network-{}-{timestamp}-{sequence}",
        std::process::id()
    );
    let existing_networks = engine
        .list_networks()
        .await
        .expect("the local network inventory should be readable before creation");
    assert!(
        network_name_is_available(&name, &existing_networks),
        "the generated test network name must not already exist"
    );

    let returned_id = engine
        .create_network(&name)
        .await
        .expect("the unique test-owned network should be created");
    let created_networks = engine
        .list_networks()
        .await
        .expect("the local network inventory should be readable after creation");
    let (created, verified_id) = verify_created_network(&returned_id, &name, &created_networks)
        .expect("the exact returned ID must identify the generated network name");
    let mut network = DisposableNetwork::new(verified_id, engine.clone());

    assert_eq!(created.driver.as_deref(), Some("bridge"));

    engine
        .remove_network(network.id())
        .await
        .expect("EngineClient should remove only the verified network ID");
    let remaining = engine
        .list_networks()
        .await
        .expect("the local network inventory should be readable after cleanup");
    assert!(
        !remaining.iter().any(|listed| listed.id == network.id()),
        "the test-owned network should be absent after exact-ID cleanup"
    );
    network.disarm();
}

/// Opt-in mutating check: connects and disconnects a running, test-owned
/// container on a uniquely named custom network, then removes only those IDs.
///
/// Requires a current local Unix Docker context and a preinstalled
/// `FERRUM_TEST_IMAGE` containing `/bin/sleep`. It never pulls an image.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "mutates a local engine using a preinstalled FERRUM_TEST_IMAGE with /bin/sleep; exact-ID non-force cleanup"]
async fn local_engine_connects_and_disconnects_network_with_verified_owned_container() {
    let image = std::env::var("FERRUM_TEST_IMAGE")
        .expect("set FERRUM_TEST_IMAGE to a preinstalled image containing /bin/sleep");
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
    assert_eq!(
        context.transport(),
        ContextTransport::LocalUnix,
        "network attachment test requires the current local Unix Docker context"
    );
    let context_name = context.name().to_owned();
    let engine = EngineClient::connect_context(&context_name)
        .await
        .expect("the current local Docker context should connect");
    engine
        .inspect_image(image)
        .await
        .expect("FERRUM_TEST_IMAGE must already be present; the test never pulls");

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let sequence = NEXT_ATTACHMENT_NAME.fetch_add(1, Ordering::Relaxed);
    let network_name = format!(
        "ferrum-live-attachment-network-{}-{timestamp}-{sequence}",
        std::process::id()
    );
    let container_name = format!(
        "ferrum-live-attachment-container-{}-{timestamp}-{sequence}",
        std::process::id()
    );

    let networks_before = engine
        .list_networks()
        .await
        .expect("network inventory should be readable before creation");
    assert!(
        network_name_is_available(&network_name, &networks_before),
        "the generated network name must not already exist"
    );
    let mut network = match engine.create_network(&network_name).await {
        Ok(returned_id) => {
            let networks_after_create = engine
                .list_networks()
                .await
                .expect("network inventory should be readable after creation");
            let Some((_, verified_id)) =
                verify_created_network(&returned_id, &network_name, &networks_after_create)
            else {
                panic!("network ownership could not be verified by exact ID and generated name");
            };
            DisposableNetwork::new(verified_id, engine.clone())
        }
        Err(_) => {
            panic!(
                "network creation returned no confirmed ID; no unverified cleanup request will be sent, and the network may remain for manual inspection"
            );
        }
    };
    let containers_before = engine
        .list_containers()
        .await
        .expect("container inventory should be readable before creation");
    assert!(
        !containers_before
            .iter()
            .any(|container| container.name == container_name),
        "the generated container name must not already exist"
    );
    let cid_directory = DisposableTempDirectory::new(&container_name);
    let pinned_host = format!(
        "unix://{}",
        engine
            .local_socket_path()
            .to_str()
            .expect("Docker context socket path should be UTF-8")
    );
    let create = Command::new("docker")
        .args([
            "--host",
            pinned_host.as_str(),
            "create",
            "--network",
            "bridge",
            "--pull",
            "never",
            "--name",
            container_name.as_str(),
            "--cidfile",
        ])
        .arg(cid_directory.cid_file())
        .args(["--entrypoint", "/bin/sleep", "--"])
        .arg(image)
        .arg("3600")
        .output()
        .expect("Docker CLI should create the stopped container from the local image");
    let returned_container_id = std::str::from_utf8(&create.stdout)
        .ok()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let mut container = DisposableContainer::new_running_unarmed(
        returned_container_id,
        context_name,
        cid_directory,
    );
    assert!(
        container.ensure_id(),
        "Docker CLI should provide a full ID for the created container"
    );

    let containers_after_create = engine
        .list_containers()
        .await
        .expect("container inventory should be readable after creation");
    assert!(
        verify_created_container(container.id(), &container_name, &containers_after_create),
        "the full container ID must match the generated name in inventory"
    );
    container.arm_with_engine(engine.clone());
    assert!(
        create.status.success(),
        "Docker CLI should create the container without pulling or starting it"
    );
    assert_owned_container_state(&engine, container.id(), ContainerState::Created).await;

    let before_connect = engine
        .inspect_network(network.id())
        .await
        .expect("the new custom network should be inspectable");
    assert_eq!(before_connect.attached_container_count, Some(0));
    assert_eq!(
        before_connect.attached_container_ids.as_deref(),
        Some(&[][..])
    );

    perform_action(&engine, container.id(), ContainerAction::Start).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;
    engine
        .connect_container_to_network(network.id(), container.id())
        .await
        .expect("the exact owned container should connect to the exact owned network");
    let connected = engine
        .inspect_network(network.id())
        .await
        .expect("network membership should be inspectable after connection");
    assert_eq!(connected.attached_container_count, Some(1));
    assert!(
        connected
            .attached_container_ids
            .as_ref()
            .is_some_and(|ids| ids == &[container.id().to_owned()]),
        "inspect should report the exact attached container ID"
    );
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;

    engine
        .disconnect_container_from_network(network.id(), container.id())
        .await
        .expect("the exact owned container should disconnect from the exact owned network");
    let disconnected = engine
        .inspect_network(network.id())
        .await
        .expect("network membership should be inspectable after disconnection");
    assert_eq!(disconnected.attached_container_count, Some(0));
    assert!(
        disconnected
            .attached_container_ids
            .as_ref()
            .is_some_and(|ids| !ids.iter().any(|id| id == container.id())),
        "inspect should no longer report the test-owned container"
    );
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;

    perform_action(&engine, container.id(), ContainerAction::Stop).await;
    assert_owned_container_state(&engine, container.id(), ContainerState::Exited).await;
    perform_action(&engine, container.id(), ContainerAction::Remove).await;
    let remaining_containers = engine
        .list_containers()
        .await
        .expect("container inventory should be readable after cleanup");
    assert!(
        !remaining_containers
            .iter()
            .any(|listed| listed.id == container.id()),
        "the test-owned container should be absent after exact-ID removal"
    );
    container.disarm();

    engine
        .remove_network(network.id())
        .await
        .expect("EngineClient should remove only the verified network ID");
    let remaining_networks = engine
        .list_networks()
        .await
        .expect("network inventory should be readable after cleanup");
    assert!(
        !remaining_networks
            .iter()
            .any(|listed| listed.id == network.id()),
        "the test-owned network should be absent after exact-ID removal"
    );
    network.disarm();
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires a current local Docker context and a preinstalled FERRUM_TEST_IMAGE"]
async fn local_engine_lists_inspects_and_reads_history_for_preinstalled_image_read_only() {
    let image_reference = std::env::var("FERRUM_TEST_IMAGE")
        .expect("set FERRUM_TEST_IMAGE to an image already installed locally");
    let image_reference = image_reference.trim();
    assert!(
        valid_test_image_reference(image_reference),
        "FERRUM_TEST_IMAGE must be a valid image reference, not a Docker CLI option"
    );

    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker CLI context discovery should succeed");
    let context = contexts
        .iter()
        .find(|context| context.is_current())
        .expect("Docker CLI should report its current context");
    assert_eq!(
        context.transport(),
        ContextTransport::LocalUnix,
        "the image test requires a local Unix Docker context"
    );
    let context_name = context.name().to_owned();
    let engine = EngineClient::connect_context(&context_name)
        .await
        .expect("the current local Docker context should connect");

    let images = engine
        .list_images()
        .await
        .expect("the engine should return its installed images");
    let details = engine
        .inspect_image(image_reference)
        .await
        .expect("the preinstalled FERRUM_TEST_IMAGE should be inspectable");
    assert!(
        images.iter().any(|image| image.id == details.id),
        "the inspected image should be present in the listed local image inventory"
    );
    let history = engine
        .image_history(&details.id)
        .await
        .expect("the preinstalled image's read-only history should be available");
    assert!(
        !history.is_empty(),
        "the preinstalled image should report at least one layer-history row"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires the current local Docker context and FERRUM_TEST_IMAGE already present locally with /bin/sh and /bin/sleep"]
async fn local_engine_logs_stream_history_follow_and_cancel() {
    let image = std::env::var("FERRUM_TEST_IMAGE").expect(
        "set FERRUM_TEST_IMAGE to a locally available image containing /bin/sh and /bin/sleep",
    );
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
    assert_eq!(
        context.transport(),
        ContextTransport::LocalUnix,
        "the logs test requires a local Unix Docker context"
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
        "ferrum-live-logs-{}-{timestamp}-{name_sequence}",
        std::process::id()
    );
    let marker_prefix = format!(
        "ferrum_logs_{}_{}_{}",
        std::process::id(),
        timestamp,
        name_sequence
    );
    let history_out = format!("{marker_prefix}_history_stdout");
    let history_err = format!("{marker_prefix}_history_stderr");
    let follow_out = format!("{marker_prefix}_follow_stdout");
    let follow_err = format!("{marker_prefix}_follow_stderr");
    let script = format!(
        "printf '%s\\n' '{history_out}'; printf '%s\\n' '{history_err}' >&2; while [ ! -f /tmp/ferrum-ready ]; do sleep 1; done; printf '%s\\n' '{follow_out}'; printf '%s\\n' '{follow_err}' >&2; exec sleep 3600"
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
            "--log-driver",
            "json-file",
            "--cidfile",
        ])
        .arg(cid_directory.cid_file())
        .args(["--entrypoint", "/bin/sh", "--"])
        .arg(image)
        .args(["-c", script.as_str()])
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

    let inspect = engine
        .inspect_container(container.id())
        .await
        .expect("the running disposable container should be inspectable");
    assert_eq!(inspect.id, container.id());
    assert_eq!(inspect.tty, Some(false));

    let mut logs = engine
        .open_container_logs(
            container.id(),
            false,
            ContainerLogsOptions {
                stdout: true,
                stderr: true,
                timestamps: true,
                follow: true,
                tail: 100,
            },
        )
        .await
        .expect("the engine should open the container's historical and follow log stream");
    let release_follow_output = Command::new("docker")
        .args([
            "--context",
            container.context_name(),
            "exec",
            container.id(),
            "/bin/sh",
            "-c",
            "printf ready > /tmp/ferrum-ready",
        ])
        .output()
        .expect("Docker CLI should be available to release the disposable log process");
    assert!(
        release_follow_output.status.success(),
        "Docker CLI should release the waiting disposable log process"
    );

    let mut found_history_out = false;
    let mut found_history_err = false;
    let mut found_follow_out = false;
    let mut found_follow_err = false;
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        while !(found_history_out && found_history_err && found_follow_out && found_follow_err) {
            let line = logs
                .next_line()
                .await
                .expect("the real container log stream should decode");
            let Some(line) = line else {
                return false;
            };
            if line.text.contains(&history_out) {
                assert_eq!(line.source, ContainerLogSource::Stdout);
                found_history_out = true;
            }
            if line.text.contains(&history_err) {
                assert_eq!(line.source, ContainerLogSource::Stderr);
                found_history_err = true;
            }
            if line.text.contains(&follow_out) {
                assert_eq!(line.source, ContainerLogSource::Stdout);
                found_follow_out = true;
            }
            if line.text.contains(&follow_err) {
                assert_eq!(line.source, ContainerLogSource::Stderr);
                found_follow_err = true;
            }
        }
        true
    })
    .await;
    assert!(
        matches!(outcome, Ok(true)),
        "the stream should deliver both historical and post-open stdout/stderr markers"
    );

    drop(logs);
    perform_action(&engine, container.id(), ContainerAction::Stop).await;
    perform_action(&engine, container.id(), ContainerAction::Remove).await;
    let inventory = engine
        .list_containers()
        .await
        .expect("the engine should return inventory after removal");
    assert!(
        !inventory.iter().any(|summary| summary.id == container.id()),
        "the removed logs-test container should be absent from inventory"
    );
    container.disarm();
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires local Docker and a preinstalled FERRUM_TEST_IMAGE containing /bin/sh, /bin/sleep, and stty"]
async fn local_engine_exec_session_is_interactive_and_preserves_container() {
    use ferrum_engine::TerminalSize;
    use tokio::io::AsyncWriteExt;

    let image =
        std::env::var("FERRUM_TEST_IMAGE").expect("set FERRUM_TEST_IMAGE to a preinstalled image");
    let image = image.trim();
    assert!(
        valid_test_image_reference(image),
        "FERRUM_TEST_IMAGE must be a valid image reference"
    );
    let contexts = EngineClient::available_contexts()
        .await
        .expect("Docker context discovery");
    let context = contexts
        .iter()
        .find(|context| context.is_current())
        .expect("current Docker context");
    assert_eq!(context.transport(), ContextTransport::LocalUnix);
    let context_name = context.name().to_owned();
    let engine = EngineClient::connect_context(&context_name)
        .await
        .expect("connect local context");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let sequence = NEXT_CONTAINER_NAME.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        "ferrum-live-exec-{}-{timestamp}-{sequence}",
        std::process::id()
    );
    let cid_directory = DisposableTempDirectory::new(&name);
    let create = Command::new("docker")
        .args([
            "--context",
            &context_name,
            "create",
            "--name",
            &name,
            "--network",
            "none",
            "--pull",
            "never",
            "--cidfile",
        ])
        .arg(cid_directory.cid_file())
        .args([
            "--entrypoint",
            "/bin/sh",
            "--",
            image,
            "-c",
            "exec sleep 3600",
        ])
        .output()
        .expect("Docker CLI create");
    assert!(create.status.success(), "create owned disposable container");
    let created_id = std::str::from_utf8(&create.stdout)
        .ok()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let mut container = DisposableContainer::new(created_id, context_name, cid_directory);
    assert!(container.ensure_id(), "obtain exact full container ID");
    let start = Command::new("docker")
        .args([
            "--context",
            container.context_name(),
            "start",
            container.id(),
        ])
        .output()
        .expect("Docker CLI start");
    assert!(start.status.success(), "start owned disposable container");

    let mut session = engine
        .open_exec_session(
            container.id(),
            &["/bin/sh".into()],
            TerminalSize {
                rows: 24,
                columns: 80,
            },
        )
        .await
        .expect("open interactive TTY exec");
    session
        .write_all(b"printf '\\033[31mFERRUM_EXEC_MARKER\\033[0m \\342\\230\\203\\n'\n")
        .await
        .expect("write marker");
    let mut transcript = Vec::new();
    read_until(&mut session, &mut transcript, "☃".as_bytes()).await;
    assert!(
        transcript
            .windows(b"FERRUM_EXEC_MARKER".len())
            .any(|window| window == b"FERRUM_EXEC_MARKER"),
        "command output marker"
    );
    assert!(
        transcript.windows(2).any(|window| window == b"\x1b["),
        "ANSI color escape preserved"
    );
    assert!(
        transcript.windows(3).any(|window| window == "☃".as_bytes()),
        "Unicode preserved"
    );
    session
        .resize_handle()
        .resize(TerminalSize {
            rows: 37,
            columns: 101,
        })
        .await
        .expect("resize TTY");
    session
        .write_all(b"stty size\n")
        .await
        .expect("write size command");
    let mut size_output = Vec::new();
    read_until(&mut session, &mut size_output, b"37 101").await;
    session
        .write_all(b"sleep 30\n")
        .await
        .expect("write sleeping command");
    tokio::time::sleep(Duration::from_millis(250)).await;
    session.write_all(&[3]).await.expect("send Ctrl+C");
    session.write_all(b"printf '\\106\\105\\122\\122\\125\\115\\137\\101\\106\\124\\105\\122\\137\\111\\116\\124\\105\\122\\122\\125\\120\\124\\n'\n").await.expect("write post-interrupt command");
    let mut interrupt_output = Vec::new();
    read_until(
        &mut session,
        &mut interrupt_output,
        b"FERRUM_AFTER_INTERRUPT",
    )
    .await;
    drop(session);
    assert_owned_container_state(&engine, container.id(), ContainerState::Running).await;
    perform_action(&engine, container.id(), ContainerAction::Stop).await;
    perform_action(&engine, container.id(), ContainerAction::Remove).await;
    container.disarm();
}

#[cfg(target_os = "linux")]
async fn read_until(session: &mut ferrum_engine::ExecSession, output: &mut Vec<u8>, needle: &[u8]) {
    use tokio::io::AsyncReadExt;
    const MAX_EXEC_OUTPUT_BYTES: usize = 64 * 1024;
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let mut chunk = [0u8; 512];
        loop {
            if output.windows(needle.len()).any(|window| window == needle) {
                break;
            }
            let count = session.read(&mut chunk).await.expect("read exec TTY");
            assert_ne!(count, 0, "TTY closed before expected output");
            assert!(
                count <= MAX_EXEC_OUTPUT_BYTES.saturating_sub(output.len()),
                "exec TTY output exceeded the 64 KiB safety limit before expected output"
            );
            output.extend_from_slice(&chunk[..count]);
        }
    })
    .await;
    if result.is_err() {
        let escaped_output = output
            .iter()
            .take(256)
            .flat_map(|byte| std::ascii::escape_default(*byte))
            .map(char::from)
            .collect::<String>();
        panic!(
            "exec TTY timed out waiting for {needle:?} after {} bytes; first escaped bytes: {escaped_output:?}",
            output.len()
        );
    }
}

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
static NEXT_NETWORK_NAME: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "linux")]
static NEXT_ATTACHMENT_NAME: AtomicU64 = AtomicU64::new(0);
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
    force_cleanup: bool,
    stop_before_cleanup: bool,
    cleanup_engine: Option<EngineClient>,
}

impl DisposableContainer {
    fn new(id: String, context_name: String, cid_directory: DisposableTempDirectory) -> Self {
        Self::with_cleanup_mode(id, context_name, cid_directory, true, false, true)
    }

    fn new_running_unarmed(
        id: String,
        context_name: String,
        cid_directory: DisposableTempDirectory,
    ) -> Self {
        Self::with_cleanup_mode(id, context_name, cid_directory, false, true, false)
    }

    fn with_cleanup_mode(
        id: String,
        context_name: String,
        cid_directory: DisposableTempDirectory,
        force_cleanup: bool,
        stop_before_cleanup: bool,
        cleanup_required: bool,
    ) -> Self {
        Self {
            id,
            context_name,
            cid_directory,
            cleanup_required,
            force_cleanup,
            stop_before_cleanup,
            cleanup_engine: None,
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
        if is_full_docker_resource_id(&self.id) {
            return Some(self.id.clone());
        }
        let id = std::fs::read_to_string(self.cid_directory.cid_file()).ok()?;
        let id = id.trim();
        is_full_docker_resource_id(id).then(|| id.to_owned())
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn context_name(&self) -> &str {
        &self.context_name
    }

    fn arm_with_engine(&mut self, engine: EngineClient) {
        self.cleanup_engine = Some(engine);
        self.cleanup_required = true;
    }

    fn disarm(&mut self) {
        self.cleanup_required = false;
    }
}

impl Drop for DisposableContainer {
    fn drop(&mut self) {
        if !self.cleanup_required {
            return;
        }
        let Some(id) = self.id_for_cleanup() else {
            return;
        };
        if let Some(engine) = self.cleanup_engine.clone() {
            run_cleanup_on_thread(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async {
                    let _ = engine
                        .perform_container_action(&id, ContainerAction::Stop)
                        .await;
                    let _ = engine
                        .perform_container_action(&id, ContainerAction::Remove)
                        .await;
                });
            });
            return;
        }
        if self.stop_before_cleanup {
            let _ = Command::new("docker")
                .args(["--context", self.context_name.as_str(), "stop", id.as_str()])
                .output();
        }
        let mut command = Command::new("docker");
        command.args(["--context", self.context_name.as_str(), "rm"]);
        if self.force_cleanup {
            command.arg("--force");
        }
        let _ = command.arg(id).output();
    }
}

#[cfg(target_os = "linux")]
fn run_cleanup_on_thread(cleanup: impl FnOnce() + Send + 'static) {
    if let Ok(task) = std::thread::Builder::new().spawn(cleanup) {
        let _ = task.join();
    }
}

#[cfg(target_os = "linux")]
struct DisposableNetwork {
    id: VerifiedNetworkId,
    engine: EngineClient,
    cleanup_required: bool,
}

#[cfg(target_os = "linux")]
struct VerifiedNetworkId(String);

#[cfg(target_os = "linux")]
impl VerifiedNetworkId {
    fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(target_os = "linux")]
impl DisposableNetwork {
    fn new(id: VerifiedNetworkId, engine: EngineClient) -> Self {
        Self {
            id,
            engine,
            cleanup_required: true,
        }
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn disarm(&mut self) {
        self.cleanup_required = false;
    }
}

#[cfg(target_os = "linux")]
impl Drop for DisposableNetwork {
    fn drop(&mut self) {
        if self.cleanup_required {
            let engine = self.engine.clone();
            let id = self.id.as_str().to_owned();
            run_cleanup_on_thread(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                let _ = runtime.block_on(engine.remove_network(&id));
            });
        }
    }
}

#[cfg(target_os = "linux")]
fn network_name_is_available(name: &str, networks: &[NetworkSummary]) -> bool {
    !networks.iter().any(|network| network.name == name)
}

#[cfg(target_os = "linux")]
fn verified_network_id(id: &str) -> Option<VerifiedNetworkId> {
    is_full_docker_resource_id(id).then(|| VerifiedNetworkId(id.to_owned()))
}

#[cfg(target_os = "linux")]
fn verify_created_network<'a>(
    returned_id: &str,
    requested_name: &str,
    networks: &'a [NetworkSummary],
) -> Option<(&'a NetworkSummary, VerifiedNetworkId)> {
    let verified_id = verified_network_id(returned_id)?;
    networks
        .iter()
        .find(|network| network.id == returned_id && network.name == requested_name)
        .map(|network| (network, verified_id))
}

#[cfg(target_os = "linux")]
fn verify_created_container(
    returned_id: &str,
    requested_name: &str,
    containers: &[ContainerSummary],
) -> bool {
    is_full_docker_resource_id(returned_id)
        && containers
            .iter()
            .any(|container| container.id == returned_id && container.name == requested_name)
}

#[cfg(target_os = "linux")]
#[test]
fn live_network_cleanup_requires_an_unused_name_and_verified_id() {
    let network = |id: &str, name: &str| NetworkSummary {
        id: id.to_owned(),
        name: name.to_owned(),
        driver: None,
        scope: None,
        created_at: None,
        internal: None,
        attachable: None,
        enable_ipv4: None,
        enable_ipv6: None,
    };
    let id = "a".repeat(64);
    let name = "ferrum-live-network-123";

    assert!(network_name_is_available(
        name,
        &[network("another-id", "another-name")]
    ));
    assert!(!network_name_is_available(
        name,
        &[network("existing-id", name)]
    ));
    let exact_inventory = [network(&id, name)];
    let verified = verify_created_network(&id, name, &exact_inventory);
    assert_eq!(
        verified.as_ref().map(|(_, verified)| verified.as_str()),
        Some(id.as_str())
    );
    assert!(verify_created_network(&id, name, &[network(&id, "different-name")]).is_none());
    assert!(verify_created_network("short-id", name, &[network("short-id", name)]).is_none());
    let non_hex = "g".repeat(64);
    assert!(verify_created_network(&non_hex, name, &[network(&non_hex, name)]).is_none());
}
#[cfg(target_os = "linux")]
#[test]
fn live_container_cleanup_requires_a_full_id_and_exact_inventory_name() {
    let container = |id: &str, name: &str| ContainerSummary {
        id: id.to_owned(),
        name: name.to_owned(),
        image: None,
        state: None,
        status: None,
    };
    let id = "a".repeat(64);
    let name = "ferrum-live-attachment-container-123";

    assert!(verify_created_container(&id, name, &[container(&id, name)]));
    assert!(!verify_created_container(
        &id,
        name,
        &[container(&id, "different-name")]
    ));
    assert!(!verify_created_container(
        &"b".repeat(64),
        name,
        &[container(&id, name)]
    ));
    assert!(!verify_created_container(
        "short-id",
        name,
        &[container("short-id", name)]
    ));
}
#[cfg(target_os = "linux")]
fn is_full_docker_resource_id(id: &str) -> bool {
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
