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
mod inventory;
mod logs;
mod retry;
mod search;
#[cfg(target_os = "linux")]
const LOG_CHANNEL_CAPACITY: usize = 16;
mod settings;

use std::{error::Error, time::Duration};

use eframe::egui::{self, Color32, RichText};
#[cfg(target_os = "linux")]
use ferrum_core::ContainerLogLine;
use ferrum_core::{ContainerAction, ContainerDetails, ContainerState, ContainerSummary};
#[cfg(target_os = "linux")]
use ferrum_engine::ContainerLogsOptions;
use ferrum_engine::{ContextTransport, DockerContext, EngineClient, EngineError, EngineInfo};
use inventory::InventoryView;
use logs::{LogBuffer, LogRetryBudget};
use retry::RetryBudget;
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

struct FerrumApp {
    runtime: Runtime,
    sender: mpsc::Sender<EngineEvent>,
    receiver: mpsc::Receiver<EngineEvent>,
    connection: ConnectionState,
    engine: Option<EngineClient>,
    engine_info: Option<EngineInfo>,
    containers: Vec<ContainerSummary>,
    inventory_view: InventoryView,
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
    log_options: ContainerLogsOptions,
    #[cfg(target_os = "linux")]
    log_notice: Option<String>,
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
        let mut app = Self {
            runtime,
            sender,
            receiver,
            #[cfg(target_os = "linux")]
            log_sender,
            #[cfg(target_os = "linux")]
            log_receiver,
            #[cfg(target_os = "linux")]
            log_task: None,
            #[cfg(target_os = "linux")]
            log_generation: 0,
            #[cfg(target_os = "linux")]
            log_buffer: LogBuffer::new(),
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
    fn action_busy(&self) -> bool {
        self.inspection
            .as_ref()
            .is_some_and(|inspection| inspection.action_busy)
    }
    fn connect(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }

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
        self.cancel_inspection();
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
                        Err(error) => self.context_error = Some(error.to_string()),
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
    }

    fn apply_connection_success(
        &mut self,
        engine: EngineClient,
        info: EngineInfo,
        containers: Vec<ContainerSummary>,
        context: egui::Context,
    ) {
        self.cancel_inspection();
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
        self.start_event_stream(engine, context);
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
        #[cfg(target_os = "linux")]
        self.cancel_log_stream();
        #[cfg(target_os = "linux")]
        self.log_buffer.reset();
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
        self.log_started = false;
        self.log_reconnected = false;
    }

    #[cfg(target_os = "linux")]
    fn start_log_stream(
        &mut self,
        id: &str,
        tty: bool,
        options: ContainerLogsOptions,
        context: egui::Context,
    ) {
        self.cancel_log_stream();
        let Some(engine) = self.engine.clone() else {
            self.log_started = true;
            self.log_notice = Some("The engine connection is unavailable.".into());
            return;
        };
        let generation = self.log_generation;
        let sender = self.log_sender.clone();
        let container_id = id.to_owned();
        self.log_options = options;
        self.log_started = true;
        self.log_notice = Some("Loading container logs…".into());
        self.log_reconnected = false;
        self.log_task = Some(self.runtime.spawn(async move {
            let mut retries = LogRetryBudget::default();
            let mut has_reconnected = false;
            loop {
                let mut stream = match engine
                    .open_container_logs(&container_id, tty, options)
                    .await
                {
                    Ok(stream) => {
                        if has_reconnected {
                            if sender.send(LogEvent::Reset { generation }).await.is_err() {
                                return;
                            }
                            context.request_repaint();
                            // Reload the recent tail after every successful reconnect.
                        }
                        stream
                    }
                    Err(error) => {
                        if !options.follow {
                            let _ = sender
                                .send(LogEvent::Ended {
                                    generation,
                                    error: Some(error.to_string()),
                                })
                                .await;
                            context.request_repaint();
                            return;
                        }
                        if retries.wait_next().await {
                            has_reconnected = true;
                            continue;
                        }
                        let _ = sender
                            .send(LogEvent::Ended {
                                generation,
                                error: Some(error.to_string()),
                            })
                            .await;
                        context.request_repaint();
                        return;
                    }
                };
                let disconnect_error = loop {
                    match stream.next_line().await {
                        Ok(Some(line)) => {
                            // The per-view budget is not renewed by replayed historical lines.
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
                            if !options.follow {
                                let _ = sender
                                    .send(LogEvent::Ended {
                                        generation,
                                        error: Some(error.to_string()),
                                    })
                                    .await;
                                context.request_repaint();
                                return;
                            }
                            break error.to_string();
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
                        error: Some(disconnect_error),
                    })
                    .await;
                context.request_repaint();
                return;
            }
        }));
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
    fn draw_sidebar(&self, ui: &mut egui::Ui) {
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
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.colored_label(Color32::from_rgb(231, 139, 90), "▏");
                    ui.label(RichText::new("Containers").strong());
                });
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
    }

