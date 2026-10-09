use crate::search::CaseInsensitiveQuery;

use eframe::egui;
use ferrum_core::{ImageDetails, ImagePullProgress, ImageSummary};
use std::fmt::Display;

const IMAGE_ROW_HEIGHT: f32 = 34.0;
const IMAGE_CELL_HEIGHT: f32 = 32.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum AppPage {
    #[default]
    Containers,
    Images,
}

impl AppPage {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Containers => "Containers",
            Self::Images => "Images",
        }
    }
}

pub(super) fn show_navigation(ui: &mut egui::Ui, active: AppPage) -> Option<AppPage> {
    let mut requested = None;
    for page in [AppPage::Containers, AppPage::Images] {
        let response = ui.add(egui::Button::new(page.title()).selected(active == page));
        if response.clicked() && active != page {
            requested = Some(page);
        }
    }
    requested
}

pub(super) struct ImageInspectionRequest {
    pub(super) list_generation: u64,
    pub(super) inspection_generation: u64,
    pub(super) id: String,
}
pub(super) struct ImageTagRequest {
    pub(super) generation: u64,
    pub(super) image_id: String,
    pub(super) repository: String,
    pub(super) tag: String,
}

#[derive(Default)]
pub(super) struct ImagesView {
    images: Vec<ImageSummary>,
    visible: Vec<usize>,
    query: String,
    matcher: CaseInsensitiveQuery,
    selected_id: Option<String>,
    details: Option<ImageDetails>,
    detail_error: Option<String>,
    inspection_loading: bool,
    loading: bool,
    loaded: bool,
    error: Option<String>,
    generation: u64,
    inspection_generation: u64,
    pull_generation: u64,
    pull_reference: String,
    pull_loading: bool,
    pull_monitoring_stopped: bool,
    pull_complete: bool,
    pull_progress: Option<ImagePullProgress>,
    pull_error: Option<String>,
    pull_requested: bool,
    cancel_requested: bool,
    tag_repository: String,
    tag_name: String,
    tag_generation: u64,
    tag_confirmation_open: bool,
    tag_confirmation_image_id: Option<String>,
    tag_confirmed: bool,
    tag_requested: bool,
    tag_loading: bool,
    tag_complete: bool,
    tag_result_target: Option<String>,
    tag_error: Option<String>,
}
impl ImagesView {
    pub(super) fn is_loading(&self) -> bool {
        self.loading
    }
    pub(super) fn has_loaded(&self) -> bool {
        self.loaded
    }
    pub(super) fn is_tagging(&self) -> bool {
        self.tag_loading
    }

    pub(super) fn is_pulling(&self) -> bool {
        self.pull_loading
    }
    #[cfg(test)]
    pub(super) fn is_monitoring_stopped(&self) -> bool {
        self.pull_monitoring_stopped
    }
    #[cfg(test)]
    pub(super) fn is_pull_complete(&self) -> bool {
        self.pull_complete
    }
    pub(super) fn take_pull_requested(&mut self) -> bool {
        std::mem::take(&mut self.pull_requested)
    }
    pub(super) fn take_cancel_requested(&mut self) -> bool {
        std::mem::take(&mut self.cancel_requested)
    }
    #[cfg(test)]
    pub(super) fn set_pull_reference(&mut self, reference: String) {
        self.pull_reference = reference;
    }
    #[cfg(test)]
    pub(super) fn pull_progress(&self) -> Option<&ImagePullProgress> {
        self.pull_progress.as_ref()
    }

    pub(super) fn invalidate(&mut self) {
        self.reset_inventory();
        self.cancel_pull();
        self.tag_generation = self.tag_generation.wrapping_add(1);
        self.tag_confirmation_open = false;
        self.tag_confirmation_image_id = None;
        self.tag_confirmed = false;
        self.tag_requested = false;
        self.tag_loading = false;
        self.tag_complete = false;
        self.tag_result_target = None;
        self.tag_error = None;
    }

    fn reset_inventory(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.loading = false;
        self.loaded = false;
        self.error = None;
        self.images.clear();
        self.visible.clear();
        self.clear_selection();
        self.tag_confirmation_open = false;
        if !self.tag_loading {
            self.tag_confirmation_image_id = None;
            self.tag_confirmed = false;
            self.tag_requested = false;
        }
    }

    pub(super) fn pull_reference_valid(&self) -> bool {
        let reference = self.pull_reference.as_str();
        !reference.is_empty()
            && reference.len() <= 512
            && !reference
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    }

