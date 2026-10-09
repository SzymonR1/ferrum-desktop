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
mod audit;
mod images;
mod inventory;
mod logs;
mod retry;
mod search;
mod volumes;
use ferrum_core::VolumeSummary;
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
    ImagePullProgress, ImageSummary,
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
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};
use tracing_subscriber::EnvFilter;

const EVENT_CHANNEL_CAPACITY: usize = 4;
const ROW_HEIGHT: f32 = 34.0;
const CELL_HEIGHT: f32 = 28.0;

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
        result: Result<ferrum_core::VolumeDetails, EngineError>,
    },
    ImagePullProgress {
        generation: u64,
        progress: ImagePullProgress,
    },
    ImagePullFinished {
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

    ImageInspected {
        generation: u64,
        inspection_generation: u64,
        id: String,
        result: Result<ImageDetails, EngineError>,
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

struct FerrumApp {
    runtime: Runtime,
    sender: mpsc::Sender<EngineEvent>,
    receiver: mpsc::Receiver<EngineEvent>,
    connection: ConnectionState,
    engine: Option<EngineClient>,
    engine_info: Option<EngineInfo>,
    containers: Vec<ContainerSummary>,
    inventory_view: InventoryView,
    page: images::AppPage,
    images: images::ImagesView,
    volumes: volumes::VolumesView,
    volume_list_task: Option<JoinHandle<()>>,
    volume_inspection_task: Option<JoinHandle<()>>,
    image_list_task: Option<JoinHandle<()>>,
    image_inspection_task: Option<JoinHandle<()>>,
    // Hold the reader until the terminal response event releases the mutation gate.
    image_pull_task: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    image_pull_progress_reporting: Option<Arc<AtomicBool>>,
    // Keep a submitted tag POST alive across page invalidation; aborting it
    // would not undo a mutation the engine may already have accepted.
    image_tag_task: Option<JoinHandle<()>>,
    // A submitted removal may already have reached Docker; never abort it as
    // though that could roll back the mutation.
    image_remove_task: Option<JoinHandle<()>>,

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
    settings_store: settings::SettingsStore,
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

        let settings_store = settings::SettingsStore::new();
        let (settings, settings_error) = settings_store.load();
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
            inventory_view: InventoryView::default(),
            page: images::AppPage::default(),
            images: images::ImagesView::default(),
            volumes: volumes::VolumesView::default(),
            volume_list_task: None,
            volume_inspection_task: None,
            image_list_task: None,
            image_inspection_task: None,
            image_pull_task: None,
            #[cfg(target_os = "linux")]
            image_pull_progress_reporting: None,
            image_tag_task: None,
            image_remove_task: None,
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

    fn invalidate_volumes(&mut self) {
        if let Some(task) = self.volume_list_task.take() {
            task.abort();
        }
        if let Some(task) = self.volume_inspection_task.take() {
            task.abort();
        }
        self.volumes.invalidate();
    }
    fn load_volumes(&mut self, context: egui::Context) {
        if self.loading
            || !cfg!(target_os = "linux")
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if let Some(task) = self.volume_list_task.take() {
            task.abort();
        }
        if let Some(task) = self.volume_inspection_task.take() {
            task.abort();
        }
        let generation = self.volumes.begin_list();
        let sender = self.sender.clone();
        self.volume_list_task = Some(self.runtime.spawn(async move {
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
    fn inspect_volume(&mut self, name: String, context: egui::Context) {
        if !cfg!(target_os = "linux")
            || self.loading
            || !matches!(&self.connection, ConnectionState::Connected)
        {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let (generation, inspection_generation, name) = self.volumes.begin_inspection(name);
        let sender = self.sender.clone();
        self.volume_inspection_task = Some(self.runtime.spawn(async move {
            let result = engine.inspect_volume(&name).await;
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
    fn invalidate_images(&mut self) {
        self.cancel_image_read_tasks();
        #[cfg(target_os = "linux")]
        if let Some(reporting) = &self.image_pull_progress_reporting {
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

    fn image_mutation_busy(&self) -> bool {
        self.image_pull_task.is_some()
            || self.image_tag_task.is_some()
            || self.image_remove_task.is_some()
            || self.images.has_mutation_in_flight()
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
        self.image_tag_task = Some(self.runtime.spawn(async move {
            let result = engine
                .tag_image(&request.image_id, &request.repository, &request.tag)
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
        self.image_remove_task = Some(self.runtime.spawn(async move {
            let result = engine.remove_image(&request.image_id).await;
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

    fn inspect_image(&mut self, request: images::ImageInspectionRequest, context: egui::Context) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        if let Some(task) = self.image_inspection_task.take() {
            task.abort();
        }
        let sender = self.sender.clone();
        self.image_inspection_task = Some(self.runtime.spawn(async move {
            let result = engine.inspect_image(&request.id).await;
            if sender
                .send(EngineEvent::ImageInspected {
                    generation: request.list_generation,
                    inspection_generation: request.inspection_generation,
                    id: request.id,
                    result,
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
        if page == images::AppPage::Images && !self.loading && self.engine.is_some() {
            self.load_images(context.clone());
        }
        if page == images::AppPage::Volumes && !self.loading && self.engine.is_some() {
            self.load_volumes(context);
        }
    }

    fn connect(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();

        self.invalidate_images();
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
        if self.loading || self.action_busy() {
            return;
        }
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();
        self.cancel_inspection();
        self.invalidate_volumes();
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
            self.load_volumes(context);
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
                            self.invalidate_volumes();
                            self.engine = None;
                            self.engine_info = None;
                            self.containers.clear();
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
                    if generation == self.volumes.generation {
                        self.volume_list_task = None;
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
                        result.map_err(|error| error.to_string()),
                    );
                    if generation == self.volumes.generation
                        && inspection_generation == self.volumes.inspection_generation
                    {
                        self.volume_inspection_task = None;
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
                } => {
                    if self.images.complete_inspection(
                        generation,
                        inspection_generation,
                        &id,
                        result,
                    ) {
                        self.image_inspection_task = None;
                    }
                }
                EngineEvent::Refreshed(result) => {
                    self.loading = false;
                    match result {
                        Ok(containers) => {
                            self.containers = containers;
                            self.inventory_view.inventory_changed(&self.containers);
                            self.refresh_error = None;
                            selected_container_changed |= self
                                .inspection
                                .as_ref()
                                .is_some_and(|inspection| !inspection.action_busy);
                        }
                        Err(error) => self.refresh_error = Some(error.to_string()),
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
                    self.event_refresh_pending = true;
                }
                EngineEvent::ContainerChanged {
                    generation,
                    container_id,
                } if generation == self.event_generation => {
                    self.event_refresh_pending = true;
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
                            self.containers = containers;
                            self.inventory_view.inventory_changed(&self.containers);
                            self.refresh_error = None;
                            selected_container_changed |= self
                                .inspection
                                .as_ref()
                                .is_some_and(|inspection| !inspection.action_busy);
                        }
                        Err(error) => self.refresh_error = Some(error.to_string()),
                    }
                }
                EngineEvent::EventStreamEnded { generation }
                    if generation == self.event_generation =>
                {
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
        self.invalidate_volumes();
        if let Some(task) = self.action_task.take() {
            task.abort();
        }
        self.selected_context = engine.selected_context_name().map(str::to_owned);
        self.engine = Some(engine.clone());
        self.engine_info = Some(info);
        self.containers = containers;
        self.inventory_view.inventory_changed(&self.containers);
        self.connection = ConnectionState::Connected;
        self.refresh_error = None;
        self.context_error = None;
        self.start_event_stream(engine, context.clone());
        if self.page == images::AppPage::Images {
            self.load_images(context.clone());
        }
        if self.page == images::AppPage::Volumes {
            self.load_volumes(context);
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
        self.action_task = Some(self.runtime.spawn(async move {
            let result = engine.perform_container_action(&id, action).await;
            audit::record_container_action_result(&id, action, result.is_ok());
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

    fn draw_volumes(&mut self, ui: &mut egui::Ui) -> Option<String> {
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
        let inspection = self.images.show(
            ui,
            connected,
            message,
            cfg!(target_os = "linux") && connected,
            mutation_busy,
        );

        if self.images.take_pull_requested() {
            #[cfg(target_os = "linux")]
            self.start_image_pull(ui.ctx().clone());
        }
        if self.images.take_hide_progress_requested() {
            self.hide_image_pull_progress();
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
        let page_loading = self.loading
            || (self.page == images::AppPage::Images
                && (self.images.is_loading()
                    || self.images.is_pulling()
                    || self.images.is_tagging()
                    || self.images.is_removing()
                    || self.image_tag_task.is_some()
                    || self.image_remove_task.is_some()));

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
                    ui.add_enabled_ui(!page_loading && !self.action_busy(), |ui| {
                        let selected_name = match self.selected_context.as_deref() {
                            Some(name) => self
                                .contexts
                                .iter()
                                .find(|context| context.name() == name)
                                .map(DockerContext::display_name)
                                .unwrap_or("Selected Docker context"),
                            None => "Automatic local discovery",
                        };
                        egui::ComboBox::from_id_salt("docker-context-selector")
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
                                            format!("{} (invalid endpoint)", context.display_name())
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
                                        *context_requested = Some(Some(context.name().to_owned()));
                                    }
                                }
                            });
                    });
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
                        let name_button = ui
                            .add_enabled_ui(!self.loading && !self.action_busy(), |ui| {
                                ui.add_sized(
                                    [name_width, CELL_HEIGHT],
                                    egui::Button::new(RichText::new(&container.name).strong())
                                        .frame(false),
                                )
                            })
                            .inner;
                        if name_button.clicked() {
                            selected = Some(index);
                        }
                        name_button.on_hover_text(&container.id);
                        let image = container.image.as_deref().unwrap_or("Not reported");
                        let image_response = ui.add_sized(
                            [image_width, CELL_HEIGHT],
                            egui::Label::new(image)
                                .truncate()
                                .show_tooltip_when_elided(true),
                        );
                        image_response.on_hover_text(image);
                        let state_text = container
                            .state
                            .as_ref()
                            .map_or("Not reported", container_state_display);
                        ui.add_sized(
                            [state_width, CELL_HEIGHT],
                            egui::Label::new(
                                RichText::new(state_text)
                                    .color(state_color(container.state.as_ref())),
                            )
                            .truncate()
                            .show_tooltip_when_elided(true),
                        );
                        let status = container.status.as_deref().unwrap_or("Not reported");
                        ui.add_sized(
                            [status_width, CELL_HEIGHT],
                            egui::Label::new(status)
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
                ui.heading(&inspection.container_name);
                detail_row(ui, "ID", Some(&inspection.container_id));
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
                    detail_row(ui, "Name", details.name.as_deref());
                    detail_row(ui, "Image", details.image.as_deref());
                    detail_row(ui, "Created", details.created.as_deref());
                    detail_row(
                        ui,
                        "State",
                        details.state.as_ref().map(container_state_display),
                    );
                    detail_row(
                        ui,
                        "Restart count",
                        inspection.restart_count_display.as_deref(),
                    );
                    detail_row(ui, "Exit code", inspection.exit_code_display.as_deref());
                    detail_row(ui, "Started at", details.started_at.as_deref());
                    detail_row(ui, "Finished at", details.finished_at.as_deref());
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
                                    detail_row(ui, "Type", mount.kind.as_deref());
                                    detail_row(ui, "Source", mount.source.as_deref());
                                    detail_row(ui, "Destination", mount.destination.as_deref());
                                    detail_row(ui, "Mode", mount.mode.as_deref());
                                    detail_row(
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

impl Drop for FerrumApp {
    fn drop(&mut self) {
        self.stop_event_stream();
        #[cfg(target_os = "linux")]
        self.cancel_log_stream();
        #[cfg(target_os = "linux")]
        self.cancel_terminal_session();
        self.cancel_image_read_tasks();
        self.invalidate_volumes();
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
            self.event_refresh_pending = false;
            self.refresh(context.clone());
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
        let mut volume_selection = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 18)))
            .show(ui, |ui| match self.page {
                images::AppPage::Containers => inspect_index = self.draw_inventory(ui),
                images::AppPage::Images => image_inspection = self.draw_images(ui),
                images::AppPage::Volumes => volume_selection = self.draw_volumes(ui),
            });
        if let Some(index) = inspect_index {
            let container = &self.containers[index];
            let id = container.id.clone();
            let name = container.name.clone();
            self.inspect_container(&id, &name, context.clone());
        }
        if let Some(request) = image_inspection {
            self.inspect_image(request, context.clone());
        }
        if let Some(name) = volume_selection {
            self.inspect_volume(name, context.clone());
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
            inventory_view: InventoryView::default(),
            page: images::AppPage::default(),
            images: images::ImagesView::default(),
            volumes: volumes::VolumesView::default(),
            volume_list_task: None,
            volume_inspection_task: None,
            image_list_task: None,
            image_inspection_task: None,
            image_pull_task: None,
            #[cfg(target_os = "linux")]
            image_pull_progress_reporting: None,
            image_tag_task: None,
            image_remove_task: None,
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
            settings_store: settings::SettingsStore::new(),
            settings_draft: settings.clone(),
            settings,
            settings_open: false,
            settings_error: None,
        };
        (app, sender)
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
            "An image operation is in progress; wait for it to finish before starting another pull, tag, or removal.",
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
