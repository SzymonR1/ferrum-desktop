use crate::{display::escape_untrusted_display_text, search::CaseInsensitiveQuery};
use eframe::egui;
use ferrum_core::{ContainerState, ContainerSummary, NetworkDetails, NetworkSummary};

const ROW_HEIGHT: f32 = 34.0;
const MAX_NETWORK_NAME_BYTES: usize = 255;

pub(super) enum NetworksAction {
    Inspect(String),
    Create {
        generation: u64,
        name: String,
    },
    Remove {
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        id: String,
        name: String,
    },
    Attach(NetworkAttachmentOperation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NetworkAttachmentOperation {
    pub(super) generation: u64,
    pub(super) inspection_generation: u64,
    pub(super) mutation_generation: u64,
    pub(super) connect: bool,
    pub(super) network_id: String,
    pub(super) container_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttachmentConfirmation {
    generation: u64,
    inspection_generation: u64,
    mutation_generation: u64,
    connect: bool,
    network_id: String,
    container_id: String,
    network_name: String,
    container_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NetworkRemovalConfirmation {
    generation: u64,
    inspection_generation: u64,
    id: String,
    name: String,
}

fn valid_network_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NETWORK_NAME_BYTES
        && !name.trim().is_empty()
        && !name.chars().any(char::is_control)
}

fn bool_label(value: Option<bool>) -> Option<&'static str> {
    value.map(|value| if value { "Yes" } else { "No" })
}
fn predefined_network(name: &str) -> bool {
    matches!(name, "bridge" | "host" | "none")
}

fn valid_network_removal_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_full_container_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Default)]
pub(super) struct NetworksView {
    networks: Vec<NetworkSummary>,
    visible: Vec<usize>,
    query: String,
    matcher: CaseInsensitiveQuery,
    selected: Option<String>,
    details: Option<NetworkDetails>,
    attached_container_count_display: Option<String>,
    ips_in_use_display: Option<String>,
    dynamic_ips_available_display: Option<String>,
    create_name: String,
    create_generation: u64,
    creating: bool,
    create_success: Option<String>,
    create_error: Option<String>,
    remove_generation: u64,
    remove_confirmation: Option<NetworkRemovalConfirmation>,
    removing: bool,
    remove_result_id: Option<String>,
    remove_result_name: Option<String>,
    remove_success: Option<String>,
    remove_error: Option<String>,
    attachment_confirmation: Option<AttachmentConfirmation>,
    attachment_generation: u64,
    attaching: bool,
    attachment_result: Option<(String, String, bool, Result<(), String>)>,

    pub(super) loading: bool,
    pub(super) loaded: bool,
    inspecting: bool,
    error: Option<String>,
    detail_error: Option<String>,
    pub(super) generation: u64,
    pub(super) inspection_generation: u64,
}