    pub(super) fn tag_target_valid(&self) -> bool {
        let repository = self.tag_repository.as_str();
        let tag = self.tag_name.as_str();
        let Some(target_length) = repository
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(tag.len()))
        else {
            return false;
        };
        !repository.is_empty()
            && !tag.is_empty()
            && target_length <= 512
            && !repository
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            && valid_docker_tag(tag)
    }

    fn tag_target(&self) -> String {
        format!("{}:{}", self.tag_repository, self.tag_name)
    }

    pub(super) fn begin_tag_confirmation(&mut self) {
        if self.tag_target_valid()
            && !self.tag_loading
            && let Some(image_id) = &self.selected_id
        {
            self.tag_confirmation_image_id = Some(image_id.clone());
            self.tag_confirmation_open = true;
            self.tag_complete = false;
            self.tag_result_target = None;
            self.tag_error = None;
        }
    }

    pub(super) fn cancel_tag_confirmation(&mut self) {
        self.tag_confirmation_open = false;
        self.tag_confirmation_image_id = None;
        self.tag_confirmed = false;
        self.tag_requested = false;
    }

    pub(super) fn confirm_tag(&mut self) {
        if self.tag_confirmation_open
            && self.tag_confirmation_image_id.is_some()
            && self.tag_target_valid()
            && !self.tag_loading
        {
            self.tag_confirmation_open = false;
            self.tag_confirmed = true;
            self.tag_requested = true;
        }
    }

    pub(super) fn take_tag_requested(&mut self) -> bool {
        std::mem::take(&mut self.tag_requested)
    }

    pub(super) fn begin_tag(&mut self) -> Option<ImageTagRequest> {
        if !std::mem::take(&mut self.tag_confirmed) || self.tag_loading || !self.tag_target_valid()
        {
            return None;
        }
        let image_id = self.tag_confirmation_image_id.take()?;
        self.tag_generation = self.tag_generation.wrapping_add(1);
        self.tag_loading = true;
        self.tag_complete = false;
        self.tag_error = None;
        self.tag_result_target = Some(self.tag_target());
        Some(ImageTagRequest {
            generation: self.tag_generation,
            image_id,
            repository: self.tag_repository.clone(),
            tag: self.tag_name.clone(),
        })
    }

    pub(super) fn finish_tag<E: Display>(
        &mut self,
        generation: u64,
        result: Result<(), E>,
    ) -> bool {
        if generation != self.tag_generation || !self.tag_loading {
            return false;
        }
        self.tag_loading = false;
        match result {
            Ok(()) => self.tag_complete = true,
            Err(error) => self.tag_error = Some(error.to_string()),
        }
        true
    }

    pub(super) fn begin_pull(&mut self) -> Option<(u64, String)> {
        if !self.pull_reference_valid() || self.pull_loading {
            return None;
        }
        self.pull_generation = self.pull_generation.wrapping_add(1);
        self.pull_loading = true;
        self.pull_monitoring_stopped = false;
        self.pull_complete = false;
        self.pull_progress = None;
        self.pull_error = None;
        Some((self.pull_generation, self.pull_reference.clone()))
    }

    pub(super) fn update_pull(&mut self, generation: u64, progress: ImagePullProgress) -> bool {
        if generation != self.pull_generation || !self.pull_loading {
            return false;
        }
        self.pull_progress = Some(progress);
        true
    }
    pub(super) fn finish_pull(&mut self, generation: u64, result: Result<(), String>) -> bool {
        if generation != self.pull_generation
            || (!self.pull_loading && !self.pull_monitoring_stopped)
        {
            return false;
        }
        self.pull_loading = false;
        self.pull_monitoring_stopped = false;
        match result {
            Ok(()) => self.pull_complete = true,
            Err(error) => self.pull_error = Some(error),
        }
        true
    }
    pub(super) fn stop_pull_monitoring(&mut self) {
        self.pull_loading = false;
        self.pull_monitoring_stopped = true;
        self.pull_progress = None;
        self.pull_error = None;
        self.pull_complete = false;
    }
    pub(super) fn cancel_pull(&mut self) {
        self.pull_generation = self.pull_generation.wrapping_add(1);
        self.pull_loading = false;
        self.pull_monitoring_stopped = false;
        self.pull_complete = false;
        self.pull_progress = None;
        self.pull_error = None;
        self.pull_requested = false;
        self.cancel_requested = false;
    }

    pub(super) fn begin_load(&mut self) -> u64 {
        self.reset_inventory();
        self.loading = true;
        self.generation
    }

    pub(super) fn complete_load<E: Display>(
        &mut self,
        generation: u64,
        result: Result<Vec<ImageSummary>, E>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        self.loading = false;
        self.loaded = true;
        match result {
            Ok(images) => {
                self.images = images;
                self.error = None;
            }
            Err(error) => {
                self.images.clear();
                self.error = Some(error.to_string());
            }
        }
        self.rebuild_visible();
        true
    }

    pub(super) fn complete_inspection<E: Display>(
        &mut self,
        list_generation: u64,
        inspection_generation: u64,
        id: &str,
        result: Result<ImageDetails, E>,
    ) -> bool {
        if self.generation != list_generation
            || self.inspection_generation != inspection_generation
            || self.selected_id.as_deref() != Some(id)
        {
            return false;
        }
        self.inspection_loading = false;
        match result {
            Ok(details) => {
                self.details = Some(details);
                self.detail_error = None;
            }
            Err(error) => {
                self.details = None;
                self.detail_error = Some(error.to_string());
            }
        }
        true
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        connected: bool,
        connection_message: &str,
        pull_allowed: bool,
        tag_allowed: bool,
        tag_request_busy: bool,
    ) -> Option<ImageInspectionRequest> {
        ui.group(|ui| {
            ui.heading("Pull public image");
            ui.label("Anonymous pulls only; private registry authentication is not supported.");
            ui.horizontal(|ui| {
                let label = ui.label("Image reference");
                ui.text_edit_singleline(&mut self.pull_reference)
                    .labelled_by(label.id);
                if ui
                    .add_enabled(
                        connected
                            && pull_allowed
                            && self.pull_reference_valid()
                            && !self.pull_loading,
                        egui::Button::new("Pull image"),
                    )
                    .clicked()
                {
                    self.pull_requested = true;
                }
                if self.pull_loading && ui.button("Stop monitoring").clicked() {
                    self.cancel_requested = true;
                }
            });
            if self.pull_loading {
                ui.spinner();
                ui.label("Pulling image…");
                if let Some(progress) = &self.pull_progress {
                    ui.label(&progress.status);
                    if let Some(current) = progress.current_bytes {
                        ui.label(format!("{current} bytes downloaded"));
                    }
                    if let Some(total) = progress.total_bytes {
                        ui.label(format!("Total: {total} bytes"));
                    }
                }
            } else if self.pull_monitoring_stopped {
                ui.label("Monitoring stopped. The image transfer may continue in the engine.");
            } else if self.pull_complete {
                ui.label("Image pull complete.");
            } else if let Some(error) = &self.pull_error {
                ui.label("Image pull failed");
                ui.label(error);
            }
        });
        if !connected {
            ui.heading("Image list unavailable");
            ui.label(connection_message);
            self.draw_tag_controls(ui, false, tag_allowed, tag_request_busy);
            return None;
        }

        let mut query_changed = false;
        ui.horizontal(|ui| {
            let label = ui.label("Search images");
            query_changed = ui
                .text_edit_singleline(&mut self.query)
                .labelled_by(label.id)
                .changed();
        });
        if query_changed {
            self.rebuild_visible();
        }

        let mut selected_id = None;
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading images…");
            });
        } else if let Some(error) = &self.error {
            ui.colored_label(
                egui::Color32::from_rgb(227, 113, 102),
                "Image inventory unavailable",
            );
            ui.label(error);
        } else if self.images.is_empty() {
            ui.heading("No images found");
            ui.label("The connected engine reported an empty image inventory.");
        } else if self.visible.is_empty() {
            ui.heading("No matching images");
            ui.label("Try a different search.");
        } else {
            selected_id = self.draw_image_list(ui);
        }

        let request = selected_id.map(|id| self.select_image(id));
        self.draw_details(ui);
        self.draw_tag_controls(ui, connected, tag_allowed, tag_request_busy);
        request
    }

    fn draw_image_list(&self, ui: &mut egui::Ui) -> Option<String> {
        let width = ui.available_width();
        let tag_width = width * 0.38;
        let id_width = width * 0.42;
        let size_width = width - tag_width - id_width;
        ui.horizontal(|ui| {
            image_cell(ui, "Repository tags", tag_width);
            image_cell(ui, "Image ID", id_width);
            image_cell(ui, "Size", size_width);
        });
        ui.separator();

        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("image-inventory")
            .max_height((ui.available_height() - 190.0).clamp(80.0, 360.0))
            .show_rows(ui, IMAGE_ROW_HEIGHT, self.visible.len(), |ui, rows| {
                for row in rows {
                    let image = &self.images[self.visible[row]];
                    let tag = image
                        .repo_tags
                        .first()
                        .map(String::as_str)
                        .unwrap_or("Untagged image");
                    ui.horizontal(|ui| {
                        let response = ui.add_sized(
                            [tag_width, IMAGE_CELL_HEIGHT],
                            egui::Button::new(egui::RichText::new(tag).strong())
                                .selected(self.selected_id.as_deref() == Some(image.id.as_str())),
                        );
                        let clicked = response.clicked();
                        response.on_hover_ui(|ui| {
                            if image.repo_tags.is_empty() {
                                ui.label("Untagged image");
                            } else {
                                ui.label("Repository tags");
                                for tag in &image.repo_tags {
                                    ui.label(tag);
                                }
                            }
                            ui.label(&image.id);
                        });
                        if clicked {
                            selected = Some(image.id.clone());
                        }
                        ui.add_sized(
                            [id_width, IMAGE_CELL_HEIGHT],
                            egui::Label::new(image.id.as_str())
                                .truncate()
                                .show_tooltip_when_elided(true),
                        );
                        ui.add_sized(
                            [size_width, IMAGE_CELL_HEIGHT],
                            egui::Label::new(format_size(image.size_bytes))
                                .truncate()
                                .show_tooltip_when_elided(true),
                        );
                    });
                    ui.add_space(2.0);
                }
            });
        selected
    }

    fn draw_details(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();
        ui.heading("Image details");
        if self.selected_id.is_none() {
            ui.label("Select an image to view its read-only details.");
        } else if self.inspection_loading {
            ui.label("Loading image details…");
        } else if let Some(error) = &self.detail_error {
            ui.colored_label(
                egui::Color32::from_rgb(227, 113, 102),
                "Image inspection unavailable",
            );
            ui.label(error);
        } else if let Some(details) = &self.details {
            let tags = if details.repo_tags.is_empty() {
                "Untagged image".to_owned()
            } else {
                details.repo_tags.join(", ")
            };
            image_detail(ui, "Image ID", Some(details.id.as_str()));
            image_detail(ui, "Repository tags", Some(tags.as_str()));
            image_detail(ui, "Created", details.created.as_deref());
            let size = format_size(details.size_bytes);
            image_detail(ui, "Size", Some(size.as_str()));
            image_detail(ui, "Architecture", details.architecture.as_deref());
            image_detail(ui, "Operating system", details.os.as_deref());
            image_detail(ui, "Author", details.author.as_deref());
        }
    }

    fn select_image(&mut self, id: String) -> ImageInspectionRequest {
        self.selected_id = Some(id.clone());
        self.details = None;
        self.detail_error = None;
        self.inspection_loading = true;
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        ImageInspectionRequest {
            list_generation: self.generation,
            inspection_generation: self.inspection_generation,
            id,
        }
    }

    fn clear_selection(&mut self) {
        self.selected_id = None;
        self.details = None;
        self.detail_error = None;
        self.inspection_loading = false;
    }

    fn rebuild_visible(&mut self) {
        self.matcher.set(&self.query);
        self.visible.clear();
        self.visible
            .extend(self.images.iter().enumerate().filter_map(|(index, image)| {
                (self.matcher.matches(&image.id)
                    || image.repo_tags.iter().any(|tag| self.matcher.matches(tag)))
                .then_some(index)
            }));
    }

    fn draw_tag_controls(
        &mut self,
        ui: &mut egui::Ui,
        connected: bool,
        tag_allowed: bool,
        tag_request_busy: bool,
    ) {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Tag selected image");
            if self.selected_id.is_none() {
                ui.label("Select an image before tagging it.");
            }
            if !tag_allowed && connected && !tag_request_busy {
                ui.label("Image tagging is available only through the local Docker Unix socket on Linux.");
            }
            if tag_request_busy && !self.tag_loading {
                ui.label("A previous image tag request is still in progress; wait for its result before starting another.");
            }
            ui.add_enabled_ui(!self.tag_loading && !self.tag_confirmation_open, |ui| {
                ui.horizontal(|ui| {
                    let label = ui.label("Repository");
                    ui.text_edit_singleline(&mut self.tag_repository)
                        .labelled_by(label.id);
                });
                ui.horizontal(|ui| {
                    let label = ui.label("Tag");
                    ui.text_edit_singleline(&mut self.tag_name)
                        .labelled_by(label.id);
                });
            });
            let target_valid = self.tag_target_valid();
            if !target_valid {
                ui.label("Enter a repository and a valid Docker tag; the combined target is limited to 512 bytes.");
            }
            if ui
                .add_enabled(
                    connected
                        && tag_allowed
                        && !tag_request_busy
                        && self.selected_id.is_some()
                        && target_valid
                        && !self.tag_loading
                        && !self.tag_confirmation_open,
                    egui::Button::new("Tag image"),
                )
                .clicked()
            {
                self.begin_tag_confirmation();
            }
            if self.tag_confirmation_open
                && let Some(image_id) = self.tag_confirmation_image_id.clone()
            {
                let target = self.tag_target();
                ui.group(|ui| {
                    ui.heading("Confirm image tag");
                    ui.label(format!(
                        "Tag image {image_id} as {target}? Docker may reassign an existing repository:tag; the image data remains. Confirm to send this request to the connected local Docker engine."
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Confirm tag").clicked() {
                            self.confirm_tag();
                        }
                        if ui.button("Cancel").clicked() {
                            self.cancel_tag_confirmation();
                        }
                    });
                });
            }
            if self.tag_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    if let Some(target) = self.tag_result_target.as_deref() {
                        ui.label(format!("Tagging image as {target}…"));
                    }
                });
            } else if self.tag_complete {
                if let Some(target) = &self.tag_result_target {
                    ui.label(format!(
                        "Image tagged as {target}. Docker may have reassigned an existing repository:tag; the image data remains."
                    ));
                }
            } else if let Some(error) = &self.tag_error {
                ui.label("Image tag request failed.");
                ui.label(error);
            }
        });
    }
}

