mod inventory;
mod settings;

use std::{error::Error, time::Duration};

use eframe::egui::{self, Color32, RichText};
use ferrum_core::{ContainerDetails, ContainerState, ContainerSummary};
use ferrum_engine::{ContextTransport, DockerContext, EngineClient, EngineError, EngineInfo};
use inventory::InventoryView;
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
    inspection_task: Option<JoinHandle<()>>,
    next_inspection_generation: u64,
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
        let mut app = Self {
            runtime,
            sender,
            receiver,
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
        if self.loading {
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

    fn receive_events(&mut self) {
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
                            self.apply_connection_success(engine, info, containers)
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
                            self.apply_connection_success(engine, info, containers)
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
            }
        }
    }

    fn apply_connection_success(
        &mut self,
        engine: EngineClient,
        info: EngineInfo,
        containers: Vec<ContainerSummary>,
    ) {
        self.cancel_inspection();
        self.selected_context = engine.selected_context_name().map(str::to_owned);
        self.engine = Some(engine);
        self.engine_info = Some(info);
        self.containers = containers;
        self.inventory_view.inventory_changed(&self.containers);
        self.connection = ConnectionState::Connected;
        self.refresh_error = None;
        self.context_error = None;
    }

    fn cancel_inspection(&mut self) {
        if let Some(task) = self.inspection_task.take() {
            task.abort();
        }
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
        });
        if let Some(engine) = self.engine.clone() {
            let sender = self.sender.clone();
            let container_id = container_id.to_owned();
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
                    ui.add_enabled_ui(!self.loading, |ui| {
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
                            .add_enabled(!self.loading, egui::Button::new(button_text))
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
                            .add_enabled_ui(!self.loading, |ui| {
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
                            .map_or("Not reported", ContainerState::label);
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
        let mut open = true;
        let mut close = false;
        egui::Window::new("Container details")
            .id(egui::Id::new("container-inspection"))
            .open(&mut open)
            .resizable(true)
            .show(&context, |ui| {
                ui.heading(&inspection.container_name);
                detail_row(ui, "ID", Some(&inspection.container_id));
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
                        details.state.as_ref().map(ContainerState::label),
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
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        if !open || close {
            self.cancel_inspection();
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
        if let Some(task) = self.inspection_task.take() {
            task.abort();
        }
    }
}

impl eframe::App for FerrumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive_events();
        let context = ui.ctx().clone();
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

        if self.loading {
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