impl NetworksView {
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.create_generation = self.create_generation.wrapping_add(1);
        self.remove_generation = self.remove_generation.wrapping_add(1);
        self.networks.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.ips_in_use_display = None;
        self.dynamic_ips_available_display = None;
        self.loading = false;
        self.loaded = false;
        self.inspecting = false;
        self.creating = false;
        self.create_success = None;
        self.create_error = None;
        self.attachment_confirmation = None;
        self.attachment_result = None;
        self.remove_confirmation = None;
        self.removing = false;
        self.remove_result_id = None;
        self.remove_result_name = None;
        self.remove_success = None;
        self.remove_error = None;
        self.error = None;
        self.detail_error = None;
    }

    pub(super) fn begin_list(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.remove_generation = self.remove_generation.wrapping_add(1);
        self.loading = true;
        self.loaded = false;
        self.error = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.ips_in_use_display = None;
        self.dynamic_ips_available_display = None;
        self.selected = None;
        self.inspecting = false;
        self.removing = false;
        self.attachment_confirmation = None;
        self.attachment_result = None;
        self.remove_confirmation = None;
        self.remove_result_id = None;
        self.remove_result_name = None;
        self.networks.clear();
        self.visible.clear();
        self.detail_error = None;
        self.generation
    }

    pub(super) fn finish_list(
        &mut self,
        generation: u64,
        result: Result<Vec<NetworkSummary>, String>,
    ) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        self.loaded = true;
        match result {
            Ok(networks) => {
                self.networks = networks;
                self.error = None;
                self.rebuild_visible();
            }
            Err(error) => {
                self.networks.clear();
                self.visible.clear();
                self.error = Some(error);
            }
        }
    }

    fn begin_create(&mut self) -> Option<NetworksAction> {
        if self.has_mutation_in_flight()
            || self.remove_confirmation.is_some()
            || self.attachment_confirmation.is_some()
            || !valid_network_name(&self.create_name)
        {
            return None;
        }
        self.create_generation = self.create_generation.wrapping_add(1);
        self.creating = true;
        self.create_success = None;
        self.create_error = None;
        self.attachment_result = None;
        Some(NetworksAction::Create {
            generation: self.create_generation,
            name: self.create_name.clone(),
        })
    }

    fn local_custom_network(&self) -> Option<(&NetworkSummary, &NetworkDetails)> {
        let id = self.selected.as_deref()?;
        let listed = self.networks.iter().find(|network| network.id == id)?;
        let details = self.details.as_ref()?;
        (details.id == listed.id
            && details.name == listed.name
            && listed.scope.as_deref() == Some("local")
            && details.scope.as_deref() == Some("local")
            && !predefined_network(&listed.name)
            && valid_network_removal_id(&listed.id)
            && !self.inspecting
            && !self.loading)
            .then_some((listed, details))
    }

    fn begin_attachment_confirmation(
        &mut self,
        connect: bool,
        container_id: &str,
        containers: &[ContainerSummary],
    ) {
        if self.has_mutation_in_flight()
            || self.attachment_confirmation.is_some()
            || self.remove_confirmation.is_some()
        {
            return;
        }
        let Some((network, details)) = self.local_custom_network() else {
            return;
        };
        let Some(ids) = details.attached_container_ids.as_ref() else {
            return;
        };
        if ids.iter().any(|id| !valid_full_container_id(id)) {
            return;
        }
        let attached = ids.iter().any(|id| id == container_id);
        if attached == connect {
            return;
        }
        let Some(container) = containers.iter().find(|container| {
            container.id == container_id
                && valid_full_container_id(&container.id)
                && container.state == Some(ContainerState::Running)
        }) else {
            return;
        };
        let network_id = network.id.clone();
        let network_name = network.name.clone();
        let attached_container_id = container.id.clone();
        let container_name = container.name.clone();
        self.attachment_generation = self.attachment_generation.wrapping_add(1);
        self.attachment_result = None;
        self.attachment_confirmation = Some(AttachmentConfirmation {
            generation: self.generation,
            inspection_generation: self.inspection_generation,
            mutation_generation: self.attachment_generation,
            connect,
            network_id,
            container_id: attached_container_id,
            network_name,
            container_name,
        });
    }

    fn attachment_confirmation_is_current(
        &self,
        confirmation: &AttachmentConfirmation,
        containers: &[ContainerSummary],
    ) -> bool {
        if self.remove_confirmation.is_some() {
            return false;
        }
        let Some((network, details)) = self.local_custom_network() else {
            return false;
        };
        let Some(ids) = details.attached_container_ids.as_ref() else {
            return false;
        };
        self.generation == confirmation.generation
            && self.inspection_generation == confirmation.inspection_generation
            && network.id == confirmation.network_id
            && details.id == confirmation.network_id
            && network.name == confirmation.network_name
            && valid_full_container_id(&confirmation.container_id)
            && ids.iter().any(|id| id == &confirmation.container_id) != confirmation.connect
            && containers.iter().any(|container| {
                container.id == confirmation.container_id
                    && container.name == confirmation.container_name
                    && container.state == Some(ContainerState::Running)
            })
    }

    fn confirm_attachment(&mut self, containers: &[ContainerSummary]) -> Option<NetworksAction> {
        let confirmation = self.attachment_confirmation.take()?;
        if self.remove_confirmation.is_some()
            || !self.attachment_confirmation_is_current(&confirmation, containers)
            || self.has_mutation_in_flight()
        {
            return None;
        }
        self.attaching = true;
        self.attachment_result = None;
        Some(NetworksAction::Attach(NetworkAttachmentOperation {
            generation: confirmation.generation,
            inspection_generation: confirmation.inspection_generation,
            mutation_generation: confirmation.mutation_generation,
            connect: confirmation.connect,
            network_id: confirmation.network_id,
            container_id: confirmation.container_id,
        }))
    }

    pub(super) fn cancel_attachment_submission(&mut self, mutation_generation: u64) {
        if self.attaching && self.attachment_generation == mutation_generation {
            self.attaching = false;
        }
    }

    pub(super) fn finish_attachment(
        &mut self,
        operation: &NetworkAttachmentOperation,
        result: Result<(), String>,
    ) -> bool {
        if !self.attaching || self.attachment_generation != operation.mutation_generation {
            return false;
        }
        self.attaching = false;
        if self.generation != operation.generation
            || self.inspection_generation != operation.inspection_generation
            || self.selected.as_deref() != Some(operation.network_id.as_str())
        {
            return false;
        }
        self.details = None;
        self.attached_container_count_display = None;
        self.ips_in_use_display = None;
        self.dynamic_ips_available_display = None;
        self.attachment_result = Some((
            operation.network_id.clone(),
            operation.container_id.clone(),
            operation.connect,
            result,
        ));
        true
    }

    pub(super) fn attachment_is_pending(
        &self,
        operation: &NetworkAttachmentOperation,
        containers: &[ContainerSummary],
    ) -> bool {
        self.attaching
            && self.generation == operation.generation
            && self.inspection_generation == operation.inspection_generation
            && self.attachment_generation == operation.mutation_generation
            && self.selected.as_deref() == Some(operation.network_id.as_str())
            && valid_full_container_id(&operation.container_id)
            && containers.iter().any(|container| {
                container.id == operation.container_id
                    && container.state == Some(ContainerState::Running)
            })
            && self.remove_confirmation.is_none()
            && self.local_custom_network().is_some()
            && self.details.as_ref().is_some_and(|details| {
                details.attached_container_ids.as_ref().is_some_and(|ids| {
                    ids.iter().any(|id| id == &operation.container_id) != operation.connect
                })
            })
    }

    pub(super) fn refresh_after_attachment(
        &mut self,
        networks: Vec<NetworkSummary>,
        details: NetworkDetails,
        selected_id: &str,
    ) {
        let attachment_result = self.attachment_result.take();
        let generation = self.begin_list();
        self.finish_list(generation, Ok(networks));
        if self
            .networks
            .iter()
            .any(|network| network.id == selected_id)
            && details.id == selected_id
        {
            let (generation, inspection_generation, id) =
                self.begin_inspection(selected_id.to_owned());
            self.finish_inspection(generation, inspection_generation, &id, Ok(details));
            self.attachment_result = attachment_result;
        }
    }

    pub(super) fn finish_create(
        &mut self,
        generation: u64,
        name: &str,
        result: Result<(), String>,
    ) -> bool {
        if generation != self.create_generation || !self.creating {
            return false;
        }
        self.creating = false;
        match result {
            Ok(()) => {
                self.create_success = Some(name.to_owned());
                self.create_error = None;
                if self.create_name == name {
                    self.create_name.clear();
                }
            }
            Err(error) => {
                self.create_success = None;
                self.create_error = Some(error);
            }
        }
        true
    }
    pub(super) fn has_mutation_in_flight(&self) -> bool {
        self.creating || self.removing || self.attaching
    }

    pub(super) fn can_remove_selected_network(&self) -> bool {
        if self.inspecting || self.loading {
            return false;
        }
        let Some(selected_id) = self.selected.as_deref() else {
            return false;
        };
        let Some(details) = self.details.as_ref() else {
            return false;
        };
        let Some(listed) = self
            .networks
            .iter()
            .find(|network| network.id == selected_id)
        else {
            return false;
        };
        valid_network_removal_id(selected_id)
            && details.id == selected_id
            && details.name == listed.name
            && !predefined_network(&listed.name)
            && details.attached_container_count == Some(0)
    }

    fn begin_remove_confirmation(&mut self) {
        if !self.has_mutation_in_flight()
            && self.attachment_confirmation.is_none()
            && self.can_remove_selected_network()
            && self.remove_confirmation.is_none()
            && let Some(details) = self.details.as_ref()
        {
            self.remove_confirmation = Some(NetworkRemovalConfirmation {
                generation: self.generation,
                inspection_generation: self.inspection_generation,
                id: details.id.clone(),
                name: details.name.clone(),
            });
            self.remove_success = None;
            self.remove_error = None;
            self.attachment_result = None;
        }
    }

    fn cancel_remove_confirmation(&mut self) {
        self.remove_confirmation = None;
    }

    fn confirmation_is_current(&self, confirmation: &NetworkRemovalConfirmation) -> bool {
        !self.has_mutation_in_flight()
            && self.attachment_confirmation.is_none()
            && confirmation.generation == self.generation
            && confirmation.inspection_generation == self.inspection_generation
            && self.selected.as_deref() == Some(confirmation.id.as_str())
            && self.details.as_ref().is_some_and(|details| {
                details.id == confirmation.id && details.name == confirmation.name
            })
            && self.can_remove_selected_network()
    }

    fn confirm_remove(&mut self) -> Option<NetworksAction> {
        let confirmation = self.remove_confirmation.take()?;
        if !self.confirmation_is_current(&confirmation) {
            return None;
        }
        self.remove_generation = self.remove_generation.wrapping_add(1);
        self.removing = true;
        self.remove_result_id = Some(confirmation.id.clone());
        self.remove_result_name = Some(confirmation.name.clone());
        self.remove_success = None;
        self.remove_error = None;
        Some(NetworksAction::Remove {
            generation: confirmation.generation,
            inspection_generation: confirmation.inspection_generation,
            removal_generation: self.remove_generation,
            id: confirmation.id,
            name: confirmation.name,
        })
    }

    pub(super) fn removal_is_pending(
        &self,
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        id: &str,
        name: &str,
    ) -> bool {
        self.removing
            && self.generation == generation
            && self.inspection_generation == inspection_generation
            && self.remove_generation == removal_generation
            && self.selected.as_deref() == Some(id)
            && self.remove_result_id.as_deref() == Some(id)
            && self.remove_result_name.as_deref() == Some(name)
            && self.can_remove_selected_network()
    }

    pub(super) fn cancel_remove_submission(&mut self, removal_generation: u64) {
        if self.removing && self.remove_generation == removal_generation {
            self.removing = false;
            self.remove_result_id = None;
            self.remove_result_name = None;
        }
    }

    pub(super) fn finish_remove(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        id: &str,
        name: &str,
        result: Result<(), String>,
    ) -> bool {
        if !self.removing
            || self.remove_generation != removal_generation
            || self.remove_result_id.as_deref() != Some(id)
            || self.remove_result_name.as_deref() != Some(name)
        {
            return false;
        }
        self.removing = false;
        self.remove_result_id = None;
        self.remove_result_name = None;
        if self.generation != generation
            || self.inspection_generation != inspection_generation
            || self.selected.as_deref() != Some(id)
        {
            return false;
        }
        self.details = None;
        self.attached_container_count_display = None;
        self.ips_in_use_display = None;
        self.dynamic_ips_available_display = None;
        self.inspecting = false;
        self.remove_confirmation = None;
        match result {
            Ok(()) => {
                self.remove_success = Some(name.to_owned());
                self.remove_error = None;
            }
            Err(error) => {
                self.remove_success = None;
                self.remove_error = Some(error);
            }
        }
        true
    }

    pub(super) fn mark_list_stale(&mut self) {
        self.loading = false;
        self.loaded = false;
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.remove_generation = self.remove_generation.wrapping_add(1);
        self.networks.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.ips_in_use_display = None;
        self.dynamic_ips_available_display = None;
        self.inspecting = false;
        self.removing = false;
        self.attachment_confirmation = None;
        self.remove_confirmation = None;
        self.remove_result_id = None;
        self.remove_result_name = None;
        self.error = None;
        self.detail_error = None;
    }

    pub(super) fn begin_inspection(&mut self, id: String) -> (u64, u64, String) {
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.selected = Some(id.clone());
        self.details = None;
        self.attached_container_count_display = None;
        self.ips_in_use_display = None;
        self.dynamic_ips_available_display = None;
        self.detail_error = None;
        self.attachment_confirmation = None;
        self.attachment_result = None;
        self.remove_confirmation = None;
        self.remove_error = None;
        self.inspecting = true;
        (self.generation, self.inspection_generation, id)
    }

    pub(super) fn finish_inspection(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        id: &str,
        result: Result<NetworkDetails, String>,
    ) {
        if generation != self.generation
            || inspection_generation != self.inspection_generation
            || self.selected.as_deref() != Some(id)
        {
            return;
        }
        self.inspecting = false;
        match result {
            Ok(details) if details.id == id => {
                self.attached_container_count_display = Some(
                    details
                        .attached_container_count
                        .map_or_else(|| "Not reported".to_owned(), |count| count.to_string()),
                );
                self.ips_in_use_display = Some(
                    details
                        .ipam_usage
                        .ips_in_use
                        .map_or_else(|| "Not reported".to_owned(), |count| count.to_string()),
                );
                self.dynamic_ips_available_display = Some(
                    details
                        .ipam_usage
                        .dynamic_ips_available
                        .map_or_else(|| "Not reported".to_owned(), |count| count.to_string()),
                );
                self.details = Some(details);
            }
            Ok(_) => {
                self.detail_error =
                    Some("The inspected network did not match the selected network.".to_owned())
            }
            Err(error) => self.detail_error = Some(error),
        }
    }

    pub(super) fn is_loading(&self) -> bool {
        self.loading || self.inspecting || self.has_mutation_in_flight()
    }

    fn rebuild_visible(&mut self) {
        self.matcher.set(&self.query);
        self.visible.clear();
        self.visible.extend(
            self.networks
                .iter()
                .enumerate()
                .filter_map(|(index, network)| {
                    (self.matcher.matches(&network.name) || self.matcher.matches(&network.id))
                        .then_some(index)
                }),
        );
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        supported: bool,
        connection_message: &str,
        network_mutation_in_flight: bool,
        container_inventory_fresh: bool,
        containers: &[ContainerSummary],
    ) -> Option<NetworksAction> {
        ui.heading("Networks");
        ui.label(
            "Inventory, default bridge creation, confirmed custom-network removal, and confirmed container connect/disconnect on the selected local Docker Unix socket on Linux.",
        );
        if !supported {
            ui.label("Networks require a connected local Docker Unix socket on Linux.");
            ui.label(connection_message);
            return None;
        }
        if !container_inventory_fresh {
            self.attachment_confirmation = None;
        }
        let operation_in_flight = network_mutation_in_flight || self.has_mutation_in_flight();
        let confirmation_open =
            self.remove_confirmation.is_some() || self.attachment_confirmation.is_some();
        let mut action = None;
        ui.group(|ui| {
            ui.heading("Create default bridge network");
            if operation_in_flight {
                ui.spinner();
                if self.creating {
                    ui.label("Creating network…");
                } else if self.removing {
                    ui.label("Removing network…");
                } else {
                    ui.label("A network operation is in progress.");
                }
            }
            if let Some(name) = &self.create_success {
                let name = escape_untrusted_display_text(name);
                ui.label(format!("Network '{name}' created."));
            }
            if let Some(error) = &self.create_error {
                ui.colored_label(egui::Color32::LIGHT_RED, "Could not create network:");
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let name_label = ui.label("Network name");
            ui.add_enabled(
                !operation_in_flight && !confirmation_open,
                egui::TextEdit::singleline(&mut self.create_name),
            )
            .labelled_by(name_label.id);
            let name_is_valid = valid_network_name(&self.create_name);
            if !self.create_name.is_empty() && !name_is_valid {
                ui.label(
                    "Enter a non-whitespace network name up to 255 UTF-8 bytes with no control characters.",
                );
            }
            if ui
                .add_enabled(
                    name_is_valid && !operation_in_flight && !confirmation_open,
                    egui::Button::new("Create network"),
                )
                .clicked()
            {
                action = self.begin_create();
            }
        });
        let search_label = ui.label("Search networks");
        let search = ui.text_edit_singleline(&mut self.query);
        if search.labelled_by(search_label.id).changed() {
            self.rebuild_visible();
        }
        if self.loading {
            ui.spinner();
            ui.label("Loading networks…");
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, "Could not load networks:");
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if self.loaded && self.error.is_none() && self.networks.is_empty() {
            ui.label("No networks found.");
        }
        if self.loaded
            && self.error.is_none()
            && !self.networks.is_empty()
            && self.visible.is_empty()
        {
            ui.label("No networks match your search.");
        }
        let mut selected = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, self.visible.len(), |ui, rows| {
                for row in rows {
                    let network = &self.networks[self.visible[row]];
                    let display_name = escape_untrusted_display_text(&network.name);
                    let response =
                        ui.add_enabled_ui(!operation_in_flight && !confirmation_open, |ui| {
                            ui.add_sized(
                                [ui.available_width(), ROW_HEIGHT],
                                egui::Button::new(display_name.as_ref())
                                    .selected(self.selected.as_deref() == Some(&network.id)),
                            )
                        });
                    if response.inner.clicked() {
                        selected = Some(network.id.clone());
                    }
                }
            });
        if let Some(error) = &self.detail_error {
            ui.label("Could not inspect network:");
            ui.label(error);
        }
        if self.inspecting {
            ui.spinner();
            ui.label("Inspecting network…");
        }
        if let Some(details) = &self.details {
            ui.separator();
            let name = escape_untrusted_display_text(&details.name);
            ui.heading(name.as_ref());
            crate::untrusted_detail_row(ui, "ID", Some(&details.id));
            crate::untrusted_detail_row(ui, "Driver", details.driver.as_deref());
            crate::untrusted_detail_row(ui, "Scope", details.scope.as_deref());
            crate::untrusted_detail_row(ui, "Created", details.created_at.as_deref());
            crate::untrusted_detail_row(ui, "Internal", bool_label(details.internal));
            crate::untrusted_detail_row(ui, "Attachable", bool_label(details.attachable));
            crate::untrusted_detail_row(ui, "Enable IPv4", bool_label(details.enable_ipv4));
            crate::untrusted_detail_row(ui, "Enable IPv6", bool_label(details.enable_ipv6));
            crate::untrusted_detail_row(
                ui,
                "Attached containers",
                self.attached_container_count_display.as_deref(),
            );
            crate::untrusted_detail_row(
                ui,
                "IPs in use/reserved (point-in-time allocation; not traffic)",
                self.ips_in_use_display.as_deref(),
            );
            crate::untrusted_detail_row(
                ui,
                "Dynamically available IPs (point-in-time allocation; not traffic)",
                self.dynamic_ips_available_display.as_deref(),
            );
        }
        let mut requested_attachment = None;
        {
            let attachment_data = self.local_custom_network().and_then(|(network, details)| {
                details
                    .attached_container_ids
                    .as_deref()
                    .map(|ids| (network.name.as_str(), ids))
            });
            ui.group(|ui| {
                if !container_inventory_fresh {
                    ui.label(
                        "Container inventory is stale. Use Refresh before changing network membership.",
                    );
                } else {
                    ui.label("Only running containers can be connected or disconnected.");
                    match attachment_data {
                        None => {
                            ui.label("A fresh local custom-network inspection with complete attached IDs is required.");
                        }
                        Some((_, _)) if containers.is_empty() => {
                            ui.label("No current containers are available for this network.");
                        }
                        Some((network_name, attached_ids)) => {
                            let display_network_name = escape_untrusted_display_text(network_name);
                            egui::ScrollArea::vertical()
                                .max_height(ROW_HEIGHT * 8.0)
                                .show_rows(ui, ROW_HEIGHT, containers.len(), |ui, rows| {
                                    for row in rows {
                                        let container = &containers[row];
                                        if !valid_full_container_id(&container.id) {
                                            continue;
                                        }
                                        let attached =
                                            attached_ids.iter().any(|id| id == &container.id);
                                        let id_prefix = &container.id[..12];
                                        let display_container_name =
                                            escape_untrusted_display_text(&container.name);
                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "{} ({id_prefix})",
                                                display_container_name
                                            ));
                                            let label = if attached {
                                                format!(
                                                    "Disconnect '{}' ({id_prefix}) from '{display_network_name}'",
                                                    display_container_name
                                                )
                                            } else {
                                                format!(
                                                    "Connect '{}' ({id_prefix}) to '{display_network_name}'",
                                                    display_container_name
                                                )
                                            };
                                            if ui
                                                .add_enabled(
                                                    container.state == Some(ContainerState::Running)
                                                        && !operation_in_flight
                                                        && !confirmation_open,
                                                    egui::Button::new(label),
                                                )
                                                .clicked()
                                            {
                                                requested_attachment =
                                                    Some((!attached, container.id.clone()));
                                            }
                                        });
                                    }
                                });
                        }
                    }
                }
            });
        }
        if let Some((connect, container_id)) = requested_attachment {
            self.begin_attachment_confirmation(connect, &container_id, containers);
        }
        ui.group(|ui| {
            if self.attaching {
                ui.spinner();
                ui.label("Updating network attachment…");
            }
            if let Some((result_network_id, _, connect, result)) = &self.attachment_result
                && self
                    .selected
                    .as_deref()
                    .is_none_or(|selected_id| selected_id == result_network_id)
            {
                match result {
                    Ok(()) => {
                        ui.label(if *connect {
                            "Network connect request succeeded; inspect current membership before another change."
                        } else {
                            "Network disconnect request succeeded; inspect current membership before another change."
                        });
                    }
                    Err(error) => {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            "Network attachment request failed; engine state may have changed.",
                        );
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                        ui.label("Inspect current membership before retrying.");
                    }
                }
            }
            if let Some(confirmation) = self.attachment_confirmation.clone() {
                if self.attachment_confirmation_is_current(&confirmation, containers) {
                    ui.group(|ui| {
                        ui.heading(if confirmation.connect {
                            "Confirm container connection"
                        } else {
                            "Confirm container disconnection"
                        });
                        let container_name =
                            escape_untrusted_display_text(&confirmation.container_name);
                        let network_name =
                            escape_untrusted_display_text(&confirmation.network_name);
                        if confirmation.connect {
                            ui.label(format!(
                                "Connect '{}' ({}) to '{}' ({}). Confirm to submit.",
                                container_name,
                                confirmation.container_id,
                                network_name,
                                confirmation.network_id
                            ));
                        } else {
                            ui.label(format!(
                                "Disconnect '{}' ({}) from '{}' ({}). This can interrupt connectivity. Confirm to submit.",
                                container_name,
                                confirmation.container_id,
                                network_name,
                                confirmation.network_id
                            ));
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .button(if confirmation.connect {
                                    "Confirm connection"
                                } else {
                                    "Confirm disconnection"
                                })
                                .clicked()
                            {
                                action = self.confirm_attachment(containers);
                            }
                            if ui.button("Cancel").clicked() {
                                self.attachment_confirmation = None;
                            }
                        });
                    });
                } else {
                    self.attachment_confirmation = None;
                }
            }
        });
        ui.group(|ui| {
            ui.heading("Remove unused custom network");
            if self.selected.is_none() {
                ui.label("Select and inspect a custom network before removal.");
            } else if self.inspecting {
                ui.label("Wait for the selected network inspection to finish.");
            } else if let Some(details) = &self.details {
                if predefined_network(&details.name) {
                    ui.label("Predefined Docker networks (bridge, host, none) cannot be removed here.");
                } else {
                    match details.attached_container_count {
                        Some(0) => {
                            ui.label("Inspect reports zero attached containers; this is only a point-in-time snapshot.");
                        }
                        Some(count) => {
                            ui.label(format!(
                                "Removal is disabled while {count} attached container(s) are reported."
                            ));
                        }
                        None => {
                            ui.label("Removal is disabled because the attached-container count was not reported.");
                        }
                    }
                }
            } else {
                ui.label("Inspect the selected network before removal.");
            }
            if self.removing
                && let Some(name) = &self.remove_result_name
            {
                ui.spinner();
                let name = escape_untrusted_display_text(name);
                ui.label(format!("Removing network '{name}'…"));
            }
            if let Some(name) = &self.remove_success {
                let name = escape_untrusted_display_text(name);
                ui.label(format!("Network '{name}' was removed."));
            }
            if let Some(error) = &self.remove_error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    "Network removal failed; the engine state may have changed.",
                );
                ui.colored_label(egui::Color32::LIGHT_RED, error);
                ui.label("Inspect the network again before another removal attempt.");
            }
            if ui
                .add_enabled(
                    self.can_remove_selected_network()
                        && !operation_in_flight
                        && self.remove_confirmation.is_none()
                        && self.attachment_confirmation.is_none(),
                    egui::Button::new("Remove network"),
                )
                .clicked()
            {
                self.begin_remove_confirmation();
            }
            if let Some(confirmation) = self.remove_confirmation.clone() {
                if self.confirmation_is_current(&confirmation) {
                    ui.group(|ui| {
                        ui.heading("Confirm network removal");
                        let name = escape_untrusted_display_text(&confirmation.name);
                        ui.label(format!(
                            "Remove custom network '{}' (ID {})? The latest inspect reported zero attached containers; this is only a point-in-time check. Docker may reject removal if dependencies change. Force and prune are not used. Confirm to submit this exact-ID request.",
                            name, confirmation.id
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Confirm network removal").clicked() {
                                action = self.confirm_remove();
                            }
                            if ui.button("Cancel").clicked() {
                                self.cancel_remove_confirmation();
                            }
                        });
                    });
                } else {
                    self.cancel_remove_confirmation();
                }
            }
        });
        action.or_else(|| selected.map(NetworksAction::Inspect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn summary(id: &str, name: &str) -> NetworkSummary {
        NetworkSummary {
            id: id.into(),
            name: name.into(),
            driver: Some("bridge".into()),
            scope: Some("local".into()),
            created_at: None,
            internal: Some(false),
            attachable: Some(false),
            enable_ipv4: Some(true),
            enable_ipv6: Some(false),
        }
    }
    fn details(id: &str, name: &str, count: Option<usize>) -> NetworkDetails {
        NetworkDetails {
            id: id.into(),
            name: name.into(),
            driver: Some("bridge".into()),
            scope: Some("local".into()),
            created_at: Some("today".into()),
            internal: Some(false),
            attachable: Some(true),
            enable_ipv4: Some(true),
            enable_ipv6: Some(false),
            attached_container_count: count,
            attached_container_ids: None,
            ipam_usage: ferrum_core::NetworkIpamUsage::default(),
        }
    }
    fn removable_view(name: &str, count: Option<usize>) -> NetworksView {
        let id = "a".repeat(64);
        let mut view = NetworksView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary(&id, name)]));
        let (_, inspection_generation, inspected_id) = view.begin_inspection(id);
        view.finish_inspection(
            generation,
            inspection_generation,
            &inspected_id,
            Ok(details(&inspected_id, name, count)),
        );
        view
    }

    fn harness_for(view: NetworksView, supported: bool) -> Harness<'static, NetworksView> {
        Harness::new_ui_state(
            move |ui, view: &mut NetworksView| {
                if let Some(NetworksAction::Inspect(id)) =
                    view.show(ui, supported, "offline", false, true, &[])
                {
                    view.begin_inspection(id);
                }
            },
            view,
        )
    }

    fn container_summary(id: &str, name: &str) -> ContainerSummary {
        ContainerSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            image: None,
            state: Some(ContainerState::Running),
            status: None,
        }
    }

    fn details_with_attachments(
        id: &str,
        name: &str,
        attached_container_ids: Option<Vec<String>>,
    ) -> NetworkDetails {
        let mut details = details(id, name, attached_container_ids.as_ref().map(Vec::len));
        details.attached_container_ids = attached_container_ids;
        details
    }

    fn attachment_view(
        network_name: &str,
        attached_container_ids: Option<Vec<String>>,
    ) -> NetworksView {
        let network_id = "a".repeat(64);
        let mut view = NetworksView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary(&network_id, network_name)]));
        let (generation, inspection_generation, selected_id) = view.begin_inspection(network_id);
        view.finish_inspection(
            generation,
            inspection_generation,
            &selected_id,
            Ok(details_with_attachments(
                &selected_id,
                network_name,
                attached_container_ids,
            )),
        );
        view
    }

    fn attachment_harness(
        view: NetworksView,
        containers: Vec<ContainerSummary>,
        container_inventory_fresh: bool,
    ) -> (
        Harness<'static, NetworksView>,
        std::sync::mpsc::Receiver<NetworksAction>,
    ) {
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let harness = Harness::new_ui_state(
            move |ui, view: &mut NetworksView| {
                if let Some(action) =
                    view.show(ui, true, "", false, container_inventory_fresh, &containers)
                    && !matches!(&action, NetworksAction::Inspect(_))
                {
                    action_sender
                        .send(action)
                        .expect("action receiver should stay open");
                }
            },
            view,
        );
        (harness, action_receiver)
    }

    #[test]
    fn attachment_actions_require_complete_membership_and_exact_inventory_ids() {
        let container_id = "b".repeat(64);
        let container = container_summary(&container_id, "worker");

        let mut missing = attachment_view("custom-net", None);
        missing.begin_attachment_confirmation(
            true,
            &container_id,
            std::slice::from_ref(&container),
        );
        assert!(missing.attachment_confirmation.is_none());

        let mut already_attached = attachment_view("custom-net", Some(vec![container_id.clone()]));
        already_attached.begin_attachment_confirmation(
            true,
            &container_id,
            std::slice::from_ref(&container),
        );
        assert!(already_attached.attachment_confirmation.is_none());

        already_attached.begin_attachment_confirmation(false, &container_id, &[]);
        assert!(already_attached.attachment_confirmation.is_none());

        let short_id = "b".repeat(12);
        already_attached.begin_attachment_confirmation(
            false,
            &container_id,
            &[container_summary(&short_id, "worker")],
        );
        assert!(already_attached.attachment_confirmation.is_none());

        already_attached.begin_attachment_confirmation(
            false,
            &container_id,
            std::slice::from_ref(&container),
        );
        assert!(already_attached.attachment_confirmation.is_some());
        already_attached.begin_inspection("a".repeat(64));
        assert!(already_attached.confirm_attachment(&[container]).is_none());
    }

    #[test]
    fn attachment_controls_are_disabled_for_nonrunning_or_unknown_containers() {
        let container_id = "b".repeat(64);
        let mut container = container_summary(&container_id, "worker");
        container.state = Some(ContainerState::Created);

        let mut connect = attachment_view("custom-net", Some(Vec::new()));
        connect.begin_attachment_confirmation(
            true,
            &container_id,
            std::slice::from_ref(&container),
        );
        assert!(connect.attachment_confirmation.is_none());

        let mut disconnect = attachment_view("custom-net", Some(vec![container_id.clone()]));
        disconnect.begin_attachment_confirmation(
            false,
            &container_id,
            std::slice::from_ref(&container),
        );
        assert!(disconnect.attachment_confirmation.is_none());

        container.state = None;
        let (mut harness, actions) = attachment_harness(
            attachment_view("custom-net", Some(Vec::new())),
            vec![container],
            true,
        );
        harness.run();
        let connect_label = format!("Connect 'worker' ({}) to 'custom-net'", &container_id[..12]);
        let button = harness.get_by_role_and_label(Role::Button, &connect_label);
        assert!(button.accesskit_node().is_disabled());
        assert!(actions.try_recv().is_err());
    }

    #[test]
    fn attachment_controls_require_fresh_container_inventory() {
        let container_id = "b".repeat(64);
        let container = container_summary(&container_id, "worker");
        let mut view = attachment_view("custom-net", Some(Vec::new()));
        view.begin_attachment_confirmation(true, &container_id, std::slice::from_ref(&container));
        assert!(view.attachment_confirmation.is_some());
        let (mut harness, actions) = attachment_harness(view, vec![container], false);

        harness.run();
        let _status = harness.get_by_role_and_label(
            Role::Label,
            "Container inventory is stale. Use Refresh before changing network membership.",
        );
        assert!(harness.state().attachment_confirmation.is_none());
        assert!(actions.try_recv().is_err());
    }

    #[test]
    fn attachment_success_refreshes_membership_before_another_operation() {
        let network_id = "a".repeat(64);
        let container_id = "b".repeat(64);
        let containers = [container_summary(&container_id, "worker")];
        let mut view = attachment_view("custom-net", Some(Vec::new()));
        view.begin_attachment_confirmation(true, &container_id, &containers);
        let operation = match view
            .confirm_attachment(&containers)
            .expect("explicit confirmation should submit")
        {
            NetworksAction::Attach(operation) => operation,
            NetworksAction::Inspect(_)
            | NetworksAction::Create { .. }
            | NetworksAction::Remove { .. } => {
                panic!("container connection should submit an attachment action")
            }
        };
        assert!(view.attachment_is_pending(&operation, &containers));
        assert!(view.finish_attachment(&operation, Ok(())));
        assert!(view.details.is_none());

        view.refresh_after_attachment(
            vec![summary(&network_id, "custom-net")],
            details_with_attachments(&network_id, "custom-net", Some(vec![container_id.clone()])),
            &network_id,
        );
        assert_eq!(
            view.details
                .as_ref()
                .and_then(|details| details.attached_container_ids.clone()),
            Some(vec![container_id.clone()])
        );
        assert!(matches!(
            view.attachment_result.as_ref().map(|result| &result.3),
            Some(Ok(()))
        ));
        assert!(!view.attaching);
    }

    #[test]
    fn stale_attachment_results_do_not_replace_current_inspection() {
        let network_id = "a".repeat(64);
        let container_id = "b".repeat(64);
        let containers = [container_summary(&container_id, "worker")];
        let mut view = attachment_view("custom-net", Some(Vec::new()));
        view.begin_attachment_confirmation(true, &container_id, &containers);
        let operation = match view
            .confirm_attachment(&containers)
            .expect("explicit confirmation should submit")
        {
            NetworksAction::Attach(operation) => operation,
            NetworksAction::Inspect(_)
            | NetworksAction::Create { .. }
            | NetworksAction::Remove { .. } => {
                panic!("container connection should submit an attachment action")
            }
        };

        view.begin_inspection(network_id.clone());
        assert!(!view.finish_attachment(&operation, Ok(())));
        assert!(!view.attaching);
        assert!(view.attachment_result.is_none());
        assert!(view.inspecting);
    }

    #[test]
    fn connection_confirmation_names_exact_targets_and_submits_only_on_confirm() {
        let network_id = "a".repeat(64);
        let container_id = "b".repeat(64);
        let container = container_summary(&container_id, "worker");
        let containers = vec![container.clone()];
        let (mut connect_harness, connect_actions) = attachment_harness(
            attachment_view("custom-net", Some(Vec::new())),
            containers.clone(),
            true,
        );

        connect_harness.run();
        connect_harness
            .get_by_role_and_label(
                Role::Button,
                &format!("Connect 'worker' ({}) to 'custom-net'", &container_id[..12]),
            )
            .click_accesskit();
        connect_harness.run();
        let connect_confirmation = format!(
            "Connect 'worker' ({container_id}) to 'custom-net' ({network_id}). Confirm to submit."
        );
        let _warning = connect_harness.get_by_role_and_label(Role::Label, &connect_confirmation);
        assert!(connect_actions.try_recv().is_err());
        connect_harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .click_accesskit();
        connect_harness.run();
        assert!(connect_actions.try_recv().is_err());

        connect_harness
            .get_by_role_and_label(
                Role::Button,
                &format!("Connect 'worker' ({}) to 'custom-net'", &container_id[..12]),
            )
            .click_accesskit();
        connect_harness.run();
        connect_harness
            .get_by_role_and_label(Role::Button, "Confirm connection")
            .click_accesskit();
        connect_harness.step();
        assert!(matches!(
            connect_actions.try_recv().expect("confirmation should submit"),
            NetworksAction::Attach(operation)
                if operation.connect
                    && operation.network_id == network_id
                    && operation.container_id == container_id
        ));

        let (mut disconnect_harness, disconnect_actions) = attachment_harness(
            attachment_view("custom-net", Some(vec![container_id.clone()])),
            containers,
            true,
        );
        disconnect_harness.run();
        disconnect_harness
            .get_by_role_and_label(
                Role::Button,
                &format!(
                    "Disconnect 'worker' ({}) from 'custom-net'",
                    &container_id[..12]
                ),
            )
            .click_accesskit();
        disconnect_harness.run();
        let disconnect_confirmation = format!(
            "Disconnect 'worker' ({container_id}) from 'custom-net' ({network_id}). This can interrupt connectivity. Confirm to submit."
        );
        let _warning =
            disconnect_harness.get_by_role_and_label(Role::Label, &disconnect_confirmation);
        disconnect_harness
            .get_by_role_and_label(Role::Button, "Confirm disconnection")
            .click_accesskit();
        disconnect_harness.step();
        assert!(matches!(
            disconnect_actions
                .try_recv()
                .expect("confirmation should submit"),
            NetworksAction::Attach(operation)
                if !operation.connect
                    && operation.network_id == network_id
                    && operation.container_id == container_id
        ));
    }

    #[test]
    fn engine_network_and_container_text_is_escaped_before_accessible_display() {
        let network_id = "a".repeat(64);
        let container_id = "b".repeat(64);
        let network_name = "custom\u{202e}net";
        let container_name = "worker\u{e0001}proc";
        let safe_network_name = "custom\\u{202e}net";
        let safe_container_name = "worker\\u{e0001}proc";
        let mut view = attachment_view(network_name, Some(Vec::new()));
        view.details
            .as_mut()
            .expect("selected network details")
            .driver = Some("bridge\u{200b}".to_owned());
        let containers = vec![container_summary(&container_id, container_name)];
        let (mut harness, action_receiver) = attachment_harness(view, containers, true);

        harness.run();
        let network_row = harness.get_by_role_and_label(Role::Button, safe_network_name);
        assert_eq!(
            network_row.accesskit_node().label().as_deref(),
            Some(safe_network_name)
        );
        let network_heading = harness.get_by_role_and_label(Role::Label, safe_network_name);
        assert_eq!(
            network_heading.accesskit_node().value().as_deref(),
            Some(safe_network_name)
        );
        let driver = harness.get_by_role_and_label(Role::Label, "bridge\\u{200b}");
        assert_eq!(
            driver.accesskit_node().value().as_deref(),
            Some("bridge\\u{200b}")
        );
        let expected_container_row = format!("{safe_container_name} ({})", &container_id[..12]);
        let container_row = harness.get_by_role_and_label(Role::Label, &expected_container_row);
        assert_eq!(
            container_row.accesskit_node().value().as_deref(),
            Some(expected_container_row.as_str())
        );
        let connect_label = format!(
            "Connect '{safe_container_name}' ({}) to '{safe_network_name}'",
            &container_id[..12]
        );
        let connect_button = harness.get_by_role_and_label(Role::Button, &connect_label);
        assert_eq!(
            connect_button.accesskit_node().label().as_deref(),
            Some(connect_label.as_str())
        );
        connect_button.click_accesskit();
        harness.run();
        let confirmation = format!(
            "Connect '{safe_container_name}' ({container_id}) to '{safe_network_name}' ({network_id}). Confirm to submit."
        );
        let confirmation_label = harness.get_by_role_and_label(Role::Label, &confirmation);
        assert_eq!(
            confirmation_label.accesskit_node().value().as_deref(),
            Some(confirmation.as_str())
        );
        harness
            .get_by_role_and_label(Role::Button, "Confirm connection")
            .click_accesskit();
        harness.step();

        assert!(matches!(
            action_receiver
                .try_recv()
                .expect("confirmation should submit exact IDs"),
            NetworksAction::Attach(operation)
                if operation.connect
                    && operation.network_id == network_id
                    && operation.container_id == container_id
        ));
    }

    #[test]
    fn accessible_create_network_name_validation_disables_invalid_names() {
        let mut empty = harness_for(NetworksView::default(), true);
        empty.run();
        assert!(
            empty
                .get_by_role_and_label(Role::Button, "Create network")
                .accesskit_node()
                .is_disabled()
        );

        let too_long = "n".repeat(MAX_NETWORK_NAME_BYTES + 1);
        let mut invalid = harness_for(NetworksView::default(), true);
        invalid.run();
        invalid
            .get_by_role_and_label(Role::TextInput, "Network name")
            .focus();
        invalid.run();
        invalid
            .get_by_role_and_label(Role::TextInput, "Network name")
            .type_text(&too_long);
        invalid.run();
        let _invalid_help = invalid.get_by_role_and_label(
            Role::Label,
            "Enter a non-whitespace network name up to 255 UTF-8 bytes with no control characters.",
        );
        assert!(
            invalid
                .get_by_role_and_label(Role::Button, "Create network")
                .accesskit_node()
                .is_disabled()
        );

        let mut valid = harness_for(NetworksView::default(), true);
        valid.run();
        valid
            .get_by_role_and_label(Role::TextInput, "Network name")
            .focus();
        valid.run();
        valid
            .get_by_role_and_label(Role::TextInput, "Network name")
            .type_text("work^net");
        valid.run();
        assert!(
            !valid
                .get_by_role_and_label(Role::Button, "Create network")
                .accesskit_node()
                .is_disabled()
        );
        let maximum_name = "n".repeat(MAX_NETWORK_NAME_BYTES);
        let mut maximum = harness_for(NetworksView::default(), true);
        maximum.run();
        maximum
            .get_by_role_and_label(Role::TextInput, "Network name")
            .focus();
        maximum.run();
        maximum
            .get_by_role_and_label(Role::TextInput, "Network name")
            .type_text(&maximum_name);
        maximum.run();
        assert!(
            !maximum
                .get_by_role_and_label(Role::Button, "Create network")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn network_creation_is_single_flight_and_rejects_stale_results() {
        let mut view = NetworksView {
            create_name: "work-net".to_owned(),
            ..NetworksView::default()
        };
        let (generation, name) = match view.begin_create() {
            Some(NetworksAction::Create { generation, name }) => (generation, name),
            _ => panic!("valid network name should begin creation"),
        };
        assert!(view.begin_create().is_none());

        let mut harness = Harness::new_ui_state(
            |ui, view: &mut NetworksView| {
                let _ = view.show(ui, true, "", true, true, &[]);
            },
            view,
        );
        harness.step();
        assert!(harness.state().creating);
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Create network")
                .accesskit_node()
                .is_disabled()
        );

        harness.state_mut().invalidate();
        assert!(!harness.state_mut().finish_create(generation, &name, Ok(())));
        let current_generation = harness.state().create_generation;
        assert!(
            !harness
                .state_mut()
                .finish_create(current_generation, "not-started", Ok(()))
        );
        assert!(harness.state().create_success.is_none());
    }

    #[test]
    fn successful_network_creation_is_announced_accessibly() {
        let mut view = NetworksView {
            create_name: "new-bridge".to_owned(),
            ..NetworksView::default()
        };
        let (generation, name) = match view.begin_create() {
            Some(NetworksAction::Create { generation, name }) => (generation, name),
            _ => panic!("valid network name should begin creation"),
        };
        assert!(view.finish_create(generation, &name, Ok(())));

        let mut harness = harness_for(view, true);
        harness.run();
        let _success = harness.get_by_role_and_label(Role::Label, "Network 'new-bridge' created.");
    }

    #[test]
    fn failed_network_creation_is_announced_and_keeps_name_for_retry() {
        let mut view = NetworksView {
            create_name: "work-net".to_owned(),
            ..NetworksView::default()
        };
        let (generation, name) = match view.begin_create() {
            Some(NetworksAction::Create { generation, name }) => (generation, name),
            _ => panic!("valid network name should begin creation"),
        };
        let message = "Network creation was rejected at the test endpoint (HTTP 409).";
        assert!(view.finish_create(generation, &name, Err(message.to_owned())));

        let mut harness = harness_for(view, true);
        harness.run();
        let _title = harness.get_by_role_and_label(Role::Label, "Could not create network:");
        let _message = harness.get_by_role_and_label(Role::Label, message);
        assert_eq!(harness.state().create_name, "work-net");
    }

    #[test]
    fn stale_list_and_inspection_results_are_rejected() {
        let mut view = NetworksView::default();
        let old = view.begin_list();
        view.finish_list(old, Ok(vec![summary("id", "bridge")]));
        let (generation, inspection, id) = view.begin_inspection("id".into());
        view.invalidate();
        view.finish_inspection(
            generation,
            inspection,
            &id,
            Ok(details("id", "bridge", Some(2))),
        );
        assert!(view.details.is_none());
        assert!(view.selected.is_none());
        view.finish_list(old, Ok(vec![summary("id", "bridge")]));
        assert!(!view.loaded);
    }

    #[test]
    fn accessible_network_navigation_and_inspection_show_counts_and_ipam_usage() {
        let mut view = NetworksView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary("network-id", "bridge-net")]));
        let mut harness = harness_for(view, true);
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "bridge-net")
            .click_accesskit();
        harness.step();
        harness.step();
        let (generation, inspection, id) = (
            harness.state().generation,
            harness.state().inspection_generation,
            "network-id",
        );
        let mut details = details(id, "bridge-net", Some(3));
        details.ipam_usage = ferrum_core::NetworkIpamUsage {
            ips_in_use: Some(5),
            dynamic_ips_available: Some(7),
        };
        harness
            .state_mut()
            .finish_inspection(generation, inspection, id, Ok(details));
        harness.run();
        let _count = harness.get_by_role_and_label(Role::Label, "3");
        let _ips_used = harness.get_by_role_and_label(
            Role::Label,
            "IPs in use/reserved (point-in-time allocation; not traffic)",
        );
        let _ips_used_value = harness.get_by_role_and_label(Role::Label, "5");
        let _dynamic_ips = harness.get_by_role_and_label(
            Role::Label,
            "Dynamically available IPs (point-in-time allocation; not traffic)",
        );
        let _dynamic_ips_value = harness.get_by_role_and_label(Role::Label, "7");
        let rendered = format!("{:?}", harness.state().details.as_ref().unwrap());
        assert!(!rendered.contains("container-id"));
    }

    #[test]
    fn accessible_empty_error_and_unsupported_states_are_distinct() {
        let mut empty = NetworksView::default();
        let generation = empty.begin_list();
        empty.finish_list(generation, Ok(Vec::new()));
        let mut harness = harness_for(empty, true);
        harness.run();
        let _ = harness.get_by_role_and_label(Role::Label, "No networks found.");
        let mut failed = NetworksView::default();
        let generation = failed.begin_list();
        failed.finish_list(generation, Err("offline".into()));
        let mut harness = harness_for(failed, true);
        harness.run();
        let _ = harness.get_by_role_and_label(Role::Label, "Could not load networks:");
        let _ = harness.get_by_role_and_label(Role::Label, "offline");
        let mut harness = harness_for(NetworksView::default(), false);
        harness.run();
        let _ = harness.get_by_role_and_label(
            Role::Label,
            "Networks require a connected local Docker Unix socket on Linux.",
        );
        let _ = harness.get_by_role_and_label(Role::Label, "offline");
    }
    #[test]
    fn accessible_loading_state_is_announced() {
        let mut view = NetworksView::default();
        view.begin_list();
        let mut harness = harness_for(view, true);
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading networks…");
    }

    #[test]
    fn accessible_search_matches_network_ids() {
        let mut view = NetworksView::default();
        let generation = view.begin_list();
        view.finish_list(
            generation,
            Ok(vec![
                summary("123abc", "bridge"),
                summary("other", "overlay"),
            ]),
        );
        let mut harness = harness_for(view, true);
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Search networks")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Search networks")
            .type_text("123abc");
        harness.run();
        let _ = harness.get_by_role_and_label(Role::Button, "bridge");
        assert_eq!(harness.state().visible.len(), 1);
    }

    #[test]
    fn missing_attached_count_is_not_reported() {
        let mut view = NetworksView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary("id", "bridge")]));
        let (_, inspection, id) = view.begin_inspection("id".into());
        view.finish_inspection(
            generation,
            inspection,
            &id,
            Ok(details("id", "bridge", None)),
        );
        assert_eq!(view.details.unwrap().attached_container_count, None);
    }
    #[test]
    fn inspection_renders_all_missing_counts_as_not_reported() {
        let mut view = NetworksView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary("id", "bridge")]));
        let (_, inspection, id) = view.begin_inspection("id".into());
        view.finish_inspection(
            generation,
            inspection,
            &id,
            Ok(details("id", "bridge", None)),
        );
        let mut harness = harness_for(view, true);
        harness.run();
        let not_reported = harness.get_all_by_role_and_label(Role::Label, "Not reported");
        assert_eq!(not_reported.count(), 3);
        let _used_label = harness.get_by_role_and_label(
            Role::Label,
            "IPs in use/reserved (point-in-time allocation; not traffic)",
        );
        let _dynamic_label = harness.get_by_role_and_label(
            Role::Label,
            "Dynamically available IPs (point-in-time allocation; not traffic)",
        );
    }
    #[test]
    fn removal_requires_a_fresh_zero_count_and_rejects_predefined_networks() {
        for (name, inspected_count, enabled) in [
            ("custom-net", None, false),
            ("custom-net", Some(None), false),
            ("custom-net", Some(Some(2)), false),
            ("bridge", Some(Some(0)), false),
            ("host", Some(Some(0)), false),
            ("none", Some(Some(0)), false),
            ("custom-net", Some(Some(0)), true),
        ] {
            let view = match inspected_count {
                Some(count) => removable_view(name, count),
                None => {
                    let mut view = NetworksView::default();
                    let generation = view.begin_list();
                    view.finish_list(generation, Ok(vec![summary(&"a".repeat(64), name)]));
                    view
                }
            };
            let mut harness = harness_for(view, true);
            harness.run();
            assert_eq!(
                harness
                    .get_by_role_and_label(Role::Button, "Remove network")
                    .accesskit_node()
                    .is_disabled(),
                !enabled,
                "unexpected remove availability for {name} with count {inspected_count:?}"
            );
        }
    }

    #[test]
    fn network_removal_requires_named_confirmation_and_cancel_sends_no_action() {
        let id = "a".repeat(64);
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut harness = Harness::new_ui_state(
            move |ui, view: &mut NetworksView| {
                if let Some(action) = view.show(ui, true, "", false, true, &[])
                    && !matches!(&action, NetworksAction::Inspect(_))
                {
                    action_sender
                        .send(action)
                        .expect("action receiver should stay open");
                }
            },
            removable_view("custom-net", Some(0)),
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Remove network")
            .click_accesskit();
        harness.run();
        let _confirmation = harness.get_by_role_and_label(Role::Label, "Confirm network removal");
        let expected_confirmation = format!(
            "Remove custom network 'custom-net' (ID {id})? The latest inspect reported zero attached containers; this is only a point-in-time check. Docker may reject removal if dependencies change. Force and prune are not used. Confirm to submit this exact-ID request."
        );
        let _warning = harness.get_by_role_and_label(Role::Label, &expected_confirmation);
        assert!(action_receiver.try_recv().is_err());
        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .click_accesskit();
        harness.run();
        assert!(action_receiver.try_recv().is_err());

        harness
            .get_by_role_and_label(Role::Button, "Remove network")
            .click_accesskit();
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Confirm network removal")
            .click_accesskit();
        harness.step();
        assert!(matches!(
            action_receiver.try_recv().expect("explicit confirmation should submit"),
            NetworksAction::Remove {
                id: submitted_id,
                name,
                ..
            } if submitted_id == id && name == "custom-net"
        ));
        assert!(harness.state().removing);
    }

    #[test]
    fn stale_network_removal_confirmation_and_results_are_rejected() {
        let mut view = removable_view("custom-net", Some(0));
        view.begin_remove_confirmation();
        view.begin_inspection("other-network".to_owned());
        assert!(view.confirm_remove().is_none());

        let mut view = removable_view("custom-net", Some(0));
        view.begin_remove_confirmation();
        let action = view
            .confirm_remove()
            .expect("fresh confirmation should submit");
        let NetworksAction::Remove {
            generation,
            inspection_generation,
            removal_generation,
            id,
            name,
        } = action
        else {
            panic!("confirmation should submit removal");
        };
        let other_id = "b".repeat(64);
        assert!(!view.finish_remove(
            generation,
            inspection_generation,
            removal_generation,
            &other_id,
            &name,
            Ok(())
        ));
        assert!(view.removing);
        assert!(!view.finish_remove(
            generation.wrapping_add(1),
            inspection_generation,
            removal_generation,
            &id,
            &name,
            Ok(())
        ));
        view.invalidate();
        assert!(!view.finish_remove(
            generation,
            inspection_generation,
            removal_generation,
            &id,
            &name,
            Ok(())
        ));
        assert!(view.remove_success.is_none());
        assert!(view.remove_error.is_none());
    }

    #[test]
    fn only_one_network_mutation_can_start_and_failed_removal_requires_reinspection() {
        let mut view = removable_view("custom-net", Some(0));
        view.create_name = "another-net".to_owned();
        view.begin_remove_confirmation();
        let action = view.confirm_remove().expect("removal should be available");
        assert!(matches!(&action, NetworksAction::Remove { .. }));
        assert!(view.begin_create().is_none());
        let NetworksAction::Remove {
            generation,
            inspection_generation,
            removal_generation,
            id,
            name,
        } = action
        else {
            unreachable!()
        };
        assert!(view.finish_remove(
            generation,
            inspection_generation,
            removal_generation,
            &id,
            &name,
            Err("Network removal was rejected (HTTP 409).".to_owned())
        ));
        assert!(!view.can_remove_selected_network());
        assert!(view.remove_error.is_some());
        let mut harness = harness_for(view, true);
        harness.run();
        let _failure = harness.get_by_role_and_label(
            Role::Label,
            "Network removal failed; the engine state may have changed.",
        );
        let _rejection =
            harness.get_by_role_and_label(Role::Label, "Network removal was rejected (HTTP 409).");
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Remove network")
                .accesskit_node()
                .is_disabled()
        );
    }
}
