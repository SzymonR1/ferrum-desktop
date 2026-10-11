#[cfg(target_os = "linux")]
async fn send_event_snapshot(
    engine: &EngineClient,
    sender: &mpsc::Sender<EngineEvent>,
    context: &egui::Context,
    generation: u64,
) {
    let result = engine.list_containers().await;
    let _ = sender
        .send(EngineEvent::EventSnapshot { generation, result })
        .await;
    context.request_repaint();
}
fn available_container_actions(state: Option<&ContainerState>) -> &'static [ContainerAction] {
    use ContainerAction::*;
    match state {
        Some(ContainerState::Created) => &[Start, Remove],
        Some(ContainerState::Running) => &[Stop, Restart, Pause],
        Some(ContainerState::Paused) => &[Unpause],
        Some(ContainerState::Exited) => &[Start, Restart, Remove],
        Some(ContainerState::Dead) => &[Remove],
        _ => &[],
    }
}
fn action_requires_confirmation(action: ContainerAction) -> bool {
    action.is_destructive()
}
fn build_engine_source_label<'a>(
    selected_context: Option<&'a str>,
    endpoint_label: Option<&'static str>,
) -> &'a str {
    if let Some(context) = selected_context {
        return context;
    }
    match endpoint_label {
        Some("configured Unix socket") => "Configured local Docker socket",
        Some("configured endpoint") => "Configured Docker endpoint",
        Some(label) => label,
        None => "Automatic local discovery",
    }
}
mod audit;
mod compose;
mod display;
#[cfg(target_os = "linux")]
mod image_build;
mod images;
mod inventory;
mod logs;
mod networks;
mod retry;
mod search;
mod volumes;
use ferrum_core::{NetworkSummary, VolumeDetails, VolumeSummary};
#[cfg(target_os = "linux")]
mod terminal;
#[cfg(target_os = "linux")]
const LOG_CHANNEL_CAPACITY: usize = 16;
mod settings;

#[cfg(target_os = "linux")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(target_os = "linux")]
use std::time::Instant;
use std::{error::Error, time::Duration};

use crate::inventory::InventoryView;
use eframe::egui::{self, Color32, RichText};
#[cfg(target_os = "linux")]
use ferrum_core::ContainerLogLine;
use ferrum_core::{
    ContainerAction, ContainerDetails, ContainerState, ContainerSummary, ImageDetails,
    ImageHistoryEntry, ImagePullProgress, ImagePushProgress, ImageSummary,
};
#[cfg(target_os = "linux")]
use ferrum_engine::ContainerLogsOptions;
#[cfg(target_os = "linux")]
use ferrum_engine::TerminalSize;
use ferrum_engine::{ContextTransport, DockerContext, EngineClient, EngineError, EngineInfo};
use logs::LogBuffer;
#[cfg(target_os = "linux")]
use retry::RetryBudget;
#[cfg(target_os = "linux")]
use terminal::TerminalModel;
#[cfg(target_os = "linux")]
use tokio::sync::watch;
use tokio::{
    runtime::Runtime,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tracing_subscriber::EnvFilter;

const EVENT_CHANNEL_CAPACITY: usize = 4;
const ROW_HEIGHT: f32 = 34.0;
const CELL_HEIGHT: f32 = 28.0;

/// Limits concurrent app-test subprocesses to keep fixture launches reliable.
#[cfg(test)]
pub(crate) static TEST_SUBPROCESS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ferrum")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "com.ferrum.desktop",
        options,
        Box::new(move |creation_context| Ok(Box::new(FerrumApp::new(creation_context, runtime)))),
    )?;
    Ok(())
}

enum ConnectionState {
    Connecting,
    Connected,
    Disconnected(String),
}
enum EngineEvent {
    Connected(Result<(EngineClient, EngineInfo, Vec<ContainerSummary>), EngineError>),
    Refreshed(Result<Vec<ContainerSummary>, EngineError>),
    Contexts(Result<Vec<DockerContext>, EngineError>),
    ContextSwitched(Result<(EngineClient, EngineInfo, Vec<ContainerSummary>), EngineError>),
    Inspected {
        generation: u64,
        result: Result<ContainerDetails, EngineError>,
    },
    ImagesListed {
        generation: u64,
        result: Result<Vec<ImageSummary>, EngineError>,
    },
    VolumesListed {
        generation: u64,
        result: Result<Vec<VolumeSummary>, EngineError>,
    },
    VolumeInspected {
        generation: u64,
        inspection_generation: u64,
        name: String,
        result: volumes::VolumeInspectionResult,
    },
    VolumeCreated {
        generation: u64,
        name: String,
        result: Result<VolumeDetails, EngineError>,
    },
    VolumeRemoved {
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        name: String,
        result: Result<(), EngineError>,
    },
    NetworksListed {
        generation: u64,
        container_generation: u64,
        network_result: Result<Vec<NetworkSummary>, EngineError>,
        container_result: Result<Vec<ContainerSummary>, EngineError>,
    },
    ComposeListed {
        generation: u64,
        result: Result<compose::ComposeDiscovery, compose::ComposeError>,
    },
    ComposeFileSelectionFinished {
        generation: u64,
        file: Option<std::path::PathBuf>,
    },
    ComposeValidationFinished {
        generation: u64,
        result: compose::ComposeValidationResult,
    },
    NetworkCreated {
        generation: u64,
        name: String,
        result: Result<String, EngineError>,
    },
    NetworkRemoved {
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        id: String,
        name: String,
        result: Result<(), EngineError>,
    },
    NetworkInspected {
        generation: u64,
        inspection_generation: u64,
        id: String,
        result: Result<ferrum_core::NetworkDetails, EngineError>,
    },
    NetworkAttachmentFinished {
        operation: networks::NetworkAttachmentOperation,
        container_generation: u64,
        mutation_result: Result<(), EngineError>,
        refreshed: Result<
            (
                Vec<NetworkSummary>,
                Vec<ContainerSummary>,
                ferrum_core::NetworkDetails,
            ),
            EngineError,
        >,
    },
    ImagePullProgress {
        generation: u64,
        progress: ImagePullProgress,
    },
    ImagePullFinished {
        generation: u64,
        result: Result<(), EngineError>,
    },
    ImagePushProgress {
        generation: u64,
        progress: ImagePushProgress,
    },
    ImagePushFinished {
        generation: u64,
        result: Result<(), EngineError>,
    },
    ImageTagged {
        generation: u64,
        result: Result<(), EngineError>,
    },
    ImageRemoved {
        generation: u64,
        result: Result<(), EngineError>,
    },
    #[cfg(target_os = "linux")]
    ImageBuildOutput {
        generation: u64,
        line: String,
    },
    #[cfg(target_os = "linux")]
    ImageBuildFinished {
        generation: u64,
        result: Result<(), image_build::ImageBuildError>,
    },

    ImageInspected {
        generation: u64,
        inspection_generation: u64,
        id: String,
        result: Result<ImageDetails, EngineError>,
        history_result: Result<Vec<ImageHistoryEntry>, EngineError>,
    },
    ActionFinished {
        generation: u64,
        action: ContainerAction,
        result: Result<(), EngineError>,
    },
    ContainerChanged {
        generation: u64,
        container_id: String,
    },
    EventSnapshot {
        generation: u64,
        result: Result<Vec<ContainerSummary>, EngineError>,
    },
    EventStreamEnded {
        generation: u64,
    },
}
#[derive(Debug, PartialEq, Eq)]
enum PendingResourceRead {
    VolumeList {
        generation: u64,
    },
    VolumeInspect {
        generation: u64,
        inspection_generation: u64,
        name: String,
    },
    NetworkList {
        generation: u64,
        container_generation: u64,
    },
    NetworkInspect {
        generation: u64,
        inspection_generation: u64,
        id: String,
    },
}
struct Inspection {
    generation: u64,
    container_id: String,
    container_name: String,
    loading: bool,
    details: Option<ContainerDetails>,
    restart_count_display: Option<String>,
    exit_code_display: Option<String>,
    error: bool,
    action_error: Option<String>,
    action_busy: bool,
    confirming_remove: bool,
    logs_open: bool,
}

#[cfg(target_os = "linux")]
enum LogEvent {
    Line {
        generation: u64,
        line: ContainerLogLine,
    },
    Reset {
        generation: u64,
    },
    Ended {
        generation: u64,
        error: Option<String>,
    },
}
#[cfg(target_os = "linux")]
const TERMINAL_OUTPUT_MESSAGES_PER_FRAME: usize = 16;
#[cfg(target_os = "linux")]
const TERMINAL_OUTPUT_CHUNK_BYTES: usize = 4096;
#[cfg(target_os = "linux")]
const TERMINAL_INPUT_CAPACITY: usize = 64;
#[cfg(target_os = "linux")]
const MAX_TERMINAL_INPUT_BYTES: usize = 64 * 1024;

#[cfg(target_os = "linux")]
struct PasteAuthorization {
    accept_after_frame: u64,
    expires_at: Instant,
}

#[cfg(target_os = "linux")]
struct TerminalView {
    model: TerminalModel,
    input: Option<mpsc::Sender<Vec<u8>>>,
    output: mpsc::Receiver<Vec<u8>>,
    resize_sender: Option<watch::Sender<TerminalSize>>,
    resize_failed: Arc<AtomicBool>,
    io_task: Option<JoinHandle<()>>,
    resize_task: Option<JoinHandle<()>>,
    ended: bool,
    paste_authorization: Option<PasteAuthorization>,
    input_notice: Option<&'static str>,
}
#[cfg(target_os = "linux")]
impl TerminalView {
    fn send_input(&mut self, bytes: &[u8]) {
        if bytes.len() > MAX_TERMINAL_INPUT_BYTES {
            self.input_notice = Some("Input exceeds the 64 KiB terminal limit.");
            return;
        }
        self.send_input_owned(bytes.to_vec());
    }

    fn send_paste(&mut self, text: &str) {
        if !self.model.bracketed_paste() {
            self.send_input(text.as_bytes());
            return;
        }
        const PASTE_START: &[u8] = b"\x1b[200~";
        const PASTE_END: &[u8] = b"\x1b[201~";
        let size = text
            .len()
            .saturating_add(PASTE_START.len() + PASTE_END.len());
        if size > MAX_TERMINAL_INPUT_BYTES {
            self.input_notice = Some("Input exceeds the 64 KiB terminal limit.");
            return;
        }
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(PASTE_START);
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(PASTE_END);
        self.send_input_owned(bytes);
    }
    fn request_paste(&mut self, context: &egui::Context) {
        if self.ended {
            return;
        }
        let frame = context.cumulative_frame_nr();
        self.paste_authorization = Some(PasteAuthorization {
            accept_after_frame: frame.saturating_add(1),
            expires_at: Instant::now() + Duration::from_secs(3),
        });
        context.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
    }

    fn deliver_paste_event(&mut self, text: &str, focused: bool, frame: u64, now: Instant) -> bool {
        if !focused {
            self.paste_authorization = None;
            return false;
        }
        let Some(authorization) = &self.paste_authorization else {
            return false;
        };
        if now >= authorization.expires_at {
            self.paste_authorization = None;
            return false;
        }
        if frame < authorization.accept_after_frame {
            return false;
        }
        self.paste_authorization = None;
        self.send_paste(text);
        true
    }

    fn send_input_owned(&mut self, bytes: Vec<u8>) {
        if self.ended {
            return;
        }
        let Some(input) = &self.input else {
            return;
        };
        if input.capacity() == 0 {
            self.input_notice = Some("Terminal input is busy; retry after it clears.");
            return;
        }
        match input.try_send(bytes) {
            Ok(()) => self.input_notice = None,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.input_notice = Some("Terminal input is busy; retry after it clears.");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => self.end_session(),
        }
    }

    fn end_session(&mut self) {
        self.ended = true;
        self.input.take();
        self.resize_sender.take();
        if let Some(task) = self.io_task.take() {
            task.abort();
        }
        if let Some(task) = self.resize_task.take() {
            task.abort();
        }
        self.paste_authorization = None;
        self.input_notice = Some("Terminal session ended; input is disabled.");
    }

    fn show(&mut self, context: &egui::Context) -> bool {
        let mut open = true;
        let mut terminal_focused = false;
        let mut copy_output = false;
        let mut request_paste = false;
        let mut request_input_focus = false;

        let mut output_messages = 0;
        while output_messages < TERMINAL_OUTPUT_MESSAGES_PER_FRAME {
            match self.output.try_recv() {
                Ok(bytes) => {
                    output_messages += 1;
                    self.model.feed(&bytes);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if !self.ended {
                        self.end_session();
                    }
                    break;
                }
            }
        }
        if output_messages == TERMINAL_OUTPUT_MESSAGES_PER_FRAME {
            context.request_repaint();
        }

        egui::Window::new("Container terminal")
            .id(egui::Id::new("container-terminal"))
            .open(&mut open)
            .resizable(true)
            .default_size([820.0, 520.0])
            .show(context, |ui| {
                let focus_id = ui.id().with("terminal-input");
                ui.horizontal(|ui| {
                    ui.label("Ctrl+C sends ETX.");
                    if ui.button("Copy output").clicked() {
                        copy_output = true;
                    }
                    if ui
                        .add_enabled(!self.ended, egui::Button::new("Paste from clipboard"))
                        .clicked()
                    {
                        request_paste = true;
                        request_input_focus = true;
                    }
                    if self.model.alternate_screen() {
                        ui.label("Alternate screen");
                    }
                });
                if self.ended {
                    ui.label("Terminal session ended. Input is disabled.");
                }
                if let Some(notice) = self.input_notice {
                    ui.label(notice);
                }
                if self.resize_failed.load(Ordering::Acquire) {
                    ui.colored_label(
                        Color32::from_rgb(224, 174, 109),
                        "Terminal resize failed; it will retry after the next window resize.",
                    );
                }
                let available = ui.available_size();
                let cell = ui.text_style_height(&egui::TextStyle::Monospace);
                let columns = (available.x / (cell * 0.62)).floor().max(1.0) as u16;
                let rows = (available.y / cell).floor().max(1.0) as u16;
                if self.model.dimensions() != (rows, columns) && !self.ended {
                    self.model.resize(rows, columns);
                    if let Some(resize) = &self.resize_sender {
                        resize.send_replace(TerminalSize { rows, columns });
                    }
                }
                let focused = ui.memory(|memory| memory.has_focus(focus_id));
                if ui
                    .add_enabled(
                        !self.ended,
                        egui::Button::new(if focused {
                            "Terminal focused"
                        } else {
                            "Focus terminal"
                        }),
                    )
                    .clicked()
                {
                    request_input_focus = true;
                }
                let scroll = egui::ScrollArea::both()
                    .stick_to_bottom(self.model.scrollback_position() == 0)
                    .show(ui, |ui| self.model.render(ui));
                let response = ui.interact(scroll.inner_rect, focus_id, egui::Sense::click());
                if (response.clicked() || request_input_focus) && !self.ended {
                    response.request_focus();
                }
                terminal_focused =
                    (response.has_focus() || response.clicked() || request_input_focus)
                        && !self.ended;
            });

        if !open {
            return false;
        }
        if copy_output {
            context.copy_text(self.model.contents());
        }
        let frame = context.cumulative_frame_nr();
        let now = Instant::now();
        if !terminal_focused {
            self.paste_authorization = None;
        }
        if request_paste && terminal_focused {
            self.request_paste(context);
        }
        if terminal_focused {
            let mut copy_shortcut = false;
            let mut scroll_changed = false;
            context.input(|input| {
                let has_native_copy = input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy));
                let has_ctrl_shift_c_key = input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::C,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.ctrl && modifiers.shift
                    )
                });
                for event in &input.events {
                    match event {
                        egui::Event::Text(text) => self.send_input(text.as_bytes()),
                        egui::Event::Copy => {
                            if input.modifiers.shift || has_ctrl_shift_c_key {
                                copy_shortcut = true;
                            } else {
                                self.send_input(&[0x03]);
                            }
                        }
                        egui::Event::Paste(text) => {
                            self.deliver_paste_event(text, true, frame, now);
                        }
                        egui::Event::MouseWheel { delta, .. }
                            if delta.y != 0.0 && !self.model.alternate_screen() =>
                        {
                            self.model.scrollback_by(if delta.y > 0.0 { 3 } else { -3 });
                            scroll_changed = true;
                        }
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } => {
                            if modifiers.ctrl && *key == egui::Key::C {
                                if modifiers.shift {
                                    copy_shortcut = true;
                                } else if !has_native_copy {
                                    self.send_input(&[0x03]);
                                }
                            } else if matches!(key, egui::Key::PageUp | egui::Key::PageDown)
                                && !modifiers.ctrl
                                && !self.model.alternate_screen()
                            {
                                let rows = isize::try_from(self.model.dimensions().0)
                                    .unwrap_or(isize::MAX);
                                self.model.scrollback_by(if *key == egui::Key::PageUp {
                                    rows
                                } else {
                                    -rows
                                });
                                scroll_changed = true;
                            } else if let Some(bytes) = terminal::key_bytes(
                                *key,
                                *modifiers,
                                self.model.application_cursor(),
                            ) {
                                self.model.scroll_to_bottom();
                                self.send_input(&bytes);
                            }
                        }
                        _ => {}
                    }
                }
            });
            if scroll_changed {
                context.request_repaint();
            }
            if copy_shortcut {
                context.copy_text(self.model.contents());
            }
        }
        if self
            .paste_authorization
            .as_ref()
            .is_some_and(|request| now >= request.expires_at)
        {
            self.paste_authorization = None;
        }
        true
    }
}

#[cfg(target_os = "linux")]
impl Drop for TerminalView {
    fn drop(&mut self) {
        if let Some(task) = self.io_task.take() {
            task.abort();
        }
        if let Some(task) = self.resize_task.take() {
            task.abort();
        }
    }
}

#[cfg(target_os = "linux")]
enum TerminalOpened {
    Opened {
        attempt_id: u64,
        input: mpsc::Sender<Vec<u8>>,
        output: mpsc::Receiver<Vec<u8>>,
        resize: watch::Sender<TerminalSize>,
        resize_failed: Arc<AtomicBool>,
        io_task: JoinHandle<()>,
        resize_task: JoinHandle<()>,
    },
    Failed {
        attempt_id: u64,
        message: String,
    },
}

#[cfg(target_os = "linux")]
trait LogLineStream {
    type Error: std::fmt::Display;

    async fn next_line(&mut self) -> Result<Option<ContainerLogLine>, Self::Error>;
}

#[cfg(target_os = "linux")]
trait LogStreamOpener {
    type Error: std::fmt::Display;
    type Stream: LogLineStream<Error = Self::Error>;

    async fn open(&mut self) -> Result<Self::Stream, Self::Error>;
}

#[cfg(target_os = "linux")]
struct EngineLogOpener {
    engine: EngineClient,
    container_id: String,
    tty: bool,
    options: ContainerLogsOptions,
}

#[cfg(target_os = "linux")]
impl LogLineStream for ferrum_engine::ContainerLogStream {
    type Error = EngineError;

    async fn next_line(&mut self) -> Result<Option<ContainerLogLine>, Self::Error> {
        ferrum_engine::ContainerLogStream::next_line(self).await
    }
}

#[cfg(target_os = "linux")]
impl LogStreamOpener for EngineLogOpener {
    type Error = EngineError;
    type Stream = ferrum_engine::ContainerLogStream;

    async fn open(&mut self) -> Result<Self::Stream, Self::Error> {
        self.engine
            .open_container_logs(&self.container_id, self.tty, self.options)
            .await
    }
}

#[cfg(target_os = "linux")]
async fn run_log_retry_loop<O: LogStreamOpener>(
    opener: &mut O,
    sender: &mpsc::Sender<LogEvent>,
    context: &egui::Context,
    generation: u64,
    follow: bool,
) {
    let mut retries = RetryBudget::default();
    let mut has_reconnected = false;
    loop {
        let mut stream = match opener.open().await {
            Ok(stream) => {
                if has_reconnected {
                    if sender.send(LogEvent::Reset { generation }).await.is_err() {
                        return;
                    }
                    context.request_repaint();
                }
                stream
            }
            Err(error) => {
                if !follow || !retries.wait_next().await {
                    let _ = sender
                        .send(LogEvent::Ended {
                            generation,
                            error: Some(error.to_string()),
                        })
                        .await;
                    context.request_repaint();
                    return;
                }
                has_reconnected = true;
                continue;
            }
        };
        let disconnect_error = loop {
            match stream.next_line().await {
                Ok(Some(line)) => {
                    if sender
                        .send(LogEvent::Line { generation, line })
                        .await
                        .is_err()
                    {
                        return;
                    }
                    context.request_repaint();
                }
                Ok(None) => {
                    let _ = sender
                        .send(LogEvent::Ended {
                            generation,
                            error: None,
                        })
                        .await;
                    context.request_repaint();
                    return;
                }
                Err(error) => {
                    if !follow {
                        let _ = sender
                            .send(LogEvent::Ended {
                                generation,
                                error: Some(error.to_string()),
                            })
                            .await;
                        context.request_repaint();
                        return;
                    }
                    break error;
                }
            }
        };
        if retries.wait_next().await {
            has_reconnected = true;
            continue;
        }
        let _ = sender
            .send(LogEvent::Ended {
                generation,
                error: Some(disconnect_error.to_string()),
            })
            .await;
        context.request_repaint();
        return;
    }
}

#[cfg(target_os = "linux")]
struct CachedDroppedLogNotice {
    dropped_lines: usize,
    font_id: egui::FontId,
    color: Color32,
    wrap_width: f32,
    galley: std::sync::Arc<egui::Galley>,
}

#[cfg(target_os = "linux")]
async fn persist_audit_record(store: &audit::AuditStore, record: audit::AuditRecord) {
    if store.persist(record).await.is_err() {
        tracing::warn!("Could not persist local audit record");
    }
}

#[cfg(not(target_os = "linux"))]
async fn persist_audit_record(_store: &audit::AuditStore, _record: audit::AuditRecord) {}

struct FerrumApp {
    runtime: Runtime,
    sender: mpsc::Sender<EngineEvent>,
    receiver: mpsc::Receiver<EngineEvent>,
    connection: ConnectionState,
    engine: Option<EngineClient>,
    engine_info: Option<EngineInfo>,
    containers: Vec<ContainerSummary>,
    // Attachment controls fail closed until the current container inventory is refreshed.
    container_inventory_fresh: bool,
    container_inventory_generation: u64,
    inventory_view: InventoryView,
    page: images::AppPage,
    images: images::ImagesView,
    volumes: volumes::VolumesView,
    networks: networks::NetworksView,
    compose: compose::ComposeView,
    // Keep submitted network mutations alive across page invalidation; the
    // engine may already have accepted them and success should refresh inventory.
    network_create_task: Option<JoinHandle<()>>,
    network_remove_task: Option<JoinHandle<()>>,
    network_attachment_task: Option<JoinHandle<()>>,
    volume_create_task: Option<JoinHandle<()>>,
    volume_remove_task: Option<JoinHandle<()>>,
    resource_read_task: Option<JoinHandle<()>>,
    pending_resource_read: Option<PendingResourceRead>,
    compose_task: Option<JoinHandle<()>>,
    compose_validation_task: Option<JoinHandle<()>>,
    image_list_task: Option<JoinHandle<()>>,
    image_inspection_task: Option<JoinHandle<()>>,
    // Hold the reader until the terminal response event releases the mutation gate.
    image_pull_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    image_pull_progress_reporting: Option<Arc<AtomicBool>>,
    // Keep a submitted push reader alive across page invalidation so it can
    // drain the terminal response and retain the image-mutation gate.
    image_push_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    image_push_progress_reporting: Option<Arc<AtomicBool>>,
    // Keep a submitted tag POST alive across page invalidation; aborting it
    // would not undo a mutation the engine may already have accepted.
    image_tag_task: Option<JoinHandle<()>>,
    // A submitted removal may already have reached Docker; never abort it as
    // though that could roll back the mutation.
    image_remove_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    image_build_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    image_build_cancel: Option<oneshot::Sender<()>>,

    loading: bool,
    refresh_error: Option<String>,
    contexts: Vec<DockerContext>,
    selected_context: Option<String>,
    context_error: Option<String>,
    contexts_error: Option<String>,
    inspection: Option<Inspection>,
    action_task: Option<JoinHandle<()>>,
    inspection_task: Option<JoinHandle<()>>,
    next_inspection_generation: u64,
    event_generation: u64,
    event_task: Option<JoinHandle<()>>,
    event_warning: Option<String>,
    event_refresh_pending: bool,
    #[cfg(target_os = "linux")]
    log_sender: mpsc::Sender<LogEvent>,
    #[cfg(target_os = "linux")]
    log_receiver: mpsc::Receiver<LogEvent>,
    #[cfg(target_os = "linux")]
    log_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    log_generation: u64,
    #[cfg(target_os = "linux")]
    log_buffer: LogBuffer,
    #[cfg(target_os = "linux")]
    log_dropped_notice: Option<CachedDroppedLogNotice>,
    #[cfg(target_os = "linux")]
    log_options: ContainerLogsOptions,
    #[cfg(target_os = "linux")]
    log_notice: Option<String>,
    #[cfg(target_os = "linux")]
    log_action_notice: Option<String>,
    #[cfg(target_os = "linux")]
    log_paused: bool,
    #[cfg(target_os = "linux")]
    log_auto_scroll: bool,
    #[cfg(target_os = "linux")]
    log_reconnected: bool,
    #[cfg(target_os = "linux")]
    log_started: bool,
    #[cfg(target_os = "linux")]
    log_search: String,
    #[cfg(target_os = "linux")]
    log_stdout: bool,
    #[cfg(target_os = "linux")]
    log_stderr: bool,
    #[cfg(target_os = "linux")]
    terminal_open_sender: mpsc::Sender<TerminalOpened>,
    #[cfg(target_os = "linux")]
    terminal: Option<TerminalView>,
    #[cfg(target_os = "linux")]
    terminal_opened: mpsc::Receiver<TerminalOpened>,
    #[cfg(target_os = "linux")]
    terminal_attempt: u64,
    #[cfg(target_os = "linux")]
    terminal_pending: bool,
    #[cfg(target_os = "linux")]
    terminal_open_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    terminal_shell: String,
    #[cfg(target_os = "linux")]
    terminal_notice: Option<String>,
    settings_store: Arc<settings::SettingsStore>,
    audit_store: audit::AuditStore,
    settings: settings::Settings,
    settings_draft: settings::Settings,
    settings_open: bool,
    settings_error: Option<settings::SettingsError>,
}