fn valid_docker_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    bytes.len() <= 128
        && (first.is_ascii_alphanumeric() || first == b'_')
        && rest
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn image_cell(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.add_sized(
        [width, IMAGE_CELL_HEIGHT],
        egui::Label::new(egui::RichText::new(text).small().strong().weak()).truncate(),
    );
}

fn image_detail(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.unwrap_or("Not reported"));
    });
}

fn format_size(size: Option<u64>) -> String {
    size.map_or_else(
        || "Not reported".to_owned(),
        |bytes| format!("{bytes} bytes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    #[derive(Default)]
    struct NavigationUi {
        page: AppPage,
    }

    struct ImagesUi {
        view: ImagesView,
        inspection: Option<ImageInspectionRequest>,
    }

    fn harness_for(view: ImagesView) -> Harness<'static, ImagesUi> {
        Harness::new_ui_state(
            |ui, state: &mut ImagesUi| {
                if let Some(request) = state.view.show(ui, true, "", true, true, false) {
                    state.inspection = Some(request);
                }
            },
            ImagesUi {
                view,
                inspection: None,
            },
        )
    }

    fn image(id: &str, tag: &str, size_bytes: u64) -> ImageSummary {
        ImageSummary {
            id: id.to_owned(),
            repo_tags: vec![tag.to_owned()],
            created_unix_seconds: Some(1_728_000_000),
            size_bytes: Some(size_bytes),
        }
    }

    fn details(id: &str, tag: &str) -> ImageDetails {
        ImageDetails {
            id: id.to_owned(),
            repo_tags: vec![tag.to_owned()],
            created: Some("2026-10-09T10:00:00Z".to_owned()),
            size_bytes: Some(1024),
            architecture: Some("arm64".to_owned()),
            os: Some("linux".to_owned()),
            author: Some("Ferrum test".to_owned()),
        }
    }

    fn loaded_view(images: Vec<ImageSummary>) -> ImagesView {
        let mut view = ImagesView::default();
        let generation = view.begin_load();
        assert!(view.complete_load(generation, Ok::<_, &str>(images)));
        view
    }
    fn taggable_view() -> ImagesView {
        let mut view = loaded_view(vec![image("sha256:image", "existing:tag", 1024)]);
        view.selected_id = Some("sha256:image".to_owned());
        view.tag_repository = "example/image".to_owned();
        view.tag_name = "release-1".to_owned();
        view
    }

    #[test]
    fn tag_target_validation_matches_bounded_docker_tag_rules() {
        let mut view = ImagesView::default();
        assert!(!view.tag_target_valid());
        view.tag_repository = "example/image".to_owned();
        view.tag_name = "release_1.2-a".to_owned();
        assert!(view.tag_target_valid());
        view.tag_name = "bad tag".to_owned();
        assert!(!view.tag_target_valid());
        view.tag_name = "-invalid".to_owned();
        assert!(!view.tag_target_valid());
        view.tag_name = "x".repeat(129);
        assert!(!view.tag_target_valid());
        view.tag_name = "v1".to_owned();
        view.tag_repository = "example\nimage".to_owned();
        assert!(!view.tag_target_valid());
        view.tag_repository = "r".repeat(509);
        assert!(view.tag_target_valid());
        view.tag_repository.push('r');
        assert!(!view.tag_target_valid());
    }

    #[test]
    fn accessible_tag_inputs_disable_invalid_target() {
        let mut view = taggable_view();
        view.tag_name = "invalid tag".to_owned();
        let mut harness = harness_for(view);
        harness.run();
        let _repository = harness.get_by_role_and_label(Role::TextInput, "Repository");
        let _tag = harness.get_by_role_and_label(Role::TextInput, "Tag");
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Tag image")
                .accesskit_node()
                .is_disabled()
        );
    }
    #[test]
    fn accessible_tag_inputs_accept_a_valid_repository_and_tag() {
        let mut view = taggable_view();
        view.tag_repository.clear();
        view.tag_name.clear();
        let mut harness = harness_for(view);
        harness.run();
        let repository = harness.get_by_role_and_label(Role::TextInput, "Repository");
        repository.focus();
        harness.run();
        harness.event(egui::Event::Text("example/service".to_owned()));
        harness.run();
        let tag = harness.get_by_role_and_label(Role::TextInput, "Tag");
        tag.focus();
        harness.run();
        harness.event(egui::Event::Text("release_1".to_owned()));
        harness.run();
        assert!(harness.state().view.tag_target_valid());
        assert!(
            !harness
                .get_by_role_and_label(Role::Button, "Tag image")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn outstanding_tag_request_disables_a_second_request() {
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, true, "", true, true, true);
            },
            taggable_view(),
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Tag image")
                .accesskit_node()
                .is_disabled()
        );
        let _notice = harness.get_by_role_and_label(
            Role::Label,
            "A previous image tag request is still in progress; wait for its result before starting another.",
        );
    }

    #[test]
    fn tagging_requires_confirmation_and_cancel_sends_no_request() {
        let mut harness = harness_for(taggable_view());
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Tag image")
            .click_accesskit();
        harness.run();
        let _confirmation = harness.get_by_role_and_label(Role::Label, "Confirm image tag");
        let _overwrite_warning = harness.get_by_role_and_label(
            Role::Label,
            "Tag image sha256:image as example/image:release-1? Docker may reassign an existing repository:tag; the image data remains. Confirm to send this request to the connected local Docker engine.",
        );

        assert!(!harness.state().view.tag_requested);
        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .click_accesskit();
        harness.run();
        assert!(!harness.state_mut().view.take_tag_requested());
        assert!(!harness.state().view.tag_loading);
    }

    #[test]
    fn confirmed_tag_shows_progress_and_success_result() {
        let mut harness = harness_for(taggable_view());
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Tag image")
            .click_accesskit();
        harness.run();
        harness.state_mut().view.selected_id = Some("sha256:other".to_owned());

        harness
            .get_by_role_and_label(Role::Button, "Confirm tag")
            .click_accesskit();
        harness.run();
        assert!(harness.state_mut().view.take_tag_requested());
        let request = harness.state_mut().view.begin_tag().unwrap();
        assert_eq!(request.image_id, "sha256:image");
        assert_eq!(request.repository, "example/image");
        assert_eq!(request.tag, "release-1");
        harness.step();
        let _progress =
            harness.get_by_role_and_label(Role::Label, "Tagging image as example/image:release-1…");
        assert!(
            harness
                .state_mut()
                .view
                .finish_tag(request.generation, Ok::<_, &str>(()))
        );
        harness.run();
        let _success = harness.get_by_role_and_label(
            Role::Label,
            "Image tagged as example/image:release-1. Docker may have reassigned an existing repository:tag; the image data remains.",
        );

        harness
            .get_by_role_and_label(Role::Button, "Tag image")
            .click_accesskit();
        harness.run();
        assert!(!harness.state().view.tag_requested);
        harness
            .get_by_role_and_label(Role::Button, "Confirm tag")
            .click_accesskit();
        harness.run();
        assert!(harness.state_mut().view.take_tag_requested());
        let repeated = harness.state_mut().view.begin_tag().unwrap();
        assert_eq!(repeated.image_id, "sha256:other");
        assert!(
            harness
                .state_mut()
                .view
                .finish_tag(repeated.generation, Ok::<_, &str>(()))
        );
        harness.state_mut().view.begin_load();
        harness.step();
        let _success_after_refresh = harness.get_by_role_and_label(
            Role::Label,
            "Image tagged as example/image:release-1. Docker may have reassigned an existing repository:tag; the image data remains.",
        );
    }

    #[test]
    fn tag_failure_is_safe_and_completion_after_invalidation_is_ignored() {
        let mut view = taggable_view();
        view.begin_tag_confirmation();
        view.confirm_tag();
        assert!(view.take_tag_requested());
        let request = view.begin_tag().unwrap();
        assert!(view.finish_tag(
            request.generation,
            Err::<(), _>("ImageTagRejected: endpoint returned HTTP 409")
        ));
        let mut harness = harness_for(view);
        harness.run();
        let _failure = harness.get_by_role_and_label(Role::Label, "Image tag request failed.");
        let _safe_error = harness
            .get_by_role_and_label(Role::Label, "ImageTagRejected: endpoint returned HTTP 409");

        let mut view = taggable_view();
        view.begin_tag_confirmation();
        view.confirm_tag();
        assert!(view.take_tag_requested());
        let stale_request = view.begin_tag().unwrap();
        view.invalidate();
        assert!(!view.finish_tag(stale_request.generation, Ok::<_, &str>(())));
        assert!(!view.tag_loading);
        assert!(!view.tag_complete);
        assert!(view.tag_error.is_none());
    }

    #[test]
    fn navigation_opens_the_images_page_through_an_accessible_action() {
        let mut harness = Harness::new_ui_state(
            |ui, state: &mut NavigationUi| {
                if let Some(page) = show_navigation(ui, state.page) {
                    state.page = page;
                }
                ui.heading(state.page.title());
            },
            NavigationUi::default(),
        );
        harness.run();
        let _containers = harness.get_by_role_and_label(Role::Label, "Containers");
        harness
            .get_by_role_and_label(Role::Button, "Images")
            .click_accesskit();
        harness.run();
        let _images = harness.get_by_role_and_label(Role::Label, "Images");
    }

    #[test]
    fn search_and_keyboard_selection_show_inspected_image_details() {
        let mut harness = harness_for(loaded_view(vec![
            image("sha256:alpine", "alpine:3.20", 1024),
            image("sha256:busybox", "busybox:1.36", 2048),
        ]));
        harness.run();
        let search = harness.get_by_role_and_label(Role::TextInput, "Search images");
        search.focus();
        harness.run();
        harness.event(egui::Event::Text("ALPINE".to_owned()));
        harness.run();
        let row = harness.get_by_role_and_label(Role::Button, "alpine:3.20");
        assert_eq!(harness.state().view.visible.len(), 1);
        row.focus();
        harness.run();
        harness.key_down(egui::Key::Space);
        harness.run();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading image details…");
        let request = harness.state_mut().inspection.take().unwrap();
        assert!(harness.state_mut().view.complete_inspection(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(details("sha256:alpine", "alpine:3.20")),
        ));
        harness.run();
        let _os = harness.get_by_role_and_label(Role::Label, "linux");
        let _architecture = harness.get_by_role_and_label(Role::Label, "arm64");
    }

    #[test]
    fn image_inspection_failure_is_visible() {
        let mut harness = harness_for(loaded_view(vec![image(
            "sha256:denied",
            "private:latest",
            64,
        )]));
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "private:latest")
            .click_accesskit();
        harness.run();
        let request = harness.state_mut().inspection.take().unwrap();
        assert!(harness.state_mut().view.complete_inspection(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Err::<ImageDetails, _>("inspection denied"),
        ));
        harness.run();
        let _error = harness.get_by_role_and_label(Role::Label, "Image inspection unavailable");
        let _message = harness.get_by_role_and_label(Role::Label, "inspection denied");
    }

    #[test]
    fn image_rows_are_virtualized_for_large_inventories() {
        let images = (0..1_000)
            .map(|index| {
                image(
                    &format!("sha256:{index:04}"),
                    &format!("example:{index:04}"),
                    index,
                )
            })
            .collect();
        let mut harness = harness_for(loaded_view(images));
        harness.run();
        assert!(
            harness.get_all_by_role(Role::Button).count() < 100,
            "the accessible tree should contain only visible image rows"
        );
        let _first_image = harness.get_by_role_and_label(Role::Button, "example:0000");
    }

    #[test]
    fn loading_error_and_successful_empty_states_are_visible() {
        let mut view = ImagesView::default();
        let generation = view.begin_load();
        let mut harness = harness_for(view);
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading images…");
        assert!(harness.state_mut().view.complete_load(
            generation,
            Err::<Vec<ImageSummary>, _>("scripted connection failure"),
        ));
        harness.run();
        let _error = harness.get_by_role_and_label(Role::Label, "Image inventory unavailable");
        let _message = harness.get_by_role_and_label(Role::Label, "scripted connection failure");

        let next_generation = harness.state_mut().view.begin_load();
        assert!(
            harness
                .state_mut()
                .view
                .complete_load(next_generation, Ok::<_, &str>(Vec::new()))
        );
        harness.run();
        let _empty = harness.get_by_role_and_label(Role::Label, "No images found");
    }

    #[test]
    fn stale_image_results_are_not_shown_after_a_context_refresh() {
        let mut view = ImagesView::default();
        let stale_generation = view.begin_load();
        let current_generation = view.begin_load();
        view.complete_load(
            stale_generation,
            Ok::<_, &str>(vec![image("old", "old:tag", 1)]),
        );
        view.complete_load(
            current_generation,
            Ok::<_, &str>(vec![image("current", "current:tag", 2)]),
        );
        let mut harness = harness_for(view);
        harness.run();
        let _current = harness.get_by_role_and_label(Role::Button, "current:tag");
        assert_eq!(harness.state().view.visible.len(), 1);

        harness
            .get_by_role_and_label(Role::Button, "current:tag")
            .click_accesskit();
        harness.run();
        let stale_inspection = harness.state_mut().inspection.take().unwrap();

        let next_generation = harness.state_mut().view.begin_load();
        harness.state_mut().view.complete_load(
            next_generation,
            Ok::<_, &str>(vec![image("fresh", "fresh:tag", 3)]),
        );
        harness.run();
        let _fresh = harness.get_by_role_and_label(Role::Button, "fresh:tag");
        assert_eq!(harness.state().view.visible.len(), 1);
        harness
            .get_by_role_and_label(Role::Button, "fresh:tag")
            .click_accesskit();
        harness.run();
        let current_inspection = harness.state_mut().inspection.take().unwrap();

        harness.state_mut().view.complete_inspection(
            stale_inspection.list_generation,
            stale_inspection.inspection_generation,
            &stale_inspection.id,
            Ok::<_, &str>(details("current", "current:tag")),
        );
        harness.run();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading image details…");

        harness.state_mut().view.complete_inspection(
            current_inspection.list_generation,
            current_inspection.inspection_generation,
            &current_inspection.id,
            Ok::<_, &str>(details("fresh", "fresh:tag")),
        );
        harness.run();
        let _fresh_tag = harness.get_by_role_and_label(Role::Label, "fresh:tag");
    }

    fn progress(
        status: &str,
        current_bytes: Option<u64>,
        total_bytes: Option<u64>,
    ) -> ImagePullProgress {
        ImagePullProgress {
            status: status.to_owned(),
            current_bytes,
            total_bytes,
        }
    }

    #[test]
    fn accessible_pull_action_starts_only_from_connected_view_without_engine_calls() {
        let mut harness = harness_for(ImagesView::default());
        harness
            .state_mut()
            .view
            .set_pull_reference("alpine:latest".to_owned());
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Pull image")
            .click_accesskit();
        harness.run();
        assert!(harness.state().view.pull_requested);
        assert!(harness.state_mut().view.take_pull_requested());
        let (generation, reference) = harness.state_mut().view.begin_pull().unwrap();
        assert_eq!(reference, "alpine:latest");
        assert!(harness.state().view.pull_loading);
        assert!(
            harness
                .state_mut()
                .view
                .update_pull(generation, progress("Downloading", Some(10), Some(100)))
        );
        assert!(
            harness
                .state_mut()
                .view
                .update_pull(generation, progress("Extracting", Some(75), None))
        );
        assert_eq!(
            harness.state().view.pull_progress.as_ref().unwrap().status,
            "Extracting"
        );
        assert_eq!(
            harness
                .state()
                .view
                .pull_progress
                .as_ref()
                .unwrap()
                .total_bytes,
            None
        );
        harness.step();
        let _latest = harness.get_by_role_and_label(Role::Label, "Extracting");
        harness.state_mut().view.finish_pull(generation, Ok(()));
        harness.run();
        let _complete = harness.get_by_role_and_label(Role::Label, "Image pull complete.");
    }

    #[test]
    fn disconnected_view_disables_pull_without_engine_access() {
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, false, "offline", false, false, false);
            },
            ImagesView::default(),
        );

        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Pull image")
                .accesskit_node()
                .is_disabled()
        );
        assert!(!harness.state().pull_requested);
    }

    #[test]
    fn pull_error_is_visible_and_stale_generations_are_ignored() {
        let mut view = ImagesView {
            pull_reference: "alpine:latest".to_owned(),
            ..ImagesView::default()
        };
        let (stale, _) = view.begin_pull().unwrap();
        view.cancel_pull();
        let (current, _) = view.begin_pull().unwrap();
        assert!(!view.update_pull(stale, progress("stale", Some(1), Some(2))));
        assert!(!view.finish_pull(stale, Ok(())));
        assert!(view.finish_pull(current, Err("Image pull failed safely.".to_owned())));
        let mut harness = harness_for(view);
        harness.run();
        let _error = harness.get_by_role_and_label(Role::Label, "Image pull failed");
        let _message = harness.get_by_role_and_label(Role::Label, "Image pull failed safely.");
    }

    #[test]
    fn reference_validation_disables_invalid_lengths_and_characters() {
        let mut view = ImagesView::default();
        assert!(!view.pull_reference_valid());
        view.pull_reference = " ".to_owned();
        assert!(!view.pull_reference_valid());
        view.pull_reference = "alpine latest".to_owned();
        assert!(!view.pull_reference_valid());
        view.pull_reference = "alpine\nlatest".to_owned();
        assert!(!view.pull_reference_valid());
        view.pull_reference = "x".repeat(513);
        assert!(!view.pull_reference_valid());
        view.pull_reference = "x".repeat(513);
        let mut invalid_harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, true, "", true, true, false);
            },
            view,
        );

        invalid_harness.run();
        assert!(
            invalid_harness
                .get_by_role_and_label(Role::Button, "Pull image")
                .accesskit_node()
                .is_disabled()
        );
        let boundary_view = ImagesView {
            pull_reference: "x".repeat(512),
            ..ImagesView::default()
        };
        let mut boundary_harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, true, "", true, true, false);
            },
            boundary_view,
        );

        boundary_harness.run();
        assert!(
            !boundary_harness
                .get_by_role_and_label(Role::Button, "Pull image")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn completed_pull_notice_survives_inventory_refresh() {
        let mut view = ImagesView {
            pull_reference: "alpine:latest".to_owned(),
            ..ImagesView::default()
        };
        let (generation, _) = view.begin_pull().unwrap();
        assert!(view.finish_pull(generation, Ok(())));
        view.begin_load();
        let mut harness = harness_for(view);
        harness.step();
        let _complete = harness.get_by_role_and_label(Role::Label, "Image pull complete.");
    }

    #[test]
    fn zero_total_displays_reported_bytes_without_a_percentage() {
        let view = ImagesView {
            pull_loading: true,
            pull_progress: Some(progress("Downloading", Some(0), Some(0))),
            ..ImagesView::default()
        };
        let mut harness = harness_for(view);
        harness.step();
        let _current = harness.get_by_role_and_label(Role::Label, "0 bytes downloaded");
        let _total = harness.get_by_role_and_label(Role::Label, "Total: 0 bytes");
    }
    #[test]
    fn accessible_stop_monitoring_requests_local_stream_abort() {
        let mut view = ImagesView {
            pull_reference: "alpine:latest".to_owned(),
            ..ImagesView::default()
        };
        let (generation, _) = view.begin_pull().unwrap();
        view.update_pull(generation, progress("Downloading", Some(10), Some(100)));
        let mut harness = harness_for(view);
        harness.step();
        harness
            .get_by_role_and_label(Role::Button, "Stop monitoring")
            .click_accesskit();
        harness.step();
        assert!(harness.state().view.cancel_requested);
        harness.state_mut().view.cancel_requested = false;
        harness.state_mut().view.stop_pull_monitoring();
        assert!(!harness.state().view.is_pulling());
        assert!(harness.state().view.is_monitoring_stopped());
        assert!(harness.state().view.pull_progress().is_none());
    }
    #[test]
    fn queued_success_after_stop_monitoring_completes_and_remains_refreshable() {
        let mut view = ImagesView::default();
        view.set_pull_reference("alpine:latest".to_owned());
        let (generation, _) = view.begin_pull().unwrap();
        view.stop_pull_monitoring();
        assert!(view.is_monitoring_stopped());
        assert!(view.finish_pull(generation, Ok(())));
        assert!(view.is_pull_complete());
        assert!(!view.is_monitoring_stopped());
        view.begin_load();
        let mut harness = harness_for(view);
        harness.step();
        let _complete = harness.get_by_role_and_label(Role::Label, "Image pull complete.");
    }

    #[test]
    fn a_new_pull_rejects_completion_from_stopped_attempt() {
        let mut view = ImagesView::default();
        view.set_pull_reference("alpine:latest".to_owned());
        let (stale_generation, _) = view.begin_pull().unwrap();
        view.stop_pull_monitoring();
        let (current_generation, _) = view.begin_pull().unwrap();
        assert_ne!(stale_generation, current_generation);
        assert!(!view.finish_pull(stale_generation, Ok(())));
        assert!(!view.update_pull(stale_generation, progress("old attempt", Some(1), Some(2))));
        assert!(view.is_pulling());
        assert!(!view.is_pull_complete());
    }
}
