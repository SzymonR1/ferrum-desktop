use std::{error::Error, time::Duration};

use eframe::egui::{self, Color32, RichText};
use ferrum_core::{ContainerState, ContainerSummary};
use ferrum_engine::{EngineClient, EngineError, EngineInfo};
use tokio::{runtime::Runtime, sync::mpsc};
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
}

struct FerrumApp {
    runtime: Runtime,
    sender: mpsc::Sender<EngineEvent>,
    receiver: mpsc::Receiver<EngineEvent>,
    connection: ConnectionState,
    engine: Option<EngineClient>,
    engine_info: Option<EngineInfo>,
    containers: Vec<ContainerSummary>,
    loading: bool,
    refresh_error: Option<String>,
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

        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let mut app = Self {
            runtime,
            sender,
            receiver,
            connection: ConnectionState::Connecting,
            engine: None,
            engine_info: None,
            containers: Vec::new(),
            loading: false,
            refresh_error: None,
        };
        app.connect(creation_context.egui_ctx.clone());
        app
    }

    fn connect(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }

        self.loading = true;
        self.connection = ConnectionState::Connecting;
        self.refresh_error = None;
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
            self.loading = false;
            match event {
                EngineEvent::Connected(Ok((engine, info, containers))) => {
                    self.engine = Some(engine);
                    self.engine_info = Some(info);
                    self.containers = containers;
                    self.connection = ConnectionState::Connected;
                    self.refresh_error = None;
                }
                EngineEvent::Connected(Err(error)) => {
                    self.engine = None;
                    self.engine_info = None;
                    self.containers.clear();
                    self.connection = ConnectionState::Disconnected(error.to_string());
                }
                EngineEvent::Refreshed(Ok(containers)) => {
                    self.containers = containers;
                    self.refresh_error = None;
                }
                EngineEvent::Refreshed(Err(error)) => {
                    self.refresh_error = Some(error.to_string());
                }
            }
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

    fn draw_header(&self, ui: &mut egui::Ui, refresh_requested: &mut bool) {
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
                    if let Some(info) = &self.engine_info {
                        ui.separator();
                        ui.label("Docker-compatible API");
                        ui.label(RichText::new(info.endpoint_label).small().weak());
                        ui.label(RichText::new(&info.version).strong());
                        ui.label(RichText::new("API").weak());
                        ui.label(&info.api_version);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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

    fn draw_inventory(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.refresh_error {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(227, 113, 102), "Refresh failed");
                ui.label("Showing the last successful inventory.");
            });
            ui.label(error);
            ui.add_space(10.0);
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
            }
            ConnectionState::Disconnected(message) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(92.0);
                    ui.colored_label(Color32::from_rgb(227, 113, 102), "Connection unavailable");
                    ui.add_space(8.0);
                    ui.label(message);
                });
            }
            ConnectionState::Connected if self.containers.is_empty() => {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.heading("No containers found");
                    ui.label("The connected engine reported an empty inventory, including stopped containers.");
                });
            }
            ConnectionState::Connected => self.draw_container_table(ui),
        }
    }

    fn draw_container_table(&self, ui: &mut egui::Ui) {
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

        egui::ScrollArea::vertical().show_rows(
            ui,
            ROW_HEIGHT,
            self.containers.len(),
            |ui, visible_rows| {
                for index in visible_rows {
                    let container = &self.containers[index];
                    ui.horizontal(|ui| {
                        let name_response = ui.add_sized(
                            [name_width, CELL_HEIGHT],
                            egui::Label::new(RichText::new(&container.name).strong())
                                .truncate()
                                .show_tooltip_when_elided(true),
                        );
                        name_response.on_hover_text(&container.id);

                        let image = container.image.as_deref().unwrap_or("Image not reported");
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
                            .map_or("State not reported", ContainerState::label);
                        ui.add_sized(
                            [state_width, CELL_HEIGHT],
                            egui::Label::new(
                                RichText::new(state_text)
                                    .color(state_color(container.state.as_ref())),
                            )
                            .truncate()
                            .show_tooltip_when_elided(true),
                        );

                        let status = container.status.as_deref().unwrap_or("Status not reported");
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
    }
}

impl eframe::App for FerrumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive_events();
        let context = ui.ctx().clone();
        self.draw_sidebar(ui);

        let mut refresh_requested = false;
        self.draw_header(ui, &mut refresh_requested);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 18)))
            .show(ui, |ui| self.draw_inventory(ui));

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