    fn draw_header(
        &mut self,
        ui: &mut egui::Ui,
        refresh_requested: &mut bool,
        settings_requested: &mut bool,
        context_requested: &mut Option<Option<String>>,
    ) {
        egui::Panel::top("header")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 14)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Containers");
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
                    ui.add_enabled_ui(!self.loading && !self.action_busy(), |ui| {
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
                        let button_text = if self.loading {
                            "Loading"
                        } else if self.engine.is_some() {
                            "Refresh"
                        } else {
                            "Retry connection"
                        };
                        if ui
                            .add_enabled(
                                !self.loading && !self.action_busy(),
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
        egui::Window::new("Container details")
            .id(egui::Id::new("container-inspection"))
            .open(&mut open)
            .resizable(true)
            .show(&context, |ui| {
                ui.heading(&inspection.container_name);
                detail_row(ui, "ID", Some(&inspection.container_id));
                #[cfg(target_os = "linux")]
                if ui.button("Container logs").clicked() {
                    open_logs = true;
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
        if open_logs && let Some(inspection) = self.inspection.as_mut() {
            inspection.logs_open = true;
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
        let id = inspection.container_id.clone();
        let tty = inspection.details.as_ref().and_then(|details| details.tty);
        let running = inspection
            .details
            .as_ref()
            .and_then(|details| details.state.as_ref())
            .is_some_and(|state| *state == ContainerState::Running);
        if !self.log_started {
            self.log_options.follow = running;
        }
        if tty == Some(true) {
            self.log_stdout = true;
            self.log_stderr = true;
        }
        let mut open = true;
        let mut refresh = false;
        let mut options_changed = false;
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
                        .selected_text(format!("Last {tail} lines"))
                        .show_ui(ui, |ui| {
                            for choice in [100, 500, 1_000, 5_000] {
                                ui.selectable_value(
                                    &mut tail,
                                    choice,
                                    format!("Last {choice} lines"),
                                );
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
                if let Some(notice) = &self.log_notice {
                    if notice.starts_with("Log stream ended:") {
                        ui.colored_label(Color32::from_rgb(224, 174, 109), notice);
                    } else {
                        ui.label(notice);
                    }
                }
                if self.log_reconnected {
                    ui.colored_label(
                        Color32::from_rgb(130, 194, 145),
                        "Reconnected; displaying recent container output.",
                    );
                }
                if self.log_buffer.dropped_lines() > 0 {
                    ui.colored_label(
                        Color32::from_rgb(224, 174, 109),
                        format!(
                            "{} older log lines dropped from the in-memory buffer.",
                            self.log_buffer.dropped_lines()
                        ),
                    );
                }
                let visible_count = self.log_buffer.visible_count();
                if visible_count == 0 {
                    if self.log_started && self.log_notice.is_none() {
                        ui.label("No log lines match the current filters.");
                    } else if !self.log_started {
                        ui.label("Load logs to view recent container output.");
                    }
                } else {
                    egui::ScrollArea::both().show_rows(ui, 20.0, visible_count, |ui, rows| {
                        for row in rows {
                            if let Some(entry) = self.log_buffer.visible_entry_at(row) {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&entry.text).monospace())
                                        .extend(),
                                );
                            }
                        }
                    });
                }
            });
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
        if !self.log_started {
            self.start_log_stream(&id, tty.unwrap_or(false), self.log_options, context);
        } else if options_changed || refresh {
            self.log_buffer.reset();
            self.start_log_stream(&id, tty.unwrap_or(false), self.log_options, context);
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
        if self.event_refresh_pending && !self.loading {
            self.event_refresh_pending = false;
            self.refresh(context.clone());
        }
        self.draw_sidebar(ui);
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
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 18)))
            .show(ui, |ui| inspect_index = self.draw_inventory(ui));
        if let Some(index) = inspect_index {
            let container = &self.containers[index];
            let id = container.id.clone();
            let name = container.name.clone();
            self.inspect_container(&id, &name, context.clone());
        }
        self.draw_inspection(context.clone());
        self.draw_settings(context.clone());

        if refresh_requested {
            self.refresh(context.clone());
        }

        if self.loading || self.action_busy() {
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
}
#[cfg(test)]
mod log_retry_budget_tests {
    use super::*;

    #[test]
    fn retries_are_bounded_for_one_view_and_refresh_starts_a_fresh_budget() {
        let mut retry = logs::LogRetryBudget::default();

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
        let mut refreshed = logs::LogRetryBudget::default();
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
    use ferrum_core::ContainerLogSource;

    fn app_with_open_inspection() -> (FerrumApp, mpsc::Sender<EngineEvent>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        #[cfg(target_os = "linux")]
        let (log_sender, log_receiver) = mpsc::channel(LOG_CHANNEL_CAPACITY);
        let settings = settings::Settings::default();
        let app = FerrumApp {
            runtime,
            sender: sender.clone(),
            receiver,
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
    #[test]
    fn reconnect_resets_the_tail_and_preserves_search_filters() {
        let (mut app, _) = app_with_open_inspection();
        app.log_search = "fresh".to_owned();
        app.log_buffer.append(ContainerLogLine {
            source: ContainerLogSource::Stdout,
            text: "old tail".to_owned(),
        });
        app.log_buffer.set_filters(&app.log_search, true, true);
        assert!(app.log_buffer.visible_indices().is_empty());

        let generation = app.log_generation;
        assert!(
            app.log_sender
                .try_send(LogEvent::Reset { generation })
                .is_ok()
        );
        app.receive_events(egui::Context::default());

        assert!(app.log_buffer.entries().is_empty());
        assert!(app.log_reconnected);
        assert_eq!(app.log_search, "fresh");

        assert!(
            app.log_sender
                .try_send(LogEvent::Line {
                    generation,
                    line: ContainerLogLine {
                        source: ContainerLogSource::Stdout,
                        text: "fresh output".to_owned(),
                    },
                })
                .is_ok()
        );
        app.receive_events(egui::Context::default());
        assert_eq!(app.log_buffer.visible_indices(), vec![0]);
        assert_eq!(app.log_buffer.entries()[0].text, "fresh output");
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
