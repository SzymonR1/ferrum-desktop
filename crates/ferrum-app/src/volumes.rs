use crate::{display::escape_untrusted_display_text, search::CaseInsensitiveQuery};
use eframe::egui;
use ferrum_core::{VolumeDetails, VolumeSummary, VolumeUsage, is_untrusted_display_character};

const ROW_HEIGHT: f32 = 34.0;
const MAX_VOLUME_NAME_BYTES: usize = 4096;

fn valid_volume_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_VOLUME_NAME_BYTES
        && !matches!(name, "." | "..")
        && !name.chars().any(is_untrusted_display_character)
}

pub(super) enum VolumeInspectionResult {
    InspectionFailed(String),
    Inspected {
        details: VolumeDetails,
        usage_result: Result<Option<VolumeUsage>, String>,
    },
}

pub(super) enum VolumesAction {
    Inspect(String),
    Create {
        generation: u64,
        name: String,
    },
    Remove {
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VolumeRemovalConfirmation {
    generation: u64,
    inspection_generation: u64,
    name: String,
}

#[derive(Default)]
pub(super) struct VolumesView {
    volumes: Vec<VolumeSummary>,
    visible: Vec<usize>,
    query: String,
    matcher: CaseInsensitiveQuery,
    selected: Option<String>,
    details: Option<VolumeDetails>,
    usage: Option<VolumeUsage>,
    create_name: String,
    create_generation: u64,
    creating: bool,
    create_success: Option<String>,
    create_error: Option<String>,
    removal_generation: u64,
    remove_confirmation: Option<VolumeRemovalConfirmation>,
    removing: bool,
    remove_result_name: Option<String>,
    remove_success: Option<String>,
    remove_error: Option<String>,
    pub(super) loading: bool,
    pub(super) loaded: bool,
    inspecting: bool,
    error: Option<String>,
    detail_error: Option<String>,
    usage_error: Option<String>,
    pub(super) generation: u64,
    pub(super) inspection_generation: u64,
}

impl VolumesView {
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        if !self.creating {
            self.create_generation = self.create_generation.wrapping_add(1);
            self.create_success = None;
            self.create_error = None;
        }
        if !self.removing {
            self.removal_generation = self.removal_generation.wrapping_add(1);
            self.remove_result_name = None;
            self.remove_success = None;
            self.remove_error = None;
        }
        self.remove_confirmation = None;
        self.volumes.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.usage = None;
        self.loading = false;
        self.loaded = false;
        self.inspecting = false;
        self.error = None;
        self.detail_error = None;
        self.usage_error = None;
    }
    pub(super) fn begin_list(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.removal_generation = self.removal_generation.wrapping_add(1);
        self.remove_confirmation = None;
        self.remove_result_name = None;
        self.loading = true;
        self.loaded = false;
        self.error = None;
        self.details = None;
        self.usage = None;
        self.selected = None;
        self.inspecting = false;
        self.volumes.clear();
        self.visible.clear();
        self.detail_error = None;
        self.usage_error = None;
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
        self.usage = None;
        self.detail_error = None;
        self.usage_error = None;
        self.inspecting = true;
        (self.generation, self.inspection_generation, name)
    }
    pub(super) fn finish_inspection(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        name: &str,
        result: VolumeInspectionResult,
    ) {
        if generation != self.generation
            || inspection_generation != self.inspection_generation
            || self.selected.as_deref() != Some(name)
        {
            return;
        }
        self.inspecting = false;
        match result {
            VolumeInspectionResult::Inspected {
                details,
                usage_result,
            } => {
                self.details = Some(details);
                match usage_result {
                    Ok(usage) => self.usage = usage,
                    Err(error) => self.usage_error = Some(error),
                }
            }
            VolumeInspectionResult::InspectionFailed(error) => {
                self.detail_error = Some(error);
            }
        }
    }
    fn begin_create(&mut self) -> Option<VolumesAction> {
        if self.has_mutation_in_flight()
            || self.remove_confirmation.is_some()
            || !valid_volume_name(&self.create_name)
        {
            return None;
        }
        self.create_generation = self.create_generation.wrapping_add(1);
        self.creating = true;
        self.create_success = None;
        self.create_error = None;
        Some(VolumesAction::Create {
            generation: self.create_generation,
            name: self.create_name.clone(),
        })
    }

    pub(super) fn creation_is_pending(&self, generation: u64, name: &str) -> bool {
        self.creating && self.create_generation == generation && self.create_name == name
    }

    pub(super) fn cancel_create_submission(&mut self, generation: u64) {
        if self.creating && self.create_generation == generation {
            self.creating = false;
        }
    }

    pub(super) fn finish_create(
        &mut self,
        generation: u64,
        name: &str,
        result: Result<(), String>,
    ) -> bool {
        if !self.creation_is_pending(generation, name) {
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

    fn selected_volume_has_zero_references(&self) -> bool {
        if self.loading || self.inspecting || !self.loaded || self.error.is_some() {
            return false;
        }
        let Some(name) = self.selected.as_deref() else {
            return false;
        };
        self.volumes.iter().any(|volume| volume.name == name)
            && self
                .details
                .as_ref()
                .is_some_and(|details| details.name == name)
            && self.usage.as_ref().and_then(|usage| usage.reference_count) == Some(0)
    }

    pub(super) fn can_remove_selected_volume(&self) -> bool {
        !self.has_mutation_in_flight()
            && self.remove_confirmation.is_none()
            && self.selected_volume_has_zero_references()
    }

    fn begin_remove_confirmation(&mut self) {
        if self.can_remove_selected_volume()
            && let Some(name) = self.selected.as_ref()
        {
            self.remove_confirmation = Some(VolumeRemovalConfirmation {
                generation: self.generation,
                inspection_generation: self.inspection_generation,
                name: name.clone(),
            });
            self.remove_success = None;
            self.remove_error = None;
        }
    }

    fn cancel_remove_confirmation(&mut self) {
        self.remove_confirmation = None;
    }

    fn confirmation_is_current(&self, confirmation: &VolumeRemovalConfirmation) -> bool {
        !self.has_mutation_in_flight()
            && confirmation.generation == self.generation
            && confirmation.inspection_generation == self.inspection_generation
            && self.selected.as_deref() == Some(confirmation.name.as_str())
            && self.selected_volume_has_zero_references()
    }

    fn confirm_remove(&mut self) -> Option<VolumesAction> {
        let confirmation = self.remove_confirmation.take()?;
        if !self.confirmation_is_current(&confirmation) {
            return None;
        }
        self.removal_generation = self.removal_generation.wrapping_add(1);
        self.removing = true;
        self.remove_result_name = Some(confirmation.name.clone());
        self.remove_success = None;
        self.remove_error = None;
        Some(VolumesAction::Remove {
            generation: confirmation.generation,
            inspection_generation: confirmation.inspection_generation,
            removal_generation: self.removal_generation,
            name: confirmation.name,
        })
    }

    pub(super) fn removal_is_pending(
        &self,
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        name: &str,
    ) -> bool {
        self.removing
            && self.generation == generation
            && self.inspection_generation == inspection_generation
            && self.removal_generation == removal_generation
            && self.selected.as_deref() == Some(name)
            && self.remove_result_name.as_deref() == Some(name)
            && self.selected_volume_has_zero_references()
    }

    pub(super) fn cancel_remove_submission(&mut self, removal_generation: u64) {
        if self.removing && self.removal_generation == removal_generation {
            self.removing = false;
            self.remove_result_name = None;
        }
    }

    pub(super) fn finish_remove(
        &mut self,
        generation: u64,
        inspection_generation: u64,
        removal_generation: u64,
        name: &str,
        result: Result<(), String>,
    ) -> bool {
        if !self.removing
            || self.removal_generation != removal_generation
            || self.remove_result_name.as_deref() != Some(name)
        {
            return false;
        }
        self.removing = false;
        self.remove_result_name = None;
        if self.generation != generation
            || self.inspection_generation != inspection_generation
            || self.selected.as_deref() != Some(name)
        {
            return false;
        }
        self.details = None;
        self.usage = None;
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
    pub(super) fn has_mutation_in_flight(&self) -> bool {
        self.creating || self.removing
    }

    pub(super) fn mark_list_stale(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.loading = false;
        self.loaded = false;
        self.volumes.clear();
        self.visible.clear();
        self.selected = None;
        self.details = None;
        self.usage = None;
        self.inspecting = false;
        self.error = None;
        self.detail_error = None;
        self.usage_error = None;
    }

    pub(super) fn is_loading(&self) -> bool {
        self.loading || self.inspecting || self.creating || self.removing
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
    ) -> Option<VolumesAction> {
        ui.heading("Volumes");
        ui.label(
            "Inventory, name-only creation, and dependency-gated removal for the selected local Docker Unix socket on Linux.",
        );
        ui.label("Creation uses Docker's default local driver. Removal requires a zero-reference usage snapshot and explicit confirmation; Ferrum never uses force. Pruning, modification, connection, and disconnection are not offered.");
        if !supported {
            ui.label(format!(
                "Volumes require a connected local Docker Unix socket on Linux. {connection_message}"
            ));
            return None;
        }

        let mut action = None;
        let mut create_requested = false;
        ui.group(|ui| {
            ui.heading("Create a volume");
            ui.label("Ferrum sends only the name; driver, labels, and options are not configurable.");
            if self.creating {
                ui.spinner();
                ui.label("Creating volume…");
            }
            if let Some(name) = &self.create_success {
                ui.label(format!("Volume '{name}' created."));
            }
            if let Some(error) = &self.create_error {
                ui.colored_label(egui::Color32::LIGHT_RED, "Could not create volume:");
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let name_label = ui.label("Volume name");
            ui.add_enabled(
                !self.has_mutation_in_flight(),
                egui::TextEdit::singleline(&mut self.create_name),
            )
            .labelled_by(name_label.id);
            let name_is_valid = valid_volume_name(&self.create_name);
            if !self.create_name.is_empty() && !name_is_valid {
                ui.label("Enter a non-whitespace volume name up to 4096 UTF-8 bytes without control or invisible formatting characters; '.' and '..' are not allowed.");
            }
            if ui
                .add_enabled(
                    !self.has_mutation_in_flight()
                        && self.remove_confirmation.is_none()
                        && name_is_valid,
                    egui::Button::new("Create volume"),
                )
                .clicked()
            {
                create_requested = true;
            }
        });
        if create_requested {
            action = self.begin_create();
        }
        ui.separator();

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
                    let display_name = escape_untrusted_display_text(&volume.name);
                    if ui
                        .add_sized(
                            [ui.available_width(), ROW_HEIGHT],
                            egui::Button::new(display_name.as_ref())
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
            let name = escape_untrusted_display_text(&details.name);
            ui.heading(name.as_ref());
            crate::untrusted_detail_row(ui, "Driver", details.driver.as_deref());
            crate::untrusted_detail_row(ui, "Scope", details.scope.as_deref());
            crate::untrusted_detail_row(ui, "Created", details.created_at.as_deref());
            crate::untrusted_detail_row(ui, "Host mountpoint", details.mountpoint.as_deref());
            let size = self
                .usage
                .as_ref()
                .and_then(|usage| usage.size_bytes)
                .map(|bytes| format!("{bytes} bytes"));
            crate::detail_row(ui, "Size", size.as_deref());
            let references = self
                .usage
                .as_ref()
                .and_then(|usage| usage.reference_count)
                .map(|count| count.to_string());
            crate::detail_row(ui, "Container references", references.as_deref());
            ui.label(
                "Container-reference count is a point-in-time snapshot and does not guarantee deletion safety.",
            );
            if let Some(error) = &self.usage_error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("Could not retrieve volume usage: {error}"),
                );
            }
        }
        ui.group(|ui| {
            ui.heading("Remove a volume");
            if self.selected.is_none() {
                ui.label("Select and inspect a volume before removal.");
            } else if self.loading || self.inspecting {
                ui.label("Wait for the selected volume inspection to finish.");
            } else if self.details.is_some() {
                match self
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.reference_count)
                {
                    Some(0) => ui.label(
                        "The latest usage snapshot reports zero container references; this is only a point-in-time check.",
                    ),
                    Some(count) => ui.label(format!(
                        "Removal is disabled while {count} container reference(s) are reported."
                    )),
                    None => ui.label(
                        "Removal is disabled because the container reference count is unavailable.",
                    ),
                };
            } else {
                ui.label("Inspect the selected volume before removal.");
            }
            if self.removing
                && let Some(name) = &self.remove_result_name
            {
                let display_name = escape_untrusted_display_text(name);
                ui.label(format!("Removing volume '{display_name}'…"));
            }
            if let Some(name) = &self.remove_success {
                let display_name = escape_untrusted_display_text(name);
                ui.label(format!("Volume '{display_name}' was removed."));
            }
            if let Some(error) = &self.remove_error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    "Volume removal failed; the engine state may have changed.",
                );
                ui.colored_label(egui::Color32::LIGHT_RED, error);
                ui.label("Inspect the volume again before another removal attempt.");
            }
            if ui
                .add_enabled(
                    self.can_remove_selected_volume(),
                    egui::Button::new("Remove volume"),
                )
                .clicked()
            {
                self.begin_remove_confirmation();
            }
            if let Some(confirmation) = self.remove_confirmation.clone() {
                if self.confirmation_is_current(&confirmation) {
                    ui.group(|ui| {
                        ui.heading("Confirm volume removal");
                        let display_name =
                            escape_untrusted_display_text(&confirmation.name);
                        ui.label(format!(
                            "Remove volume '{}' and its data? The latest usage snapshot reports zero container references, but it can change before deletion. Docker may reject removal if the volume becomes in use. Force is not used.",
                            display_name
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Confirm volume removal").clicked() {
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
        if action.is_none()
            && let Some(name) = selected
        {
            action = Some(VolumesAction::Inspect(name));
        }
        action
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

    fn summary(name: &str) -> VolumeSummary {
        VolumeSummary {
            name: name.into(),
            driver: Some("local".into()),
            scope: Some("local".into()),
        }
    }

    fn details(name: &str) -> VolumeDetails {
        VolumeDetails {
            name: name.into(),
            driver: Some("local".into()),
            scope: Some("local".into()),
            created_at: Some("today".into()),
            mountpoint: Some(format!("/var/lib/docker/volumes/{name}")),
        }
    }

    fn usage(size_bytes: Option<u64>, reference_count: Option<u64>) -> VolumeUsage {
        VolumeUsage {
            size_bytes,
            reference_count,
        }
    }

    fn inspection_result(
        details: VolumeDetails,
        usage: Result<Option<VolumeUsage>, String>,
    ) -> VolumeInspectionResult {
        VolumeInspectionResult::Inspected {
            details,
            usage_result: usage,
        }
    }

    fn loaded_view(names: &[&str]) -> VolumesView {
        let mut view = VolumesView::default();
        let generation = view.begin_list();
        view.finish_list(
            generation,
            Ok(names.iter().map(|name| summary(name)).collect()),
        );
        view
    }
    fn removable_view(name: &str, reference_count: Option<u64>) -> VolumesView {
        let mut view = loaded_view(&[name]);
        let (generation, inspection_generation, requested_name) =
            view.begin_inspection(name.to_owned());
        view.finish_inspection(
            generation,
            inspection_generation,
            &requested_name,
            inspection_result(details(name), Ok(Some(usage(Some(128), reference_count)))),
        );
        view
    }

    fn harness_for(view: VolumesView, supported: bool) -> Harness<'static, VolumesView> {
        Harness::new_ui_state(
            move |ui, view: &mut VolumesView| {
                if let Some(VolumesAction::Inspect(name)) = view.show(ui, supported, "offline") {
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
            inspection_result(details("data"), Ok(Some(usage(Some(12), Some(1))))),
        );
        assert!(view.details.is_none());
        assert!(view.usage.is_none());
        assert!(view.selected.is_none());
    }
    #[test]
    fn inspection_failure_is_separate_from_usage_failure() {
        let mut view = loaded_view(&["data"]);
        let (generation, inspection_generation, name) = view.begin_inspection("data".into());
        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            VolumeInspectionResult::InspectionFailed("inspect unavailable".into()),
        );
        assert_eq!(view.detail_error.as_deref(), Some("inspect unavailable"));
        assert!(view.details.is_none());
        assert!(view.usage.is_none());
        assert!(view.usage_error.is_none());
        assert!(!view.inspecting);
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
        let mut view = loaded_view(&["data"]);
        let (generation, inspection, name) = view.begin_inspection("data".into());
        view.finish_inspection(
            generation,
            inspection,
            &name,
            inspection_result(details("data"), Ok(None)),
        );
        assert_eq!(
            view.details.as_ref().unwrap().mountpoint.as_deref(),
            Some("/var/lib/docker/volumes/data")
        );
    }

    #[test]
    fn accessible_volume_row_opens_inspection_and_renders_details() {
        let view = loaded_view(&["project-data"]);
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
            inspection_result(details(name), Ok(Some(usage(Some(1536), Some(2))))),
        );
        harness.run();
        let _mountpoint =
            harness.get_by_role_and_label(Role::Label, "/var/lib/docker/volumes/project-data");
        let _size = harness.get_by_role_and_label(Role::Label, "1536 bytes");
        let _references = harness.get_by_role_and_label(Role::Label, "2");
        let _snapshot_note = harness.get_by_role_and_label(
            Role::Label,
            "Container-reference count is a point-in-time snapshot and does not guarantee deletion safety.",
        );
    }

    #[test]
    fn oversized_engine_volume_details_are_omitted_from_accessible_rendering() {
        let mut oversized_details = details("data");
        let oversized = "x".repeat(4 * 1024 + 1);
        oversized_details.driver = Some(oversized.clone());
        oversized_details.scope = Some(oversized.clone());
        oversized_details.created_at = Some(oversized.clone());
        oversized_details.mountpoint = Some(oversized);

        let mut view = loaded_view(&["data"]);
        let (generation, inspection_generation, name) = view.begin_inspection("data".into());
        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            inspection_result(oversized_details, Ok(None)),
        );

        let mut harness = harness_for(view, true);
        harness.run();
        let omitted = harness.get_all_by_role_and_label(Role::Label, "[display text omitted]");
        assert_eq!(omitted.count(), 4);
    }

    #[test]
    fn accessible_usage_preserves_zero_as_a_reported_value() {
        let mut view = loaded_view(&["empty"]);
        let (generation, inspection_generation, name) = view.begin_inspection("empty".into());
        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            inspection_result(details("empty"), Ok(Some(usage(Some(0), Some(0))))),
        );
        let mut harness = harness_for(view, true);
        harness.run();
        let _zero_bytes = harness.get_by_role_and_label(Role::Label, "0 bytes");
        let _zero_references = harness.get_by_role_and_label(Role::Label, "0");
    }

    #[test]
    fn unavailable_usage_metrics_render_as_not_reported() {
        let mut view = loaded_view(&["unknown"]);
        let (generation, inspection_generation, name) = view.begin_inspection("unknown".into());
        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            inspection_result(details("unknown"), Ok(None)),
        );
        assert!(view.details.is_some());
        assert!(view.usage.is_none());
        let mut harness = harness_for(view, true);
        harness.run();
        let not_reported = harness.get_all_by_role_and_label(Role::Label, "Not reported");
        assert_eq!(not_reported.count(), 2);
        let _size = harness.get_by_role_and_label(Role::Label, "Size");
        let _references = harness.get_by_role_and_label(Role::Label, "Container references");
    }

    #[test]
    fn absent_size_stays_not_reported_while_zero_references_are_shown() {
        let mut view = loaded_view(&["partial"]);
        let (generation, inspection_generation, name) = view.begin_inspection("partial".into());
        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            inspection_result(details("partial"), Ok(Some(usage(None, Some(0))))),
        );
        let mut harness = harness_for(view, true);
        harness.run();
        let not_reported = harness.get_all_by_role_and_label(Role::Label, "Not reported");
        assert_eq!(not_reported.count(), 1);
        let _size = harness.get_by_role_and_label(Role::Label, "Size");
        let _references = harness.get_by_role_and_label(Role::Label, "Container references");
        let _zero_references = harness.get_by_role_and_label(Role::Label, "0");
    }

    #[test]
    fn usage_error_keeps_inspected_details_and_renders_separately() {
        let mut view = loaded_view(&["data"]);
        let (generation, inspection_generation, name) = view.begin_inspection("data".into());
        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            inspection_result(
                details("data"),
                Err("The engine could not return safe volume usage information.".into()),
            ),
        );
        assert_eq!(
            view.details.as_ref().map(|details| details.name.as_str()),
            Some("data")
        );
        assert!(view.detail_error.is_none());
        assert!(view.usage.is_none());
        let mut harness = harness_for(view, true);
        harness.run();
        let _mountpoint =
            harness.get_by_role_and_label(Role::Label, "/var/lib/docker/volumes/data");
        let not_reported = harness.get_all_by_role_and_label(Role::Label, "Not reported");
        assert_eq!(not_reported.count(), 2);
        let _size = harness.get_by_role_and_label(Role::Label, "Size");
        let _references = harness.get_by_role_and_label(Role::Label, "Container references");
        let _usage_error = harness.get_by_role_and_label(
            Role::Label,
            "Could not retrieve volume usage: The engine could not return safe volume usage information.",
        );
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
        let view = loaded_view(&["data"]);
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
            inspection_result(details("data"), Ok(Some(usage(Some(99), Some(3))))),
        );
        harness.run();
        assert!(harness.state().details.is_none());
        assert!(harness.state().usage.is_none());
    }

    #[test]
    fn selection_change_rejects_stale_usage_for_previous_volume() {
        let mut view = loaded_view(&["first", "second"]);
        let stale = view.begin_inspection("first".into());
        let (generation, inspection_generation, name) = view.begin_inspection("second".into());
        view.finish_inspection(
            stale.0,
            stale.1,
            &stale.2,
            inspection_result(details("first"), Ok(Some(usage(Some(99), Some(3))))),
        );
        assert_eq!(view.selected.as_deref(), Some("second"));
        assert!(view.details.is_none());
        assert!(view.usage.is_none());

        view.finish_inspection(
            generation,
            inspection_generation,
            &name,
            inspection_result(details("second"), Ok(None)),
        );
        assert_eq!(
            view.details.as_ref().map(|details| details.name.as_str()),
            Some("second")
        );
        assert!(view.usage.is_none());
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
    #[cfg(target_os = "linux")]
    #[test]
    fn pending_volume_creation_survives_page_invalidation() {
        let mut view = VolumesView {
            create_name: "ferrum-data".to_owned(),
            ..VolumesView::default()
        };
        let VolumesAction::Create { generation, name } =
            view.begin_create().expect("a valid name should submit")
        else {
            panic!("the valid volume name should return a create action");
        };

        view.invalidate();

        assert!(view.creation_is_pending(generation, &name));
        assert!(view.has_mutation_in_flight());
        assert!(view.begin_create().is_none());
        assert!(view.finish_create(generation, &name, Ok(())));
        assert_eq!(view.create_success.as_deref(), Some("ferrum-data"));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn accessible_create_volume_form_submits_name_and_announces_success() {
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut harness = Harness::new_ui_state(
            move |ui, view: &mut VolumesView| {
                if let Some(action) = view.show(ui, true, "offline") {
                    action_sender
                        .send(action)
                        .expect("action receiver should stay open");
                }
            },
            VolumesView::default(),
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Create volume")
                .accesskit_node()
                .is_disabled()
        );

        harness
            .get_by_role_and_label(Role::TextInput, "Volume name")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "Volume name")
            .type_text("ferrum-data");
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Create volume")
            .click_accesskit();
        harness.step();
        harness.step();

        let (generation, name) = match action_receiver
            .try_recv()
            .expect("the Volumes UI should request creation")
        {
            VolumesAction::Create { generation, name } => (generation, name),
            VolumesAction::Inspect(_) | VolumesAction::Remove { .. } => {
                panic!("Create volume should return a create action")
            }
        };
        assert_eq!(name, "ferrum-data");
        assert!(harness.state().creating);
        let _progress = harness.get_by_role_and_label(Role::Label, "Creating volume…");
        assert!(harness.state_mut().finish_create(generation, &name, Ok(())));
        harness.run();
        let _success = harness.get_by_role_and_label(Role::Label, "Volume 'ferrum-data' created.");
        assert!(harness.state().create_name.is_empty());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn volume_create_form_enforces_name_boundaries_accessibly() {
        for invalid_name in [
            "  ",
            ".",
            "..",
            "line\nbreak",
            "name\u{202e}",
            "name\u{e0001}",
            "name\u{e007f}",
        ] {
            let view = VolumesView {
                create_name: invalid_name.to_owned(),
                ..VolumesView::default()
            };
            let mut harness = Harness::new_ui_state(
                |ui, view: &mut VolumesView| {
                    let _ = view.show(ui, true, "");
                },
                view,
            );
            harness.run();
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Create volume")
                    .accesskit_node()
                    .is_disabled(),
                "invalid volume name {invalid_name:?} must not be submittable"
            );
        }

        let view = VolumesView {
            create_name: "é".repeat(MAX_VOLUME_NAME_BYTES / "é".len()),
            ..VolumesView::default()
        };
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut VolumesView| {
                let _ = view.show(ui, true, "");
            },
            view,
        );
        harness.run();
        assert!(
            !harness
                .get_by_role_and_label(Role::Button, "Create volume")
                .accesskit_node()
                .is_disabled(),
            "a name at the UTF-8 byte limit should be submittable"
        );

        let view = VolumesView {
            create_name: "é".repeat(MAX_VOLUME_NAME_BYTES / "é".len() + 1),
            ..VolumesView::default()
        };
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut VolumesView| {
                let _ = view.show(ui, true, "");
            },
            view,
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Create volume")
                .accesskit_node()
                .is_disabled(),
            "a name exceeding the UTF-8 byte limit must not be submittable"
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn failed_volume_creation_preserves_the_name_for_retry() {
        let mut view = VolumesView {
            create_name: "ferrum-data".to_owned(),
            ..VolumesView::default()
        };
        let VolumesAction::Create { generation, name } =
            view.begin_create().expect("a valid name should submit")
        else {
            panic!("the valid volume name should return a create action");
        };
        assert!(view.finish_create(
            generation,
            &name,
            Err("The engine rejected volume creation.".to_owned())
        ));

        let mut harness = Harness::new_ui_state(
            |ui, view: &mut VolumesView| {
                let _ = view.show(ui, true, "");
            },
            view,
        );
        harness.run();
        assert_eq!(harness.state().create_name, "ferrum-data");
        assert!(
            !harness
                .get_by_role_and_label(Role::Button, "Create volume")
                .accesskit_node()
                .is_disabled()
        );
        let _error_heading = harness.get_by_role_and_label(Role::Label, "Could not create volume:");
        let _error =
            harness.get_by_role_and_label(Role::Label, "The engine rejected volume creation.");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn volume_removal_button_requires_reported_zero_container_references() {
        for reference_count in [Some(1), None] {
            let mut harness = harness_for(removable_view("project-data", reference_count), true);
            harness.run();
            assert!(
                harness
                    .get_by_role_and_label(Role::Button, "Remove volume")
                    .accesskit_node()
                    .is_disabled()
            );
        }

        let mut harness = harness_for(removable_view("project-data", Some(0)), true);
        harness.run();
        assert!(
            !harness
                .get_by_role_and_label(Role::Button, "Remove volume")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn accessible_volume_removal_requires_named_confirmation_and_cancel_is_safe() {
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut harness = Harness::new_ui_state(
            move |ui, view: &mut VolumesView| {
                if let Some(action) = view.show(ui, true, "")
                    && matches!(&action, VolumesAction::Remove { .. })
                {
                    action_sender
                        .send(action)
                        .expect("action receiver should stay open");
                }
            },
            removable_view("project-data", Some(0)),
        );
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Remove volume")
            .click_accesskit();
        harness.run();
        let _heading = harness.get_by_role_and_label(Role::Label, "Confirm volume removal");
        let warning = "Remove volume 'project-data' and its data? The latest usage snapshot reports zero container references, but it can change before deletion. Docker may reject removal if the volume becomes in use. Force is not used.";
        let _warning = harness.get_by_role_and_label(Role::Label, warning);
        assert!(action_receiver.try_recv().is_err());

        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .click_accesskit();
        harness.run();
        assert!(action_receiver.try_recv().is_err());
        assert!(!harness.state().removing);

        harness
            .get_by_role_and_label(Role::Button, "Remove volume")
            .click_accesskit();
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Confirm volume removal")
            .click_accesskit();
        harness.step();
        assert!(matches!(
            action_receiver
                .try_recv()
                .expect("explicit confirmation should submit"),
            VolumesAction::Remove {
                name,
                ..
            } if name == "project-data"
        ));
        assert!(harness.state().removing);
    }

    #[cfg(target_os = "linux")]
    fn assert_removable_volume_name_renders_and_removes(raw_name: &str, display_name: &str) {
        let (action_sender, action_receiver) = std::sync::mpsc::channel();
        let mut view = removable_view(raw_name, Some(0));
        view.details.as_mut().unwrap().mountpoint =
            Some("/var/lib/docker/volumes/safe-mountpoint".into());
        let mut harness = Harness::new_ui_state(
            move |ui, view: &mut VolumesView| {
                if let Some(action @ VolumesAction::Remove { .. }) = view.show(ui, true, "") {
                    action_sender
                        .send(action)
                        .expect("action receiver should stay open");
                }
            },
            view,
        );

        harness.run();
        let _row = harness.get_by_role_and_label(Role::Button, display_name);
        let _details = harness.get_by_role_and_label(Role::Label, display_name);
        let _mountpoint =
            harness.get_by_role_and_label(Role::Label, "/var/lib/docker/volumes/safe-mountpoint");
        harness
            .get_by_role_and_label(Role::Button, "Remove volume")
            .click_accesskit();
        harness.run();
        let warning = format!(
            "Remove volume '{display_name}' and its data? The latest usage snapshot reports zero container references, but it can change before deletion. Docker may reject removal if the volume becomes in use. Force is not used."
        );
        let _confirmation = harness.get_by_role_and_label(Role::Label, &warning);
        harness
            .get_by_role_and_label(Role::Button, "Confirm volume removal")
            .click_accesskit();
        harness.step();

        let VolumesAction::Remove {
            generation,
            inspection_generation,
            removal_generation,
            name,
        } = action_receiver
            .try_recv()
            .expect("explicit confirmation should submit")
        else {
            panic!("confirmation should submit a removal");
        };
        assert_eq!(
            name, raw_name,
            "removing a displayed volume should submit its original engine name"
        );
        harness.run();
        let _progress = harness
            .get_by_role_and_label(Role::Label, &format!("Removing volume '{display_name}'…"));
        assert!(harness.state_mut().finish_remove(
            generation,
            inspection_generation,
            removal_generation,
            raw_name,
            Ok(()),
        ));
        harness.run();
        let _success = harness.get_by_role_and_label(
            Role::Label,
            &format!("Volume '{display_name}' was removed."),
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn engine_supplied_volume_names_are_escaped_before_accessible_rendering() {
        assert_removable_volume_name_renders_and_removes(
            "engine\u{202e}volume",
            "engine\\u{202e}volume",
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn oversized_engine_volume_names_are_omitted_from_accessible_rendering() {
        let raw_name = "x".repeat(MAX_VOLUME_NAME_BYTES + 1);
        assert_removable_volume_name_renders_and_removes(&raw_name, "[display text omitted]");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_volume_removal_confirmation_and_results_are_rejected() {
        let mut view = removable_view("project-data", Some(0));
        view.begin_remove_confirmation();
        view.begin_inspection("other-volume".to_owned());
        assert!(view.confirm_remove().is_none());

        let mut view = removable_view("project-data", Some(0));
        view.begin_remove_confirmation();
        let action = view
            .confirm_remove()
            .expect("fresh confirmation should submit");
        let VolumesAction::Remove {
            generation,
            inspection_generation,
            removal_generation,
            name,
        } = action
        else {
            panic!("confirmation should submit a removal");
        };
        view.invalidate();
        assert!(view.has_mutation_in_flight());
        assert!(!view.finish_remove(
            generation,
            inspection_generation,
            removal_generation,
            &name,
            Ok(())
        ));
        assert!(!view.has_mutation_in_flight());
        assert!(view.remove_success.is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_volume_removal_discards_usage_snapshot_and_requires_reinspection() {
        let mut view = removable_view("project-data", Some(0));
        view.begin_remove_confirmation();
        let action = view
            .confirm_remove()
            .expect("zero-reference volume should be removable");
        let VolumesAction::Remove {
            generation,
            inspection_generation,
            removal_generation,
            name,
        } = action
        else {
            panic!("confirmation should submit a removal");
        };

        assert!(view.finish_remove(
            generation,
            inspection_generation,
            removal_generation,
            &name,
            Err("Volume removal was rejected (HTTP 409).".to_owned())
        ));
        assert!(view.details.is_none());
        assert!(view.usage.is_none());
        assert_eq!(
            view.remove_error.as_deref(),
            Some("Volume removal was rejected (HTTP 409).")
        );
        assert!(!view.can_remove_selected_volume());
    }
}
