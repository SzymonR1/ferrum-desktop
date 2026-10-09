use crate::search::CaseInsensitiveQuery;
use eframe::egui;
use ferrum_core::{NetworkDetails, NetworkSummary};

const ROW_HEIGHT: f32 = 34.0;

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
        self.networks.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.attached_container_count_display = None;
        self.loading = false;
        self.loaded = false;
        self.inspecting = false;
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
        self.loading || self.inspecting
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
    ) -> Option<String> {
        ui.heading("Networks");
        ui.label("Read-only inventory for the selected local Docker Unix socket on Linux.");
        ui.label("Network create, remove, connect, and disconnect operations are not offered.");
        if !supported {
            ui.label("Networks require a connected local Docker Unix socket on Linux.");
            ui.label(connection_message);
            return None;
        }
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
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

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
                if let Some(id) = view.show(ui, supported, "offline") {
                    view.begin_inspection(id);
                }
            },
            view,
        )
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
