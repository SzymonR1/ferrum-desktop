use crate::{display::escape_untrusted_display_text, search::CaseInsensitiveQuery};

use eframe::egui;
use ferrum_core::{
    ImageDetails, ImagePullProgress, ImagePushProgress, ImagePushTarget, ImageSummary,
};
use std::fmt::Display;

const IMAGE_ROW_HEIGHT: f32 = 34.0;
const IMAGE_CELL_HEIGHT: f32 = 32.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum AppPage {
    #[default]
    Containers,
    Images,
    Volumes,
    Networks,
}

impl AppPage {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Containers => "Containers",
            Self::Images => "Images",
            Self::Volumes => "Volumes",
            Self::Networks => "Networks",
        }
    }
}

pub(super) fn show_navigation(ui: &mut egui::Ui, active: AppPage) -> Option<AppPage> {
    let mut requested = None;
    for page in [
        AppPage::Containers,
        AppPage::Images,
        AppPage::Volumes,
        AppPage::Networks,
    ] {
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
pub(super) struct ImageRemoveRequest {
    pub(super) generation: u64,
    pub(super) image_id: String,
}
pub(super) struct ImageTagRequest {
    pub(super) generation: u64,
    pub(super) image_id: String,
    pub(super) repository: String,
    pub(super) tag: String,
}
pub(super) struct ImagePushRequest {
    pub(super) generation: u64,
    pub(super) image_id: String,
    pub(super) target: ImagePushTarget,
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
    // Survives view invalidation; only the matching terminal event releases it.
    active_pull_generation: Option<u64>,
    pull_reference: String,
    pull_loading: bool,
    pull_progress_hidden: bool,
    pull_complete: bool,
    pull_progress: Option<ImagePullProgress>,
    pull_error: Option<String>,
    pull_requested: bool,
    hide_progress_requested: bool,
    push_generation: u64,
    // Survives view invalidation; only the matching terminal event releases it.
    active_push_generation: Option<u64>,
    push_target_reference: String,
    push_confirmation_open: bool,
    push_confirmation_image_id: Option<String>,
    push_confirmation_target: Option<ImagePushTarget>,
    push_confirmed: bool,
    push_requested: bool,
    hide_push_progress_requested: bool,
    push_loading: bool,
    push_progress_hidden: bool,
    push_complete: bool,
    // An ambiguous push result keeps the mutation gate latched for this session.
    push_outcome_uncertain: bool,
    push_progress: Option<ImagePushProgress>,
    push_error: Option<String>,
    push_result_image_id: Option<String>,
    push_result_target: Option<String>,
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
    remove_generation: u64,
    remove_confirmation_open: bool,
    remove_confirmation_image_id: Option<String>,
    remove_confirmed: bool,
    remove_requested: bool,
    remove_loading: bool,
    remove_complete: bool,
    remove_error: Option<String>,
    remove_result_image_id: Option<String>,
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
    pub(super) fn is_pushing(&self) -> bool {
        self.push_loading
    }
    #[cfg(test)]
    pub(super) fn is_pull_progress_hidden(&self) -> bool {
        self.pull_progress_hidden
    }
    #[cfg(test)]
    pub(super) fn is_pull_complete(&self) -> bool {
        self.pull_complete
    }
    #[cfg(test)]
    pub(super) fn set_push_source_for_test(&mut self, image_id: &str, target_reference: &str) {
        self.selected_id = Some(image_id.to_owned());
        self.push_target_reference = target_reference.to_owned();
    }
    #[cfg(test)]
    pub(super) fn push_progress(&self) -> Option<&ImagePushProgress> {
        self.push_progress.as_ref()
    }
    #[cfg(test)]
    pub(super) fn is_push_complete(&self) -> bool {
        self.push_complete
    }
    #[cfg(test)]
    pub(super) fn is_push_outcome_uncertain(&self) -> bool {
        self.push_outcome_uncertain
    }
    pub(super) fn has_mutation_in_flight(&self) -> bool {
        self.active_pull_generation.is_some()
            || self.active_push_generation.is_some()
            || self.tag_loading
            || self.remove_loading
    }
    pub(super) fn take_pull_requested(&mut self) -> bool {
        std::mem::take(&mut self.pull_requested)
    }
    pub(super) fn take_hide_progress_requested(&mut self) -> bool {
        std::mem::take(&mut self.hide_progress_requested)
    }
    pub(super) fn take_push_requested(&mut self) -> bool {
        std::mem::take(&mut self.push_requested)
    }
    pub(super) fn take_hide_push_progress_requested(&mut self) -> bool {
        std::mem::take(&mut self.hide_push_progress_requested)
    }
    #[cfg(test)]
    pub(super) fn set_pull_reference(&mut self, reference: String) {
        self.pull_reference = reference;
    }
    #[cfg(test)]
    pub(super) fn pull_progress(&self) -> Option<&ImagePullProgress> {
        self.pull_progress.as_ref()
    }
    #[cfg(test)]
    pub(super) fn pull_error(&self) -> Option<&str> {
        self.pull_error.as_deref()
    }

    pub(super) fn invalidate(&mut self) {
        self.reset_inventory();
        self.invalidate_pull_view();
        self.invalidate_push_view();
        self.tag_generation = self.tag_generation.wrapping_add(1);
        self.tag_confirmation_open = false;
        self.tag_confirmation_image_id = None;
        self.tag_confirmed = false;
        self.tag_requested = false;
        self.tag_loading = false;
        self.tag_complete = false;
        self.tag_result_target = None;
        self.tag_error = None;
        self.remove_generation = self.remove_generation.wrapping_add(1);
        self.remove_confirmation_open = false;
        self.remove_confirmation_image_id = None;
        self.remove_confirmed = false;
        self.remove_requested = false;
        self.remove_loading = false;
        self.remove_complete = false;
        self.remove_error = None;
        self.remove_result_image_id = None;
    }

    pub(super) fn is_removing(&self) -> bool {
        self.remove_loading
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
        self.push_target_reference.clear();
        self.push_confirmation_open = false;
        self.push_confirmation_image_id = None;
        self.push_confirmation_target = None;
        self.push_confirmed = false;
        self.push_requested = false;
        self.hide_push_progress_requested = false;
        self.tag_confirmation_open = false;
        if !self.tag_loading {
            self.tag_confirmation_image_id = None;
            self.tag_confirmed = false;
            self.tag_requested = false;
        }
        self.remove_confirmation_open = false;
        if !self.remove_loading {
            self.remove_confirmation_image_id = None;
            self.remove_confirmed = false;
            self.remove_requested = false;
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
    pub(super) fn begin_remove_confirmation(&mut self) {
        if !self.remove_loading
            && !self.remove_confirmation_open
            && let Some(image_id) = &self.selected_id
        {
            self.remove_confirmation_image_id = Some(image_id.clone());
            self.remove_confirmation_open = true;
            self.remove_complete = false;
            self.remove_error = None;
        }
    }

    pub(super) fn cancel_remove_confirmation(&mut self) {
        self.remove_confirmation_open = false;
        self.remove_confirmation_image_id = None;
        self.remove_confirmed = false;
        self.remove_requested = false;
    }

    pub(super) fn confirm_remove(&mut self) {
        if self.remove_confirmation_open
            && self.remove_confirmation_image_id.is_some()
            && !self.remove_loading
        {
            self.remove_confirmation_open = false;
            self.remove_confirmed = true;
            self.remove_requested = true;
        }
    }

    pub(super) fn take_remove_requested(&mut self) -> bool {
        std::mem::take(&mut self.remove_requested)
    }

    pub(super) fn begin_remove(&mut self) -> Option<ImageRemoveRequest> {
        if !std::mem::take(&mut self.remove_confirmed) || self.has_mutation_in_flight() {
            return None;
        }
        let image_id = self.remove_confirmation_image_id.take()?;
        self.remove_generation = self.remove_generation.wrapping_add(1);
        self.remove_loading = true;
        self.remove_complete = false;
        self.remove_error = None;
        self.remove_result_image_id = Some(image_id.clone());
        Some(ImageRemoveRequest {
            generation: self.remove_generation,
            image_id,
        })
    }

    pub(super) fn finish_remove<E: Display>(
        &mut self,
        generation: u64,
        result: Result<(), E>,
    ) -> bool {
        if generation != self.remove_generation || !self.remove_loading {
            return false;
        }
        self.remove_loading = false;
        match result {
            Ok(()) => self.remove_complete = true,
            Err(error) => self.remove_error = Some(error.to_string()),
        }
        true
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
        if !std::mem::take(&mut self.tag_confirmed)
            || self.has_mutation_in_flight()
            || !self.tag_target_valid()
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
        if !self.pull_reference_valid() || self.has_mutation_in_flight() {
            return None;
        }
        self.pull_generation = self.pull_generation.wrapping_add(1);
        self.active_pull_generation = Some(self.pull_generation);
        self.pull_loading = true;
        self.pull_progress_hidden = false;
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
        if self.active_pull_generation != Some(generation) {
            return false;
        }
        self.active_pull_generation = None;
        if generation != self.pull_generation || (!self.pull_loading && !self.pull_progress_hidden)
        {
            return false;
        }
        self.pull_loading = false;
        self.pull_progress_hidden = false;
        match result {
            Ok(()) => self.pull_complete = true,
            Err(error) => self.pull_error = Some(error.to_string()),
        }
        true
    }
    pub(super) fn hide_pull_progress(&mut self) {
        self.pull_loading = false;
        self.pull_progress_hidden = true;
        self.pull_progress = None;
        self.pull_error = None;
        self.pull_complete = false;
    }
    // Invalidate only the displayed generation; the active operation still owns the mutation gate.
    fn invalidate_pull_view(&mut self) {
        self.pull_generation = self.pull_generation.wrapping_add(1);
        self.pull_loading = false;
        self.pull_progress_hidden = false;
        self.pull_complete = false;
        self.pull_progress = None;
        self.pull_error = None;
        self.pull_requested = false;
        self.hide_progress_requested = false;
    }

    pub(super) fn push_target_valid(&self) -> bool {
        let Some(image_id) = self.selected_id.as_deref() else {
            return false;
        };
        ImagePushTarget::is_valid_reference(&self.push_target_reference)
            && self
                .images
                .iter()
                .find(|image| image.id == image_id)
                .is_some_and(|image| {
                    image
                        .repo_tags
                        .iter()
                        .any(|tag| tag == &self.push_target_reference)
                })
    }

    pub(super) fn begin_push_confirmation(&mut self) {
        if !self.push_target_valid()
            || self.push_loading
            || self.push_confirmation_open
            || self.has_mutation_in_flight()
        {
            return;
        }
        let Some(image_id) = self.selected_id.clone() else {
            return;
        };
        let Some(target) = ImagePushTarget::parse(&self.push_target_reference) else {
            return;
        };
        self.push_confirmation_image_id = Some(image_id);
        self.push_confirmation_target = Some(target);
        self.push_confirmation_open = true;
        self.push_complete = false;
        self.push_result_image_id = None;
        self.push_result_target = None;
        self.push_error = None;
    }

    pub(super) fn confirm_push(&mut self) {
        if self.push_confirmation_open
            && self.push_confirmation_image_id.is_some()
            && self.push_confirmation_target.is_some()
            && !self.has_mutation_in_flight()
        {
            self.push_confirmation_open = false;
            self.push_confirmed = true;
            self.push_requested = true;
        }
    }

    pub(super) fn cancel_push_confirmation(&mut self) {
        self.push_confirmation_open = false;
        self.push_confirmation_image_id = None;
        self.push_confirmation_target = None;
        self.push_confirmed = false;
        self.push_requested = false;
    }

    pub(super) fn begin_push(&mut self) -> Option<ImagePushRequest> {
        if !std::mem::take(&mut self.push_confirmed) || self.has_mutation_in_flight() {
            self.cancel_push_confirmation();
            return None;
        }
        let image_id = self.push_confirmation_image_id.take()?;
        let target = self.push_confirmation_target.take()?;
        let target_is_still_local = self
            .images
            .iter()
            .find(|image| image.id == image_id)
            .is_some_and(|image| image.repo_tags.iter().any(|tag| tag == target.as_str()));
        if !target_is_still_local {
            self.cancel_push_confirmation();
            return None;
        }

        self.push_generation = self.push_generation.wrapping_add(1);
        self.active_push_generation = Some(self.push_generation);
        self.push_loading = true;
        self.push_progress_hidden = false;
        self.push_complete = false;
        self.push_outcome_uncertain = false;
        self.push_progress = None;
        self.push_error = None;
        self.push_result_image_id = Some(image_id.clone());
        self.push_result_target = Some(target.as_str().to_owned());
        Some(ImagePushRequest {
            generation: self.push_generation,
            image_id,
            target,
        })
    }

    pub(super) fn update_push(&mut self, generation: u64, progress: ImagePushProgress) -> bool {
        if generation != self.push_generation || !self.push_loading {
            return false;
        }
        self.push_progress = Some(progress);
        true
    }

    pub(super) fn finish_push(&mut self, generation: u64, result: Result<(), String>) -> bool {
        if self.active_push_generation != Some(generation) || self.push_outcome_uncertain {
            return false;
        }
        self.active_push_generation = None;
        if generation != self.push_generation || (!self.push_loading && !self.push_progress_hidden)
        {
            self.push_loading = false;
            self.push_progress_hidden = false;
            self.push_progress = None;
            self.push_complete = false;
            self.push_error = None;
            self.push_result_image_id = None;
            self.push_result_target = None;
            return false;
        }
        self.push_loading = false;
        self.push_progress_hidden = false;
        self.push_outcome_uncertain = false;
        match result {
            Ok(()) => self.push_complete = true,
            Err(error) => self.push_error = Some(error),
        }
        true
    }

    pub(super) fn finish_push_uncertain(&mut self, generation: u64, error: String) -> bool {
        if self.active_push_generation != Some(generation) {
            return false;
        }
        self.push_loading = false;
        self.push_progress_hidden = false;
        self.push_complete = false;
        self.push_progress = None;
        self.push_error = Some(error);
        self.push_outcome_uncertain = true;
        true
    }

    pub(super) fn hide_push_progress(&mut self) {
        self.push_loading = false;
        self.push_progress_hidden = true;
        self.push_progress = None;
        self.push_error = None;
        self.push_complete = false;
    }

    // Invalidate only the displayed generation; an active/uncertain push keeps the mutation gate.
    fn invalidate_push_view(&mut self) {
        let push_is_active = self.active_push_generation.is_some();
        self.push_generation = self.push_generation.wrapping_add(1);
        self.push_loading = false;
        self.push_progress_hidden = push_is_active && !self.push_outcome_uncertain;
        self.push_complete = false;
        self.push_progress = None;
        self.push_requested = false;
        self.hide_push_progress_requested = false;
        if !push_is_active && !self.push_outcome_uncertain {
            self.push_error = None;
            self.push_result_image_id = None;
            self.push_result_target = None;
        }
        self.push_confirmation_open = false;
        self.push_confirmation_image_id = None;
        self.push_confirmation_target = None;
        self.push_confirmed = false;
        self.push_target_reference.clear();
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
        operations_allowed: bool,
        mutation_busy: bool,
    ) -> Option<ImageInspectionRequest> {
        ui.group(|ui| {
            ui.heading("Pull public image");
            ui.label("Anonymous pulls only; private registry authentication is not supported.");
            if mutation_busy && !self.pull_loading {
                ui.label("An image operation is in progress; wait for it to finish before starting another pull, push, tag, or removal.");
            }
            ui.horizontal(|ui| {
                let label = ui.label("Image reference");
                ui.text_edit_singleline(&mut self.pull_reference)
                    .labelled_by(label.id);
                if ui
                    .add_enabled(
                        connected
                            && operations_allowed
                            && self.pull_reference_valid()
                            && !self.pull_loading
                            && !mutation_busy,
                        egui::Button::new("Pull image"),
                    )
                    .clicked()
                {
                    self.pull_requested = true;
                }
                if self.pull_loading && ui.button("Hide progress").clicked() {
                    self.hide_progress_requested = true;
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
            } else if self.pull_progress_hidden {
                ui.label("Progress hidden. The Docker image transfer may continue; other image operations remain disabled until Ferrum finishes reading the pull response.");
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
            self.draw_remove_controls(ui, false, operations_allowed, mutation_busy);
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
        self.draw_tag_controls(ui, connected, operations_allowed, mutation_busy);
        self.draw_push_controls(ui, connected, operations_allowed, mutation_busy);
        self.draw_remove_controls(ui, connected, operations_allowed, mutation_busy);
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
                    let safe_tag = escape_untrusted_display_text(tag);
                    let safe_image_id = escape_untrusted_display_text(&image.id);
                    ui.horizontal(|ui| {
                        let response = ui.add_sized(
                            [tag_width, IMAGE_CELL_HEIGHT],
                            egui::Button::new(egui::RichText::new(safe_tag.as_ref()).strong())
                                .selected(self.selected_id.as_deref() == Some(image.id.as_str())),
                        );
                        let clicked = response.clicked();
                        response.on_hover_ui(|ui| {
                            ui.strong("Repository tags");
                            if image.repo_tags.is_empty() {
                                ui.label("Untagged image");
                            } else {
                                for tag in &image.repo_tags {
                                    let safe_tag = escape_untrusted_display_text(tag);
                                    ui.label(safe_tag.as_ref());
                                }
                            }
                            ui.label(safe_image_id.as_ref());
                        });
                        if clicked {
                            selected = Some(image.id.clone());
                        }
                        ui.add_sized(
                            [id_width, IMAGE_CELL_HEIGHT],
                            egui::Label::new(safe_image_id.as_ref())
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
        self.push_target_reference = self
            .images
            .iter()
            .find(|image| image.id == id)
            .and_then(|image| {
                image
                    .repo_tags
                    .iter()
                    .find(|tag| ImagePushTarget::is_valid_reference(tag))
            })
            .cloned()
            .unwrap_or_default();
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
        self.push_target_reference.clear();
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
        operations_allowed: bool,
        mutation_busy: bool,
    ) {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Tag selected image");
            if self.selected_id.is_none() {
                ui.label("Select an image before tagging it.");
            }
            if !operations_allowed && connected && !mutation_busy {
                ui.label("Image tagging is available only through the local Docker Unix socket on Linux.");
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
                        && operations_allowed
                        && !mutation_busy
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
                let safe_image_id = escape_untrusted_display_text(&image_id);
                ui.group(|ui| {
                    ui.heading("Confirm image tag");
                    ui.label(format!(
                        "Tag image {safe_image_id} as {target}? Docker may reassign an existing repository:tag; the image data remains. Confirm to send this request to the connected local Docker engine."
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
    fn draw_push_controls(
        &mut self,
        ui: &mut egui::Ui,
        connected: bool,
        operations_allowed: bool,
        mutation_busy: bool,
    ) {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Push selected image");
            if self.selected_id.is_none() {
                ui.label("Select an image before pushing it.");
            }
            if !operations_allowed && connected {
                ui.label("Image pushing is available only through the local Docker Unix socket on Linux.");
            }
            ui.add_enabled_ui(
                self.selected_id.is_some() && !self.push_loading && !self.push_confirmation_open,
                |ui| {
                    ui.horizontal(|ui| {
                        let label = ui.label("Existing local repository:tag");
                        ui.text_edit_singleline(&mut self.push_target_reference)
                            .labelled_by(label.id);
                    });
                },
            );
            let target_valid = self.push_target_valid();
            if self.selected_id.is_some() && !target_valid {
                ui.label("Select a tagged repository reference already present on this image; tag it first if needed.");
            }
            if ui
                .add_enabled(
                    connected
                        && operations_allowed
                        && !mutation_busy
                        && target_valid
                        && !self.push_loading
                        && !self.push_confirmation_open,
                    egui::Button::new("Push image"),
                )
                .clicked()
            {
                self.begin_push_confirmation();
            }
            if self.push_confirmation_open
                && let (Some(image_id), Some(target)) = (
                    self.push_confirmation_image_id.clone(),
                    self.push_confirmation_target.as_ref(),
                )
            {
                let safe_image_id = escape_untrusted_display_text(&image_id);
                let safe_target = escape_untrusted_display_text(target.as_str());
                let mut confirm_requested = false;
                let mut cancel_requested = false;
                ui.group(|ui| {
                    ui.heading("Confirm image push");
                    ui.label(format!(
                        "Push image {safe_image_id} as {safe_target}? This sends image layers and metadata to the registry and may disclose secrets. Only anonymous access is used; registries that require credentials will reject the push. Confirm to send this request to the connected local Docker engine."
                    ));
                    ui.horizontal(|ui| {
                        confirm_requested = ui.button("Confirm push").clicked();
                        cancel_requested = ui.button("Cancel").clicked();
                    });
                });
                if confirm_requested {
                    self.confirm_push();
                }
                if cancel_requested {
                    self.cancel_push_confirmation();
                }
            }
            if self.push_outcome_uncertain {
                ui.colored_label(
                    egui::Color32::from_rgb(224, 174, 109),
                    "Push outcome is unknown. The daemon may still be transferring layers; Ferrum keeps image mutations blocked for this session.",
                );
                if let Some(target) = self.push_result_target.as_deref() {
                    let safe_target = escape_untrusted_display_text(target);
                    ui.label(format!("Unresolved push target: {safe_target}"));
                }
                ui.label("Verify registry state before using another image client. Closing Ferrum does not guarantee the transfer stopped.");
                if let Some(error) = &self.push_error {
                    ui.label(error);
                }
            } else if self.push_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    if let Some(target) = self.push_result_target.as_deref() {
                        let safe_target = escape_untrusted_display_text(target);
                        ui.label(format!("Pushing {safe_target}…"));
                    }
                });
                if let Some(progress) = &self.push_progress {
                    ui.label(&progress.status);
                    if let Some(current) = progress.current_bytes {
                        ui.label(format!("{current} bytes uploaded"));
                    }
                    if let Some(total) = progress.total_bytes {
                        ui.label(format!("Total: {total} bytes"));
                    }
                }
                if ui.button("Hide push progress").clicked() {
                    self.hide_push_progress_requested = true;
                }
            } else if self.push_progress_hidden {
                ui.label("Progress hidden. The registry transfer may continue; other image operations remain disabled until Ferrum receives the terminal response.");
            } else if self.push_complete {
                if let Some(target) = &self.push_result_target {
                    let safe_target = escape_untrusted_display_text(target);
                    ui.label(format!("Image push completed for {safe_target}."));
                }
            } else if let Some(error) = &self.push_error {
                ui.label("Image push request failed.");
                ui.label(error);
            }
        });
    }
    fn draw_remove_controls(
        &mut self,
        ui: &mut egui::Ui,
        connected: bool,
        operations_allowed: bool,
        mutation_busy: bool,
    ) {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Remove selected image");
            if self.selected_id.is_none() {
                ui.label("Select an image before removing it.");
            }
            if !operations_allowed && connected {
                ui.label("Image removal is available only through the local Docker Unix socket on Linux.");
            }
            if ui
                .add_enabled(
                    connected && operations_allowed && !mutation_busy
                        && self.selected_id.is_some() && !self.remove_loading
                        && !self.remove_confirmation_open,
                    egui::Button::new("Remove image"),
                )
                .clicked()
            {
                self.begin_remove_confirmation();
            }
            if self.remove_confirmation_open
                && let Some(image_id) = self.remove_confirmation_image_id.clone()
            {
                let tags = self.images.iter()
                    .find(|image| image.id == image_id)
                    .map(|image| if image.repo_tags.is_empty() {
                        "Untagged image".to_owned()
                    } else {
                        image.repo_tags.join(", ")
                    })
                    .unwrap_or_else(|| "Tags unavailable".to_owned());
                let safe_image_id = escape_untrusted_display_text(&image_id);
                let safe_tags = escape_untrusted_display_text(&tags);
                ui.group(|ui| {
                    ui.heading("Confirm image removal");
                    ui.label(format!(
                        "Remove image ID {safe_image_id} (known tags: {safe_tags})? Force is not used. Untagged parent images are preserved. The engine may refuse images in use by containers or with other tags. If this removes the last reference, image data may be deleted. Confirm to send this request to the connected local Docker engine."
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Confirm removal").clicked() {
                            self.confirm_remove();
                        }
                        if ui.button("Cancel").clicked() {
                            self.cancel_remove_confirmation();
                        }
                    });
                });
            }
            if self.remove_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    if let Some(image_id) = self.remove_result_image_id.as_deref() {
                        let safe_image_id = escape_untrusted_display_text(image_id);
                        ui.label(format!("Removing image {safe_image_id}…"));
                    }
                });
            } else if self.remove_complete {
                if let Some(image_id) = &self.remove_result_image_id {
                    let safe_image_id = escape_untrusted_display_text(image_id);
                    ui.label(format!("Image removal succeeded for {safe_image_id}."));
                }
            } else if let Some(error) = &self.remove_error {
                ui.label("Image removal failed; the engine state may have changed.");
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
    let text = escape_untrusted_display_text(text);
    ui.add_sized(
        [width, IMAGE_CELL_HEIGHT],
        egui::Label::new(egui::RichText::new(text.as_ref()).small().strong().weak()).truncate(),
    );
}

fn image_detail(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    let value = value.map(escape_untrusted_display_text);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.as_deref().unwrap_or("Not reported"));
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
                if let Some(request) = state.view.show(ui, true, "", true, false) {
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
    fn pushable_view() -> ImagesView {
        let mut view = loaded_view(vec![image(
            "sha256:image",
            "registry.example/team/app:stable",
            1024,
        )]);
        view.selected_id = Some("sha256:image".to_owned());
        view.push_target_reference = "registry.example/team/app:stable".to_owned();
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
                view.show(ui, true, "", true, true);
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
            "An image operation is in progress; wait for it to finish before starting another pull, push, tag, or removal.",
        );
    }

    #[test]
    fn an_in_flight_image_mutation_disables_competing_operations() {
        let mut view = taggable_view();
        view.pull_reference = "alpine:3.20".to_owned();
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, true, "", true, true);
            },
            view,
        );
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Pull image")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Tag image")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Remove image")
                .accesskit_node()
                .is_disabled()
        );
        let _busy_notice = harness.get_by_role_and_label(
            Role::Label,
            "An image operation is in progress; wait for it to finish before starting another pull, push, tag, or removal.",
        );

        let mut active = taggable_view();
        active.tag_loading = true;
        active.remove_confirmed = true;
        assert!(active.begin_remove().is_none());
        active.pull_reference = "alpine:3.20".to_owned();
        assert!(active.begin_pull().is_none());
        let mut active = taggable_view();
        active.remove_loading = true;
        active.tag_confirmed = true;
        assert!(active.begin_tag().is_none());
        active.pull_reference = "alpine:3.20".to_owned();
        assert!(active.begin_pull().is_none());
    }
    #[test]
    fn an_active_pull_blocks_tag_and_remove_model_requests_until_terminal_event() {
        let mut view = taggable_view();
        view.set_pull_reference("alpine:3.20".to_owned());
        let (generation, _) = view.begin_pull().unwrap();
        view.tag_confirmed = true;
        view.tag_confirmation_image_id = Some("sha256:image".to_owned());
        view.remove_confirmed = true;
        view.remove_confirmation_image_id = Some("sha256:image".to_owned());

        assert!(view.has_mutation_in_flight());
        assert!(view.begin_tag().is_none());
        assert!(view.begin_remove().is_none());
        assert!(view.begin_pull().is_none());

        assert!(view.finish_pull(generation, Ok(())));
        assert!(!view.has_mutation_in_flight());
    }

    #[test]
    fn engine_image_id_is_escaped_in_tag_confirmation_and_request_preserves_exact_id() {
        let original_id = "sha256:image\u{202e}";
        let mut view = taggable_view();
        view.selected_id = Some(original_id.to_owned());
        let mut harness = harness_for(view);
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Tag image")
            .click_accesskit();
        harness.run();

        let safe_id = "sha256:image\\u{202e}";
        let expected_confirmation = format!(
            "Tag image {safe_id} as example/image:release-1? Docker may reassign an existing repository:tag; the image data remains. Confirm to send this request to the connected local Docker engine."
        );
        let confirmation = harness.get_by_role_and_label(Role::Label, &expected_confirmation);
        assert_eq!(
            confirmation.accesskit_node().value().as_deref(),
            Some(expected_confirmation.as_str())
        );

        harness
            .get_by_role_and_label(Role::Button, "Confirm tag")
            .click_accesskit();
        harness.run();
        assert!(harness.state_mut().view.take_tag_requested());
        let request = harness
            .state_mut()
            .view
            .begin_tag()
            .expect("confirmed tag request");
        assert_eq!(request.image_id, original_id);
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
    fn navigation_opens_the_volumes_page_through_an_accessible_action() {
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
        harness
            .get_by_role_and_label(Role::Button, "Volumes")
            .click_accesskit();
        harness.run();
        assert_eq!(harness.state().page, AppPage::Volumes);
        let _volumes = harness.get_by_role_and_label(Role::Label, "Volumes");
    }
    #[test]
    fn navigation_opens_the_networks_page_through_an_accessible_action() {
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
        harness
            .get_by_role_and_label(Role::Button, "Networks")
            .click_accesskit();
        harness.run();
        assert_eq!(harness.state().page, AppPage::Networks);
        let _networks = harness.get_by_role_and_label(Role::Label, "Networks");
    }

    #[test]
    fn image_removal_requires_named_confirmation_and_cancel_sends_no_request() {
        let mut harness = harness_for(taggable_view());
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "Remove image")
            .click_accesskit();
        harness.run();
        let _confirmation = harness.get_by_role_and_label(Role::Label, "Confirm image removal");
        let _warning = harness.get_by_role_and_label(
            Role::Label,
            "Remove image ID sha256:image (known tags: existing:tag)? Force is not used. Untagged parent images are preserved. The engine may refuse images in use by containers or with other tags. If this removes the last reference, image data may be deleted. Confirm to send this request to the connected local Docker engine.",
        );
        assert!(!harness.state().view.remove_requested);
        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .click_accesskit();
        harness.run();
        assert!(!harness.state_mut().view.take_remove_requested());
        assert!(!harness.state().view.remove_loading);
    }

    #[test]
    fn image_removal_snapshots_selected_id_and_stale_results_are_ignored() {
        let mut view = taggable_view();
        view.begin_remove_confirmation();
        view.selected_id = Some("sha256:changed".to_owned());
        view.confirm_remove();
        assert!(view.take_remove_requested());
        let request = view.begin_remove().unwrap();
        assert_eq!(request.image_id, "sha256:image");
        assert!(view.finish_remove(request.generation, Ok::<_, &str>(())));
        assert!(view.remove_complete);

        let mut view = taggable_view();
        view.begin_remove_confirmation();
        view.confirm_remove();
        let stale = view.begin_remove().unwrap();
        view.invalidate();
        assert!(!view.finish_remove(stale.generation, Ok::<_, &str>(())));
        assert!(!view.remove_complete);
        assert!(view.remove_error.is_none());
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
    fn untrusted_image_tags_and_author_are_escaped_before_accessible_display() {
        let image_id = "sha256:untrusted";
        let tag = "repository\u{202e}tag";
        let safe_tag = "repository\\u{202e}tag";
        let mut harness = harness_for(loaded_view(vec![image(image_id, tag, 1024)]));

        harness.run();
        let image_row = harness.get_by_role_and_label(Role::Button, safe_tag);
        assert_eq!(
            image_row.accesskit_node().label().as_deref(),
            Some(safe_tag)
        );
        image_row.click_accesskit();
        harness.run();
        let request = harness.state_mut().inspection.take().unwrap();
        let mut details = details(&request.id, tag);
        details.author = Some("author\u{e0001}text\u{200b}".to_owned());
        assert!(harness.state_mut().view.complete_inspection(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(details),
        ));
        harness.run();

        let tag_label = harness.get_by_role_and_label(Role::Label, safe_tag);
        assert_eq!(
            tag_label.accesskit_node().value().as_deref(),
            Some(safe_tag)
        );
        let safe_author = "author\\u{e0001}text\\u{200b}";
        let author_label = harness.get_by_role_and_label(Role::Label, safe_author);
        assert_eq!(
            author_label.accesskit_node().value().as_deref(),
            Some(safe_author)
        );
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
                view.show(ui, false, "offline", false, false);
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
    fn pull_error_is_visible_and_invalidated_generations_are_ignored() {
        let mut view = ImagesView {
            pull_reference: "alpine:latest".to_owned(),
            ..ImagesView::default()
        };
        let (stale, _) = view.begin_pull().unwrap();
        view.invalidate_pull_view();
        assert!(view.has_mutation_in_flight());
        assert!(!view.update_pull(stale, progress("stale", Some(1), Some(2))));
        assert!(!view.finish_pull(stale, Ok(())));
        assert!(!view.has_mutation_in_flight());

        let (current, _) = view.begin_pull().unwrap();
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
                view.show(ui, true, "", true, false);
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
                view.show(ui, true, "", true, false);
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
    fn accessible_hide_progress_keeps_pull_mutation_active_until_completion() {
        let mut view = taggable_view();
        view.set_pull_reference("alpine:latest".to_owned());
        let (generation, _) = view.begin_pull().unwrap();
        view.update_pull(generation, progress("Downloading", Some(10), Some(100)));
        let mut harness = Harness::new_ui_state(
            |ui, state: &mut ImagesUi| {
                let mutation_busy = state.view.has_mutation_in_flight();
                if let Some(request) = state.view.show(ui, true, "", true, mutation_busy) {
                    state.inspection = Some(request);
                }
            },
            ImagesUi {
                view,
                inspection: None,
            },
        );
        harness.step();
        harness
            .get_by_role_and_label(Role::Button, "Hide progress")
            .click_accesskit();
        harness.step();
        assert!(harness.state_mut().view.take_hide_progress_requested());
        harness.state_mut().view.hide_pull_progress();
        harness.step();
        assert!(!harness.state().view.is_pulling());
        assert!(harness.state().view.is_pull_progress_hidden());
        assert!(harness.state().view.has_mutation_in_flight());
        assert!(harness.state().view.pull_progress().is_none());
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Pull image")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Tag image")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Remove image")
                .accesskit_node()
                .is_disabled()
        );
        let _hidden_notice = harness.get_by_role_and_label(
            Role::Label,
            "Progress hidden. The Docker image transfer may continue; other image operations remain disabled until Ferrum finishes reading the pull response.",
        );
        let _busy_notice = harness.get_by_role_and_label(
            Role::Label,
            "An image operation is in progress; wait for it to finish before starting another pull, push, tag, or removal.",
        );
        assert!(harness.state_mut().view.finish_pull(generation, Ok(())));
        assert!(!harness.state().view.has_mutation_in_flight());
        assert!(harness.state().view.is_pull_complete());
    }

    #[test]
    fn queued_success_after_hidden_progress_completes_and_remains_refreshable() {
        let mut view = ImagesView::default();
        view.set_pull_reference("alpine:latest".to_owned());
        let (generation, _) = view.begin_pull().unwrap();
        view.hide_pull_progress();
        assert!(view.is_pull_progress_hidden());
        assert!(view.has_mutation_in_flight());
        assert!(view.finish_pull(generation, Ok(())));
        assert!(view.is_pull_complete());
        assert!(!view.is_pull_progress_hidden());
        view.begin_load();
        let mut harness = harness_for(view);
        harness.step();
        let _complete = harness.get_by_role_and_label(Role::Label, "Image pull complete.");
    }

    #[test]
    fn invalidation_hides_stale_pull_data_without_releasing_active_mutation() {
        let mut view = ImagesView::default();
        view.set_pull_reference("alpine:latest".to_owned());
        let (generation, _) = view.begin_pull().unwrap();
        view.update_pull(generation, progress("old context", Some(10), Some(100)));

        view.invalidate();
        assert!(view.has_mutation_in_flight());
        assert!(!view.is_pulling());
        assert!(!view.is_pull_progress_hidden());
        assert!(view.pull_progress().is_none());
        assert!(view.begin_pull().is_none());
        assert!(!view.finish_pull(generation, Err("stale context failure".to_owned())));
        assert!(!view.has_mutation_in_flight());
        assert!(view.pull_error.is_none());
        assert!(!view.pull_complete);
    }

    #[test]
    fn a_new_pull_stays_blocked_after_hiding_until_previous_terminal_event() {
        let mut view = ImagesView::default();
        view.set_pull_reference("alpine:latest".to_owned());
        let (previous_generation, _) = view.begin_pull().unwrap();
        view.hide_pull_progress();
        assert!(view.begin_pull().is_none());
        assert!(view.has_mutation_in_flight());
        assert!(view.finish_pull(previous_generation, Ok(())));

        let (current_generation, _) = view.begin_pull().unwrap();
        assert_ne!(previous_generation, current_generation);
        assert!(!view.finish_pull(previous_generation, Ok(())));
        assert!(!view.update_pull(
            previous_generation,
            progress("old attempt", Some(1), Some(2))
        ));
        assert!(view.is_pulling());
        assert!(!view.is_pull_complete());
    }
    #[test]
    fn image_push_requires_confirmation_and_cancel_sends_no_request() {
        let mut view = loaded_view(vec![image(
            "sha256:image",
            "registry.example/team/app:stable",
            1024,
        )]);
        view.selected_id = Some("sha256:image".to_owned());
        view.push_target_reference = "registry.example/team/app:stable".to_owned();
        let mut harness = harness_for(view);
        harness.run();

        harness
            .get_by_role_and_label(Role::Button, "Push image")
            .click_accesskit();
        harness.run();
        let _heading = harness.get_by_role_and_label(Role::Label, "Confirm image push");
        let _warning = harness.get_by_role_and_label(
            Role::Label,
            "Push image sha256:image as registry.example/team/app:stable? This sends image layers and metadata to the registry and may disclose secrets. Only anonymous access is used; registries that require credentials will reject the push. Confirm to send this request to the connected local Docker engine.",
        );
        assert!(!harness.state().view.push_requested);

        harness
            .get_by_role_and_label(Role::Button, "Cancel")
            .click_accesskit();
        harness.run();
        assert!(!harness.state_mut().view.take_push_requested());
        assert!(!harness.state().view.push_loading);
    }
    #[test]
    fn image_push_button_requires_a_tag_owned_by_the_selected_image() {
        let mut view = pushable_view();
        view.push_target_reference = "registry.example/other:stable".to_owned();
        let mut harness = harness_for(view);
        harness.run();

        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Push image")
                .accesskit_node()
                .is_disabled()
        );
        let _reason = harness.get_by_role_and_label(
            Role::Label,
            "Select a tagged repository reference already present on this image; tag it first if needed.",
        );
    }

    #[test]
    fn confirmed_push_keeps_the_selected_source_and_tracks_terminal_progress() {
        let mut view = pushable_view();
        view.begin_push_confirmation();
        view.selected_id = Some("sha256:other".to_owned());
        view.push_target_reference = "other:tag".to_owned();
        view.confirm_push();
        assert!(view.take_push_requested());

        let request = view.begin_push().expect("confirmed local tag should start");
        assert_eq!(request.image_id, "sha256:image");
        assert_eq!(request.target.as_str(), "registry.example/team/app:stable");
        assert!(view.has_mutation_in_flight());
        assert!(view.update_push(
            request.generation,
            ImagePushProgress {
                status: "Pushing".to_owned(),
                current_bytes: Some(17),
                total_bytes: Some(100),
            },
        ));
        assert_eq!(
            view.push_progress,
            Some(ImagePushProgress {
                status: "Pushing".to_owned(),
                current_bytes: Some(17),
                total_bytes: Some(100),
            })
        );
        assert!(view.finish_push(request.generation, Ok(())));
        assert!(view.push_complete);
        assert!(!view.has_mutation_in_flight());
    }

    #[test]
    fn hidden_or_invalidated_push_keeps_its_mutation_gate_until_terminal_event() {
        let mut view = pushable_view();
        view.begin_push_confirmation();
        view.confirm_push();
        let request = view.begin_push().unwrap();
        view.hide_push_progress();
        assert!(view.push_progress_hidden);
        assert!(view.has_mutation_in_flight());

        view.pull_reference = "alpine:latest".to_owned();
        view.tag_confirmed = true;
        view.tag_confirmation_image_id = Some("sha256:image".to_owned());
        view.remove_confirmed = true;
        view.remove_confirmation_image_id = Some("sha256:image".to_owned());
        assert!(view.begin_pull().is_none());
        assert!(view.begin_tag().is_none());
        assert!(view.begin_remove().is_none());

        view.invalidate();
        assert!(view.has_mutation_in_flight());
        assert!(!view.is_pushing());
        assert!(!view.finish_push(request.generation, Err("stale push result".to_owned())));
        assert!(!view.has_mutation_in_flight());
        assert!(view.push_error.is_none());
    }
    #[test]
    fn uncertain_push_outcome_latches_the_gate_and_displays_recovery_warning() {
        let mut view = pushable_view();
        view.begin_push_confirmation();
        view.confirm_push();
        let request = view.begin_push().unwrap();
        assert!(view.finish_push_uncertain(
            request.generation,
            "push connection outcome is unknown".to_owned()
        ));
        assert!(view.is_push_outcome_uncertain());
        assert!(view.has_mutation_in_flight());

        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                let mutation_busy = view.has_mutation_in_flight();
                view.show(ui, true, "", true, mutation_busy);
            },
            view,
        );
        harness.run();
        let _warning = harness.get_by_role_and_label(
            Role::Label,
            "Push outcome is unknown. The daemon may still be transferring layers; Ferrum keeps image mutations blocked for this session.",
        );
        assert!(
            harness
                .get_by_role_and_label(Role::Button, "Push image")
                .accesskit_node()
                .is_disabled()
        );
    }
}
