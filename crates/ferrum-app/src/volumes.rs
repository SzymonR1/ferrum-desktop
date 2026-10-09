use crate::search::CaseInsensitiveQuery;
use eframe::egui;
use ferrum_core::{VolumeDetails, VolumeSummary};

const ROW_HEIGHT: f32 = 34.0;

#[derive(Default)]
pub(super) struct VolumesView {
    volumes: Vec<VolumeSummary>,
    visible: Vec<usize>,
    query: String,
    matcher: CaseInsensitiveQuery,
    selected: Option<String>,
    details: Option<VolumeDetails>,
    pub(super) loading: bool,
    pub(super) loaded: bool,
    inspecting: bool,
    error: Option<String>,
    detail_error: Option<String>,
    pub(super) generation: u64,
    pub(super) inspection_generation: u64,
}

impl VolumesView {
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.volumes.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
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
        self.selected = None;
        self.inspecting = false;
        self.volumes.clear();
        self.visible.clear();
        self.detail_error = None;
        self.generation
    }
    pub(super) fn finish_list(
        &mut self,
        generation: u64,
        result: Result<Vec<VolumeSummary>, String>,
    ) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        self.loaded = true;
        match result {
            Ok(volumes) => {
                self.volumes = volumes;
                self.error = None;
                self.rebuild_visible();
            }
            Err(error) => {
                self.volumes.clear();
                self.visible.clear();
                self.error = Some(error);
            }
        }
    }
    pub(super) fn begin_inspection(&mut self, name: String) -> (u64, u64, String) {
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.selected = Some(name.clone());
        self.details = None;
        self.detail_error = None;
        self.inspecting = true;
        (self.generation, self.inspection_generation, name)
    }
    pub(super) fn finish_inspection(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        name: &str,
        result: Result<VolumeDetails, String>,
    ) {
        if generation != self.generation
            || inspection_generation != self.inspection_generation
            || self.selected.as_deref() != Some(name)
        {
            return;
        }
        self.inspecting = false;
        match result {
            Ok(details) => self.details = Some(details),
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
            self.volumes
                .iter()
                .enumerate()
                .filter_map(|(index, volume)| self.matcher.matches(&volume.name).then_some(index)),
        );
    }
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        supported: bool,
        connection_message: &str,
    ) -> Option<String> {
        ui.heading("Volumes");
        ui.label("Read-only inventory for the selected local Docker Unix socket on Linux.");
        ui.label(
            "Create, remove, connect, disconnect, network operations, and usage reporting are not offered.",
        );
        if !supported {
            ui.label(format!(
                "Volumes require a connected local Docker Unix socket on Linux. {connection_message}"
            ));
            return None;
        }

        let search_label = ui.label("Search volumes");
        let search = ui.text_edit_singleline(&mut self.query);
        let query_changed = search.labelled_by(search_label.id).changed();
        if query_changed {
            self.rebuild_visible();
        }
        if self.loading {
            ui.spinner();
            ui.label("Loading volumes…");
        }
        if let Some(error) = &self.error {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("Could not load volumes: {error}"),
            );
        }
        if self.loaded && self.error.is_none() && self.volumes.is_empty() {
            ui.label("No volumes found.");
        }

        if self.loaded
            && self.error.is_none()
            && !self.volumes.is_empty()
            && self.visible.is_empty()
        {
            ui.label("No volumes match your search.");
        }
        let mut selected = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, self.visible.len(), |ui, rows| {
                for row in rows {
                    let volume = &self.volumes[self.visible[row]];
                    if ui
                        .add_sized(
                            [ui.available_width(), ROW_HEIGHT],
                            egui::Button::new(&volume.name)
                                .selected(self.selected.as_deref() == Some(&volume.name)),
                        )
                        .clicked()
                    {
                        selected = Some(volume.name.clone());
                    }
                }
            });
        if let Some(error) = &self.detail_error {
            ui.label(format!("Could not inspect volume: {error}"));
        }
        if self.inspecting {
            ui.spinner();
            ui.label("Inspecting volume…");
        }
        if let Some(details) = &self.details {
            ui.separator();
            ui.heading(&details.name);
            value(ui, "Driver", details.driver.as_deref());
            value(ui, "Scope", details.scope.as_deref());
            value(ui, "Created", details.created_at.as_deref());
            value(ui, "Host mountpoint", details.mountpoint.as_deref());
        }
        selected
    }
}
fn value(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.label(value.unwrap_or("Not reported"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    fn summary(name: &str) -> VolumeSummary {
        VolumeSummary {
            name: name.into(),
            driver: Some("local".into()),
            scope: Some("local".into()),
        }
    }

    fn harness_for(view: VolumesView, supported: bool) -> Harness<'static, VolumesView> {
        Harness::new_ui_state(
            move |ui, view: &mut VolumesView| {
                let selection = view.show(ui, supported, "offline");
                if let Some(name) = selection {
                    view.begin_inspection(name);
                }
            },
            view,
        )
    }
    #[test]
    fn list_and_inspection_transitions_reject_stale_results() {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        assert!(view.loading);
        view.finish_list(generation, Ok(vec![summary("data")]));
        assert!(view.loaded);
        let (list_generation, inspection_generation, name) = view.begin_inspection("data".into());
        assert!(view.inspecting);
        view.invalidate();
        view.finish_inspection(
            list_generation,
            inspection_generation,
            &name,
            Ok(VolumeDetails {
                name: "data".into(),
                driver: None,
                scope: None,
                created_at: None,
                mountpoint: None,
            }),
        );
        assert!(view.details.is_none());
        assert!(view.selected.is_none());
    }
    #[test]
    fn listing_error_and_empty_are_distinct() {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Err("offline".into()));
        assert_eq!(view.error.as_deref(), Some("offline"));
        let generation = view.begin_list();
        view.finish_list(generation, Ok(Vec::new()));
        assert!(view.error.is_none());
        assert!(view.loaded && view.volumes.is_empty());
    }
    #[test]
    fn inspection_exposes_mountpoint_as_plain_detail_data() {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary("data")]));
        let (generation, inspection, name) = view.begin_inspection("data".into());
        view.finish_inspection(
            generation,
            inspection,
            &name,
            Ok(VolumeDetails {
                name: "data".into(),
                driver: Some("local".into()),
                scope: None,
                created_at: Some("now".into()),
                mountpoint: Some("/var/lib/docker/volumes/data".into()),
            }),
        );
        assert_eq!(
            view.details.as_ref().unwrap().mountpoint.as_deref(),
            Some("/var/lib/docker/volumes/data")
        );
    }

    #[test]
    fn accessible_volume_row_opens_inspection_and_renders_details() {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary("project-data")]));
        let mut harness = harness_for(view, true);
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "project-data")
            .click_accesskit();
        harness.step();
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Inspecting volume…");
        assert!(harness.state().inspecting);
        let (generation, inspection_generation, name) = (
            harness.state().generation,
            harness.state().inspection_generation,
            "project-data",
        );
        harness.state_mut().finish_inspection(
            generation,
            inspection_generation,
            name,
            Ok(VolumeDetails {
                name: name.into(),
                driver: Some("local".into()),
                scope: Some("local".into()),
                created_at: Some("today".into()),
                mountpoint: Some("/var/lib/docker/volumes/project-data".into()),
            }),
        );
        harness.run();
        let _mountpoint =
            harness.get_by_role_and_label(Role::Label, "/var/lib/docker/volumes/project-data");
    }

    #[test]
    fn accessible_view_distinguishes_empty_error_and_unavailable_engine() {
        let mut empty = VolumesView::default();
        let generation = empty.begin_list();
        empty.finish_list(generation, Ok(Vec::new()));
        let mut harness = harness_for(empty, true);
        harness.run();
        let _empty = harness.get_by_role_and_label(Role::Label, "No volumes found.");

        let mut failed = VolumesView::default();
        let generation = failed.begin_list();
        failed.finish_list(generation, Err("offline".into()));
        let mut harness = harness_for(failed, true);
        harness.run();
        let _error = harness.get_by_role_and_label(Role::Label, "Could not load volumes: offline");

        let mut harness = harness_for(VolumesView::default(), false);
        harness.run();
        let _limits = harness.get_by_role_and_label(
            Role::Label,
            "Volumes require a connected local Docker Unix socket on Linux. offline",
        );
    }

    #[test]
    fn accessible_row_stale_inspection_is_rejected_after_list_refresh() {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        view.finish_list(generation, Ok(vec![summary("data")]));
        let mut harness = harness_for(view, true);
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "data")
            .click_accesskit();
        harness.step();
        harness.step();
        let stale = (
            harness.state().generation,
            harness.state().inspection_generation,
        );
        let new_generation = harness.state_mut().begin_list();
        harness
            .state_mut()
            .finish_list(new_generation, Ok(vec![summary("data")]));
        harness.state_mut().finish_inspection(
            stale.0,
            stale.1,
            "data",
            Ok(VolumeDetails {
                name: "data".into(),
                driver: None,
                scope: None,
                created_at: None,
                mountpoint: None,
            }),
        );
        harness.run();
        assert!(harness.state().details.is_none());
    }

    #[test]
    fn accessible_search_shows_filtered_volume_without_scrolling() {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        let mut volumes: Vec<_> = (0..500)
            .map(|index| summary(&format!("volume-{index:03}")))
            .collect();
        volumes.push(summary("target-volume"));
        view.finish_list(generation, Ok(volumes));
        let mut harness = harness_for(view, true);
        harness.run();
        let search = harness.get_by_role_and_label(Role::TextInput, "Search volumes");
        search.focus();
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Search volumes")
            .type_text("target-volume");
        harness.run();
        let _target = harness.get_by_role_and_label(Role::Button, "target-volume");
        assert_eq!(harness.state().visible.len(), 1);
    }
}
