use crate::search::CaseInsensitiveQuery;
use eframe::egui;
use ferrum_core::{NetworkDetails, NetworkSummary};

const ROW_HEIGHT: f32 = 34.0;
const MAX_NETWORK_NAME_BYTES: usize = 255;

pub(super) enum NetworksAction {
    Inspect(String),
    Create { generation: u64, name: String },
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

#[derive(Default)]
pub(super) struct NetworksView {
    networks: Vec<NetworkSummary>,
    visible: Vec<usize>,
    query: String,
    matcher: CaseInsensitiveQuery,
    selected: Option<String>,
    details: Option<NetworkDetails>,
    attached_container_count_display: Option<String>,
    create_name: String,
    create_generation: u64,
    creating: bool,
    create_success: Option<String>,
    create_error: Option<String>,
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
        self.networks.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.loading = false;
        self.loaded = false;
        self.inspecting = false;
        self.creating = false;
        self.create_success = None;
        self.create_error = None;
        self.error = None;
        self.detail_error = None;
    }

    pub(super) fn begin_list(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.loading = true;
        self.loaded = false;
        self.error = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.selected = None;
        self.inspecting = false;
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
        if self.creating || !valid_network_name(&self.create_name) {
            return None;
        }
        self.create_generation = self.create_generation.wrapping_add(1);
        self.creating = true;
        self.create_success = None;
        self.create_error = None;
        Some(NetworksAction::Create {
            generation: self.create_generation,
            name: self.create_name.clone(),
        })
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

    pub(super) fn mark_list_stale(&mut self) {
        self.loading = false;
        self.loaded = false;
        self.networks.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.inspecting = false;
        self.error = None;
        self.detail_error = None;
    }

    pub(super) fn begin_inspection(&mut self, id: String) -> (u64, u64, String) {
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.selected = Some(id.clone());
        self.details = None;
        self.attached_container_count_display = None;
        self.detail_error = None;
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
            Ok(details) => {
                self.attached_container_count_display = Some(
                    details
                        .attached_container_count
                        .map_or_else(|| "Not reported".to_owned(), |count| count.to_string()),
                );
                self.details = Some(details);
            }
            Err(error) => self.detail_error = Some(error),
        }
    }

    pub(super) fn is_loading(&self) -> bool {
        self.loading || self.inspecting || self.creating
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
        create_in_flight: bool,
    ) -> Option<NetworksAction> {
        ui.heading("Networks");
        ui.label(
            "Inventory and default bridge network creation for the selected local Docker Unix socket on Linux.",
        );
        ui.label("Network removal, connect, and disconnect operations are not offered.");
        if !supported {
            ui.label("Networks require a connected local Docker Unix socket on Linux.");
            ui.label(connection_message);
            return None;
        }
        let mut action = None;
        ui.group(|ui| {
            ui.heading("Create default bridge network");
            let operation_in_flight = create_in_flight || self.creating;
            if operation_in_flight {
                ui.spinner();
                ui.label("Creating network…");
            }
            if let Some(name) = &self.create_success {
                ui.label(format!("Network '{name}' created."));
            }
            if let Some(error) = &self.create_error {
                ui.colored_label(egui::Color32::LIGHT_RED, "Could not create network:");
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let name_label = ui.label("Network name");
            ui.add_enabled(
                !operation_in_flight,
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
                    name_is_valid && !operation_in_flight,
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
                    if ui
                        .add_sized(
                            [ui.available_width(), ROW_HEIGHT],
                            egui::Button::new(&network.name)
                                .selected(self.selected.as_deref() == Some(&network.id)),
                        )
                        .clicked()
                    {
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
            ui.heading(&details.name);
            crate::detail_row(ui, "ID", Some(&details.id));
            crate::detail_row(ui, "Driver", details.driver.as_deref());
            crate::detail_row(ui, "Scope", details.scope.as_deref());
            crate::detail_row(ui, "Created", details.created_at.as_deref());
            crate::detail_row(ui, "Internal", bool_label(details.internal));
            crate::detail_row(ui, "Attachable", bool_label(details.attachable));
            crate::detail_row(ui, "Enable IPv4", bool_label(details.enable_ipv4));
            crate::detail_row(ui, "Enable IPv6", bool_label(details.enable_ipv6));
            crate::detail_row(
                ui,
                "Attached containers",
                self.attached_container_count_display.as_deref(),
            );
        }
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
        }
    }
    fn harness_for(view: NetworksView, supported: bool) -> Harness<'static, NetworksView> {
        Harness::new_ui_state(
            move |ui, view: &mut NetworksView| {
                if let Some(NetworksAction::Inspect(id)) =
                    view.show(ui, supported, "offline", false)
                {
                    view.begin_inspection(id);
                }
            },
            view,
        )
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
                let _ = view.show(ui, true, "", true);
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
    fn accessible_network_navigation_and_inspection_show_only_count() {
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
        harness.state_mut().finish_inspection(
            generation,
            inspection,
            id,
            Ok(details(id, "bridge-net", Some(3))),
        );
        harness.run();
        let _count = harness.get_by_role_and_label(Role::Label, "3");
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
    fn inspection_renders_missing_attached_count_as_not_reported() {
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
        let _not_reported = harness.get_by_role_and_label(Role::Label, "Not reported");
    }
}