impl FerrumApp {
    fn new(creation_context: &eframe::CreationContext<'_>, runtime: Runtime) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(29, 39, 49);
        visuals.window_fill = Color32::from_rgb(29, 39, 49);
        visuals.extreme_bg_color = Color32::from_rgb(20, 28, 36);
        visuals.override_text_color = Some(Color32::from_rgb(226, 232, 235));
        visuals.selection.bg_fill = Color32::from_rgb(168, 89, 55);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(41, 53, 64);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(59, 73, 84);
        creation_context.egui_ctx.set_visuals(visuals);

        let settings_store = Arc::new(settings::SettingsStore::new());
        let (settings, settings_error) = settings_store.load();
        #[cfg(target_os = "linux")]
        let audit_store = audit::AuditStore::new(Arc::clone(&settings_store));
        #[cfg(not(target_os = "linux"))]
        let audit_store = audit::AuditStore::disabled();
        if settings_error.is_some() {
            tracing::warn!(
                error_kind = "settings",
                "Could not load application settings"
            );
        }
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        #[cfg(target_os = "linux")]
        let (log_sender, log_receiver) = mpsc::channel(LOG_CHANNEL_CAPACITY);
        #[cfg(target_os = "linux")]
        let (terminal_open_sender, terminal_opened) = mpsc::channel(1);
        let mut app = Self {
            runtime,
            sender,
            receiver,
            #[cfg(target_os = "linux")]
            log_sender,
            #[cfg(target_os = "linux")]
            terminal_open_sender,
            #[cfg(target_os = "linux")]
            terminal_opened,
            #[cfg(target_os = "linux")]
            terminal: None,
            #[cfg(target_os = "linux")]
            terminal_attempt: 0,
            #[cfg(target_os = "linux")]
            terminal_pending: false,
            #[cfg(target_os = "linux")]
            terminal_open_task: None,
            #[cfg(target_os = "linux")]
            terminal_shell: "/bin/sh".into(),
            #[cfg(target_os = "linux")]
            terminal_notice: None,
            #[cfg(target_os = "linux")]
            log_receiver,
            #[cfg(target_os = "linux")]
            log_task: None,
            #[cfg(target_os = "linux")]
            log_generation: 0,
            #[cfg(target_os = "linux")]
            log_buffer: LogBuffer::new(),
            #[cfg(target_os = "linux")]
            log_dropped_notice: None,
            #[cfg(target_os = "linux")]
            log_options: ContainerLogsOptions {
                stdout: true,
                stderr: true,
                timestamps: true,
                follow: false,
                tail: 500,
            },
            #[cfg(target_os = "linux")]
            log_notice: None,
            #[cfg(target_os = "linux")]
            log_action_notice: None,
            #[cfg(target_os = "linux")]
            log_paused: false,
            #[cfg(target_os = "linux")]
            log_auto_scroll: true,
            #[cfg(target_os = "linux")]
            log_reconnected: false,
            #[cfg(target_os = "linux")]
            log_started: false,
            #[cfg(target_os = "linux")]
            log_search: String::new(),
            #[cfg(target_os = "linux")]
            log_stdout: true,
            #[cfg(target_os = "linux")]
            log_stderr: true,
            connection: if settings.auto_connect_on_startup {
                ConnectionState::Connecting
            } else {
                ConnectionState::Disconnected(
                    "Automatic connection is disabled. Use Retry connection to connect.".into(),
                )
            },
            engine: None,
            engine_info: None,
            containers: Vec::new(),
            container_inventory_fresh: false,
            container_inventory_generation: 0,
            inventory_view: InventoryView::default(),
            page: images::AppPage::default(),
            images: images::ImagesView::default(),
            volumes: volumes::VolumesView::default(),
            networks: networks::NetworksView::default(),
            compose: compose::ComposeView::default(),
            network_create_task: None,
            network_remove_task: None,
            network_attachment_task: None,
            volume_create_task: None,
            volume_remove_task: None,
            resource_read_task: None,
            pending_resource_read: None,
            compose_task: None,
            compose_validation_task: None,
            image_list_task: None,
            image_inspection_task: None,
            image_pull_task: None,
            #[cfg(target_os = "linux")]
            image_pull_progress_reporting: None,
            image_push_task: None,
            #[cfg(target_os = "linux")]
            image_push_progress_reporting: None,
            image_tag_task: None,
            image_remove_task: None,
            #[cfg(target_os = "linux")]
            image_build_task: None,
            #[cfg(target_os = "linux")]
            image_build_cancel: None,
            loading: false,
            refresh_error: None,

            contexts: Vec::new(),
            selected_context: None,
            context_error: None,
            contexts_error: None,
            inspection: None,
            inspection_task: None,
            next_inspection_generation: 0,
            event_generation: 0,
            event_task: None,
            action_task: None,
            event_warning: None,
            event_refresh_pending: false,
            audit_store,
            settings_store,
            settings_draft: settings.clone(),
            settings,
            settings_open: false,
            settings_error,
        };
        let context_sender = app.sender.clone();
        let context = creation_context.egui_ctx.clone();
        app.runtime.spawn(async move {
            let contexts = EngineClient::available_contexts().await;
            if context_sender
                .send(EngineEvent::Contexts(contexts))
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        });
        if app.settings.auto_connect_on_startup {
            app.connect(creation_context.egui_ctx.clone());
        }
        app
    }
    #[cfg(target_os = "linux")]
    fn start_terminal(&mut self, container_id: String, shell: String, context: egui::Context) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        self.terminal_attempt = self.terminal_attempt.wrapping_add(1);
        let attempt_id = self.terminal_attempt;
        self.terminal_pending = true;
        self.terminal_notice = None;
        let sender = self.terminal_open_sender.clone();
        self.terminal_open_task = Some(self.runtime.spawn(async move {
            let size = TerminalSize { rows: 24, columns: 80 };
            let command = [shell];
            let session = match engine.open_exec_session(&container_id, &command, size).await {
                Ok(session) => session,
                Err(error) => {
                    let _ = sender
                        .send(TerminalOpened::Failed {
                            attempt_id,
                            message: error.to_string(),
                        })
                        .await;
                    context.request_repaint();
                    return;
                }
            };
            let resize_handle = session.resize_handle();
            let resize_worker_handle = resize_handle.clone();
            let resize_failed = Arc::new(AtomicBool::new(false));
            let worker_resize_failed = Arc::clone(&resize_failed);
            let resize_context = context.clone();
            let (resize_sender, mut resize_receiver) = watch::channel(size);
            let resize_task = tokio::spawn(async move {
                while resize_receiver.changed().await.is_ok() {
                    let size = *resize_receiver.borrow_and_update();
                    if resize_worker_handle.resize(size).await.is_err() {
                        worker_resize_failed.store(true, Ordering::Release);
                    } else {
                        worker_resize_failed.store(false, Ordering::Release);
                    }
                    resize_context.request_repaint();
                }
            });

            let (mut reader, mut writer) = tokio::io::split(session);
            let (input, mut input_rx) = mpsc::channel::<Vec<u8>>(TERMINAL_INPUT_CAPACITY);
            let (output_tx, output) = mpsc::channel::<Vec<u8>>(64);
            let io_context = context.clone();
            let io_task = tokio::spawn(async move {
                let mut buffer = [0u8; TERMINAL_OUTPUT_CHUNK_BYTES];
                loop {
                    tokio::select! {
                        result = tokio::io::AsyncReadExt::read(&mut reader, &mut buffer) => {
                            match result {
                                Ok(0) | Err(_) => break,
                                Ok(count) => if output_tx.send(buffer[..count].to_vec()).await.is_err() { break; },
                            }
                        }
                        bytes = input_rx.recv() => match bytes {
                            Some(bytes) => if tokio::io::AsyncWriteExt::write_all(&mut writer, &bytes).await.is_err() { break; },
                            None => break,
                        }
                    }
                    io_context.request_repaint();
                }
                io_context.request_repaint();
            });
            let _ = sender.send(TerminalOpened::Opened {
                attempt_id,
                input,
                output,
                resize: resize_sender,
                resize_failed,
                io_task,
                resize_task,
            }).await;
            context.request_repaint();
        }));
    }
    #[cfg(target_os = "linux")]
    fn cancel_terminal_session(&mut self) {
        self.terminal_attempt = self.terminal_attempt.wrapping_add(1);
        if let Some(task) = self.terminal_open_task.take() {
            task.abort();
        }
        self.terminal_pending = false;
        self.terminal.take();
        while let Ok(opened) = self.terminal_opened.try_recv() {
            if let TerminalOpened::Opened {
                io_task,
                resize_task,
                ..
            } = opened
            {
                io_task.abort();
                resize_task.abort();
            }
        }
    }
    fn action_busy(&self) -> bool {
        self.inspection
            .as_ref()
            .is_some_and(|inspection| inspection.action_busy)
    }
    fn cancel_image_read_tasks(&mut self) {
        if let Some(task) = self.image_list_task.take() {
            task.abort();
        }
        if let Some(task) = self.image_inspection_task.take() {
            task.abort();
        }
    }

    fn hide_image_pull_progress(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(reporting) = &self.image_pull_progress_reporting {
            reporting.store(false, Ordering::Release);
        }
        self.images.hide_pull_progress();
    }
    #[cfg(target_os = "linux")]
    fn hide_image_push_progress(&mut self) {
        if let Some(reporting) = &self.image_push_progress_reporting {
            reporting.store(false, Ordering::Release);
        }
        self.images.hide_push_progress();
    }

    fn invalidate_volumes(&mut self) {
        self.pending_resource_read = None;
        self.volumes.invalidate();
    }
    fn invalidate_networks(&mut self) {
        self.pending_resource_read = None;
        self.networks.invalidate();
    }
    fn invalidate_compose(&mut self) {
        if let Some(task) = self.compose_task.take() {
            task.abort();
        }
        if let Some(task) = self.compose_validation_task.take() {
            task.abort();
        }
        self.compose.invalidate();
    }

    fn cancel_compose_validation(&mut self, context: egui::Context) {
        if let Some(task) = self.compose_validation_task.take() {
            task.abort();
        }
        self.compose.cancel_validation();
        context.request_repaint();
    }

    fn select_compose_file(&mut self, override_file: bool, context: egui::Context) {
        #[cfg(target_os = "linux")]
        {
            if self.compose.is_file_selection_running() || self.compose.is_validation_running() {
                return;
            }
            if let Some(task) = self.compose_validation_task.take() {
                task.abort();
            }
            let generation = if override_file {
                self.compose.begin_override_selection()
            } else {
                self.compose.begin_file_selection()
            };
            context.request_repaint();
            let sender = self.sender.clone();
            self.compose_validation_task = Some(self.runtime.spawn(async move {
                let file = rfd::AsyncFileDialog::new()
                    .add_filter("Compose YAML files", &["yaml", "yml"])
                    .pick_file()
                    .await
                    .map(|file| file.path().to_path_buf());
                if sender
                    .send(EngineEvent::ComposeFileSelectionFinished { generation, file })
                    .await
                    .is_ok()
                {
                    context.request_repaint();
                }
            }));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (self, override_file, context);
    }

    #[cfg(target_os = "linux")]
    fn start_compose_validation(
        &mut self,
        files: Vec<std::path::PathBuf>,
        generation: u64,
        context: egui::Context,
    ) {
        context.request_repaint();
        if let Some(task) = self.compose_validation_task.take() {
            task.abort();
        }
        let sender = self.sender.clone();
        self.compose_validation_task = Some(self.runtime.spawn(async move {
            let result = compose::validate_compose_files(&files).await;
            if sender
                .send(EngineEvent::ComposeValidationFinished { generation, result })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn load_compose(&mut self, context: egui::Context) {
        #[cfg(target_os = "linux")]
        {
            if self.loading || self.compose.is_loading() {
                return;
            }
            let Some(socket_path) = self
                .engine
                .as_ref()
                .map(|engine| engine.local_socket_path().to_path_buf())
            else {
                return;
            };
            let generation = self.compose.begin_load();
            let sender = self.sender.clone();
            self.compose_task = Some(self.runtime.spawn(async move {
                let result = compose::discover_projects(&socket_path).await;
                if sender
                    .send(EngineEvent::ComposeListed { generation, result })
                    .await
                    .is_ok()
                {
                    context.request_repaint();
                }
            }));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (self, context);
    }
    fn invalidate_resource_reads(&mut self) {
        self.pending_resource_read = None;
        self.volumes.invalidate();
        self.networks.invalidate();
        self.invalidate_container_inventory();
    }
    fn load_volumes(&mut self, context: egui::Context) {
        if self.loading
            || self.volume_mutation_busy()
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        let generation = self.volumes.begin_list();
        self.pending_resource_read = Some(PendingResourceRead::VolumeList { generation });
        self.start_pending_resource_read(context);
    }
    fn inspect_volume(&mut self, name: String, context: egui::Context) {
        if !cfg!(target_os = "linux")
            || self.loading
            || self.volume_mutation_busy()
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        let (generation, inspection_generation, name) = self.volumes.begin_inspection(name);
        self.pending_resource_read = Some(PendingResourceRead::VolumeInspect {
            generation,
            inspection_generation,
            name,
        });
        self.start_pending_resource_read(context);
    }
    fn start_volume_create(&mut self, generation: u64, name: String, context: egui::Context) {
        if !self.volumes.creation_is_pending(generation, &name) {
            return;
        }
        if self.volume_create_task.is_some()
            || self.volume_remove_task.is_some()
            || self.loading
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
            || self.page != images::AppPage::Volumes
        {
            self.volumes.cancel_create_submission(generation);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.volumes.finish_create(
                generation,
                &name,
                Err("The selected engine connection is unavailable.".to_owned()),
            );
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.volume_create_task = Some(self.runtime.spawn(async move {
            let result = engine.create_volume(&name).await;
            #[cfg(target_os = "linux")]
            persist_audit_record(&audit_store, audit::record_volume_create_result(&result)).await;
            if sender
                .send(EngineEvent::VolumeCreated {
                    generation,
                    name,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }
    fn start_volume_remove(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        name: String,
        context: egui::Context,
    ) {
        if self.volume_create_task.is_some()
            || self.volume_remove_task.is_some()
            || self.loading
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
            || self.page != images::AppPage::Volumes
            || !self.volumes.removal_is_pending(
                generation,
                inspection_generation,
                removal_generation,
                &name,
            )
        {
            self.volumes.cancel_remove_submission(removal_generation);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.volumes.finish_remove(
                generation,
                inspection_generation,
                removal_generation,
                &name,
                Err("The selected engine connection is unavailable.".to_owned()),
            );
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.volume_remove_task = Some(self.runtime.spawn(async move {
            let result = engine.remove_volume(&name).await;
            #[cfg(target_os = "linux")]
            persist_audit_record(&audit_store, audit::record_volume_remove_result(&result)).await;
            if sender
                .send(EngineEvent::VolumeRemoved {
                    generation,
                    inspection_generation,
                    removal_generation,
                    name,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }
    fn invalidate_container_inventory(&mut self) {
        self.container_inventory_generation = self.container_inventory_generation.wrapping_add(1);
        self.container_inventory_fresh = false;
        self.refresh_error = None;
    }

    fn replace_container_inventory(&mut self, containers: Vec<ContainerSummary>) {
        self.container_inventory_generation = self.container_inventory_generation.wrapping_add(1);
        self.containers = containers;
        self.inventory_view.inventory_changed(&self.containers);
        self.container_inventory_fresh = true;
        self.refresh_error = None;
    }
    fn load_networks(&mut self, context: egui::Context) {
        if self.loading
            || self.network_mutation_busy()
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        self.invalidate_container_inventory();
        let generation = self.networks.begin_list();
        let container_generation = self.container_inventory_generation;
        self.pending_resource_read = Some(PendingResourceRead::NetworkList {
            generation,
            container_generation,
        });
        self.start_pending_resource_read(context);
    }
    fn network_task_in_flight(&self) -> bool {
        self.network_create_task.is_some()
            || self.network_remove_task.is_some()
            || self.network_attachment_task.is_some()
    }

    fn network_mutation_busy(&self) -> bool {
        self.network_task_in_flight() || self.networks.has_mutation_in_flight()
    }
    fn volume_mutation_busy(&self) -> bool {
        self.volume_create_task.is_some()
            || self.volume_remove_task.is_some()
            || self.volumes.has_mutation_in_flight()
    }

    fn start_network_create(&mut self, generation: u64, name: String, context: egui::Context) {
        if self.network_task_in_flight()
            || self.loading
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.network_create_task = Some(self.runtime.spawn(async move {
            let result = engine.create_network(&name).await;
            persist_audit_record(
                &audit_store,
                audit::record_network_mutation_result(
                    result.as_deref().ok(),
                    audit::NetworkMutation::Create,
                    result.is_ok(),
                ),
            )
            .await;
            if sender
                .send(EngineEvent::NetworkCreated {
                    generation,
                    name,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }
    fn start_network_remove(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        id: String,
        name: String,
        context: egui::Context,
    ) {
        if self.network_task_in_flight()
            || self.loading
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
            || self.page != images::AppPage::Networks
            || !self.networks.removal_is_pending(
                generation,
                inspection_generation,
                removal_generation,
                &id,
                &name,
            )
        {
            self.networks.cancel_remove_submission(removal_generation);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.networks.cancel_remove_submission(removal_generation);
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.network_remove_task = Some(self.runtime.spawn(async move {
            let result = engine.remove_network(&id).await;
            persist_audit_record(
                &audit_store,
                audit::record_network_mutation_result(
                    Some(&id),
                    audit::NetworkMutation::Remove,
                    result.is_ok(),
                ),
            )
            .await;
            if sender
                .send(EngineEvent::NetworkRemoved {
                    generation,
                    inspection_generation,
                    removal_generation,
                    id,
                    name,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn start_network_attachment(
        &mut self,
        operation: networks::NetworkAttachmentOperation,
        context: egui::Context,
    ) {
        if self.network_task_in_flight()
            || self.loading
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
            || self.page != images::AppPage::Networks
            || !self.container_inventory_fresh
            || !self
                .networks
                .attachment_is_pending(&operation, &self.containers)
        {
            self.networks
                .cancel_attachment_submission(operation.mutation_generation);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.networks
                .cancel_attachment_submission(operation.mutation_generation);
            return;
        };
        let container_generation = self.container_inventory_generation;
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.network_attachment_task = Some(self.runtime.spawn(async move {
            let mutation_result = if operation.connect {
                engine
                    .connect_container_to_network(&operation.network_id, &operation.container_id)
                    .await
            } else {
                engine
                    .disconnect_container_from_network(
                        &operation.network_id,
                        &operation.container_id,
                    )
                    .await
            };
            if let Some(record) = audit::record_network_attachment_result(
                &operation.network_id,
                &operation.container_id,
                if operation.connect {
                    audit::NetworkMutation::Connect
                } else {
                    audit::NetworkMutation::Disconnect
                },
                mutation_result.is_ok(),
            ) {
                persist_audit_record(&audit_store, record).await;
            }
            let refreshed = async {
                let networks = engine.list_networks().await?;
                let containers = engine.list_containers().await?;
                let details = engine.inspect_network(&operation.network_id).await?;
                Ok::<_, EngineError>((networks, containers, details))
            }
            .await;
            if sender
                .send(EngineEvent::NetworkAttachmentFinished {
                    container_generation,
                    operation,
                    mutation_result,
                    refreshed,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn inspect_network(&mut self, id: String, context: egui::Context) {
        if !cfg!(target_os = "linux")
            || self.loading
            || self.network_mutation_busy()
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        let (generation, inspection_generation, id) = self.networks.begin_inspection(id);
        self.pending_resource_read = Some(PendingResourceRead::NetworkInspect {
            generation,
            inspection_generation,
            id,
        });
        self.start_pending_resource_read(context);
    }
    fn start_pending_resource_read(&mut self, context: egui::Context) {
        if self.resource_read_task.is_some() {
            return;
        }
        let Some(operation) = self.pending_resource_read.take() else {
            return;
        };
        if self.loading || !matches!(&self.connection, ConnectionState::Connected) {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let sender = self.sender.clone();
        match operation {
            PendingResourceRead::VolumeList { generation }
                if self.page == images::AppPage::Volumes
                    && generation == self.volumes.generation =>
            {
                self.resource_read_task = Some(self.runtime.spawn(async move {
                    let result = engine.list_volumes().await;
                    if sender
                        .send(EngineEvent::VolumesListed { generation, result })
                        .await
                        .is_ok()
                    {
                        context.request_repaint();
                    }
                }));
            }
            PendingResourceRead::VolumeInspect {
                generation,
                inspection_generation,
                name,
            } if self.page == images::AppPage::Volumes
                && generation == self.volumes.generation
                && inspection_generation == self.volumes.inspection_generation =>
            {
                self.resource_read_task = Some(self.runtime.spawn(async move {
                    let result = match engine.inspect_volume(&name).await {
                        Ok(details) => {
                            let usage_result = engine
                                .volume_usage(&name)
                                .await
                                .map_err(|error| error.to_string());
                            volumes::VolumeInspectionResult::Inspected {
                                details,
                                usage_result,
                            }
                        }
                        Err(error) => {
                            volumes::VolumeInspectionResult::InspectionFailed(error.to_string())
                        }
                    };
                    if sender
                        .send(EngineEvent::VolumeInspected {
                            generation,
                            inspection_generation,
                            name,
                            result,
                        })
                        .await
                        .is_ok()
                    {
                        context.request_repaint();
                    }
                }));
            }
            PendingResourceRead::NetworkList {
                generation,
                container_generation,
            } if self.page == images::AppPage::Networks
                && generation == self.networks.generation =>
            {
                self.resource_read_task = Some(self.runtime.spawn(async move {
                    let (network_result, container_result) =
                        tokio::join!(engine.list_networks(), engine.list_containers());
                    if sender
                        .send(EngineEvent::NetworksListed {
                            generation,
                            container_generation,
                            network_result,
                            container_result,
                        })
                        .await
                        .is_ok()
                    {
                        context.request_repaint();
                    }
                }));
            }
            PendingResourceRead::NetworkInspect {
                generation,
                inspection_generation,
                id,
            } if self.page == images::AppPage::Networks
                && generation == self.networks.generation
                && inspection_generation == self.networks.inspection_generation =>
            {
                self.resource_read_task = Some(self.runtime.spawn(async move {
                    let result = engine.inspect_network(&id).await;
                    if sender
                        .send(EngineEvent::NetworkInspected {
                            generation,
                            inspection_generation,
                            id,
                            result,
                        })
                        .await
                        .is_ok()
                    {
                        context.request_repaint();
                    }
                }));
            }
            _ => {}
        }
    }
    fn invalidate_images(&mut self) {
        self.cancel_image_read_tasks();
        #[cfg(target_os = "linux")]
        if let Some(reporting) = &self.image_pull_progress_reporting {
            reporting.store(false, Ordering::Release);
        }
        #[cfg(target_os = "linux")]
        if let Some(reporting) = &self.image_push_progress_reporting {
            reporting.store(false, Ordering::Release);
        }
        self.images.invalidate();
    }

    fn load_images(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.connect(context);
            return;
        };
        self.cancel_image_read_tasks();
        let generation = self.images.begin_load();
        let sender = self.sender.clone();
        self.image_list_task = Some(self.runtime.spawn(async move {
            let result = engine.list_images().await;
            if sender
                .send(EngineEvent::ImagesListed { generation, result })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn image_mutation_tasks_busy(&self) -> bool {
        self.image_pull_task.is_some()
            || self.image_push_task.is_some()
            || self.image_tag_task.is_some()
            || self.image_remove_task.is_some()
    }
    fn image_mutation_busy(&self) -> bool {
        self.image_mutation_tasks_busy() || self.images.has_mutation_in_flight()
    }
    #[cfg(target_os = "linux")]
    fn image_build_start_blocked(&self) -> bool {
        self.image_mutation_tasks_busy() || self.images.has_competing_mutation_in_flight()
    }
    #[cfg(target_os = "linux")]
    fn record_unsubmitted_image_build(&self) {
        let audit_store = self.audit_store.clone();
        drop(self.runtime.spawn(async move {
            Self::persist_image_build_outcome(&audit_store, audit::ImageBuildOutcome::NotSubmitted)
                .await;
        }));
    }

    #[cfg(target_os = "linux")]
    async fn persist_image_build_outcome(
        audit_store: &audit::AuditStore,
        outcome: audit::ImageBuildOutcome,
    ) {
        persist_audit_record(audit_store, audit::record_image_build_result(outcome)).await;
    }

    #[cfg(target_os = "linux")]
    fn image_build_busy(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.image_build_task.is_some() || self.images.build_blocks_context_switch()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    #[cfg(target_os = "linux")]
    fn start_image_build(&mut self, context: egui::Context) {
        if self.image_build_start_blocked() {
            self.images.cancel_build_confirmation();
            self.record_unsubmitted_image_build();
            return;
        }
        let Some(engine) = self.engine.as_ref() else {
            self.images.cancel_build_confirmation();
            self.record_unsubmitted_image_build();
            return;
        };
        let socket = engine.local_socket_path().to_path_buf();
        let Some((generation, draft)) = self.images.begin_build() else {
            self.images.cancel_build_confirmation();
            self.record_unsubmitted_image_build();
            return;
        };
        let (output_sender, mut output_receiver) = mpsc::channel(32);
        let (cancel_sender, mut cancel_receiver) = oneshot::channel();
        self.image_build_cancel = Some(cancel_sender);
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        let repaint = context.clone();
        self.image_build_task = Some(self.runtime.spawn(async move {
            let preparation = tokio::task::spawn_blocking(move || {
                let request = draft.validate()?;
                let config = image_build::create_private_config()?;
                let command = image_build::build_command(
                    std::ffi::OsStr::new("docker"),
                    &request,
                    &socket,
                    config.path(),
                )?;
                Ok::<_, image_build::ImageBuildError>((command, config))
            })
            .await;
            let (command, config) = match preparation {
                Ok(Ok(prepared)) => prepared,
                Ok(Err(error)) => {
                    Self::persist_image_build_outcome(
                        &audit_store,
                        audit::ImageBuildOutcome::NotSubmitted,
                    )
                    .await;
                    if sender
                        .send(EngineEvent::ImageBuildFinished {
                            generation,
                            result: Err(error),
                        })
                        .await
                        .is_ok()
                    {
                        repaint.request_repaint();
                    }
                    return;
                }
                Err(_) => {
                    let error = image_build::ImageBuildError::DockerUnavailable;
                    Self::persist_image_build_outcome(
                        &audit_store,
                        audit::ImageBuildOutcome::NotSubmitted,
                    )
                    .await;
                    if sender
                        .send(EngineEvent::ImageBuildFinished {
                            generation,
                            result: Err(error),
                        })
                        .await
                        .is_ok()
                    {
                        repaint.request_repaint();
                    }
                    return;
                }
            };
            if !matches!(
                cancel_receiver.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ) {
                drop(command);
                drop(config);
                let error = image_build::ImageBuildError::Cancelled;
                Self::persist_image_build_outcome(
                    &audit_store,
                    audit::ImageBuildOutcome::NotSubmitted,
                )
                .await;
                if sender
                    .send(EngineEvent::ImageBuildFinished {
                        generation,
                        result: Err(error),
                    })
                    .await
                    .is_ok()
                {
                    repaint.request_repaint();
                }
                return;
            }
            let (probe_sender, mut probe_receiver) = mpsc::channel(8);
            let (_probe_cancel_sender, probe_cancel_receiver) = oneshot::channel();
            let probe_result =
                match image_build::version_command(std::ffi::OsStr::new("docker"), config.path()) {
                    Ok(probe) => {
                        let probe = async {
                            let runner = image_build::run_command(
                                probe,
                                probe_cancel_receiver,
                                probe_sender,
                            );
                            tokio::pin!(runner);
                            loop {
                                tokio::select! {
                                    result = &mut runner => return result,
                                    line = probe_receiver.recv() => {
                                        if line.is_none() {
                                            return runner.await;
                                        }
                                    }
                                }
                            }
                        };
                        matches!(
                            tokio::time::timeout(Duration::from_secs(5), probe).await,
                            Ok(Ok(()))
                        )
                    }
                    Err(_) => false,
                };
            let cancelled_before_submit = !matches!(
                cancel_receiver.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            );
            let result = if cancelled_before_submit {
                drop(output_sender);
                Err(image_build::ImageBuildError::Cancelled)
            } else if probe_result {
                let runner = tokio::spawn(image_build::run_command(
                    command,
                    cancel_receiver,
                    output_sender,
                ));
                while let Some(line) = output_receiver.recv().await {
                    if sender
                        .send(EngineEvent::ImageBuildOutput { generation, line })
                        .await
                        .is_err()
                    {
                        break;
                    }
                    repaint.request_repaint();
                }
                runner
                    .await
                    .unwrap_or(Err(image_build::ImageBuildError::Failed))
            } else {
                drop(output_sender);
                Err(image_build::ImageBuildError::DockerUnavailable)
            };
            let outcome = if cancelled_before_submit {
                audit::ImageBuildOutcome::NotSubmitted
            } else {
                match result {
                    Ok(()) => audit::ImageBuildOutcome::Success,
                    Err(
                        image_build::ImageBuildError::DockerUnavailable
                        | image_build::ImageBuildError::InvalidContext
                        | image_build::ImageBuildError::InvalidTarget,
                    ) => audit::ImageBuildOutcome::NotSubmitted,
                    Err(_) => audit::ImageBuildOutcome::RequestError,
                }
            };
            Self::persist_image_build_outcome(&audit_store, outcome).await;
            if sender
                .send(EngineEvent::ImageBuildFinished { generation, result })
                .await
                .is_ok()
            {
                repaint.request_repaint();
            }
            drop(config);
        }));
    }

    #[cfg(target_os = "linux")]
    fn cancel_image_build(&mut self) {
        if let Some(cancel) = self.image_build_cancel.take() {
            let _ = cancel.send(());
        }
    }

    #[cfg(target_os = "linux")]
    fn start_image_pull(&mut self, context: egui::Context) {
        if self.image_mutation_busy() {
            return;
        }

        let Some(engine) = self.engine.clone() else {
            return;
        };
        let Some((generation, reference)) = self.images.begin_pull() else {
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        let progress_reporting = Arc::new(AtomicBool::new(true));
        self.image_pull_progress_reporting = Some(Arc::clone(&progress_reporting));
        self.image_pull_task = Some(self.runtime.spawn(async move {
            let result = async {
                let mut stream = engine.open_image_pull(&reference).await?;
                // Progress reporting may stop, but the reader must drain through the terminal response.
                while let Some(progress) = stream.next_progress().await? {
                    if progress_reporting.load(Ordering::Acquire) {
                        if sender
                            .send(EngineEvent::ImagePullProgress {
                                generation,
                                progress,
                            })
                            .await
                            .is_ok()
                        {
                            context.request_repaint();
                        } else {
                            progress_reporting.store(false, Ordering::Release);
                        }
                    }
                }
                Ok::<(), EngineError>(())
            }
            .await;
            persist_audit_record(
                &audit_store,
                audit::record_image_mutation_result(audit::ImageMutation::Pull, &result),
            )
            .await;
            if sender
                .send(EngineEvent::ImagePullFinished { generation, result })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }
    #[cfg(target_os = "linux")]
    fn start_image_push(&mut self, context: egui::Context) {
        if self.image_pull_task.is_some()
            || self.image_push_task.is_some()
            || self.image_tag_task.is_some()
            || self.image_remove_task.is_some()
            || self.image_build_task.is_some()
        {
            self.images.cancel_push_confirmation();
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.images.cancel_push_confirmation();
            return;
        };
        let Some(request) = self.images.begin_push() else {
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        let progress_reporting = Arc::new(AtomicBool::new(true));
        self.image_push_progress_reporting = Some(Arc::clone(&progress_reporting));
        self.image_push_task = Some(self.runtime.spawn(async move {
            let result = async {
                let mut stream = engine
                    .open_image_push(&request.image_id, &request.target)
                    .await?;
                // Stop rendering stale progress, but drain until a terminal response.
                while let Some(progress) = stream.next_progress().await? {
                    if progress_reporting.load(Ordering::Acquire) {
                        if sender
                            .send(EngineEvent::ImagePushProgress {
                                generation: request.generation,
                                progress,
                            })
                            .await
                            .is_ok()
                        {
                            context.request_repaint();
                        } else {
                            progress_reporting.store(false, Ordering::Release);
                        }
                    }
                }
                Ok::<(), EngineError>(())
            }
            .await;
            persist_audit_record(
                &audit_store,
                audit::record_image_mutation_result(audit::ImageMutation::Push, &result),
            )
            .await;
            if sender
                .send(EngineEvent::ImagePushFinished {
                    generation: request.generation,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }
    #[cfg(target_os = "linux")]
    fn start_image_tag(&mut self, context: egui::Context) {
        if self.image_mutation_busy() {
            self.images.cancel_tag_confirmation();
            return;
        }
        if self.image_tag_task.is_some() {
            self.images.cancel_tag_confirmation();
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.images.cancel_tag_confirmation();
            return;
        };
        let Some(request) = self.images.begin_tag() else {
            return;
        };

        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.image_tag_task = Some(self.runtime.spawn(async move {
            let result = engine
                .tag_image(&request.image_id, &request.repository, &request.tag)
                .await;
            persist_audit_record(
                &audit_store,
                audit::record_image_mutation_result(audit::ImageMutation::Tag, &result),
            )
            .await;
            if sender
                .send(EngineEvent::ImageTagged {
                    generation: request.generation,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }
    #[cfg(target_os = "linux")]
    fn start_image_remove(&mut self, context: egui::Context) {
        if self.image_mutation_busy() {
            self.images.cancel_remove_confirmation();
            return;
        }
        if self.image_remove_task.is_some() {
            self.images.cancel_remove_confirmation();
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.images.cancel_remove_confirmation();
            return;
        };
        let Some(request) = self.images.begin_remove() else {
            return;
        };
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.image_remove_task = Some(self.runtime.spawn(async move {
            let result = engine.remove_image(&request.image_id).await;
            persist_audit_record(
                &audit_store,
                audit::record_image_mutation_result(audit::ImageMutation::Remove, &result),
            )
            .await;
            if sender
                .send(EngineEvent::ImageRemoved {
                    generation: request.generation,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn finish_image_remove(&mut self, generation: u64, result: Result<(), EngineError>) -> bool {
        let succeeded = result.is_ok();
        self.images
            .finish_remove(generation, result.map_err(|error| error.to_string()));
        self.image_remove_task = None;
        succeeded && self.page == images::AppPage::Images
    }

    // Even a stale UI result can signal a successful mutation; refresh the
    // currently visible Images inventory without showing that stale result.
    fn finish_image_tag(&mut self, generation: u64, result: Result<(), EngineError>) -> bool {
        let succeeded = result.is_ok();
        self.images
            .finish_tag(generation, result.map_err(|error| error.to_string()));
        self.image_tag_task = None;
        succeeded && self.page == images::AppPage::Images
    }
    fn finish_image_push(&mut self, generation: u64, result: Result<(), EngineError>) -> bool {
        #[cfg(target_os = "linux")]
        let outcome_unknown = matches!(
            &result,
            Err(EngineError::ImagePushOutcomeUnknown { .. }
                | EngineError::ImagePushTimedOut { .. }
                | EngineError::ImagePushStreamInterrupted { .. })
        );
        #[cfg(not(target_os = "linux"))]
        let outcome_unknown = false;
        if outcome_unknown {
            if let Err(error) = result {
                // Do not unlock other image mutations when the daemon may still
                // be pushing and Ferrum did not receive a terminal response.
                self.images
                    .finish_push_uncertain(generation, error.to_string());
            }
            self.image_push_task = None;
            #[cfg(target_os = "linux")]
            {
                self.image_push_progress_reporting = None;
            }
            return false;
        }

        let succeeded = result.is_ok();
        let applied = self
            .images
            .finish_push(generation, result.map_err(|error| error.to_string()));
        self.image_push_task = None;
        #[cfg(target_os = "linux")]
        {
            self.image_push_progress_reporting = None;
        }
        applied && succeeded && self.page == images::AppPage::Images
    }

    fn inspect_image(&mut self, request: images::ImageInspectionRequest, context: egui::Context) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if let Some(task) = self.image_inspection_task.take() {
            task.abort();
        }
        let sender = self.sender.clone();
        self.image_inspection_task = Some(self.runtime.spawn(async move {
            let (result, history_result) = tokio::join!(
                engine.inspect_image(&request.id),
                engine.image_history(&request.id),
            );
            if sender
                .send(EngineEvent::ImageInspected {
                    generation: request.list_generation,
                    inspection_generation: request.inspection_generation,
                    id: request.id,
                    result,
                    history_result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn set_page(&mut self, page: images::AppPage, context: egui::Context) {
        if self.page == page {
            return;
        }
        let previous = self.page;
        self.page = page;
        if previous == images::AppPage::Images {
            self.invalidate_images();
        }
        if previous == images::AppPage::Volumes {
            self.invalidate_volumes();
        }
        if previous == images::AppPage::Networks {
            self.invalidate_networks();
        }
        if previous == images::AppPage::Compose {
            self.invalidate_compose();
        }
        if page == images::AppPage::Images && !self.loading && self.engine.is_some() {
            self.load_images(context.clone());
        }
        if page == images::AppPage::Volumes && !self.loading && self.engine.is_some() {
            self.load_volumes(context.clone());
        }
        if page == images::AppPage::Networks && !self.loading && self.engine.is_some() {
            self.load_networks(context.clone());
        }
        if page == images::AppPage::Compose
            && !self.loading
            && self.engine.is_some()
            && matches!(&self.connection, ConnectionState::Connected)
        {
            self.load_compose(context);
        }
    }

    fn connect(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();

        self.invalidate_images();
        self.invalidate_compose();
        self.loading = true;
        self.connection = ConnectionState::Connecting;
        self.refresh_error = None;
        self.context_error = None;
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = async {
                let engine = EngineClient::connect_default().await?;
                let info = engine.info().clone();
                let containers = engine.list_containers().await?;
                Ok::<_, EngineError>((engine, info, containers))
            }
            .await;
            match &result {
                Ok((_, _, containers)) => tracing::info!(
                    container_count = containers.len(),
                    "Loaded local container inventory"
                ),
                Err(_) => tracing::warn!(
                    error_kind = "connection",
                    "Could not load local container inventory"
                ),
            }

            if sender.send(EngineEvent::Connected(result)).await.is_ok() {
                context.request_repaint();
            }
        });
    }
    fn switch_context(&mut self, selected: Option<String>, context: egui::Context) {
        if self.loading
            || self.action_busy()
            || self.network_mutation_busy()
            || self.volume_mutation_busy()
            || self.image_build_busy()
        {
            return;
        }
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();
        self.invalidate_resource_reads();
        self.invalidate_compose();
        self.loading = true;
        self.context_error = None;
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = async {
                let engine = match selected.as_deref() {
                    Some(name) => EngineClient::connect_context(name).await?,
                    None => EngineClient::connect_default().await?,
                };
                let info = engine.info().clone();
                let containers = engine.list_containers().await?;
                Ok::<_, EngineError>((engine, info, containers))
            }
            .await;
            if sender
                .send(EngineEvent::ContextSwitched(result))
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        });
    }

    fn refresh(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }

        if self.page == images::AppPage::Images {
            self.load_images(context);
            return;
        }
        if self.page == images::AppPage::Volumes {
            if self.engine.is_some() {
                self.load_volumes(context);
            } else {
                self.connect(context);
            }
            return;
        }
        if self.page == images::AppPage::Networks {
            if self.engine.is_some() {
                self.load_networks(context);
            } else {
                self.connect(context);
            }
            return;
        }
        if self.page == images::AppPage::Compose {
            if self.engine.is_some() {
                self.load_compose(context);
            } else {
                self.connect(context);
            }
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.connect(context);
            return;
        };

        self.loading = true;
        self.refresh_error = None;
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = engine.list_containers().await;
            match &result {
                Ok(containers) => tracing::info!(
                    container_count = containers.len(),
                    "Refreshed local container inventory"
                ),
                Err(_) => tracing::warn!(
                    error_kind = "inventory",
                    "Could not refresh local container inventory"
                ),
            }
            if sender.send(EngineEvent::Refreshed(result)).await.is_ok() {
                context.request_repaint();
            }
        });
    }

    fn receive_events(&mut self, context: egui::Context) {
        let mut selected_container_changed = false;
        let mut action_error_after_reinspection = None;
        #[cfg(target_os = "linux")]
        if !self.log_paused {
            for _ in 0..256 {
                let Ok(event) = self.log_receiver.try_recv() else {
                    break;
                };
                match event {
                    LogEvent::Line { generation, line } if generation == self.log_generation => {
                        self.log_buffer.append(line);
                        self.log_notice = None;
                    }
                    LogEvent::Reset { generation } if generation == self.log_generation => {
                        self.log_buffer.reset();
                        self.log_notice = None;
                        self.log_reconnected = true;
                    }
                    LogEvent::Ended { generation, error } if generation == self.log_generation => {
                        self.log_task = None;
                        self.log_notice = Some(match error {
                            Some(error) => format!("Log stream ended: {error}"),
                            None if self.log_options.follow => "The log stream is complete.".into(),
                            None => "No more historical log lines.".into(),
                        });
                    }
                    _ => {}
                }
            }
        }
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                EngineEvent::Contexts(Ok(contexts)) => {
                    self.contexts = contexts;
                    self.contexts_error = None;
                }
                EngineEvent::Contexts(Err(error)) => {
                    self.contexts.clear();
                    self.contexts_error = Some(error.to_string());
                }
                EngineEvent::Connected(result) => {
                    self.loading = false;
                    match result {
                        Ok((engine, info, containers)) => {
                            self.apply_connection_success(engine, info, containers, context.clone())
                        }
                        Err(error) => {
                            #[cfg(target_os = "linux")]
                            self.cancel_terminal_session();
                            self.invalidate_images();
                            self.engine = None;
                            self.engine_info = None;
                            self.containers.clear();
                            self.invalidate_container_inventory();
                            self.inventory_view.inventory_changed(&self.containers);
                            self.selected_context = None;
                            self.connection = ConnectionState::Disconnected(error.to_string());
                        }
                    }
                }
                EngineEvent::ContextSwitched(result) => {
                    self.loading = false;
                    match result {
                        Ok((engine, info, containers)) => {
                            self.apply_connection_success(engine, info, containers, context.clone())
                        }
                        Err(error) => {
                            self.context_error = Some(error.to_string());
                            if self.page == images::AppPage::Images {
                                self.load_images(context.clone());
                            }
                        }
                    }
                }
                EngineEvent::VolumesListed { generation, result } => {
                    self.volumes
                        .finish_list(generation, result.map_err(|error| error.to_string()));
                    self.resource_read_task = None;
                    self.start_pending_resource_read(context.clone());
                }
                EngineEvent::ComposeListed { generation, result } => {
                    if self.compose.finish_load(generation, result) {
                        self.compose_task = None;
                    }
                }
                EngineEvent::ComposeFileSelectionFinished { generation, file } => {
                    if self
                        .compose
                        .finish_file_selection(generation, file.as_deref())
                    {
                        self.compose_validation_task = None;
                        if self.compose.is_validation_running() {
                            self.start_compose_validation(
                                self.compose.selected_files().to_vec(),
                                generation,
                                context.clone(),
                            );
                        }
                    }
                }
                EngineEvent::ComposeValidationFinished { generation, result } => {
                    if self.compose.finish_validation(generation, result) {
                        self.compose_validation_task = None;
                    }
                }
                EngineEvent::VolumeInspected {
                    generation,
                    inspection_generation,
                    name,
                    result,
                } => {
                    self.volumes.finish_inspection(
                        generation,
                        inspection_generation,
                        &name,
                        result,
                    );
                    self.resource_read_task = None;
                    self.start_pending_resource_read(context.clone());
                }
                EngineEvent::VolumeCreated {
                    generation,
                    name,
                    result,
                } => {
                    self.volumes.finish_create(
                        generation,
                        &name,
                        result.map(|_| ()).map_err(|error| error.to_string()),
                    );
                    self.volume_create_task = None;
                    self.volumes.mark_list_stale();
                    if self.page == images::AppPage::Volumes {
                        self.load_volumes(context.clone());
                    }
                }
                EngineEvent::VolumeRemoved {
                    generation,
                    inspection_generation,
                    removal_generation,
                    name,
                    result,
                } => {
                    self.volumes.finish_remove(
                        generation,
                        inspection_generation,
                        removal_generation,
                        &name,
                        result.map_err(|error| error.to_string()),
                    );
                    self.volume_remove_task = None;
                    self.volumes.mark_list_stale();
                    if self.page == images::AppPage::Volumes {
                        self.load_volumes(context.clone());
                    }
                }
                EngineEvent::NetworksListed {
                    generation,
                    container_generation,
                    network_result,
                    container_result,
                } => {
                    let request_is_current = generation == self.networks.generation
                        && container_generation == self.container_inventory_generation;
                    self.networks.finish_list(
                        generation,
                        network_result.map_err(|error| error.to_string()),
                    );
                    if request_is_current {
                        match container_result {
                            Ok(containers) => self.replace_container_inventory(containers),
                            Err(error) => {
                                self.invalidate_container_inventory();
                                self.refresh_error = Some(error.to_string());
                            }
                        }
                    }
                    self.resource_read_task = None;
                    self.start_pending_resource_read(context.clone());
                }
                EngineEvent::NetworkCreated {
                    generation,
                    name,
                    result,
                } => {
                    let succeeded = result.is_ok();
                    self.networks.finish_create(
                        generation,
                        &name,
                        result.map(|_| ()).map_err(|error| error.to_string()),
                    );
                    self.network_create_task = None;
                    if succeeded && self.page == images::AppPage::Networks {
                        self.networks.mark_list_stale();
                        self.load_networks(context.clone());
                    }
                }
                EngineEvent::NetworkRemoved {
                    generation,
                    inspection_generation,
                    removal_generation,
                    id,
                    name,
                    result,
                } => {
                    let succeeded = result.is_ok();
                    self.networks.finish_remove(
                        generation,
                        inspection_generation,
                        removal_generation,
                        &id,
                        &name,
                        result.map_err(|error| error.to_string()),
                    );
                    self.network_remove_task = None;
                    if succeeded && self.page == images::AppPage::Networks {
                        self.networks.mark_list_stale();
                        self.load_networks(context.clone());
                    }
                }
                EngineEvent::NetworkInspected {
                    generation,
                    inspection_generation,
                    id,
                    result,
                } => {
                    self.networks.finish_inspection(
                        generation,
                        inspection_generation,
                        &id,
                        result.map_err(|error| error.to_string()),
                    );
                    self.resource_read_task = None;
                    self.start_pending_resource_read(context.clone());
                }
                EngineEvent::NetworkAttachmentFinished {
                    operation,
                    container_generation,
                    mutation_result,
                    refreshed,
                } => {
                    let applied = self.networks.finish_attachment(
                        &operation,
                        mutation_result.map_err(|error| error.to_string()),
                    );
                    self.network_attachment_task = None;
                    if let Ok((networks, containers, details)) = refreshed {
                        if container_generation == self.container_inventory_generation {
                            self.replace_container_inventory(containers);
                        } else if !self.container_inventory_fresh
                            && self.page == images::AppPage::Networks
                        {
                            self.event_refresh_pending = true;
                        }
                        if applied {
                            self.networks.refresh_after_attachment(
                                networks,
                                details,
                                &operation.network_id,
                            );
                        }
                    } else {
                        self.invalidate_container_inventory();
                        if applied {
                            self.networks.mark_list_stale();
                        }
                        self.refresh_error = Some(
                            "Could not refresh container inventory after network change.".into(),
                        );
                    }
                }
                #[cfg(target_os = "linux")]
                EngineEvent::ImageBuildOutput { generation, line } => {
                    if self.images.update_build_output(generation, line) {
                        context.request_repaint();
                    }
                }
                #[cfg(target_os = "linux")]
                EngineEvent::ImageBuildFinished { generation, result } => {
                    self.images.finish_build(generation, result);
                    self.image_build_task = None;
                    self.image_build_cancel = None;
                    self.invalidate_images();
                    if self.engine.is_some() {
                        self.load_images(context.clone());
                    }
                }
                EngineEvent::ImagesListed { generation, result } => {
                    if self.images.complete_load(generation, result) {
                        self.image_list_task = None;
                    }
                }
                EngineEvent::ImagePullProgress {
                    generation,
                    progress,
                } => {
                    if self.images.update_pull(generation, progress) {
                        context.request_repaint();
                    }
                }
                EngineEvent::ImagePullFinished { generation, result } => {
                    let succeeded = result.is_ok();
                    let applied = self
                        .images
                        .finish_pull(generation, result.map_err(|error| error.to_string()));
                    self.image_pull_task = None;
                    #[cfg(target_os = "linux")]
                    {
                        self.image_pull_progress_reporting = None;
                    }
                    if applied && succeeded && self.page == images::AppPage::Images {
                        self.load_images(context.clone());
                    }
                }
                EngineEvent::ImagePushProgress {
                    generation,
                    progress,
                } => {
                    if self.images.update_push(generation, progress) {
                        context.request_repaint();
                    }
                }
                EngineEvent::ImagePushFinished { generation, result } => {
                    if self.finish_image_push(generation, result) {
                        self.load_images(context.clone());
                    }
                }
                EngineEvent::ImageTagged { generation, result } => {
                    if self.finish_image_tag(generation, result) {
                        self.load_images(context.clone());
                    }
                }
                EngineEvent::ImageRemoved { generation, result } => {
                    if self.finish_image_remove(generation, result) {
                        self.load_images(context.clone());
                    }
                }

                EngineEvent::ImageInspected {
                    generation,
                    inspection_generation,
                    id,
                    result,
                    history_result,
                } => {
                    let details_applied = self.images.complete_inspection(
                        generation,
                        inspection_generation,
                        &id,
                        result,
                    );
                    let history_applied = self.images.complete_history(
                        generation,
                        inspection_generation,
                        &id,
                        history_result,
                    );
                    if details_applied || history_applied {
                        self.image_inspection_task = None;
                    }
                }
                EngineEvent::Refreshed(result) => {
                    self.loading = false;
                    match result {
                        Ok(containers) => {
                            self.replace_container_inventory(containers);
                            selected_container_changed |= self
                                .inspection
                                .as_ref()
                                .is_some_and(|inspection| !inspection.action_busy);
                        }
                        Err(error) => {
                            self.invalidate_container_inventory();
                            self.refresh_error = Some(error.to_string());
                        }
                    }
                }
                EngineEvent::Inspected { generation, result } => {
                    if let Some(inspection) = &mut self.inspection
                        && inspection.generation == generation
                    {
                        self.inspection_task = None;
                        match result {
                            Ok(details) => {
                                inspection.restart_count_display =
                                    details.restart_count.map(|value| value.to_string());
                                inspection.exit_code_display =
                                    details.exit_code.map(|value| value.to_string());
                                inspection.details = Some(details);
                                inspection.loading = false;
                                inspection.error = false;
                            }
                            Err(_) => {
                                inspection.loading = false;
                                inspection.error = true;
                            }
                        }
                    }
                }
                EngineEvent::ActionFinished {
                    generation,
                    action,
                    result,
                } => {
                    self.action_task = None;
                    let matches_inspection = self
                        .inspection
                        .as_ref()
                        .is_some_and(|inspection| inspection.generation == generation);
                    let action_error = result.as_ref().err().map(ToString::to_string);
                    if let Some(inspection) = self.inspection.as_mut()
                        && inspection.generation == generation
                    {
                        inspection.action_busy = false;
                        inspection.confirming_remove = false;
                        inspection.action_error = action_error.clone();
                    }

                    let removed =
                        matches_inspection && result.is_ok() && action == ContainerAction::Remove;
                    if removed {
                        self.cancel_inspection();
                    } else if matches_inspection {
                        action_error_after_reinspection = action_error;
                        selected_container_changed = true;
                    }
                    self.invalidate_container_inventory();
                    self.event_refresh_pending = true;
                }
                EngineEvent::ContainerChanged {
                    generation,
                    container_id,
                } if generation == self.event_generation => {
                    self.event_refresh_pending = true;
                    self.invalidate_container_inventory();
                    selected_container_changed |=
                        self.inspection.as_ref().is_some_and(|inspection| {
                            !inspection.action_busy && inspection.container_id == container_id
                        });
                }
                EngineEvent::EventSnapshot { generation, result }
                    if generation == self.event_generation =>
                {
                    if self.loading {
                        self.event_refresh_pending = true;
                    }
                    match result {
                        Ok(containers) => {
                            self.replace_container_inventory(containers);
                            selected_container_changed |= self
                                .inspection
                                .as_ref()
                                .is_some_and(|inspection| !inspection.action_busy);
                        }
                        Err(error) => {
                            self.invalidate_container_inventory();
                            self.refresh_error = Some(error.to_string());
                        }
                    }
                }
                EngineEvent::EventStreamEnded { generation }
                    if generation == self.event_generation =>
                {
                    self.invalidate_container_inventory();
                    if self.page == images::AppPage::Networks {
                        self.event_refresh_pending = true;
                    }
                    self.event_warning = Some(
                        "Container event updates are unavailable; use Refresh to update inventory."
                            .into(),
                    );
                    self.event_task = None;
                }
                _ => {}
            }
        }
        if selected_container_changed
            && let Some((id, name)) = self.inspection.as_ref().map(|inspection| {
                (
                    inspection.container_id.clone(),
                    inspection.container_name.clone(),
                )
            })
        {
            self.inspect_container(&id, &name, context);
            if let Some(action_error) = action_error_after_reinspection.take()
                && let Some(inspection) = self.inspection.as_mut()
            {
                inspection.action_error = Some(action_error);
            }
        }
        #[cfg(target_os = "linux")]
        self.receive_terminal_opened();
    }

    #[cfg(target_os = "linux")]
    fn receive_terminal_opened(&mut self) {
        while let Ok(opened) = self.terminal_opened.try_recv() {
            match opened {
                TerminalOpened::Opened {
                    attempt_id,
                    io_task,
                    resize_task,
                    ..
                } if attempt_id != self.terminal_attempt => {
                    io_task.abort();
                    resize_task.abort();
                }
                TerminalOpened::Opened {
                    input,
                    output,
                    resize,
                    resize_failed,
                    io_task,
                    resize_task,
                    ..
                } => {
                    self.terminal_pending = false;
                    self.terminal_open_task = None;
                    self.terminal_notice = None;
                    self.terminal = Some(TerminalView {
                        model: TerminalModel::default(),
                        input: Some(input),
                        output,
                        resize_sender: Some(resize),
                        resize_failed,
                        io_task: Some(io_task),
                        resize_task: Some(resize_task),
                        ended: false,
                        paste_authorization: None,
                        input_notice: None,
                    });
                }
                TerminalOpened::Failed {
                    attempt_id,
                    message,
                } if attempt_id == self.terminal_attempt => {
                    self.terminal_pending = false;
                    self.terminal_open_task = None;
                    self.terminal_notice = Some(message);
                }
                TerminalOpened::Failed { .. } => {}
            }
        }
    }
    fn apply_connection_success(
        &mut self,
        engine: EngineClient,
        info: EngineInfo,
        containers: Vec<ContainerSummary>,
        context: egui::Context,
    ) {
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();
        self.cancel_inspection();
        self.invalidate_resource_reads();
        self.invalidate_compose();
        if let Some(task) = self.action_task.take() {
            task.abort();
        }
        self.selected_context = engine.selected_context_name().map(str::to_owned);
        self.engine = Some(engine.clone());
        self.engine_info = Some(info);
        self.replace_container_inventory(containers);
        self.connection = ConnectionState::Connected;
        self.refresh_error = None;
        self.context_error = None;
        self.start_event_stream(engine, context.clone());
        if self.page == images::AppPage::Images {
            self.load_images(context.clone());
        }
        if self.page == images::AppPage::Volumes {
            self.load_volumes(context.clone());
        }
        if self.page == images::AppPage::Networks {
            self.load_networks(context.clone());
        }
        if self.page == images::AppPage::Compose {
            self.load_compose(context);
        }
    }

    fn stop_event_stream(&mut self) {
        if let Some(task) = self.event_task.take() {
            task.abort();
        }
        self.event_generation = self.event_generation.wrapping_add(1);
        self.event_refresh_pending = false;
    }

    fn cancel_inspection(&mut self) {
        if let Some(task) = self.inspection_task.take() {
            task.abort();
        }
        self.cancel_log_stream();
        self.log_buffer.reset();
        self.log_action_notice = None;
        self.inspection = None;
    }

    fn inspect_container(
        &mut self,
        container_id: &str,
        container_name: &str,
        context: egui::Context,
    ) {
        self.cancel_inspection();
        self.next_inspection_generation = self.next_inspection_generation.wrapping_add(1);
        let generation = self.next_inspection_generation;
        self.inspection = Some(Inspection {
            generation,
            container_id: container_id.to_owned(),
            container_name: container_name.to_owned(),
            loading: true,
            details: None,
            restart_count_display: None,
            exit_code_display: None,
            error: false,
            action_error: None,
            action_busy: false,
            confirming_remove: false,
            logs_open: false,
        });
        if let Some(engine) = self.engine.clone() {
            let container_id = container_id.to_owned();
            let sender = self.sender.clone();
            self.inspection_task = Some(self.runtime.spawn(async move {
                let result = engine.inspect_container(&container_id).await;
                if sender
                    .send(EngineEvent::Inspected { generation, result })
                    .await
                    .is_ok()
                {
                    context.request_repaint();
                }
            }));
        }
    }
    #[cfg(target_os = "linux")]
    fn cancel_log_stream(&mut self) {
        if let Some(task) = self.log_task.take() {
            task.abort();
        }
        self.log_generation = self.log_generation.wrapping_add(1);
        self.log_paused = false;
        self.log_started = false;
        self.log_reconnected = false;
        self.log_action_notice = None;
    }

    #[cfg(target_os = "linux")]
    fn start_log_stream(
        &mut self,
        id: String,
        tty: bool,
        options: ContainerLogsOptions,
        context: egui::Context,
    ) {
        self.cancel_log_stream();
        let Some(engine) = self.engine.clone() else {
            self.log_started = true;
            self.log_notice = Some("The engine connection is unavailable.".into());
            context.request_repaint();
            return;
        };
        let generation = self.log_generation;
        let sender = self.log_sender.clone();
        let container_id = id;
        self.log_options = options;
        self.log_started = true;
        self.log_notice = Some("Loading container logs…".into());
        self.log_reconnected = false;
        let stream_context = context.clone();
        self.log_task = Some(self.runtime.spawn(async move {
            let mut opener = EngineLogOpener {
                engine,
                container_id,
                tty,
                options,
            };
            run_log_retry_loop(
                &mut opener,
                &sender,
                &stream_context,
                generation,
                options.follow,
            )
            .await;
        }));
        context.request_repaint();
    }

    fn run_action(&mut self, action: ContainerAction, context: egui::Context) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let Some(inspection) = self.inspection.as_mut() else {
            return;
        };
        if inspection.action_busy
            || inspection.loading
            || (action_requires_confirmation(action) && !inspection.confirming_remove)
        {
            return;
        }
        inspection.action_busy = true;
        inspection.confirming_remove = false;
        inspection.action_error = None;
        let id = inspection.container_id.clone();
        let generation = inspection.generation;
        let sender = self.sender.clone();
        let audit_store = self.audit_store.clone();
        self.action_task = Some(self.runtime.spawn(async move {
            let result = engine.perform_container_action(&id, action).await;
            persist_audit_record(
                &audit_store,
                audit::record_container_action_result(&id, action, result.is_ok()),
            )
            .await;
            if sender
                .send(EngineEvent::ActionFinished {
                    generation,
                    action,
                    result,
                })
                .await
                .is_ok()
            {
                context.request_repaint();
            }
        }));
    }

    fn start_event_stream(&mut self, engine: EngineClient, context: egui::Context) {
        #[cfg(target_os = "linux")]
        {
            self.stop_event_stream();
            self.event_generation = self.event_generation.wrapping_add(1);
            let generation = self.event_generation;
            self.event_warning = None;
            let sender = self.sender.clone();
            self.event_task = Some(self.runtime.spawn(async move {
                let mut retries = RetryBudget::default();
                let mut stream = loop {
                    match engine.subscribe_container_changes().await {
                        Ok(stream) => {
                            send_event_snapshot(&engine, &sender, &context, generation).await;
                            retries.reset();
                            break stream;
                        }
                        Err(_) => {
                            if retries.wait_next().await {
                                continue;
                            }
                            let _ = sender
                                .send(EngineEvent::EventStreamEnded { generation })
                                .await;
                            context.request_repaint();
                            return;
                        }
                    }
                };
                loop {
                    match stream.next_change().await {
                        Ok(Some(change)) => {
                            if sender
                                .send(EngineEvent::ContainerChanged {
                                    generation,
                                    container_id: change.container_id,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                            context.request_repaint();
                        }
                        Ok(None) | Err(_) => {
                            let mut reconnected = false;
                            while retries.wait_next().await {
                                if let Ok(new_stream) = engine.subscribe_container_changes().await {
                                    stream = new_stream;
                                    retries.reset();
                                    send_event_snapshot(&engine, &sender, &context, generation)
                                        .await;
                                    reconnected = true;
                                    break;
                                }
                            }
                            if !reconnected {
                                let _ = sender
                                    .send(EngineEvent::EventStreamEnded { generation })
                                    .await;
                                context.request_repaint();
                                return;
                            }
                        }
                    }
                }
            }));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (engine, context);
    }
    fn draw_sidebar(&self, ui: &mut egui::Ui) -> Option<images::AppPage> {
        let mut page_requested = None;
        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(218.0)
            .show(ui, |ui| {
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Ferrum").size(22.0).strong());
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Container workbench").small().weak());
                ui.add_space(24.0);
                page_requested = images::show_navigation(ui, self.page);
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    match &self.connection {
                        ConnectionState::Connecting => {
                            ui.label("Connecting to local engine");
                        }
                        ConnectionState::Connected => {
                            ui.label("Engine connection active");
                        }
                        ConnectionState::Disconnected(_) => {
                            ui.label("Engine connection unavailable");
                        }
                    }
                    ui.add_space(10.0);
                });
            });
        page_requested
    }

    fn draw_volumes(&mut self, ui: &mut egui::Ui) -> Option<volumes::VolumesAction> {
        if let Some(error) = &self.context_error {
            ui.colored_label(Color32::from_rgb(224, 174, 109), "Context switch failed");
            ui.label("The previous engine connection remains active.");
            ui.label(error);
            ui.add_space(10.0);
        }
        if self.loading {
            ui.spinner();
            ui.heading("Waiting for the engine context");
            return self
                .volumes
                .show(ui, false, "Context selection is in progress.");
        }
        if self.engine.is_some()
            && cfg!(target_os = "linux")
            && !self.volumes.loading
            && !self.volumes.loaded
            && !self.volume_mutation_busy()
        {
            self.load_volumes(ui.ctx().clone());
        }
        let supported = cfg!(target_os = "linux")
            && self.engine.is_some()
            && matches!(&self.connection, ConnectionState::Connected);
        let message = match &self.connection {
            ConnectionState::Connecting => "Connecting to the selected local engine.",
            ConnectionState::Disconnected(message) => message.as_str(),
            ConnectionState::Connected if self.engine.is_none() => {
                "The engine connection is unavailable."
            }
            ConnectionState::Connected if !cfg!(target_os = "linux") => {
                "Volume access is implemented only on Linux for a local Docker Unix socket."
            }
            ConnectionState::Connected => "",
        };
        self.volumes.show(ui, supported, message)
    }
    fn draw_networks(&mut self, ui: &mut egui::Ui) -> Option<networks::NetworksAction> {
        let network_mutation_in_flight = self.network_mutation_busy();
        if let Some(error) = &self.context_error {
            ui.colored_label(Color32::from_rgb(224, 174, 109), "Context switch failed");
            ui.label("The previous engine connection remains active.");
            ui.label(error);
            ui.add_space(10.0);
        }
        if self.loading {
            ui.spinner();
            ui.heading("Waiting for the engine context");
            return self.networks.show(
                ui,
                false,
                "Context selection is in progress.",
                network_mutation_in_flight,
                self.container_inventory_fresh,
                &self.containers,
            );
        }
        if self.engine.is_some()
            && cfg!(target_os = "linux")
            && !network_mutation_in_flight
            && !self.networks.loading
            && (!self.networks.loaded
                || (!self.container_inventory_fresh && self.refresh_error.is_none()))
        {
            self.load_networks(ui.ctx().clone());
        }
        let supported = cfg!(target_os = "linux")
            && self.engine.is_some()
            && matches!(&self.connection, ConnectionState::Connected);
        let message = match &self.connection {
            ConnectionState::Connecting => "Connecting to the selected local engine.",
            ConnectionState::Disconnected(message) => message.as_str(),
            ConnectionState::Connected if self.engine.is_none() => {
                "The engine connection is unavailable."
            }
            ConnectionState::Connected if !cfg!(target_os = "linux") => {
                "Network access is implemented only on Linux for a local Docker Unix socket."
            }
            ConnectionState::Connected => "",
        };
        self.networks.show(
            ui,
            supported,
            message,
            network_mutation_in_flight,
            self.container_inventory_fresh,
            &self.containers,
        )
    }
    fn draw_compose(&mut self, ui: &mut egui::Ui) -> compose::ComposeActions {
        if self.loading {
            ui.spinner();
            ui.heading("Waiting for the engine context");
            return self.compose.show(
                ui,
                false,
                "Compose projects will load when the engine context change finishes.",
                false,
                cfg!(target_os = "linux"),
            );
        }
        let engine_selected = self.engine.is_some();
        let connected = matches!(&self.connection, ConnectionState::Connected);
        if cfg!(target_os = "linux")
            && engine_selected
            && connected
            && !self.compose.is_loading()
            && !self.compose.has_loaded()
        {
            self.load_compose(ui.ctx().clone());
        }
        let available = cfg!(target_os = "linux") && engine_selected;
        let message = match (&self.connection, available) {
            (_, false) if !cfg!(target_os = "linux") => {
                "Compose project discovery is implemented only on Linux with a local Docker Unix socket."
            }
            (ConnectionState::Connecting, _) => "Connecting to the selected local Docker Engine.",
            (ConnectionState::Disconnected(_), true) => {
                "The selected local Docker Engine is unavailable; the displayed project snapshot may be stale."
            }
            (ConnectionState::Disconnected(_), false) | (ConnectionState::Connected, false) => {
                "The selected local Docker Engine is unavailable."
            }
            (ConnectionState::Connected, true) => "",
        };
        self.compose.show(
            ui,
            available,
            message,
            cfg!(target_os = "linux") && !self.loading,
            cfg!(target_os = "linux"),
        )
    }
    fn draw_images(&mut self, ui: &mut egui::Ui) -> Option<images::ImageInspectionRequest> {
        if let Some(error) = &self.context_error {
            ui.colored_label(Color32::from_rgb(224, 174, 109), "Context switch failed");
            ui.label("The previous engine connection remains active.");
            ui.label(error);
            ui.add_space(10.0);
        }
        if self.loading && !self.images.is_loading() {
            ui.spinner();
            ui.heading("Waiting for the engine context");
            ui.label("Images will load when the connection change finishes.");
            return None;
        }
        if !self.loading
            && matches!(&self.connection, ConnectionState::Connected)
            && self.engine.is_some()
            && !self.images.is_loading()
            && !self.images.has_loaded()
        {
            self.load_images(ui.ctx().clone());
        }
        let (connected, message) = match &self.connection {
            ConnectionState::Connecting => (
                false,
                "Connecting to a local engine; images will appear when it responds.",
            ),
            ConnectionState::Disconnected(message) => (false, message.as_str()),
            ConnectionState::Connected if self.engine.is_none() => {
                (false, "The engine connection is unavailable.")
            }
            ConnectionState::Connected => (true, ""),
        };
        let mutation_busy = self.image_mutation_busy();
        let build_context_label = build_engine_source_label(
            self.selected_context.as_deref(),
            self.engine_info.as_ref().map(|info| info.endpoint_label),
        );
        let operations_allowed = cfg!(target_os = "linux")
            && connected
            && self
                .engine
                .as_ref()
                .is_some_and(|engine| !engine.local_socket_path().as_os_str().is_empty());
        let inspection = self.images.show(
            ui,
            connected,
            message,
            operations_allowed,
            mutation_busy,
            build_context_label,
        );

        #[cfg(target_os = "linux")]
        {
            if self.images.take_build_requested() {
                self.start_image_build(ui.ctx().clone());
            }
            if self.images.take_build_cancel_requested() {
                self.cancel_image_build();
            }
        }

        if self.images.take_pull_requested() {
            #[cfg(target_os = "linux")]
            self.start_image_pull(ui.ctx().clone());
        }
        if self.images.take_hide_progress_requested() {
            self.hide_image_pull_progress();
        }
        if self.images.take_push_requested() {
            #[cfg(target_os = "linux")]
            self.start_image_push(ui.ctx().clone());
        }
        if self.images.take_hide_push_progress_requested() {
            #[cfg(target_os = "linux")]
            self.hide_image_push_progress();
        }
        if self.images.take_tag_requested() {
            #[cfg(target_os = "linux")]
            self.start_image_tag(ui.ctx().clone());
        }
        if self.images.take_remove_requested() {
            #[cfg(target_os = "linux")]
            self.start_image_remove(ui.ctx().clone());
        }

        inspection
    }

    fn draw_header(
        &mut self,
        ui: &mut egui::Ui,
        refresh_requested: &mut bool,
        settings_requested: &mut bool,
        context_requested: &mut Option<Option<String>>,
    ) {
        let image_build_busy = self.image_build_busy();
        let page_loading = self.loading
            || (self.page == images::AppPage::Images
                && (self.images.is_loading()
                    || self.images.is_pulling()
                    || self.images.is_pushing()
                    || self.images.is_tagging()
                    || self.images.is_removing()
                    || self.image_tag_task.is_some()
                    || self.image_remove_task.is_some()))
            || (self.page == images::AppPage::Volumes && self.volumes.is_loading())
            || (self.page == images::AppPage::Compose && self.compose.is_loading());
        let network_mutation_in_flight = self.network_mutation_busy();
        let volume_mutation_in_flight = self.volume_mutation_busy();

        egui::Panel::top("header")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 14)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(self.page.title());
                    ui.add_space(12.0);
                    match &self.connection {
                        ConnectionState::Connecting => {
                            ui.colored_label(Color32::from_rgb(224, 174, 109), "Connecting");
                        }
                        ConnectionState::Connected => {
                            ui.colored_label(Color32::from_rgb(130, 194, 145), "Connected");
                        }
                        ConnectionState::Disconnected(_) => {
                            ui.colored_label(Color32::from_rgb(227, 113, 102), "Disconnected");
                        }
                    }
                    ui.add_enabled_ui(
                        !page_loading
                            && !self.action_busy()
                            && !network_mutation_in_flight
                            && !volume_mutation_in_flight
                            && !image_build_busy,
                        |ui| {
                            let selected_name = match self.selected_context.as_deref() {
                                Some(name) => self
                                    .contexts
                                    .iter()
                                    .find(|context| context.name() == name)
                                    .map(DockerContext::display_name)
                                    .unwrap_or("Selected Docker context"),
                                None => "Automatic local discovery",
                            };
                            egui::ComboBox::from_label("Docker context")
                                .selected_text(selected_name)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(
                                            self.selected_context.is_none(),
                                            "Automatic local discovery",
                                        )
                                        .clicked()
                                    {
                                        *context_requested = Some(None);
                                    }
                                    for context in &self.contexts {
                                        let label = match context.transport() {
                                            ContextTransport::LocalUnix => {
                                                context.display_name().to_owned()
                                            }
                                            ContextTransport::Unsupported => {
                                                format!(
                                                    "{} (unsupported transport)",
                                                    context.display_name()
                                                )
                                            }
                                            ContextTransport::Invalid => {
                                                format!(
                                                    "{} (invalid endpoint)",
                                                    context.display_name()
                                                )
                                            }
                                        };
                                        let response = ui.add_enabled(
                                            context.transport() == ContextTransport::LocalUnix,
                                            egui::Button::new(label).selected(
                                                self.selected_context.as_deref()
                                                    == Some(context.name()),
                                            ),
                                        );
                                        if response.clicked() {
                                            *context_requested =
                                                Some(Some(context.name().to_owned()));
                                        }
                                    }
                                });
                        },
                    );
                    if network_mutation_in_flight {
                        ui.label(
                            "A network mutation is in progress; context switching is disabled.",
                        );
                    }
                    if volume_mutation_in_flight {
                        ui.label(
                            "A volume mutation is in progress; context switching is disabled.",
                        );
                    }
                    if image_build_busy {
                        ui.label(
                            "An image build is awaiting confirmation or running; context switching is disabled.",
                        );
                    }
                    if let Some(info) = &self.engine_info {
                        ui.separator();
                        ui.label("Docker-compatible API");
                        ui.label(RichText::new(info.endpoint_label).small().weak());
                        ui.label(RichText::new(&info.version).strong());
                        ui.label(RichText::new("API").weak());
                        ui.label(&info.api_version);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Settings").clicked() {
                            *settings_requested = true;
                        }
                        if self.page != images::AppPage::Compose {
                            let button_text = if page_loading {
                                "Loading"
                            } else if self.engine.is_some() {
                                "Refresh"
                            } else {
                                "Retry connection"
                            };
                            if ui
                                .add_enabled(
                                    !page_loading && !self.action_busy(),
                                    egui::Button::new(button_text),
                                )
                                .clicked()
                            {
                                *refresh_requested = true;
                            }
                        }
                    });
                });
            });
    }

    fn draw_inventory(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        if let Some(error) = &self.contexts_error {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(224, 174, 109),
                    "Context selection unavailable",
                );
                ui.label("Automatic local socket discovery remains available.");
            });
            ui.label(error);
            ui.add_space(10.0);
        }
        if let Some(error) = &self.context_error {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(227, 113, 102), "Context switch failed");
                ui.label("The previous engine connection and inventory are unchanged.");
            });
            ui.label(error);
            ui.add_space(10.0);
        }

        if let Some(error) = &self.refresh_error {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(227, 113, 102), "Refresh failed");
                ui.label("Showing the last successful inventory.");
            });
            ui.label(error);
            ui.add_space(10.0);
        }

        if let Some(warning) = &self.event_warning {
            ui.colored_label(Color32::from_rgb(224, 174, 109), warning);
            ui.add_space(10.0);
        }

        if matches!(&self.connection, ConnectionState::Connected) && !self.containers.is_empty() {
            self.inventory_view.show_controls(ui, &self.containers);
        }
        match &self.connection {
            ConnectionState::Connecting => {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.spinner();
                    ui.add_space(12.0);
                    ui.heading("Connecting to a local container engine");
                    ui.label("The container inventory will appear when the engine responds.");
                });
                None
            }
            ConnectionState::Disconnected(message) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(92.0);
                    ui.colored_label(Color32::from_rgb(227, 113, 102), "Connection unavailable");
                    ui.add_space(8.0);
                    ui.label(message);
                });
                None
            }
            ConnectionState::Connected if self.containers.is_empty() => {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.heading("No containers found");
                    ui.label("The connected engine reported an empty inventory, including stopped containers.");
                });
                None
            }
            ConnectionState::Connected if self.inventory_view.visible().is_empty() => {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.heading("No matching containers");
                    ui.label("Try a different search.");
                });
                None
            }
            ConnectionState::Connected => self.draw_container_table(ui),
        }
    }

    fn draw_container_table(&self, ui: &mut egui::Ui) -> Option<usize> {
        let available_width = ui.available_width();
        let name_width = available_width * 0.24;
        let image_width = available_width * 0.34;
        let state_width = available_width * 0.15;
        let status_width = available_width - name_width - image_width - state_width;
        ui.horizontal(|ui| {
            add_cell(ui, "Name", name_width);
            add_cell(ui, "Image", image_width);
            add_cell(ui, "State", state_width);
            add_cell(ui, "Status", status_width);
        });
        ui.separator();
        let visible = self.inventory_view.visible();
        let mut selected = None;
        egui::ScrollArea::vertical().show_rows(
            ui,
            ROW_HEIGHT,
            visible.len(),
            |ui, visible_rows| {
                for row in visible_rows {
                    let index = visible[row];
                    let container = &self.containers[index];
                    ui.horizontal(|ui| {
                        let display_name = display::escape_untrusted_display_text(&container.name);
                        let name_button = ui
                            .add_enabled_ui(!self.loading && !self.action_busy(), |ui| {
                                ui.add_sized(
                                    [name_width, CELL_HEIGHT],
                                    egui::Button::new(
                                        RichText::new(display_name.as_ref()).strong(),
                                    )
                                    .frame(false),
                                )
                            })
                            .inner;
                        if name_button.clicked() {
                            selected = Some(index);
                        }
                        let display_id = display::escape_untrusted_display_text(&container.id);
                        name_button.on_hover_text(display_id.as_ref());
                        let image = container.image.as_deref().unwrap_or("Not reported");
                        let display_image = display::escape_untrusted_display_text(image);
                        let image_response = ui.add_sized(
                            [image_width, CELL_HEIGHT],
                            egui::Label::new(display_image.as_ref())
                                .truncate()
                                .show_tooltip_when_elided(true),
                        );
                        image_response.on_hover_text(display_image.as_ref());
                        let state_text = container
                            .state
                            .as_ref()
                            .map_or("Not reported", container_state_display);
                        let display_state = display::escape_untrusted_display_text(state_text);
                        ui.add_sized(
                            [state_width, CELL_HEIGHT],
                            egui::Label::new(
                                RichText::new(display_state.as_ref())
                                    .color(state_color(container.state.as_ref())),
                            )
                            .truncate()
                            .show_tooltip_when_elided(true),
                        );
                        let status = container.status.as_deref().unwrap_or("Not reported");
                        let display_status = display::escape_untrusted_display_text(status);
                        ui.add_sized(
                            [status_width, CELL_HEIGHT],
                            egui::Label::new(display_status.as_ref())
                                .truncate()
                                .show_tooltip_when_elided(true),
                        );
                    });
                    ui.add_space(2.0);
                }
            },
        );
        selected
    }
    fn draw_inspection(&mut self, context: egui::Context) {
        let Some(inspection) = &self.inspection else {
            return;
        };
        let action_busy = inspection.action_busy;
        let mut open = true;
        let mut close = false;
        let mut requested_action = None;
        let mut confirm_remove = false;
        let mut cancel_remove = false;
        #[cfg(target_os = "linux")]
        let mut open_logs = false;
        #[cfg(target_os = "linux")]
        let mut open_terminal = false;
        #[cfg(target_os = "linux")]
        let mut cancel_terminal_open = false;
        egui::Window::new("Container details")
            .id(egui::Id::new("container-inspection"))
            .open(&mut open)
            .resizable(true)
            .show(&context, |ui| {
                let container_name =
                    display::escape_untrusted_display_text(&inspection.container_name);
                ui.heading(container_name.as_ref());
                untrusted_detail_row(ui, "ID", Some(&inspection.container_id));
                #[cfg(target_os = "linux")]
                if ui
                    .add_enabled(
                        !inspection.loading && !inspection.error && inspection.details.is_some(),
                        egui::Button::new("Container logs"),
                    )
                    .clicked()
                {
                    open_logs = true;
                }
                #[cfg(target_os = "linux")]
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Shell")
                        .selected_text(if self.terminal_shell == "/bin/bash" {
                            "/bin/bash"
                        } else {
                            "/bin/sh"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.terminal_shell,
                                "/bin/sh".into(),
                                "/bin/sh",
                            );
                            ui.selectable_value(
                                &mut self.terminal_shell,
                                "/bin/bash".into(),
                                "/bin/bash",
                            );
                        });
                    let running = inspection
                        .details
                        .as_ref()
                        .and_then(|details| details.state.as_ref())
                        .is_some_and(|state| *state == ContainerState::Running);
                    if ui
                        .add_enabled(
                            running
                                && self.engine.is_some()
                                && !self.terminal_pending
                                && self.terminal.is_none(),
                            egui::Button::new("Open terminal"),
                        )
                        .clicked()
                    {
                        open_terminal = true;
                    }
                    if self.terminal_pending && ui.button("Cancel terminal opening").clicked() {
                        cancel_terminal_open = true;
                    }
                });
                if let Some(notice) = self.terminal_notice.as_deref() {
                    ui.colored_label(Color32::from_rgb(227, 113, 102), notice);
                }
                let state = inspection
                    .details
                    .as_ref()
                    .and_then(|details| details.state.as_ref());
                let actions = available_container_actions(state);
                if !actions.is_empty() {
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        for action in actions {
                            if *action == ContainerAction::Remove && inspection.confirming_remove {
                                continue;
                            }
                            if ui
                                .add_enabled(
                                    !inspection.action_busy && !inspection.loading,
                                    egui::Button::new(action.label()),
                                )
                                .clicked()
                            {
                                if action_requires_confirmation(*action) {
                                    confirm_remove = true;
                                } else {
                                    requested_action = Some(*action);
                                }
                            }
                        }
                    });
                    if inspection.confirming_remove {
                        ui.colored_label(
                            Color32::from_rgb(224, 174, 109),
                            "Remove this container? Its volumes will not be removed.",
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !inspection.action_busy,
                                    egui::Button::new("Confirm remove"),
                                )
                                .clicked()
                            {
                                requested_action = Some(ContainerAction::Remove);
                            }
                            if ui
                                .add_enabled(!inspection.action_busy, egui::Button::new("Cancel"))
                                .clicked()
                            {
                                cancel_remove = true;
                            }
                        });
                    }
                }
                if inspection.action_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Performing container action…");
                    });
                }
                if let Some(error) = &inspection.action_error {
                    ui.colored_label(Color32::from_rgb(227, 113, 102), "Container action failed");
                    ui.label(error);
                }
                if inspection.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading inspection…");
                    });
                } else if inspection.error {
                    ui.colored_label(Color32::from_rgb(227, 113, 102), "Inspection unavailable");
                    ui.label("The engine could not provide container details.");
                }
                if let Some(details) = &inspection.details {
                    untrusted_detail_row(ui, "Name", details.name.as_deref());
                    untrusted_detail_row(ui, "Image", details.image.as_deref());
                    untrusted_detail_row(ui, "Created", details.created.as_deref());
                    untrusted_detail_row(
                        ui,
                        "State",
                        details.state.as_ref().map(container_state_display),
                    );
                    untrusted_detail_row(
                        ui,
                        "Restart count",
                        inspection.restart_count_display.as_deref(),
                    );
                    untrusted_detail_row(ui, "Exit code", inspection.exit_code_display.as_deref());
                    untrusted_detail_row(ui, "Started at", details.started_at.as_deref());
                    untrusted_detail_row(ui, "Finished at", details.finished_at.as_deref());
                    ui.separator();
                    ui.strong("Mounts");
                    match &details.mounts {
                        None => {
                            ui.label("Not reported");
                        }
                        Some(mounts) if mounts.is_empty() => {
                            ui.label("No mounts reported");
                        }
                        Some(mounts) => {
                            for mount in mounts {
                                ui.group(|ui| {
                                    ui.strong("Mount");
                                    untrusted_detail_row(ui, "Type", mount.kind.as_deref());
                                    untrusted_detail_row(ui, "Source", mount.source.as_deref());
                                    untrusted_detail_row(
                                        ui,
                                        "Destination",
                                        mount.destination.as_deref(),
                                    );
                                    untrusted_detail_row(ui, "Mode", mount.mode.as_deref());
                                    untrusted_detail_row(
                                        ui,
                                        "Read/write",
                                        mount
                                            .read_write
                                            .map(|value| if value { "Yes" } else { "No" }),
                                    );
                                });
                            }
                        }
                    }
                }
                if ui
                    .add_enabled(!inspection.action_busy, egui::Button::new("Close"))
                    .clicked()
                {
                    close = true;
                }
            });
        if confirm_remove && let Some(inspection) = self.inspection.as_mut() {
            inspection.confirming_remove = true;
        }
        if cancel_remove && let Some(inspection) = self.inspection.as_mut() {
            inspection.confirming_remove = false;
        }
        if (!open || close) && !action_busy {
            self.cancel_inspection();
        } else if let Some(action) = requested_action {
            self.run_action(action, context.clone());
        }
        #[cfg(target_os = "linux")]
        if cancel_terminal_open || ((!open || close) && self.terminal_pending) {
            self.cancel_terminal_session();
        }
        #[cfg(target_os = "linux")]
        if open_logs && let Some(inspection) = self.inspection.as_mut() {
            inspection.logs_open = true;
        }
        #[cfg(target_os = "linux")]
        if open_terminal && let Some(inspection) = &self.inspection {
            self.start_terminal(
                inspection.container_id.clone(),
                self.terminal_shell.clone(),
                context.clone(),
            );
        }
        #[cfg(target_os = "linux")]
        self.draw_logs(context.clone());
    }
    #[cfg(target_os = "linux")]
    fn draw_logs(&mut self, context: egui::Context) {
        let Some(inspection) = self.inspection.as_ref() else {
            return;
        };
        if !inspection.logs_open {
            return;
        }
        let starting_stream = !self.log_started;
        let tty = inspection.details.as_ref().and_then(|details| details.tty);
        let running = inspection
            .details
            .as_ref()
            .and_then(|details| details.state.as_ref())
            .is_some_and(|state| *state == ContainerState::Running);
        if starting_stream {
            self.log_options.follow = running;
        }
        if tty == Some(true) {
            self.log_stdout = true;
            self.log_stderr = true;
        }
        let mut open = true;
        let mut refresh = false;
        let mut options_changed = false;
        let mut copy_logs = false;
        let mut export_logs = false;
        egui::Window::new("Container logs")
            .id(egui::Id::new("container-logs"))
            .open(&mut open)
            .resizable(true)
            .default_size([800.0, 520.0])
            .show(&context, |ui| {
                if tty.is_none() {
                    ui.colored_label(
                        Color32::from_rgb(224, 174, 109),
                        "Logs are unavailable because the container TTY mode was not reported.",
                    );
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Search");
                    ui.text_edit_singleline(&mut self.log_search);
                    if tty == Some(true) {
                        ui.add_enabled(false, egui::Checkbox::new(&mut self.log_stdout, "Stdout"));
                        ui.add_enabled(false, egui::Checkbox::new(&mut self.log_stderr, "Stderr"));
                        ui.label("TTY logs use combined output.");
                    } else {
                        let stdout_changed = ui.checkbox(&mut self.log_stdout, "Stdout").changed();
                        let stderr_changed = ui.checkbox(&mut self.log_stderr, "Stderr").changed();
                        options_changed |= stdout_changed || stderr_changed;
                        if !self.log_stdout && !self.log_stderr {
                            if stderr_changed {
                                self.log_stderr = true;
                            } else {
                                self.log_stdout = true;
                            }
                        }
                    }
                    options_changed |= ui
                        .checkbox(&mut self.log_options.timestamps, "Timestamps")
                        .changed();
                    let mut tail = self.log_options.tail;
                    egui::ComboBox::from_id_salt("container-logs-tail")
                        .selected_text(match tail {
                            100 => "Last 100 lines",
                            500 => "Last 500 lines",
                            1_000 => "Last 1000 lines",
                            5_000 => "Last 5000 lines",
                            _ => "Custom tail",
                        })
                        .show_ui(ui, |ui| {
                            for (choice, label) in [
                                (100, "Last 100 lines"),
                                (500, "Last 500 lines"),
                                (1_000, "Last 1000 lines"),
                                (5_000, "Last 5000 lines"),
                            ] {
                                ui.selectable_value(&mut tail, choice, label);
                            }
                        });
                    if tail != self.log_options.tail {
                        self.log_options.tail = tail;
                        options_changed = true;
                    }
                    options_changed |= ui
                        .checkbox(&mut self.log_options.follow, "Follow")
                        .changed();
                    if ui.button("Refresh").clicked() {
                        refresh = true;
                    }
                });
                self.log_buffer.set_filters(
                    &self.log_search,
                    tty == Some(true) || self.log_stdout,
                    tty == Some(true) || self.log_stderr,
                );
                let has_visible_content = self.log_buffer.visible_count() > 0;
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            self.log_started && self.log_task.is_some(),
                            egui::Button::new(if self.log_paused { "Resume" } else { "Pause" }),
                        )
                        .clicked()
                    {
                        self.log_paused = !self.log_paused;
                    }
                    if self.log_paused {
                        ui.colored_label(Color32::from_rgb(224, 174, 109), "Paused");
                    }
                    ui.checkbox(&mut self.log_auto_scroll, "Auto-scroll");
                    copy_logs = ui
                        .add_enabled(has_visible_content, egui::Button::new("Copy"))
                        .clicked();
                    export_logs = ui
                        .add_enabled(has_visible_content, egui::Button::new("Export"))
                        .clicked();
                });
                if let Some(notice) = &self.log_notice {
                    if notice.starts_with("Log stream ended:") {
                        ui.colored_label(Color32::from_rgb(224, 174, 109), notice);
                    } else {
                        ui.label(notice);
                    }
                }
                if let Some(notice) = &self.log_action_notice {
                    ui.label(notice);
                }
                if self.log_reconnected {
                    ui.colored_label(
                        Color32::from_rgb(130, 194, 145),
                        "Reconnected; displaying recent container output.",
                    );
                }
                let dropped_lines = self.log_buffer.dropped_lines();
                if dropped_lines > 0 {
                    let color = Color32::from_rgb(224, 174, 109);
                    let font_id = egui::TextStyle::Body.resolve(ui.style());
                    let wrap_width = ui.available_width();
                    let needs_rebuild = self.log_dropped_notice.as_ref().is_none_or(|notice| {
                        notice.dropped_lines != dropped_lines
                            || notice.font_id != font_id
                            || notice.color != color
                            || notice.wrap_width != wrap_width
                    });
                    if needs_rebuild {
                        let text = format!(
                            "{} older log lines dropped from the in-memory buffer.",
                            dropped_lines
                        );
                        let galley = ui
                            .painter()
                            .layout(text, font_id.clone(), color, wrap_width);
                        self.log_dropped_notice = Some(CachedDroppedLogNotice {
                            dropped_lines,
                            font_id,
                            color,
                            wrap_width,
                            galley,
                        });
                    }
                    if let Some(notice) = &self.log_dropped_notice {
                        ui.add(egui::Label::new(std::sync::Arc::clone(&notice.galley)));
                    }
                }
                let visible_count = self.log_buffer.visible_count();
                if visible_count == 0 {
                    if self.log_started && self.log_notice.is_none() {
                        ui.label("No log lines match the current filters.");
                    } else if !self.log_started {
                        ui.label("Load logs to view recent container output.");
                    }
                } else {
                    egui::ScrollArea::both()
                        .stick_to_bottom(self.log_auto_scroll)
                        .show_rows(ui, 20.0, visible_count, |ui, rows| {
                            for row in rows {
                                if let Some(entry) = self.log_buffer.visible_entry_at(row) {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(logs::source_label(&entry.source))
                                                .monospace(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&entry.text).monospace(),
                                            )
                                            .extend(),
                                        );
                                    });
                                }
                            }
                        });
                }
            });
        if copy_logs {
            self.log_action_notice = None;
            context.copy_text(self.log_buffer.visible_text());
        }
        if export_logs {
            self.log_action_notice = None;
            let text = self.log_buffer.visible_text();
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("container-logs.txt")
                .add_filter("Text files", &["txt"])
                .save_file()
            {
                let result = logs::export_visible_logs(&path, &text);
                self.log_action_notice = Some(match result {
                    Ok(()) => "Visible logs exported.".into(),
                    Err(_) => "Could not export visible logs.".into(),
                });
            }
        }
        if !open {
            if let Some(inspection) = self.inspection.as_mut() {
                inspection.logs_open = false;
            }
            self.cancel_log_stream();
            self.log_buffer.reset();
            self.log_reconnected = false;
            return;
        }
        if self.log_buffer.entries().is_empty() && self.log_notice.is_none() && !self.log_started {
            self.log_notice = Some("Load logs to view recent container output.".into());
        }
        if tty.is_none() {
            if self.log_started || self.log_task.is_some() {
                self.cancel_log_stream();
            }
            self.log_notice = Some(
                "Logs are unavailable because the container TTY mode was not reported.".into(),
            );
            return;
        }
        if tty == Some(true) {
            self.log_options.stdout = true;
            self.log_options.stderr = true;
        } else {
            self.log_options.stdout = self.log_stdout;
            self.log_options.stderr = self.log_stderr;
        }
        if (starting_stream || options_changed || refresh)
            && let Some(id) = self
                .inspection
                .as_ref()
                .map(|inspection| inspection.container_id.clone())
        {
            if !starting_stream {
                self.log_buffer.reset();
            }
            self.start_log_stream(id, tty.unwrap_or(false), self.log_options, context);
        }
    }
    fn draw_settings(&mut self, context: egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        let mut close_requested = false;
        egui::Window::new("Settings")
            .open(&mut open)
            .show(&context, |ui| {
                ui.checkbox(
                    &mut self.settings_draft.auto_connect_on_startup,
                    "Connect automatically on startup",
                );
                ui.label("This setting applies the next time Ferrum starts.");
                if let Some(error) = self.settings_error {
                    ui.colored_label(Color32::from_rgb(227, 113, 102), error.message());
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        match self.settings_store.save(&self.settings_draft) {
                            Ok(()) => {
                                self.settings = self.settings_draft.clone();
                                self.settings_error = None;
                                close_requested = true;
                            }
                            Err(error) => self.settings_error = Some(error),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.settings_draft = self.settings.clone();
                        self.settings_error = None;
                        close_requested = true;
                    }
                });
            });
        self.settings_open = open && !close_requested;
    }
}
fn detail_row(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.label(value.unwrap_or("Not reported"));
    });
}

fn untrusted_detail_row(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    let value = value.map(display::escape_untrusted_display_text);
    detail_row(ui, label, value.as_deref());
}

impl Drop for FerrumApp {
    fn drop(&mut self) {
        self.stop_event_stream();
        #[cfg(target_os = "linux")]
        self.cancel_log_stream();
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();
        #[cfg(target_os = "linux")]
        self.cancel_image_build();
        self.cancel_image_read_tasks();
        self.invalidate_compose();
        self.pending_resource_read = None;
        if let Some(task) = self.resource_read_task.take() {
            task.abort();
        }
        if let Some(task) = self.inspection_task.take() {
            task.abort();
        }
        if let Some(task) = self.action_task.take() {
            task.abort();
        }
    }
}

impl eframe::App for FerrumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.receive_events(context.clone());
        #[cfg(target_os = "linux")]
        let terminal_closed = self
            .terminal
            .as_mut()
            .is_some_and(|terminal| !terminal.show(&context));
        #[cfg(target_os = "linux")]
        if terminal_closed {
            self.cancel_terminal_session();
        }
        if self.event_refresh_pending && !self.loading {
            if self.page == images::AppPage::Networks && self.network_mutation_busy() {
                // The mutation's refresh may predate this event; keep the event queued.
            } else {
                self.event_refresh_pending = false;
                self.refresh(context.clone());
            }
        }
        let page_requested = self.draw_sidebar(ui);
        if let Some(page) = page_requested {
            self.set_page(page, context.clone());
        }
        let mut refresh_requested = false;
        let mut settings_requested = false;

        let mut context_requested = None;
        self.draw_header(
            ui,
            &mut refresh_requested,
            &mut settings_requested,
            &mut context_requested,
        );
        if let Some(selected) = context_requested
            && selected != self.selected_context
        {
            self.switch_context(selected, context.clone());
        }
        if settings_requested {
            self.settings_draft = self.settings.clone();
            self.settings_open = true;
        }
        let mut inspect_index = None;
        let mut image_inspection = None;
        let mut volume_action = None;
        let mut network_action = None;
        let mut compose_actions = compose::ComposeActions::default();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 18)))
            .show(ui, |ui| match self.page {
                images::AppPage::Containers => inspect_index = self.draw_inventory(ui),
                images::AppPage::Images => image_inspection = self.draw_images(ui),
                images::AppPage::Volumes => volume_action = self.draw_volumes(ui),
                images::AppPage::Networks => network_action = self.draw_networks(ui),
                images::AppPage::Compose => compose_actions = self.draw_compose(ui),
            });
        if compose_actions.cancel_validation {
            self.cancel_compose_validation(context.clone());
        }
        if let Some(index) = compose_actions.remove_override {
            self.compose.remove_override(index);
        }
        if compose_actions.select_base_file {
            self.select_compose_file(false, context.clone());
        } else if compose_actions.add_override_file {
            self.select_compose_file(true, context.clone());
        } else if compose_actions.validate_files
            && let Some((generation, files)) = self.compose.begin_validation()
        {
            self.start_compose_validation(files, generation, context.clone());
        }
        if compose_actions.refresh {
            self.refresh(context.clone());
        }
        if let Some(index) = inspect_index {
            let container = &self.containers[index];
            let id = container.id.clone();
            let name = container.name.clone();
            self.inspect_container(&id, &name, context.clone());
        }
        if let Some(request) = image_inspection {
            self.inspect_image(request, context.clone());
        }
        if let Some(action) = volume_action {
            match action {
                volumes::VolumesAction::Inspect(name) => {
                    self.inspect_volume(name, context.clone());
                }
                volumes::VolumesAction::Create { generation, name } => {
                    self.start_volume_create(generation, name, context.clone());
                }
                volumes::VolumesAction::Remove {
                    generation,
                    inspection_generation,
                    removal_generation,
                    name,
                } => self.start_volume_remove(
                    generation,
                    inspection_generation,
                    removal_generation,
                    name,
                    context.clone(),
                ),
            }
        }
        if let Some(action) = network_action {
            match action {
                networks::NetworksAction::Inspect(id) => {
                    self.inspect_network(id, context.clone());
                }
                networks::NetworksAction::Create { generation, name } => {
                    self.start_network_create(generation, name, context.clone());
                }
                networks::NetworksAction::Remove {
                    generation,
                    inspection_generation,
                    removal_generation,
                    id,
                    name,
                } => self.start_network_remove(
                    generation,
                    inspection_generation,
                    removal_generation,
                    id,
                    name,
                    context.clone(),
                ),
                networks::NetworksAction::Attach(operation) => {
                    self.start_network_attachment(operation, context.clone());
                }
            }
        }
        self.draw_inspection(context.clone());
        self.draw_settings(context.clone());

        if refresh_requested {
            self.refresh(context.clone());
        }

        if self.loading
            || self.action_busy()
            || (self.page == images::AppPage::Images
                && (self.images.is_loading()
                    || self.images.is_pulling()
                    || self.images.is_tagging()
                    || self.images.is_removing()
                    || self.image_remove_task.is_some()))
            || (self.page == images::AppPage::Volumes && self.volumes.is_loading())
            || (self.page == images::AppPage::Networks
                && (self.networks.is_loading() || self.network_mutation_busy()))
            || (self.page == images::AppPage::Compose && self.compose.is_loading())
        {
            context.request_repaint_after(Duration::from_millis(250));
        }
    }
}
fn add_cell(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.add_sized(
        [width, CELL_HEIGHT],
        egui::Label::new(RichText::new(text).small().strong().weak()).truncate(),
    );
}

fn state_color(state: Option<&ContainerState>) -> Color32 {
    match state {
        Some(ContainerState::Running) => Color32::from_rgb(130, 194, 145),
        Some(ContainerState::Restarting | ContainerState::Paused | ContainerState::Stopping) => {
            Color32::from_rgb(224, 174, 109)
        }
        Some(ContainerState::Dead) => Color32::from_rgb(227, 113, 102),
        Some(_) | None => Color32::from_rgb(164, 177, 188),
    }
}

fn container_state_display(state: &ContainerState) -> &str {
    match state {
        ContainerState::Unknown(value) if value.is_empty() => r#""""#,
        _ => state.label(),
    }
}

#[cfg(test)]
mod container_state_display_tests {
    use super::*;

    #[test]
    fn displays_unknown_state_text_exactly() {
        assert_eq!(
            container_state_display(&ContainerState::Unknown(String::new())),
            r#""""#
        );
        assert_eq!(
            container_state_display(&ContainerState::Unknown("checkpointing".to_owned())),
            "checkpointing"
        );
    }
}

#[cfg(test)]
mod lifecycle_action_tests {
    use super::*;
    use ferrum_core::ContainerAction;

    #[test]
    fn offers_only_state_appropriate_actions() {
        let cases = [
            (
                Some(ContainerState::Created),
                &[ContainerAction::Start, ContainerAction::Remove][..],
            ),
            (
                Some(ContainerState::Running),
                &[
                    ContainerAction::Stop,
                    ContainerAction::Restart,
                    ContainerAction::Pause,
                ][..],
            ),
            (
                Some(ContainerState::Paused),
                &[ContainerAction::Unpause][..],
            ),
            (
                Some(ContainerState::Exited),
                &[
                    ContainerAction::Start,
                    ContainerAction::Restart,
                    ContainerAction::Remove,
                ][..],
            ),
            (Some(ContainerState::Dead), &[ContainerAction::Remove][..]),
        ];

        for (state, expected) in cases {
            assert_eq!(
                available_container_actions(state.as_ref()),
                expected,
                "unexpected actions for {state:?}"
            );
        }
    }

    #[test]
    fn hides_actions_for_transitional_unknown_and_missing_states() {
        let states = [
            Some(ContainerState::Restarting),
            Some(ContainerState::Removing),
            Some(ContainerState::Stopping),
            Some(ContainerState::Unknown("checkpointing".to_owned())),
            None,
        ];

        for state in states {
            assert!(
                available_container_actions(state.as_ref()).is_empty(),
                "unexpected actions for {state:?}"
            );
        }
    }

    #[test]
    fn only_remove_requires_confirmation() {
        for action in [
            ContainerAction::Start,
            ContainerAction::Stop,
            ContainerAction::Restart,
            ContainerAction::Pause,
            ContainerAction::Unpause,
        ] {
            assert!(
                !action_requires_confirmation(action),
                "{action:?} should not require confirmation"
            );
        }
        assert!(action_requires_confirmation(ContainerAction::Remove));
    }
}

#[cfg(test)]
mod log_buffer_tests {
    use super::*;
    use ferrum_core::{ContainerLogLine, ContainerLogSource};

    fn line(source: ContainerLogSource, text: &str) -> ContainerLogLine {
        ContainerLogLine {
            source,
            text: text.to_owned(),
        }
    }

    #[test]
    fn evicts_oldest_lines_when_line_limit_is_exceeded() {
        let mut buffer = logs::LogBuffer::new();
        for index in 0..10_001 {
            buffer.append(line(ContainerLogSource::Stdout, &index.to_string()));
        }

        assert_eq!(buffer.entries().len(), 10_000);
        assert_eq!(buffer.dropped_lines(), 1);
        assert_eq!(buffer.entries()[0].text, "1");
        assert_eq!(buffer.entries()[9_999].text, "10000");
    }

    #[test]
    fn evicts_oldest_lines_to_stay_within_retained_byte_limit() {
        let mut buffer = logs::LogBuffer::new();
        let text = "x".repeat(64 * 1024);
        for _ in 0..129 {
            buffer.append(line(ContainerLogSource::Stdout, &text));
        }

        assert_eq!(buffer.retained_bytes(), 8 * 1024 * 1024);
        assert_eq!(buffer.entries().len(), 128);
        assert_eq!(buffer.dropped_lines(), 1);
        assert!(buffer.entries().iter().all(|entry| entry.text == text));
    }

    #[test]
    fn searches_case_insensitively_with_unicode_and_returns_matching_indices() {
        let mut buffer = logs::LogBuffer::new();
        buffer.append(line(ContainerLogSource::Stdout, "Σervice ready"));
        buffer.append(line(ContainerLogSource::Stderr, "connection refused"));
        buffer.append(line(ContainerLogSource::Stdout, "SERVICE ready"));

        buffer.set_filters("σERVICE", true, true);

        assert_eq!(buffer.visible_indices(), vec![0]);
        buffer.set_filters("service", true, true);
        assert_eq!(buffer.visible_indices(), vec![2]);
    }

    #[test]
    fn filters_stdout_stderr_and_combined_lines_by_stream_selection() {
        let mut buffer = logs::LogBuffer::new();
        buffer.append(line(ContainerLogSource::Stdout, "out"));
        buffer.append(line(ContainerLogSource::Stderr, "err"));
        buffer.append(line(ContainerLogSource::Combined, "tty"));

        buffer.set_filters("", true, false);
        assert_eq!(buffer.visible_indices(), vec![0, 2]);

        buffer.set_filters("", false, true);
        assert_eq!(buffer.visible_indices(), vec![1, 2]);

        buffer.set_filters("", true, true);
        assert_eq!(buffer.visible_indices(), vec![0, 1, 2]);
    }
    #[test]
    fn visible_text_joins_filtered_rows_labels_sources_and_preserves_blank_lines() {
        let mut buffer = logs::LogBuffer::new();
        buffer.append(line(ContainerLogSource::Stdout, "match first"));
        buffer.append(line(ContainerLogSource::Stderr, "hidden stream"));
        buffer.append(line(ContainerLogSource::Combined, "combined output"));
        buffer.append(line(ContainerLogSource::Stdout, ""));
        buffer.append(line(ContainerLogSource::Stdout, "match second"));

        buffer.set_filters("", true, false);

        assert_eq!(
            buffer.visible_text(),
            "stdout: match first\ncombined: combined output\nstdout: \nstdout: match second"
        );
        buffer.set_filters("second", true, true);
        assert_eq!(buffer.visible_text(), "stdout: match second");
        buffer.set_filters("", true, false);
        assert_eq!(
            buffer.visible_text(),
            "stdout: match first\ncombined: combined output\nstdout: \nstdout: match second"
        );
        buffer.set_filters("", false, true);
        assert_eq!(
            buffer.visible_text(),
            "stderr: hidden stream\ncombined: combined output"
        );
    }

    #[test]
    fn visible_text_escapes_terminal_controls_and_preserves_rows() {
        let mut buffer = logs::LogBuffer::new();
        buffer.append(line(
            ContainerLogSource::Stdout,
            "before\u{1b}]52;c;payload\u{7}after",
        ));
        buffer.append(line(ContainerLogSource::Stderr, "c1\u{85}end"));

        assert_eq!(
            buffer.visible_text(),
            "stdout: before\\x1b]52;c;payload\\x07after\nstderr: c1\\x85end"
        );
    }

    #[test]
    fn log_rows_escape_bidi_and_invisible_formatting_before_display_and_copy() {
        let mut buffer = logs::LogBuffer::new();
        buffer.append(line(
            ContainerLogSource::Stdout,
            "invoice 123\u{202e}txt\u{200b}.pdf\u{e0001}\u{e0020}en\u{e007e}\u{e007f}",
        ));

        assert_eq!(
            buffer.visible_entry_at(0).expect("log row").text,
            "invoice 123\\u{202e}txt\\u{200b}.pdf\\u{e0001}\\u{e0020}en\\u{e007e}\\u{e007f}"
        );
        assert_eq!(
            buffer.visible_text(),
            "stdout: invoice 123\\u{202e}txt\\u{200b}.pdf\\u{e0001}\\u{e0020}en\\u{e007e}\\u{e007f}"
        );
    }

    #[test]
    fn escaped_log_expansion_respects_the_retention_budget() {
        let mut buffer = logs::LogBuffer::new();
        let spoofing_text = "\u{202e}".repeat(1_048_577);

        buffer.append(line(ContainerLogSource::Stdout, &spoofing_text));

        assert!(buffer.entries().is_empty());
        assert_eq!(buffer.retained_bytes(), 0);
        assert_eq!(buffer.dropped_lines(), 1);
    }

    #[test]
    fn reset_clears_entries_and_drop_count_but_preserves_filters() {
        let mut buffer = logs::LogBuffer::new();
        for index in 0..10_001 {
            buffer.append(line(ContainerLogSource::Stdout, &index.to_string()));
        }
        buffer.set_filters("9999", true, false);
        assert!(!buffer.visible_indices().is_empty());

        buffer.reset();

        assert!(buffer.entries().is_empty());
        assert_eq!(buffer.retained_bytes(), 0);
        assert_eq!(buffer.dropped_lines(), 0);
        assert!(buffer.visible_indices().is_empty());

        buffer.append(line(ContainerLogSource::Stdout, "reconnected 9999"));
        assert_eq!(buffer.visible_indices(), vec![0]);
    }

    #[test]
    #[ignore = "manual release-mode log-buffer timing sample; no timing threshold"]
    fn measures_bounded_log_buffer_append_throughput() {
        use std::time::Instant;

        const LINE_COUNT: usize = 100_000;
        const MEASURED_RUNS: usize = 7;
        let text = "x".repeat(512);
        let mut durations = Vec::with_capacity(MEASURED_RUNS);
        let mut retained = None;

        for run in 0..=MEASURED_RUNS {
            let mut buffer = logs::LogBuffer::new();
            let started = Instant::now();
            for _ in 0..LINE_COUNT {
                buffer.append(line(ContainerLogSource::Stdout, &text));
            }
            let elapsed = started.elapsed();
            assert_eq!(buffer.entries().len(), 10_000);
            assert_eq!(buffer.dropped_lines(), 90_000);
            assert!(buffer.retained_bytes() <= 8 * 1024 * 1024);
            if run > 0 {
                durations.push(elapsed);
            }
            retained = Some(buffer.retained_bytes());
        }

        durations.sort_unstable();
        let median = durations[MEASURED_RUNS / 2];
        let maximum = durations[MEASURED_RUNS - 1];
        let median_lines_per_second = LINE_COUNT as f64 / median.as_secs_f64();
        eprintln!(
            "log-buffer append: {MEASURED_RUNS} runs of {LINE_COUNT} synthetic 512-byte lines; retained={} bytes, median={}us max={}us throughput={median_lines_per_second:.0} lines/s",
            retained.expect("at least one measurement"),
            median.as_micros(),
            maximum.as_micros()
        );
    }
}
#[cfg(all(test, target_os = "linux"))]
mod log_retry_budget_tests {
    use super::*;

    #[test]
    fn retries_are_bounded_for_one_view_and_refresh_starts_a_fresh_budget() {
        let mut retry = RetryBudget::default();

        assert_eq!(
            retry.next_delay(),
            Some(std::time::Duration::from_millis(250))
        );
        assert_eq!(
            retry.next_delay(),
            Some(std::time::Duration::from_millis(500))
        );
        assert_eq!(
            retry.next_delay(),
            Some(std::time::Duration::from_millis(1_000))
        );
        assert_eq!(retry.next_delay(), None);

        // The view's retry budget is fixed; manual refresh starts a new task.
        let mut refreshed = RetryBudget::default();
        assert_eq!(
            refreshed.next_delay(),
            Some(std::time::Duration::from_millis(250))
        );
    }
}

#[cfg(test)]
mod event_reconciliation_tests {
    use super::*;
    #[test]
    fn build_engine_source_label_distinguishes_configured_socket_from_discovery() {
        assert_eq!(
            build_engine_source_label(None, Some("configured Unix socket")),
            "Configured local Docker socket"
        );
        assert_eq!(
            build_engine_source_label(None, Some("system Docker socket")),
            "system Docker socket"
        );
        assert_eq!(
            build_engine_source_label(Some("development"), Some("selected Docker context")),
            "development"
        );
        assert_eq!(
            build_engine_source_label(None, None),
            "Automatic local discovery"
        );
    }
    #[cfg(target_os = "linux")]
    use egui_kittest::kittest::NodeT;
    #[cfg(target_os = "linux")]
    use ferrum_core::ContainerLogSource;

    fn app_with_open_inspection() -> (FerrumApp, mpsc::Sender<EngineEvent>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        #[cfg(target_os = "linux")]
        let (terminal_open_sender, terminal_opened) = mpsc::channel(1);
        #[cfg(target_os = "linux")]
        let (log_sender, log_receiver) = mpsc::channel(LOG_CHANNEL_CAPACITY);
        let settings = settings::Settings::default();
        let app = FerrumApp {
            runtime,
            sender: sender.clone(),
            receiver,
            #[cfg(target_os = "linux")]
            terminal_open_sender,
            #[cfg(target_os = "linux")]
            terminal_opened,
            #[cfg(target_os = "linux")]
            terminal: None,
            #[cfg(target_os = "linux")]
            terminal_attempt: 0,
            #[cfg(target_os = "linux")]
            terminal_pending: false,
            #[cfg(target_os = "linux")]
            terminal_open_task: None,
            #[cfg(target_os = "linux")]
            terminal_shell: "/bin/sh".into(),
            #[cfg(target_os = "linux")]
            terminal_notice: None,
            #[cfg(target_os = "linux")]
            log_sender,
            #[cfg(target_os = "linux")]
            log_receiver,
            connection: ConnectionState::Connected,
            engine: None,
            engine_info: None,
            containers: vec![ContainerSummary {
                id: "container-id".to_owned(),
                name: "example".to_owned(),
                image: Some("example:old".to_owned()),
                state: Some(ContainerState::Running),
                status: Some("Up".to_owned()),
            }],
            container_inventory_fresh: true,
            container_inventory_generation: 0,
            inventory_view: InventoryView::default(),
            page: images::AppPage::default(),
            images: images::ImagesView::default(),
            volumes: volumes::VolumesView::default(),
            networks: networks::NetworksView::default(),
            compose: compose::ComposeView::default(),
            volume_create_task: None,
            volume_remove_task: None,
            resource_read_task: None,
            compose_task: None,
            compose_validation_task: None,
            pending_resource_read: None,
            image_list_task: None,
            image_inspection_task: None,
            image_pull_task: None,
            #[cfg(target_os = "linux")]
            image_pull_progress_reporting: None,
            image_push_task: None,
            #[cfg(target_os = "linux")]
            image_push_progress_reporting: None,
            image_tag_task: None,
            network_create_task: None,
            network_remove_task: None,
            network_attachment_task: None,
            image_remove_task: None,
            #[cfg(target_os = "linux")]
            image_build_task: None,
            #[cfg(target_os = "linux")]
            image_build_cancel: None,
            loading: false,
            refresh_error: None,
            contexts: Vec::new(),
            selected_context: None,
            context_error: None,
            contexts_error: None,
            inspection: Some(Inspection {
                generation: 4,
                container_id: "container-id".to_owned(),
                container_name: "example".to_owned(),
                loading: false,
                details: Some(ContainerDetails {
                    id: "container-id".to_owned(),
                    name: Some("example".to_owned()),
                    image: Some("example:old".to_owned()),
                    created: None,
                    state: Some(ContainerState::Running),
                    restart_count: None,
                    exit_code: None,
                    started_at: None,
                    finished_at: None,
                    tty: None,
                    mounts: None,
                }),
                restart_count_display: None,
                exit_code_display: None,
                error: false,
                action_error: None,
                action_busy: false,
                confirming_remove: false,
                logs_open: false,
            }),
            action_task: None,
            inspection_task: None,
            next_inspection_generation: 4,
            event_generation: 7,
            event_task: None,
            event_warning: None,
            event_refresh_pending: false,
            #[cfg(target_os = "linux")]
            log_task: None,
            #[cfg(target_os = "linux")]
            log_generation: 0,
            #[cfg(target_os = "linux")]
            log_buffer: LogBuffer::new(),
            #[cfg(target_os = "linux")]
            log_dropped_notice: None,
            #[cfg(target_os = "linux")]
            log_options: ContainerLogsOptions {
                stdout: true,
                stderr: true,
                timestamps: true,
                follow: false,
                tail: 500,
            },
            #[cfg(target_os = "linux")]
            log_notice: None,
            #[cfg(target_os = "linux")]
            log_action_notice: None,
            #[cfg(target_os = "linux")]
            log_paused: false,
            #[cfg(target_os = "linux")]
            log_auto_scroll: true,
            #[cfg(target_os = "linux")]
            log_reconnected: false,
            #[cfg(target_os = "linux")]
            log_started: false,
            #[cfg(target_os = "linux")]
            log_search: String::new(),
            #[cfg(target_os = "linux")]
            log_stdout: true,
            #[cfg(target_os = "linux")]
            log_stderr: true,
            settings_store: Arc::new(settings::SettingsStore::new()),
            audit_store: audit::AuditStore::disabled(),
            settings_draft: settings.clone(),
            settings,
            settings_open: false,
            settings_error: None,
        };
        (app, sender)
    }
    #[cfg(target_os = "linux")]
    fn app_with_zero_reference_volume() -> (FerrumApp, mpsc::Sender<EngineEvent>) {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Volumes;
        let generation = app.volumes.begin_list();
        app.volumes.finish_list(
            generation,
            Ok(vec![ferrum_core::VolumeSummary {
                name: "owned-data".to_owned(),
                driver: Some("local".to_owned()),
                scope: Some("local".to_owned()),
            }]),
        );
        let (generation, inspection_generation, name) =
            app.volumes.begin_inspection("owned-data".to_owned());
        app.volumes.finish_inspection(
            generation,
            inspection_generation,
            &name,
            volumes::VolumeInspectionResult::Inspected {
                details: ferrum_core::VolumeDetails {
                    name: name.clone(),
                    driver: Some("local".to_owned()),
                    scope: Some("local".to_owned()),
                    created_at: None,
                    mountpoint: None,
                },
                usage_result: Ok(Some(ferrum_core::VolumeUsage {
                    size_bytes: Some(0),
                    reference_count: Some(0),
                })),
            },
        );
        (app, sender)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn successful_network_creation_refreshes_the_current_inventory_in_headless_app() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Networks;
        let previous_generation = app.networks.begin_list();
        app.networks
            .finish_list(previous_generation, Ok(Vec::new()));
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut harness = Harness::new_ui_state(
            move |ui, app: &mut FerrumApp| {
                let context = ui.ctx().clone();
                app.receive_events(context);
                if let Some(action) = app.networks.show(
                    ui,
                    true,
                    "",
                    false,
                    app.container_inventory_fresh,
                    &app.containers,
                ) {
                    let _ = action_sender.send(action);
                }
            },
            app,
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Network name")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Network name")
            .type_text("work-net");
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Create network")
            .click();
        harness.step();
        let (generation, name) = match action_receiver
            .try_recv()
            .expect("the Networks UI should request creation")
        {
            networks::NetworksAction::Create { generation, name } => (generation, name),
            networks::NetworksAction::Inspect(_)
            | networks::NetworksAction::Remove { .. }
            | networks::NetworksAction::Attach(_) => {
                panic!("the create button should return a create action")
            }
        };

        harness
            .state()
            .runtime
            .block_on(sender.send(EngineEvent::NetworkCreated {
                generation,
                name,
                result: Ok("created-network-id".to_owned()),
            }))
            .expect("create result should be queued");
        harness.step();
        assert!(harness.state().networks.generation > previous_generation);
        assert!(harness.state().networks.loading);
        assert!(!harness.state().networks.loaded);
        assert!(harness.state().network_create_task.is_none());
        let _success = harness.get_by_role_and_label(Role::Label, "Network 'work-net' created.");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn successful_volume_creation_refreshes_inventory_after_page_invalidation() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Volumes;
        let previous_generation = app.volumes.begin_list();
        app.volumes.finish_list(previous_generation, Ok(Vec::new()));
        app.volume_create_task = Some(app.runtime.spawn(std::future::pending()));
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut harness = Harness::new_ui_state(
            move |ui, app: &mut FerrumApp| {
                let context = ui.ctx().clone();
                app.receive_events(context);
                if let Some(action) = app.volumes.show(ui, true, "") {
                    let _ = action_sender.send(action);
                }
            },
            app,
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Volume name")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Volume name")
            .type_text("app-data");
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Create volume")
            .click();
        harness.step();
        let (generation, name) = match action_receiver
            .try_recv()
            .expect("the Volumes UI should request creation")
        {
            volumes::VolumesAction::Create { generation, name } => (generation, name),
            volumes::VolumesAction::Inspect(_) | volumes::VolumesAction::Remove { .. } => {
                panic!("the create button should return a create action")
            }
        };

        harness.state_mut().volumes.invalidate();
        harness
            .state()
            .runtime
            .block_on(sender.send(EngineEvent::VolumeCreated {
                generation,
                name: name.clone(),
                result: Ok(VolumeDetails {
                    name: name.clone(),
                    driver: None,
                    scope: None,
                    created_at: None,
                    mountpoint: None,
                }),
            }))
            .expect("create result should be queued");
        harness.step();

        assert!(harness.state().volumes.generation > previous_generation);
        assert!(harness.state().volumes.loading);
        assert!(!harness.state().volumes.loaded);
        assert!(harness.state().volume_create_task.is_none());
        let _success = harness.get_by_role_and_label(Role::Label, "Volume 'app-data' created.");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn volume_removal_results_refresh_current_inventory_in_headless_app() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        for result in [
            Ok(()),
            Err(EngineError::VolumeRemovalTimedOut {
                endpoint_label: "selected local Docker socket",
            }),
        ] {
            let succeeded = result.is_ok();
            let (app, sender) = app_with_zero_reference_volume();
            let previous_generation = app.volumes.generation;
            let (action_sender, action_receiver) = std::sync::mpsc::channel();
            let mut harness = Harness::new_ui_state(
                move |ui, app: &mut FerrumApp| {
                    let context = ui.ctx().clone();
                    app.receive_events(context);
                    if let Some(action) = app.volumes.show(ui, true, "")
                        && matches!(&action, volumes::VolumesAction::Remove { .. })
                    {
                        action_sender
                            .send(action)
                            .expect("action receiver should stay open");
                    }
                },
                app,
            );
            harness.run();
            harness
                .get_by_role_and_label(Role::Button, "Remove volume")
                .click_accesskit();
            harness.run();
            harness
                .get_by_role_and_label(Role::Button, "Confirm volume removal")
                .click_accesskit();
            harness.step();
            let (generation, inspection_generation, removal_generation, name) =
                match action_receiver
                    .try_recv()
                    .expect("confirmed removal should return an action")
                {
                    volumes::VolumesAction::Remove {
                        generation,
                        inspection_generation,
                        removal_generation,
                        name,
                    } => (generation, inspection_generation, removal_generation, name),
                    volumes::VolumesAction::Inspect(_) | volumes::VolumesAction::Create { .. } => {
                        panic!("confirmation should return a remove action")
                    }
                };
            let pending_task = harness.state().runtime.spawn(std::future::pending());
            harness.state_mut().volume_remove_task = Some(pending_task);
            harness
                .state()
                .runtime
                .block_on(sender.send(EngineEvent::VolumeRemoved {
                    generation,
                    inspection_generation,
                    removal_generation,
                    name,
                    result,
                }))
                .expect("removal result should be queued");
            harness.step();

            assert!(harness.state().volumes.generation > previous_generation);
            assert!(harness.state().volumes.loading);
            assert!(!harness.state().volumes.loaded);
            assert!(harness.state().volume_remove_task.is_none());
            if succeeded {
                let _success =
                    harness.get_by_role_and_label(Role::Label, "Volume 'owned-data' was removed.");
            } else {
                let _error = harness.get_by_role_and_label(
                    Role::Label,
                    "Volume removal failed; the engine state may have changed.",
                );
            }
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pending_volume_removal_disables_context_switching_accessibly() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.volume_remove_task = Some(app.runtime.spawn(std::future::pending()));

        app.switch_context(Some("other-context".to_owned()), egui::Context::default());
        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
        assert!(app.volume_remove_task.is_some());

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let mut refresh_requested = false;
                let mut settings_requested = false;
                let mut context_requested = None;
                app.draw_header(
                    ui,
                    &mut refresh_requested,
                    &mut settings_requested,
                    &mut context_requested,
                );
            },
            app,
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::ComboBox, "Docker context")
                .accesskit_node()
                .is_disabled()
        );
        let _status = harness.get_by_role_and_label(
            Role::Label,
            "A volume mutation is in progress; context switching is disabled.",
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pending_volume_creation_disables_context_switching_accessibly() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.volume_create_task = Some(app.runtime.spawn(std::future::pending()));

        app.switch_context(Some("other-context".to_owned()), egui::Context::default());
        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
        assert!(app.volume_create_task.is_some());

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let mut refresh_requested = false;
                let mut settings_requested = false;
                let mut context_requested = None;
                app.draw_header(
                    ui,
                    &mut refresh_requested,
                    &mut settings_requested,
                    &mut context_requested,
                );
            },
            app,
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::ComboBox, "Docker context")
                .accesskit_node()
                .is_disabled()
        );
        let _status = harness.get_by_role_and_label(
            Role::Label,
            "A volume mutation is in progress; context switching is disabled.",
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn successful_network_removal_refreshes_the_current_inventory_in_headless_app() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Networks;
        let id = "a".repeat(64);
        let list_generation = app.networks.begin_list();
        app.networks.finish_list(
            list_generation,
            Ok(vec![NetworkSummary {
                id: id.clone(),
                name: "custom-net".to_owned(),
                driver: Some("bridge".to_owned()),
                scope: Some("local".to_owned()),
                created_at: None,
                internal: Some(false),
                attachable: Some(false),
                enable_ipv4: Some(true),
                enable_ipv6: Some(false),
            }]),
        );
        let (generation, inspection_generation, selected_id) =
            app.networks.begin_inspection(id.clone());
        app.networks.finish_inspection(
            generation,
            inspection_generation,
            &selected_id,
            Ok(ferrum_core::NetworkDetails {
                id: id.clone(),
                name: "custom-net".to_owned(),
                driver: Some("bridge".to_owned()),
                scope: Some("local".to_owned()),
                created_at: None,
                internal: Some(false),
                attachable: Some(false),
                enable_ipv4: Some(true),
                enable_ipv6: Some(false),
                attached_container_count: Some(0),
                attached_container_ids: None,
                ipam_usage: ferrum_core::NetworkIpamUsage::default(),
            }),
        );
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut harness = Harness::new_ui_state(
            move |ui, app: &mut FerrumApp| {
                let context = ui.ctx().clone();
                app.receive_events(context);
                if let Some(action) = app.networks.show(
                    ui,
                    true,
                    "",
                    false,
                    app.container_inventory_fresh,
                    &app.containers,
                ) {
                    action_sender
                        .send(action)
                        .expect("action receiver should stay open");
                }
            },
            app,
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Remove network")
            .click_accesskit();
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Confirm network removal")
            .click_accesskit();
        harness.step();
        let networks::NetworksAction::Remove {
            generation,
            inspection_generation,
            removal_generation,
            id: removed_id,
            name,
        } = action_receiver
            .try_recv()
            .expect("explicit confirmation should submit removal")
        else {
            panic!("confirmation should submit network removal");
        };
        assert_eq!(removed_id, id);
        let remove_task = harness.state().runtime.spawn(std::future::pending());
        harness.state_mut().network_remove_task = Some(remove_task);
        harness
            .state()
            .runtime
            .block_on(sender.send(EngineEvent::NetworkRemoved {
                generation,
                inspection_generation,
                removal_generation,
                id: removed_id,
                name,
                result: Ok(()),
            }))
            .expect("removal completion should be queued");
        harness.step();
        assert!(harness.state().networks.generation > list_generation);
        assert!(harness.state().networks.loading);
        assert!(!harness.state().networks.loaded);
        assert!(harness.state().network_remove_task.is_none());
        let _success =
            harness.get_by_role_and_label(Role::Label, "Network 'custom-net' was removed.");
    }

    #[test]
    fn successful_image_mutations_refresh_only_when_images_is_current() {
        let (mut app, _sender) = app_with_open_inspection();
        app.page = images::AppPage::Images;
        // Stale view generations suppress result notices, but successful daemon
        // mutations still refresh the currently visible Images inventory.
        assert!(app.finish_image_tag(1, Ok(())));
        assert!(app.finish_image_remove(1, Ok(())));

        app.page = images::AppPage::Containers;
        assert!(!app.finish_image_tag(2, Ok(())));
        assert!(!app.finish_image_remove(2, Ok(())));

        app.page = images::AppPage::Images;
        assert!(!app.finish_image_remove(
            3,
            Err(EngineError::ImageRemovalRejected {
                endpoint_label: "test endpoint",
                status: 409
            })
        ));
    }

    #[test]
    fn reconnect_snapshot_discards_stale_inspection_and_schedules_a_reinspection() {
        let (mut app, sender) = app_with_open_inspection();
        let snapshot_container = ContainerSummary {
            id: "container-id".to_owned(),
            name: "example".to_owned(),
            image: Some("example:new".to_owned()),
            state: Some(ContainerState::Exited),
            status: Some("Exited (0)".to_owned()),
        };
        assert!(
            sender
                .try_send(EngineEvent::EventSnapshot {
                    generation: 7,
                    result: Ok(vec![snapshot_container.clone()]),
                })
                .is_ok()
        );

        app.receive_events(egui::Context::default());

        assert_eq!(app.containers, vec![snapshot_container]);
        assert_reinspection_is_pending(&app);
    }

    #[test]
    fn external_change_refreshes_the_matching_open_inspection() {
        let (mut app, sender) = app_with_open_inspection();
        assert!(
            sender
                .try_send(EngineEvent::ContainerChanged {
                    generation: 7,
                    container_id: "container-id".to_owned(),
                })
                .is_ok()
        );

        app.receive_events(egui::Context::default());

        assert!(app.event_refresh_pending);
        assert_reinspection_is_pending(&app);
    }
    #[test]
    fn manual_refresh_revalidates_open_inspection() {
        let (mut app, sender) = app_with_open_inspection();
        app.loading = true;
        let refreshed_container = ContainerSummary {
            id: "container-id".to_owned(),
            name: "example".to_owned(),
            image: Some("example:new".to_owned()),
            state: Some(ContainerState::Exited),
            status: Some("Exited (0)".to_owned()),
        };
        assert!(
            sender
                .try_send(EngineEvent::Refreshed(Ok(vec![
                    refreshed_container.clone()
                ])))
                .is_ok()
        );

        app.receive_events(egui::Context::default());

        assert!(!app.loading);
        assert_eq!(app.containers, vec![refreshed_container]);
        assert_reinspection_is_pending(&app);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn event_retry_budget_is_bounded_and_resets_after_successful_subscription() {
        let mut retries = RetryBudget::default();
        for delay in [250, 500, 1_000] {
            assert_eq!(retries.next_delay(), Some(Duration::from_millis(delay)));
        }
        assert_eq!(retries.next_delay(), None);

        retries.reset();
        assert_eq!(retries.next_delay(), Some(Duration::from_millis(250)));
    }

    #[test]
    fn lifecycle_failure_survives_followup_inspection_refresh() {
        let (mut app, sender) = app_with_open_inspection();
        let action_error = EngineError::ContainerActionRejected {
            action: ContainerAction::Stop,
            endpoint_label: "test endpoint",
            status: 409,
        };
        let expected_error = action_error.to_string();
        assert!(
            sender
                .try_send(EngineEvent::ActionFinished {
                    generation: 4,
                    action: ContainerAction::Stop,
                    result: Err(action_error),
                })
                .is_ok()
        );

        app.receive_events(egui::Context::default());

        let inspection = app.inspection.as_ref().expect("inspection remains open");
        assert_reinspection_is_pending(&app);
        assert_eq!(
            inspection.action_error.as_deref(),
            Some(expected_error.as_str())
        );
    }
    #[cfg(target_os = "linux")]
    enum ScriptedLine {
        Line(&'static str),
        Fail,
        Eof,
    }

    #[cfg(target_os = "linux")]
    struct ScriptedStream(std::collections::VecDeque<ScriptedLine>);

    #[cfg(target_os = "linux")]
    impl LogLineStream for ScriptedStream {
        type Error = String;
        async fn next_line(&mut self) -> Result<Option<ContainerLogLine>, String> {
            match self.0.pop_front().expect("scripted stream has an event") {
                ScriptedLine::Line(text) => Ok(Some(ContainerLogLine {
                    source: ContainerLogSource::Stdout,
                    text: text.to_owned(),
                })),
                ScriptedLine::Fail => Err("scripted disconnect".to_owned()),
                ScriptedLine::Eof => Ok(None),
            }
        }
    }

    #[cfg(target_os = "linux")]
    struct ScriptedOpener {
        streams: std::collections::VecDeque<ScriptedStream>,
        opens: usize,
    }

    #[cfg(target_os = "linux")]
    impl LogStreamOpener for ScriptedOpener {
        type Error = String;
        type Stream = ScriptedStream;

        async fn open(&mut self) -> Result<Self::Stream, String> {
            self.opens += 1;
            self.streams
                .pop_front()
                .ok_or_else(|| "unexpected extra open".to_owned())
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn production_retry_loop_reopens_and_replays_tail_without_losing_filters() {
        use std::collections::VecDeque;

        let (mut app, _) = app_with_open_inspection();
        app.log_search = "fresh".to_owned();
        app.log_options.follow = true;
        let generation = app.log_generation;
        let mut opener = ScriptedOpener {
            streams: VecDeque::from([
                ScriptedStream(VecDeque::from([
                    ScriptedLine::Line("old history"),
                    ScriptedLine::Fail,
                ])),
                ScriptedStream(VecDeque::from([
                    ScriptedLine::Line("replayed history"),
                    ScriptedLine::Line("fresh output"),
                    ScriptedLine::Eof,
                ])),
            ]),
            opens: 0,
        };

        app.runtime.block_on(run_log_retry_loop(
            &mut opener,
            &app.log_sender,
            &egui::Context::default(),
            generation,
            true,
        ));

        assert_eq!(opener.opens, 2);

        app.receive_events(egui::Context::default());
        app.log_buffer
            .set_filters(&app.log_search, app.log_stdout, app.log_stderr);
        assert!(app.log_reconnected);
        assert_eq!(app.log_search, "fresh");
        assert_eq!(app.log_buffer.entries().len(), 2);
        assert_eq!(app.log_buffer.entries()[0].text, "replayed history");
        assert_eq!(app.log_buffer.entries()[1].text, "fresh output");
        assert_eq!(
            app.log_buffer
                .entries()
                .iter()
                .filter(|line| line.text == "replayed history")
                .count(),
            1
        );
        assert_eq!(
            app.log_buffer
                .entries()
                .iter()
                .filter(|line| line.text == "fresh output")
                .count(),
            1
        );
        assert!(
            app.log_buffer
                .entries()
                .iter()
                .all(|line| line.text != "old history")
        );
        assert_eq!(
            app.log_notice.as_deref(),
            Some("The log stream is complete.")
        );
        assert_eq!(app.log_buffer.visible_count(), 1);
        assert_eq!(
            app.log_buffer.visible_entry_at(0).unwrap().text,
            "fresh output"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn paused_log_events_remain_queued_until_resume() {
        let (mut app, _) = app_with_open_inspection();
        app.log_paused = true;
        let generation = app.log_generation;
        assert!(
            app.log_sender
                .try_send(LogEvent::Line {
                    generation,
                    line: ContainerLogLine {
                        source: ContainerLogSource::Stdout,
                        text: "queued output".to_owned(),
                    },
                })
                .is_ok()
        );

        app.receive_events(egui::Context::default());
        assert!(app.log_buffer.entries().is_empty());

        app.log_paused = false;
        app.receive_events(egui::Context::default());
        assert_eq!(app.log_buffer.visible_count(), 1);
        assert_eq!(app.log_buffer.entries()[0].text, "queued output");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dropped_log_count_is_accessible_in_logs_window() {
        use egui::accesskit::Role;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };

        let (mut app, _) = app_with_open_inspection();
        {
            let inspection = app.inspection.as_mut().unwrap();
            inspection.logs_open = true;
            inspection.details.as_mut().unwrap().tty = Some(false);
        }
        for _ in 0..10_001 {
            app.log_buffer.append(ContainerLogLine {
                source: ContainerLogSource::Stdout,
                text: "retained output".to_owned(),
            });
        }
        assert_eq!(app.log_buffer.dropped_lines(), 1);

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let context = ui.ctx().clone();
                app.receive_events(context.clone());
                app.draw_inspection(context);
            },
            app,
        );
        harness.run();
        assert!(
            !harness
                .get_by_role_and_label(
                    Role::Label,
                    "1 older log lines dropped from the in-memory buffer."
                )
                .accesskit_node()
                .is_disabled()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn logs_window_controls_expose_pause_resume_filters_copy_and_export_state() {
        use egui::accesskit::Role;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };

        let (mut app, _) = app_with_open_inspection();
        let details = app.inspection.as_mut().unwrap().details.as_mut().unwrap();
        details.state = Some(ContainerState::Exited);
        details.tty = Some(false);
        app.log_started = true;
        app.log_options.follow = true;
        app.log_task = Some(app.runtime.spawn(std::future::pending::<()>()));
        app.log_search = "keep".to_owned();
        app.log_buffer.append(ContainerLogLine {
            source: ContainerLogSource::Stdout,
            text: "keep initial".to_owned(),
        });
        app.log_buffer.append(ContainerLogLine {
            source: ContainerLogSource::Stderr,
            text: "exclude initial".to_owned(),
        });

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let context = ui.ctx().clone();
                app.receive_events(context.clone());
                app.draw_inspection(context);
            },
            app,
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Container logs")
            .click();
        harness.run();
        let export = harness.get_by_role_and_label(Role::Button, "Export");
        assert!(!export.accesskit_node().is_disabled());

        harness.get_by_role_and_label(Role::Button, "Pause").click();
        harness.run();
        assert!(harness.state().log_paused);
        assert!(
            harness
                .state()
                .log_sender
                .try_send(LogEvent::Line {
                    generation: harness.state().log_generation,
                    line: ContainerLogLine {
                        source: ContainerLogSource::Stdout,
                        text: "keep queued".to_owned(),
                    },
                })
                .is_ok()
        );
        assert!(
            harness
                .state()
                .log_buffer
                .entries()
                .iter()
                .any(
                    |line| line.source == ContainerLogSource::Stdout && line.text == "keep initial"
                )
        );
        assert!(
            harness
                .state()
                .log_buffer
                .entries()
                .iter()
                .any(|line| line.source == ContainerLogSource::Stderr
                    && line.text == "exclude initial")
        );
        assert!(
            !harness
                .state()
                .log_buffer
                .entries()
                .iter()
                .any(|line| line.text == "keep queued")
        );

        harness
            .get_by_role_and_label(Role::Button, "Resume")
            .click();
        harness.run();
        assert!(!harness.state().log_paused);
        assert!(
            harness
                .state()
                .log_buffer
                .entries()
                .iter()
                .any(|line| line.text == "keep queued")
        );
        assert!(harness.state().log_auto_scroll);
        harness
            .get_by_role_and_label(Role::CheckBox, "Auto-scroll")
            .click();
        harness.run();
        assert!(!harness.state().log_auto_scroll);
        harness
            .get_by_role_and_label(Role::CheckBox, "Auto-scroll")
            .click();
        harness.run();
        assert!(harness.state().log_auto_scroll);

        harness.get_by_role_and_label(Role::Button, "Copy").click();
        harness.step();
        assert!(
            harness
                .output()
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(
                    command,
                    egui::OutputCommand::CopyText(text)
                        if text == "stdout: keep initial\nstdout: keep queued"
                ))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn inspection_and_logs_dialog_expose_loading_stream_and_close_states() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        let inspection = app.inspection.as_mut().unwrap();
        inspection.loading = true;
        inspection.details = None;
        app.log_started = false;
        app.log_task = None;

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let context = ui.ctx().clone();
                app.receive_events(context.clone());
                app.draw_inspection(context);
            },
            app,
        );
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading inspection…");
        let logs_button = harness.get_by_role_and_label(Role::Button, "Container logs");
        assert!(logs_button.accesskit_node().is_disabled());
        harness.state_mut().inspection.as_mut().unwrap().loading = false;
        harness.state_mut().inspection.as_mut().unwrap().details = Some(ContainerDetails {
            id: "container-id".to_owned(),
            name: Some("example".to_owned()),
            image: Some("example:old".to_owned()),
            created: None,
            state: Some(ContainerState::Running),
            restart_count: None,
            exit_code: None,
            started_at: None,
            finished_at: None,
            tty: Some(false),
            mounts: None,
        });
        harness.run();
        let logs_button = harness.get_by_role_and_label(Role::Button, "Container logs");
        assert!(!logs_button.accesskit_node().is_disabled());
        logs_button.click();
        harness.run();
        let _unavailable =
            harness.get_by_role_and_label(Role::Label, "The engine connection is unavailable.");
        assert!(harness.state().log_started);
        assert!(harness.state().log_task.is_none());

        harness.state_mut().log_notice = None;
        harness.run();
        let _empty =
            harness.get_by_role_and_label(Role::Label, "No log lines match the current filters.");

        let generation = harness.state().log_generation;
        assert!(
            harness
                .state()
                .log_sender
                .try_send(LogEvent::Ended {
                    generation,
                    error: Some("scripted disconnect".into()),
                })
                .is_ok()
        );
        harness.run();
        let _stream_error =
            harness.get_by_role_and_label(Role::Label, "Log stream ended: scripted disconnect");

        assert!(
            harness
                .state()
                .log_sender
                .try_send(LogEvent::Reset { generation })
                .is_ok()
        );
        assert!(
            harness
                .state()
                .log_sender
                .try_send(LogEvent::Line {
                    generation,
                    line: ContainerLogLine {
                        source: ContainerLogSource::Stdout,
                        text: "fresh output".to_owned(),
                    },
                })
                .is_ok()
        );
        harness.run();
        let _reconnect = harness.get_by_role_and_label(
            Role::Label,
            "Reconnected; displaying recent container output.",
        );

        let app = harness.into_state();
        let mut logs_harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                app.draw_logs(ui.ctx().clone());
            },
            app,
        );
        logs_harness.run();
        logs_harness
            .get_by_role_and_label(Role::Button, "Close window")
            .click();
        logs_harness.run();
        assert!(!logs_harness.state().log_started);
        assert!(logs_harness.state().log_task.is_none());
        assert!(
            logs_harness
                .state()
                .inspection
                .as_ref()
                .is_none_or(|inspection| !inspection.logs_open)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn terminal_shell_selector_and_open_control_are_accessible() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (app, _) = app_with_open_inspection();
        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| app.draw_inspection(ui.ctx().clone()),
            app,
        );
        harness.run();
        let _shell = harness.get_by_role_and_label(Role::ComboBox, "Shell");
        let _open_terminal = harness.get_by_role_and_label(Role::Button, "Open terminal");
        assert_eq!(harness.state().terminal_shell, "/bin/sh");
        assert!(!harness.state().terminal_pending);
        assert!(harness.state().terminal.is_none());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn accessible_cancel_control_aborts_opening_without_container_actions() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.terminal_pending = true;
        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| app.draw_inspection(ui.ctx().clone()),
            app,
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Cancel terminal opening")
            .click();
        harness.run();
        assert!(!harness.state().terminal_pending);
        assert!(harness.state().terminal.is_none());
        assert!(harness.state().action_task.is_none());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pending_network_creation_disables_context_selector_accessibly() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.network_create_task = Some(app.runtime.spawn(std::future::pending()));
        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let mut refresh_requested = false;
                let mut settings_requested = false;
                let mut context_requested = None;
                app.draw_header(
                    ui,
                    &mut refresh_requested,
                    &mut settings_requested,
                    &mut context_requested,
                );
            },
            app,
        );

        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::ComboBox, "Docker context")
                .accesskit_node()
                .is_disabled()
        );
        let _status = harness.get_by_role_and_label(
            Role::Label,
            "A network mutation is in progress; context switching is disabled.",
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn context_switch_is_rejected_while_network_creation_is_pending() {
        let (mut app, _) = app_with_open_inspection();
        app.network_create_task = Some(app.runtime.spawn(std::future::pending()));

        app.switch_context(Some("other-context".to_owned()), egui::Context::default());

        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
        assert!(app.network_create_task.is_some());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pending_network_removal_disables_context_selector_accessibly() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.network_remove_task = Some(app.runtime.spawn(std::future::pending()));
        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let mut refresh_requested = false;
                let mut settings_requested = false;
                let mut context_requested = None;
                app.draw_header(
                    ui,
                    &mut refresh_requested,
                    &mut settings_requested,
                    &mut context_requested,
                );
            },
            app,
        );

        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::ComboBox, "Docker context")
                .accesskit_node()
                .is_disabled()
        );
        let _status = harness.get_by_role_and_label(
            Role::Label,
            "A network mutation is in progress; context switching is disabled.",
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn context_switch_is_rejected_while_network_removal_is_pending() {
        let (mut app, _) = app_with_open_inspection();
        app.network_remove_task = Some(app.runtime.spawn(std::future::pending()));

        app.switch_context(Some("other-context".to_owned()), egui::Context::default());

        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
        assert!(app.network_remove_task.is_some());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pending_image_build_confirmation_disables_context_switching() {
        let (mut app, _) = app_with_open_inspection();
        app.images.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        app.images.build_repository = "example/service".to_owned();
        app.images.build_tag = "release-1".to_owned();
        assert!(
            app.images
                .begin_build_confirmation("Automatic local discovery")
        );

        app.switch_context(Some("other-context".to_owned()), egui::Context::default());

        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn confirmed_image_build_is_not_blocked_at_start_gate() {
        let (mut app, _) = app_with_open_inspection();
        app.images.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        app.images.build_repository = "example/service".to_owned();
        app.images.build_tag = "release-1".to_owned();
        assert!(
            app.images
                .begin_build_confirmation("Automatic local discovery")
        );
        app.images.confirm_build();
        assert!(app.images.take_build_requested());

        assert!(!app.image_build_start_blocked());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn competing_image_mutation_blocks_image_build_start() {
        let (mut app, _) = app_with_open_inspection();
        app.images.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        app.images.build_repository = "example/service".to_owned();
        app.images.build_tag = "release-1".to_owned();
        assert!(
            app.images
                .begin_build_confirmation("Automatic local discovery")
        );
        app.images.confirm_build();
        assert!(app.images.take_build_requested());
        app.image_pull_task = Some(app.runtime.spawn(std::future::pending()));

        assert!(app.image_build_start_blocked());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn rejected_confirmed_build_persists_redacted_not_submitted_outcome() {
        let (mut app, _) = app_with_open_inspection();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ferrum-build-audit-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        app.audit_store = audit::AuditStore::new(Arc::new(settings::SettingsStore::at(
            root.join("settings.toml"),
        )));
        app.images.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        app.images.build_repository = "example/service".to_owned();
        app.images.build_tag = "release-1".to_owned();
        assert!(
            app.images
                .begin_build_confirmation("Automatic local discovery")
        );
        app.images.confirm_build();
        assert!(app.images.take_build_requested());
        app.image_pull_task = Some(app.runtime.spawn(std::future::pending()));

        app.start_image_build(egui::Context::default());

        assert!(app.engine.is_none());
        assert!(app.image_build_task.is_none());
        app.image_pull_task.take().unwrap().abort();
        let audit_path = root.join("audit.jsonl");
        app.runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    if std::fs::read_to_string(&audit_path)
                        .is_ok_and(|contents| !contents.trim().is_empty())
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the rejected build outcome should be persisted");
        });
        let contents = std::fs::read_to_string(&audit_path).unwrap();
        let record = serde_json::from_str::<serde_json::Value>(&contents).unwrap();
        assert_eq!(record["schema_version"], 1);
        assert_eq!(record["event"], "image_mutation");
        assert_eq!(record["action"], "build");
        assert_eq!(record["outcome"], "not_submitted");
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
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn active_image_build_disables_context_switching() {
        let (mut app, _) = app_with_open_inspection();
        app.images.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        app.images.build_repository = "example/service".to_owned();
        app.images.build_tag = "release-1".to_owned();
        assert!(
            app.images
                .begin_build_confirmation("Automatic local discovery")
        );
        app.images.confirm_build();
        assert!(app.images.take_build_requested());
        assert!(app.images.begin_build().is_some());
        app.image_build_task = Some(app.runtime.spawn(std::future::pending()));

        app.switch_context(Some("other-context".to_owned()), egui::Context::default());

        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
        assert!(app.image_build_task.is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pending_image_build_disables_context_selector_accessibly() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.images.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        app.images.build_repository = "example/service".to_owned();
        app.images.build_tag = "release-1".to_owned();
        assert!(
            app.images
                .begin_build_confirmation("Automatic local discovery")
        );

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let mut refresh_requested = false;
                let mut settings_requested = false;
                let mut context_requested = None;
                app.draw_header(
                    ui,
                    &mut refresh_requested,
                    &mut settings_requested,
                    &mut context_requested,
                );
            },
            app,
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::ComboBox, "Docker context")
                .accesskit_node()
                .is_disabled()
        );
        let _status = harness.get_by_role_and_label(
            Role::Label,
            "An image build is awaiting confirmation or running; context switching is disabled.",
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn only_one_network_mutation_can_start_in_the_app() {
        let (mut app, _) = app_with_open_inspection();
        app.network_create_task = Some(app.runtime.spawn(std::future::pending()));
        app.start_network_remove(
            0,
            0,
            0,
            "a".repeat(64),
            "custom-net".to_owned(),
            egui::Context::default(),
        );
        assert!(app.network_create_task.is_some());
        assert!(app.network_remove_task.is_none());

        let (mut app, _) = app_with_open_inspection();
        app.network_remove_task = Some(app.runtime.spawn(std::future::pending()));
        app.start_network_create(0, "new-net".to_owned(), egui::Context::default());
        assert!(app.network_remove_task.is_some());
        assert!(app.network_create_task.is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pending_network_attachment_survives_invalidation_and_blocks_context_switch() {
        let (mut app, _) = app_with_open_inspection();
        app.network_attachment_task = Some(app.runtime.spawn(std::future::pending()));

        app.invalidate_resource_reads();
        app.switch_context(Some("other-context".to_owned()), egui::Context::default());
        app.start_network_create(0, "new-net".to_owned(), egui::Context::default());

        assert!(!app.loading);
        assert_eq!(app.selected_context, None);
        assert!(app.network_attachment_task.is_some());
        assert!(app.network_create_task.is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn network_page_refreshes_container_state_and_fails_closed_when_unavailable() {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Networks;
        app.container_inventory_fresh = false;
        let generation = app.networks.generation;
        let container_generation = app.container_inventory_generation;
        let exited = ContainerSummary {
            id: "container-id".to_owned(),
            name: "example".to_owned(),
            image: Some("example:old".to_owned()),
            state: Some(ContainerState::Exited),
            status: Some("Exited".to_owned()),
        };
        app.runtime
            .block_on(sender.send(EngineEvent::NetworksListed {
                generation,
                container_generation,
                network_result: Ok(Vec::new()),
                container_result: Ok(vec![exited]),
            }))
            .expect("fresh network-page inventories should be queued");
        app.receive_events(egui::Context::default());

        assert!(app.container_inventory_fresh);
        assert_eq!(app.containers[0].state, Some(ContainerState::Exited));

        app.container_inventory_fresh = true;
        app.runtime
            .block_on(sender.send(EngineEvent::ContainerChanged {
                generation: app.event_generation,
                container_id: "container-id".to_owned(),
            }))
            .expect("container state changes should be queued");
        app.receive_events(egui::Context::default());
        let container_generation = app.container_inventory_generation;
        assert!(!app.container_inventory_fresh);

        app.runtime
            .block_on(sender.send(EngineEvent::NetworksListed {
                generation,
                container_generation,
                network_result: Ok(Vec::new()),
                container_result: Err(EngineError::UnsupportedPlatform),
            }))
            .expect("failed container inventories should be queued");
        app.receive_events(egui::Context::default());
        assert!(!app.container_inventory_fresh);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_network_list_does_not_replace_current_container_inventory() {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Networks;
        let stale_generation = app.networks.generation;
        let stale_container_generation = app.container_inventory_generation;
        app.invalidate_resource_reads();
        app.container_inventory_fresh = false;
        let stale_container = ContainerSummary {
            id: "container-id".to_owned(),
            name: "example".to_owned(),
            image: Some("example:old".to_owned()),
            state: Some(ContainerState::Exited),
            status: Some("Exited".to_owned()),
        };

        app.runtime
            .block_on(sender.send(EngineEvent::NetworksListed {
                generation: stale_generation,
                container_generation: stale_container_generation,
                network_result: Ok(Vec::new()),
                container_result: Ok(vec![stale_container]),
            }))
            .expect("stale network list result should be queued");
        app.receive_events(egui::Context::default());

        assert!(!app.container_inventory_fresh);
        assert_eq!(app.containers[0].state, Some(ContainerState::Running));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn context_switch_aborts_a_pending_exec_open() {
        let (mut app, _) = app_with_open_inspection();
        app.terminal_pending = true;
        app.terminal_open_task = Some(app.runtime.spawn(std::future::pending()));
        app.switch_context(Some("other-context".to_owned()), egui::Context::default());
        assert!(!app.terminal_pending);
        assert!(app.terminal_open_task.is_none());
        assert!(app.loading);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_exec_open_updates_terminal_state_and_is_visible() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, _) = app_with_open_inspection();
        app.terminal_attempt = 1;
        app.terminal_pending = true;
        let message = EngineError::ExecSessionRejected {
            endpoint_label: "test socket",
            status: 403,
        }
        .to_string();
        assert!(
            app.terminal_open_sender
                .try_send(TerminalOpened::Failed {
                    attempt_id: 1,
                    message: message.clone(),
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());
        assert!(!app.terminal_pending);
        assert_eq!(app.terminal_notice.as_deref(), Some(message.as_str()));

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| app.draw_inspection(ui.ctx().clone()),
            app,
        );
        harness.run();
        let _notice = harness.get_by_role_and_label(Role::Label, message.as_str());
    }

    #[test]
    fn large_inventory_is_virtualized_and_accessible_to_keyboard_users() {
        use egui::accesskit::Role;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };

        struct InventoryUi {
            app: FerrumApp,
            selected: Option<usize>,
        }

        let (mut app, _) = app_with_open_inspection();
        app.containers = (0..1_000)
            .map(|index| ContainerSummary {
                id: format!("id-{index:04}"),
                name: format!("container-{index:04}"),
                image: Some(format!("image-{index:04}")),
                state: Some(ContainerState::Running),
                status: Some("Up".to_owned()),
            })
            .collect();
        app.inventory_view.inventory_changed(&app.containers);

        let mut harness = Harness::new_ui_state(
            |ui, state: &mut InventoryUi| {
                if let Some(index) = state.app.draw_inventory(ui) {
                    state.selected = Some(index);
                }
            },
            InventoryUi {
                app,
                selected: None,
            },
        );
        harness.run();
        assert_eq!(harness.state().app.inventory_view.visible().len(), 1_000);
        assert!(
            harness.get_all_by_role(Role::Button).count() < 100,
            "the accessible tree should contain only the visible container rows"
        );

        let first = harness.get_by_role_and_label(Role::Button, "container-0000");
        first.click_accesskit();
        harness.run();
        assert_eq!(harness.state().selected, Some(0));
        harness.state_mut().selected = None;
        let search = harness.get_by_role_and_label(Role::TextInput, "Search");
        search.focus();
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::TextInput, "Search")
                .is_focused()
        );

        let mut row_focused = false;
        for _ in 0..8 {
            harness.key_down(egui::Key::Tab);
            harness.run();
            harness.key_up(egui::Key::Tab);
            harness.run();
            if harness
                .get_by_role_and_label(Role::Button, "container-0000")
                .is_focused()
            {
                row_focused = true;
                break;
            }
        }
        assert!(row_focused, "Tab navigation should reach the first row");
        harness.key_down(egui::Key::Space);
        harness.run();
        assert_eq!(harness.state().selected, Some(0));

        let search = harness.get_by_role_and_label(Role::TextInput, "Search");
        search.focus();
        harness.run();
        let search = harness.get_by_role_and_label(Role::TextInput, "Search");
        search.type_text("container-0999");
        harness.run();
        assert_eq!(harness.state().app.inventory_view.visible().len(), 1);
        let last = harness.get_by_role_and_label(Role::Button, "container-0999");
        assert!(!last.accesskit_node().is_disabled());
    }

    #[test]
    fn untrusted_container_text_is_escaped_in_inventory_and_inspection_accessibility() {
        use egui::accesskit::Role;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };

        let (mut app, _) = app_with_open_inspection();
        let inspection = app.inspection.as_mut().expect("open inspection");
        inspection.container_name = "inspected\u{e0001}container".to_owned();
        let details = inspection.details.as_mut().expect("inspection details");
        details.name = Some("detail\u{200b}container".to_owned());
        details.image = Some("inspect-image\u{202e}:tag".to_owned());
        app.containers = vec![ContainerSummary {
            id: "a".repeat(64),
            name: "worker\u{202e}proc".to_owned(),
            image: Some("image\u{200b}:latest".to_owned()),
            state: Some(ContainerState::Unknown("mystery\u{e0001}".to_owned())),
            status: Some("Up\u{2060}".to_owned()),
        }];
        app.inventory_view.inventory_changed(&app.containers);
        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let _ = app.draw_inventory(ui);
                app.draw_inspection(ui.ctx().clone());
            },
            app,
        );
        harness.run();

        let name = "worker\\u{202e}proc";
        let name_button = harness.get_by_role_and_label(Role::Button, name);
        assert_eq!(name_button.accesskit_node().label().as_deref(), Some(name));
        let image = harness.get_by_role_and_label(Role::Label, "image\\u{200b}:latest");
        assert_eq!(
            image.accesskit_node().value().as_deref(),
            Some("image\\u{200b}:latest")
        );
        let state = harness.get_by_role_and_label(Role::Label, "mystery\\u{e0001}");
        assert_eq!(
            state.accesskit_node().value().as_deref(),
            Some("mystery\\u{e0001}")
        );
        let status = harness.get_by_role_and_label(Role::Label, "Up\\u{2060}");
        assert_eq!(
            status.accesskit_node().value().as_deref(),
            Some("Up\\u{2060}")
        );
        let inspection_name = "inspected\\u{e0001}container";
        let heading = harness.get_by_role_and_label(Role::Label, inspection_name);
        assert_eq!(
            heading.accesskit_node().value().as_deref(),
            Some(inspection_name)
        );
        let detail_name = "detail\\u{200b}container";
        let name_detail = harness.get_by_role_and_label(Role::Label, detail_name);
        assert_eq!(
            name_detail.accesskit_node().value().as_deref(),
            Some(detail_name)
        );
        let detail_image = "inspect-image\\u{202e}:tag";
        let image_detail = harness.get_by_role_and_label(Role::Label, detail_image);
        assert_eq!(
            image_detail.accesskit_node().value().as_deref(),
            Some(detail_image)
        );
    }

    #[test]
    fn inventory_connection_and_refresh_states_are_accessible() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut connecting, sender) = app_with_open_inspection();
        connecting.connection = ConnectionState::Connecting;
        connecting.loading = true;
        let mut loading_harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let _ = app.draw_inventory(ui);
            },
            connecting,
        );
        loading_harness.step();
        let _connecting = loading_harness
            .get_by_role_and_label(Role::Label, "Connecting to a local container engine");
        let _loading = loading_harness.get_by_role_and_label(
            Role::Label,
            "The container inventory will appear when the engine responds.",
        );

        let connection_error = EngineError::NoLocalSocket;
        let connection_diagnostic = connection_error.to_string();
        assert!(
            sender
                .try_send(EngineEvent::Connected(Err(connection_error)))
                .is_ok()
        );
        loading_harness
            .state_mut()
            .receive_events(egui::Context::default());
        loading_harness.run();
        assert!(!loading_harness.state().loading);
        assert!(loading_harness.state().containers.is_empty());
        let _unavailable =
            loading_harness.get_by_role_and_label(Role::Label, "Connection unavailable");
        let _diagnostic =
            loading_harness.get_by_role_and_label(Role::Label, connection_diagnostic.as_str());

        let (mut refresh_failed, refresh_sender) = app_with_open_inspection();
        refresh_failed.loading = true;
        refresh_failed
            .inventory_view
            .inventory_changed(&refresh_failed.containers);
        let last_inventory = refresh_failed.containers.clone();
        let refresh_error = EngineError::InventoryRequest {
            endpoint_label: "selected local socket",
        };
        let refresh_diagnostic = refresh_error.to_string();
        assert!(
            refresh_sender
                .try_send(EngineEvent::Refreshed(Err(refresh_error)))
                .is_ok()
        );
        refresh_failed.receive_events(egui::Context::default());
        assert!(!refresh_failed.loading);
        assert_eq!(refresh_failed.containers, last_inventory);
        assert_eq!(
            refresh_failed.refresh_error.as_deref(),
            Some(refresh_diagnostic.as_str())
        );
        let mut refresh_harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                let _ = app.draw_inventory(ui);
            },
            refresh_failed,
        );
        refresh_harness.run();
        let _failure = refresh_harness.get_by_role_and_label(Role::Label, "Refresh failed");
        let _diagnostic =
            refresh_harness.get_by_role_and_label(Role::Label, refresh_diagnostic.as_str());
        let _preserved = refresh_harness
            .get_by_role_and_label(Role::Label, "Showing the last successful inventory.");
        let _last_inventory = refresh_harness.get_by_role_and_label(Role::Button, "example");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn accessible_hide_progress_keeps_pull_reader_and_gate_until_terminal_event() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};

        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Images;
        app.images.set_pull_reference("alpine:latest".to_owned());
        let (generation, _) = app.images.begin_pull().unwrap();
        let (task_finish, task_wait) = tokio::sync::oneshot::channel();
        let (task_started, task_started_wait) = tokio::sync::oneshot::channel();
        let task = app.runtime.spawn(async move {
            let _ = task_started.send(());
            let _ = task_wait.await;
        });
        app.runtime
            .block_on(task_started_wait)
            .expect("fake pull reader should start");
        let abort = task.abort_handle();
        app.image_pull_task = Some(task);
        let progress_reporting = Arc::new(AtomicBool::new(true));
        app.image_pull_progress_reporting = Some(Arc::clone(&progress_reporting));
        assert!(
            sender
                .try_send(EngineEvent::ImagePullProgress {
                    generation,
                    progress: ImagePullProgress {
                        status: "Downloading".to_owned(),
                        current_bytes: Some(10),
                        total_bytes: Some(100),
                    },
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());

        let mut harness = Harness::new_ui_state(
            |ui, app: &mut FerrumApp| {
                app.draw_images(ui);
            },
            app,
        );
        harness.step();
        harness
            .get_by_role_and_label(Role::Button, "Hide progress")
            .click_accesskit();
        harness.run();
        harness.state().runtime.block_on(tokio::task::yield_now());
        assert!(!harness.state().images.is_pulling());
        assert!(harness.state().images.is_pull_progress_hidden());
        assert!(harness.state().images.pull_progress().is_none());
        assert!(harness.state().images.has_mutation_in_flight());
        assert!(harness.state().image_mutation_busy());
        assert!(harness.state().image_pull_task.is_some());
        assert!(!progress_reporting.load(Ordering::Acquire));
        assert!(!abort.is_finished());
        let _hidden_notice = harness.get_by_role_and_label(
            Role::Label,
            "Progress hidden. The Docker image transfer may continue; other image operations remain disabled until Ferrum finishes reading the pull response.",
        );
        let _busy_notice = harness.get_by_role_and_label(
            Role::Label,
            "An image operation is in progress; wait for it to finish before starting another pull, push, tag, or removal.",
        );
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Pull image")
                .accesskit_node()
                .is_disabled()
        );

        let context = egui::Context::default();
        harness.state_mut().start_image_pull(context.clone());
        harness.state_mut().start_image_tag(context.clone());
        harness.state_mut().start_image_remove(context);
        assert!(harness.state().image_pull_task.is_some());
        assert!(harness.state().image_tag_task.is_none());
        assert!(harness.state().image_remove_task.is_none());
        assert!(harness.state().images.has_mutation_in_flight());

        assert!(task_finish.send(()).is_ok());
        harness.state().runtime.block_on(tokio::task::yield_now());
        assert!(abort.is_finished());
        assert!(harness.state().image_mutation_busy());

        assert!(
            sender
                .try_send(EngineEvent::ImagePullProgress {
                    generation,
                    progress: ImagePullProgress {
                        status: "late progress".to_owned(),
                        current_bytes: Some(50),
                        total_bytes: Some(100),
                    },
                })
                .is_ok()
        );
        harness.state_mut().receive_events(egui::Context::default());
        assert!(harness.state().images.pull_progress().is_none());

        harness.state_mut().loading = true;
        assert!(
            sender
                .try_send(EngineEvent::ImagePullFinished {
                    generation,
                    result: Ok(()),
                })
                .is_ok()
        );
        harness.state_mut().receive_events(egui::Context::default());
        assert!(harness.state().image_pull_task.is_none());
        assert!(harness.state().image_pull_progress_reporting.is_none());
        assert!(!harness.state().images.has_mutation_in_flight());
        assert!(!harness.state().image_mutation_busy());
        assert!(harness.state().images.is_pull_complete());
        harness.state_mut().loading = false;
        harness.run();
        let _complete = harness.get_by_role_and_label(Role::Label, "Image pull complete.");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn page_invalidation_makes_late_pull_events_stale_but_releases_the_task() {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Images;
        app.images.set_pull_reference("alpine:latest".to_owned());
        let (generation, _) = app.images.begin_pull().unwrap();
        app.images.update_pull(
            generation,
            ImagePullProgress {
                status: "old context".to_owned(),
                current_bytes: Some(10),
                total_bytes: Some(100),
            },
        );

        let (task_finish, task_wait) = tokio::sync::oneshot::channel();
        let (task_started, task_started_wait) = tokio::sync::oneshot::channel();
        let pull_task = app.runtime.spawn(async move {
            let _ = task_started.send(());
            let _ = task_wait.await;
        });
        app.runtime
            .block_on(task_started_wait)
            .expect("fake pull reader should start");
        let pull_abort = pull_task.abort_handle();
        app.image_pull_task = Some(pull_task);
        let progress_reporting = Arc::new(AtomicBool::new(true));
        app.image_pull_progress_reporting = Some(Arc::clone(&progress_reporting));

        let list_task = app.runtime.spawn(std::future::pending());
        let list_abort = list_task.abort_handle();
        app.image_list_task = Some(list_task);
        let inspection_task = app.runtime.spawn(std::future::pending());
        let inspection_abort = inspection_task.abort_handle();
        app.image_inspection_task = Some(inspection_task);

        app.set_page(images::AppPage::Containers, egui::Context::default());
        app.runtime.block_on(tokio::task::yield_now());
        assert!(list_abort.is_finished());
        assert!(inspection_abort.is_finished());
        assert!(!pull_abort.is_finished());
        assert!(!progress_reporting.load(Ordering::Acquire));
        assert!(app.image_pull_task.is_some());
        assert!(app.image_mutation_busy());
        assert!(!app.images.is_pulling());
        assert!(app.images.pull_progress().is_none());
        let context = egui::Context::default();
        app.start_image_pull(context.clone());
        app.start_image_tag(context.clone());
        app.start_image_remove(context);
        assert!(app.image_pull_task.is_some());
        assert!(app.image_tag_task.is_none());
        assert!(app.image_remove_task.is_none());
        assert!(app.image_mutation_busy());

        assert!(task_finish.send(()).is_ok());
        app.runtime.block_on(tokio::task::yield_now());
        assert!(pull_abort.is_finished());
        assert!(app.image_mutation_busy());
        assert!(
            sender
                .try_send(EngineEvent::ImagePullProgress {
                    generation,
                    progress: ImagePullProgress {
                        status: "stale progress".to_owned(),
                        current_bytes: Some(90),
                        total_bytes: Some(100),
                    },
                })
                .is_ok()
        );
        assert!(
            sender
                .try_send(EngineEvent::ImagePullFinished {
                    generation,
                    result: Err(EngineError::NoLocalSocket),
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());

        assert!(app.image_pull_task.is_none());
        assert!(app.image_pull_progress_reporting.is_none());
        assert!(!app.image_mutation_busy());
        assert!(!app.images.has_mutation_in_flight());
        assert!(app.images.pull_progress().is_none());
        assert!(app.images.pull_error().is_none());
        assert!(!app.images.is_pull_complete());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn page_invalidation_keeps_push_worker_and_gate_until_terminal_event() {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Images;
        let mut view = images::ImagesView::default();
        let list_generation = view.begin_load();
        assert!(view.complete_load(
            list_generation,
            Ok::<_, &str>(vec![ImageSummary {
                id: "sha256:image".to_owned(),
                repo_tags: vec!["registry.example/team/app:stable".to_owned()],
                created_unix_seconds: None,
                size_bytes: None,
            }]),
        ));
        view.set_push_source_for_test("sha256:image", "registry.example/team/app:stable");
        view.begin_push_confirmation();
        view.confirm_push();
        assert!(view.take_push_requested());
        let request = view.begin_push().expect("confirmed push should start");
        app.images = view;

        let (task_finish, task_wait) = tokio::sync::oneshot::channel();
        let (task_started, task_started_wait) = tokio::sync::oneshot::channel();
        let task = app.runtime.spawn(async move {
            let _ = task_started.send(());
            let _ = task_wait.await;
        });
        app.runtime
            .block_on(task_started_wait)
            .expect("fake push reader should start");
        let push_abort = task.abort_handle();
        app.image_push_task = Some(task);
        let progress_reporting = Arc::new(AtomicBool::new(true));
        app.image_push_progress_reporting = Some(Arc::clone(&progress_reporting));

        assert!(
            sender
                .try_send(EngineEvent::ImagePushProgress {
                    generation: request.generation,
                    progress: ImagePushProgress {
                        status: "Pushing".to_owned(),
                        current_bytes: Some(10),
                        total_bytes: Some(100),
                    },
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());
        assert_eq!(
            app.images
                .push_progress()
                .map(|progress| progress.current_bytes),
            Some(Some(10))
        );

        app.set_page(images::AppPage::Containers, egui::Context::default());
        app.runtime.block_on(tokio::task::yield_now());
        assert!(!progress_reporting.load(Ordering::Acquire));
        assert!(app.image_push_task.is_some());
        assert!(!push_abort.is_finished());
        assert!(app.image_mutation_busy());
        assert!(!app.images.is_pushing());
        assert!(app.images.has_mutation_in_flight());

        let context = egui::Context::default();
        app.start_image_pull(context.clone());
        app.start_image_push(context.clone());
        app.start_image_tag(context.clone());
        app.start_image_remove(context);
        assert!(app.image_push_task.is_some());
        assert!(app.image_pull_task.is_none());
        assert!(app.image_tag_task.is_none());
        assert!(app.image_remove_task.is_none());

        assert!(
            sender
                .try_send(EngineEvent::ImagePushProgress {
                    generation: request.generation,
                    progress: ImagePushProgress {
                        status: "stale progress".to_owned(),
                        current_bytes: Some(90),
                        total_bytes: Some(100),
                    },
                })
                .is_ok()
        );
        assert!(
            sender
                .try_send(EngineEvent::ImagePushFinished {
                    generation: request.generation,
                    result: Ok(()),
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());
        assert!(app.image_push_task.is_none());
        assert!(app.image_push_progress_reporting.is_none());
        assert!(!app.image_mutation_busy());
        assert!(!app.images.has_mutation_in_flight());
        assert!(app.images.push_progress().is_none());
        assert!(!app.images.is_push_complete());
        assert!(app.image_list_task.is_none());

        assert!(task_finish.send(()).is_ok());
        app.runtime.block_on(tokio::task::yield_now());
        assert!(push_abort.is_finished());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn ambiguous_push_completion_keeps_image_mutations_locked() {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Images;
        let mut view = images::ImagesView::default();
        let list_generation = view.begin_load();
        assert!(view.complete_load(
            list_generation,
            Ok::<_, &str>(vec![ImageSummary {
                id: "sha256:image".to_owned(),
                repo_tags: vec!["registry.example/team/app:stable".to_owned()],
                created_unix_seconds: None,
                size_bytes: None,
            }]),
        ));
        view.set_push_source_for_test("sha256:image", "registry.example/team/app:stable");
        view.begin_push_confirmation();
        view.confirm_push();
        assert!(view.take_push_requested());
        let request = view.begin_push().expect("confirmed push should start");
        app.images = view;
        app.image_push_task = Some(app.runtime.spawn(std::future::pending()));
        app.image_push_progress_reporting = Some(Arc::new(AtomicBool::new(true)));

        assert!(
            sender
                .try_send(EngineEvent::ImagePushFinished {
                    generation: request.generation,
                    result: Err(EngineError::ImagePushTimedOut {
                        endpoint_label: "local Docker socket",
                    }),
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());

        assert!(app.image_push_task.is_none());
        assert!(app.image_push_progress_reporting.is_none());
        assert!(app.images.is_push_outcome_uncertain());
        assert!(app.images.has_mutation_in_flight());
        assert!(app.image_mutation_busy());
        assert!(app.image_list_task.is_none());

        let context = egui::Context::default();
        app.start_image_pull(context.clone());
        app.start_image_push(context.clone());
        app.start_image_tag(context.clone());
        app.start_image_remove(context);
        assert!(app.image_pull_task.is_none());
        assert!(app.image_push_task.is_none());
        assert!(app.image_tag_task.is_none());
        assert!(app.image_remove_task.is_none());
        assert!(app.image_mutation_busy());
    }

    #[test]
    fn leaving_compose_aborts_its_reads_and_invalidates_late_results() {
        let (mut app, _) = app_with_open_inspection();
        app.page = images::AppPage::Compose;
        let generation = app.compose.begin_load();
        let validation_generation = app.compose.begin_file_selection();
        assert!(app.compose.finish_file_selection(
            validation_generation,
            Some(std::path::Path::new("/private/compose.yaml")),
        ));
        let task = app.runtime.spawn(std::future::pending::<()>());
        let abort = task.abort_handle();
        app.compose_task = Some(task);
        let validation_task = app.runtime.spawn(std::future::pending::<()>());
        let validation_abort = validation_task.abort_handle();
        app.compose_validation_task = Some(validation_task);

        app.set_page(images::AppPage::Containers, egui::Context::default());
        app.runtime.block_on(tokio::task::yield_now());

        assert!(abort.is_finished());
        assert!(validation_abort.is_finished());
        assert!(app.compose_task.is_none());
        assert!(app.compose_validation_task.is_none());
        assert!(!app.compose.is_loading());
        assert!(!app.compose.has_loaded());
        assert!(!app.compose.is_validation_running());
        assert!(
            !app.compose
                .finish_load(generation, Err(compose::ComposeError::DockerCliMissing),)
        );
        assert!(!app.compose.finish_validation(
            validation_generation,
            compose::ComposeValidationResult {
                version: None,
                outcome: Ok(()),
            },
        ));
    }
    #[test]
    fn refreshing_disconnected_volumes_retries_engine_connection() {
        let (mut app, _) = app_with_open_inspection();
        app.page = images::AppPage::Volumes;
        app.connection = ConnectionState::Disconnected("offline".to_owned());

        app.refresh(egui::Context::default());

        assert!(app.loading);
        assert!(app.engine.is_none());
        assert!(app.resource_read_task.is_none());
    }
    #[test]
    fn refreshing_disconnected_networks_retries_engine_connection() {
        let (mut app, _) = app_with_open_inspection();
        app.page = images::AppPage::Networks;
        app.connection = ConnectionState::Disconnected("offline".to_owned());

        app.refresh(egui::Context::default());

        assert!(app.loading);
        assert!(app.engine.is_none());
        assert!(app.resource_read_task.is_none());
    }

    #[test]
    fn volume_and_network_reads_share_one_active_task_and_latest_pending_read() {
        let (mut app, sender) = app_with_open_inspection();
        app.page = images::AppPage::Volumes;
        let active_generation = app.volumes.begin_list();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel::<()>();
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel::<()>();
        let task_sender = sender.clone();
        let task = app.runtime.spawn(async move {
            let _ = started_sender.send(());
            if release_receiver.await.is_ok() {
                let _ = task_sender
                    .send(EngineEvent::VolumesListed {
                        generation: active_generation,
                        result: Ok(Vec::new()),
                    })
                    .await;
            }
        });
        app.runtime
            .block_on(started_receiver)
            .expect("fake volume list task should start");
        let abort = task.abort_handle();
        app.resource_read_task = Some(task);

        app.load_volumes(egui::Context::default());
        let list_generation = app.volumes.generation;
        assert!(matches!(
            &app.pending_resource_read,
            Some(PendingResourceRead::VolumeList { generation }) if *generation == list_generation
        ));
        assert!(app.resource_read_task.is_some());
        app.inspect_volume("latest".to_owned(), egui::Context::default());
        assert!(matches!(
            &app.pending_resource_read,
            Some(PendingResourceRead::VolumeInspect { name, .. }) if name == "latest"
        ));
        assert!(app.resource_read_task.is_some());
        app.page = images::AppPage::Networks;
        app.load_networks(egui::Context::default());
        assert!(matches!(
            &app.pending_resource_read,
            Some(PendingResourceRead::NetworkList { .. })
        ));
        app.inspect_network("older".to_owned(), egui::Context::default());
        app.inspect_network("latest-network".to_owned(), egui::Context::default());
        assert!(matches!(
            &app.pending_resource_read,
            Some(PendingResourceRead::NetworkInspect { id, .. }) if id == "latest-network"
        ));
        assert!(app.resource_read_task.is_some());

        app.set_page(images::AppPage::Images, egui::Context::default());
        assert!(app.pending_resource_read.is_none());
        assert!(app.resource_read_task.is_some());
        assert!(!abort.is_finished(), "invalidation must retain active read");

        assert!(release_sender.send(()).is_ok());
        app.runtime.block_on(tokio::task::yield_now());
        app.receive_events(egui::Context::default());
        assert!(app.resource_read_task.is_none());
        assert!(!app.volumes.loaded, "stale page result must not render");
    }

    fn assert_reinspection_is_pending(app: &FerrumApp) {
        let inspection = app.inspection.as_ref().expect("inspection stays open");
        assert_eq!(inspection.generation, 5);
        assert!(inspection.loading);
        assert!(
            inspection.details.is_none(),
            "event changes must not leave stale details or action availability"
        );
    }
}
#[cfg(all(test, target_os = "linux"))]
mod terminal_ui_tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    struct TestTerminalUi {
        terminal: Option<TerminalView>,
    }
    struct TestTerminalParts {
        terminal: TerminalView,
        input: mpsc::Receiver<Vec<u8>>,
        resize: watch::Receiver<TerminalSize>,
        output: mpsc::Sender<Vec<u8>>,
    }

    fn test_terminal(runtime: &Runtime) -> TestTerminalParts {
        let initial_size = TerminalSize {
            rows: 24,
            columns: 80,
        };
        let (resize_sender, resize_receiver) = watch::channel(initial_size);
        let (input, input_receiver) = mpsc::channel(TERMINAL_INPUT_CAPACITY);
        let (output_sender, output) = mpsc::channel(64);
        let io_task = runtime.spawn(std::future::pending());
        let resize_task = runtime.spawn(std::future::pending());
        TestTerminalParts {
            terminal: TerminalView {
                model: TerminalModel::new(1, 1),
                input: Some(input),
                output,
                resize_sender: Some(resize_sender),
                resize_failed: Arc::new(AtomicBool::new(false)),
                io_task: Some(io_task),
                resize_task: Some(resize_task),
                ended: false,
                paste_authorization: None,
                input_notice: None,
            },
            input: input_receiver,
            resize: resize_receiver,
            output: output_sender,
        }
    }

    fn harness_for(terminal: TerminalView) -> Harness<'static, TestTerminalUi> {
        Harness::new_ui_state(
            |ui, state: &mut TestTerminalUi| {
                if state
                    .terminal
                    .as_mut()
                    .is_some_and(|terminal| !terminal.show(ui.ctx()))
                {
                    state.terminal = None;
                }
            },
            TestTerminalUi {
                terminal: Some(terminal),
            },
        )
    }

    #[test]
    fn focused_terminal_sends_ctrl_c_and_resizes_then_closes_exec_only() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let parts = test_terminal(&runtime);
        let mut input = parts.input;
        let resize = parts.resize;
        let output = parts.output;
        output.try_send(b"\x1b[?2004h".to_vec()).unwrap();
        let mut harness = harness_for(parts.terminal);
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Focus terminal")
            .click();
        harness.run();
        harness.event(egui::Event::Paste("unrequested".to_owned()));
        harness.run();
        assert!(matches!(
            input.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        harness
            .get_by_role_and_label(Role::Button, "Paste from clipboard")
            .click();
        harness.run();
        harness.event(egui::Event::Paste("button paste".to_owned()));
        harness.run();
        assert_eq!(
            input.try_recv().unwrap(),
            b"\x1b[200~button paste\x1b[201~".to_vec()
        );
        harness.key_down_modifiers(egui::Modifiers::CTRL, egui::Key::C);
        harness.run();
        assert_eq!(input.try_recv().unwrap(), vec![0x03]);
        harness.event_modifiers(egui::Event::Copy, egui::Modifiers::CTRL);
        harness.run();
        assert_eq!(input.try_recv().unwrap(), vec![0x03]);
        let resized = resize.borrow();
        assert!(resized.rows != 1 || resized.columns != 1);
        drop(resized);
        harness
            .get_by_role_and_label(Role::Button, "Close window")
            .click();
        harness.run();
        assert!(harness.state().terminal.is_none());
        assert!(matches!(
            input.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn page_up_scrolls_back_and_output_eof_disables_input_accessibly() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let parts = test_terminal(&runtime);
        let mut input = parts.input;
        let output = parts.output;
        let mut harness = harness_for(parts.terminal);
        let history = (0..40)
            .map(|row| format!("history-{row}\r\n"))
            .collect::<String>();
        output.try_send(history.into_bytes()).unwrap();
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Focus terminal")
            .click();
        harness.run();
        let before_scroll = harness.state().terminal.as_ref().unwrap().model.contents();
        harness.key_down(egui::Key::PageUp);
        harness.run();
        let terminal = harness.state().terminal.as_ref().unwrap();
        assert!(terminal.model.scrollback_position() > 0);
        assert_ne!(terminal.model.contents(), before_scroll);
        assert!(matches!(
            input.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        harness.key_down(egui::Key::PageDown);
        harness.run();
        let terminal = harness.state().terminal.as_ref().unwrap();
        assert_eq!(terminal.model.scrollback_position(), 0);
        assert_eq!(terminal.model.contents(), before_scroll);

        drop(output);
        harness.run();
        let _ended = harness
            .get_by_role_and_label(Role::Label, "Terminal session ended. Input is disabled.");
        let terminal = harness.state().terminal.as_ref().unwrap();
        assert!(terminal.ended);
        assert!(terminal.input.is_none());
        assert!(terminal.resize_sender.is_none());
        assert!(terminal.io_task.is_none());
        assert!(terminal.resize_task.is_none());
    }
    #[test]
    fn paste_requires_fresh_focus_and_wraps_bracketed_mode() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let parts = test_terminal(&runtime);
        let mut terminal = parts.terminal;
        let mut input = parts.input;
        terminal.model.feed(b"\x1b[?2004h");
        let context = egui::Context::default();
        assert!(!terminal.deliver_paste_event("unrequested", false, 1, Instant::now(),));
        terminal.request_paste(&context);
        let frame = context.cumulative_frame_nr() + 1;
        let now = Instant::now();
        assert!(!terminal.deliver_paste_event("lost focus", false, frame, now));
        assert!(!terminal.deliver_paste_event("after cancellation", false, frame, now));
        assert!(matches!(
            input.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        terminal.request_paste(&context);
        let frame = context.cumulative_frame_nr() + 1;
        assert!(terminal.deliver_paste_event("paste", true, frame, Instant::now()));
        assert_eq!(
            input.try_recv().unwrap(),
            b"\x1b[200~paste\x1b[201~".to_vec()
        );
        terminal.request_paste(&context);
        let frame = context.cumulative_frame_nr() + 1;
        let expired = Instant::now() + Duration::from_secs(4);
        assert!(!terminal.deliver_paste_event("expired", true, frame, expired));
        assert!(matches!(
            input.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn alternate_screen_keeps_page_up_and_wheel_out_of_scrollback() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let parts = test_terminal(&runtime);
        let mut input = parts.input;
        let output = parts.output;
        output.try_send(b"\x1b[?1049h".to_vec()).unwrap();
        let mut harness = harness_for(parts.terminal);
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Focus terminal")
            .click();
        harness.run();
        harness.key_down(egui::Key::PageUp);
        harness.run();
        assert_eq!(input.try_recv().unwrap(), b"\x1b[5~".to_vec());
        assert_eq!(
            harness
                .state()
                .terminal
                .as_ref()
                .unwrap()
                .model
                .scrollback_position(),
            0
        );

        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 10.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        assert_eq!(
            harness
                .state()
                .terminal
                .as_ref()
                .unwrap()
                .model
                .scrollback_position(),
            0
        );
        assert!(matches!(
            input.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
}
