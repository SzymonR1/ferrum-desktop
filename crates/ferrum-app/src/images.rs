use crate::{display::escape_untrusted_display_text, search::CaseInsensitiveQuery};

#[cfg(target_os = "linux")]
use crate::image_build::{ImageBuildError, ImageBuildRequest};
use eframe::egui;
use ferrum_core::{ImageDetails, ImageHistoryEntry, ImagePullProgress, ImageSummary};
use std::fmt::Display;
#[cfg(target_os = "linux")]
use std::{collections::VecDeque, path::PathBuf};

#[cfg(target_os = "linux")]
const BUILD_OUTPUT_MAX_BYTES: usize = 2 * 1024 * 1024;
#[cfg(target_os = "linux")]
const BUILD_OUTPUT_MAX_LINES: usize = 10_000;
#[cfg(target_os = "linux")]
#[derive(Default)]
pub(super) struct BuildOutputBuffer {
    pub(super) lines: VecDeque<String>,
    pub(super) retained_bytes: usize,
    pub(super) dropped_lines: usize,
}

#[cfg(target_os = "linux")]
impl BuildOutputBuffer {
    fn push(&mut self, line: String) {
        let line = crate::display::escape_untrusted_display_text_with_limit(line, 16 * 1024)
            .unwrap_or_else(|| "[build output omitted]".to_owned());
        self.retained_bytes = self.retained_bytes.saturating_add(line.len());
        self.lines.push_back(line);
        while self.lines.len() > BUILD_OUTPUT_MAX_LINES
            || self.retained_bytes > BUILD_OUTPUT_MAX_BYTES
        {
            if let Some(removed) = self.lines.pop_front() {
                self.retained_bytes = self.retained_bytes.saturating_sub(removed.len());
                self.dropped_lines = self.dropped_lines.saturating_add(1);
            } else {
                break;
            }
        }
    }
}

const IMAGE_ROW_HEIGHT: f32 = 34.0;
const IMAGE_CELL_HEIGHT: f32 = 32.0;
const IMAGE_HISTORY_ROW_HEIGHT: f32 = 30.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum AppPage {
    #[default]
    Containers,
    Images,
    Volumes,
    Networks,
    Compose,
}

impl AppPage {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Containers => "Containers",
            Self::Images => "Images",
            Self::Volumes => "Volumes",
            Self::Networks => "Networks",
            Self::Compose => "Compose",
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
        AppPage::Compose,
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
    history: Vec<ImageHistoryEntry>,
    history_error: Option<String>,
    history_loading: bool,
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
    #[cfg(target_os = "linux")]
    pub(super) build_context: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    pub(super) build_repository: String,
    #[cfg(target_os = "linux")]
    pub(super) build_tag: String,
    #[cfg(target_os = "linux")]
    pub(super) build_output: BuildOutputBuffer,
    #[cfg(target_os = "linux")]
    pub(super) build_complete: bool,
    #[cfg(target_os = "linux")]
    pub(super) build_error: Option<String>,
    #[cfg(target_os = "linux")]
    build_confirmation_context_label: Option<String>,
    #[cfg(target_os = "linux")]
    build_generation: u64,
    #[cfg(target_os = "linux")]
    active_build_generation: Option<u64>,
    #[cfg(target_os = "linux")]
    build_confirmation_open: bool,
    #[cfg(target_os = "linux")]
    build_confirmed: bool,
    #[cfg(target_os = "linux")]
    build_requested: bool,
    #[cfg(target_os = "linux")]
    build_cancel_requested: bool,
    #[cfg(target_os = "linux")]
    build_loading: bool,
    #[cfg(target_os = "linux")]
    build_cancel_enabled: bool,
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
    #[cfg(target_os = "linux")]
    pub(super) fn build_target_valid(&self) -> bool {
        crate::image_build::valid_target(&self.build_repository, &self.build_tag)
            && self.build_context.is_some()
    }
    #[cfg(target_os = "linux")]
    pub(super) fn build_blocks_context_switch(&self) -> bool {
        self.build_confirmation_open
            || self.build_confirmed
            || self.build_requested
            || self.build_loading
            || self.active_build_generation.is_some()
    }

    #[cfg(target_os = "linux")]
    pub(super) fn begin_build_confirmation(&mut self, context_label: &str) -> bool {
        if self.build_target_valid() && !self.has_mutation_in_flight() && !self.build_loading {
            self.build_confirmation_open = true;
            self.build_confirmation_context_label =
                Some(escape_untrusted_display_text(context_label).into_owned());
            self.build_complete = false;
            self.build_error = None;
            return true;
        }
        false
    }
    #[cfg(target_os = "linux")]
    pub(super) fn cancel_build_confirmation(&mut self) {
        self.build_confirmation_open = false;
        self.build_confirmed = false;
        self.build_requested = false;
        self.build_confirmation_context_label = None;
    }
    #[cfg(target_os = "linux")]
    pub(super) fn confirm_build(&mut self) {
        if self.build_confirmation_open && self.build_target_valid() && !self.build_loading {
            self.build_confirmation_open = false;
            self.build_confirmed = true;
            self.build_requested = true;
        }
    }
    #[cfg(target_os = "linux")]
    pub(super) fn take_build_requested(&mut self) -> bool {
        std::mem::take(&mut self.build_requested)
    }
    #[cfg(target_os = "linux")]
    pub(super) fn begin_build(&mut self) -> Option<(u64, ImageBuildRequest)> {
        if !std::mem::take(&mut self.build_confirmed) || self.has_mutation_in_flight() {
            return None;
        }
        let context = self.build_context.clone()?;
        let request = ImageBuildRequest::unchecked(
            context,
            self.build_repository.clone(),
            self.build_tag.clone(),
        );
        self.build_generation = self.build_generation.wrapping_add(1);
        self.active_build_generation = Some(self.build_generation);
        self.build_loading = true;
        self.build_cancel_enabled = true;
        self.build_cancel_requested = false;
        self.build_complete = false;
        self.build_error = None;
        self.build_output = BuildOutputBuffer::default();
        Some((self.build_generation, request))
    }
    #[cfg(target_os = "linux")]
    pub(super) fn update_build_output(&mut self, generation: u64, line: String) -> bool {
        if self.active_build_generation != Some(generation) || !self.build_loading {
            return false;
        }
        self.build_output.push(line);
        true
    }
    #[cfg(target_os = "linux")]
    pub(super) fn request_cancel_build(&mut self) {
        if self.build_loading && self.build_cancel_enabled {
            self.build_cancel_requested = true;
            self.build_cancel_enabled = false;
        }
    }
    #[cfg(target_os = "linux")]
    pub(super) fn take_build_cancel_requested(&mut self) -> bool {
        std::mem::take(&mut self.build_cancel_requested)
    }
    #[cfg(target_os = "linux")]
    pub(super) fn finish_build(
        &mut self,
        generation: u64,
        result: Result<(), ImageBuildError>,
    ) -> bool {
        if self.active_build_generation != Some(generation) {
            return false;
        }
        self.active_build_generation = None;
        self.build_loading = false;
        self.build_cancel_enabled = false;
        self.build_cancel_requested = false;
        match result {
            Ok(()) => self.build_complete = true,
            Err(error) => self.build_error = Some(error.to_string()),
        }
        true
    }

    pub(super) fn is_pulling(&self) -> bool {
        self.pull_loading
    }
    #[cfg(test)]
    pub(super) fn is_pull_progress_hidden(&self) -> bool {
        self.pull_progress_hidden
    }
    #[cfg(test)]
    pub(super) fn is_pull_complete(&self) -> bool {
        self.pull_complete
    }
    #[cfg(target_os = "linux")]
    fn draw_build_controls(
        &mut self,
        ui: &mut egui::Ui,
        allowed: bool,
        mutation_busy: bool,
        context_label: &str,
    ) {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.heading("Build local image");
            if (self.build_confirmation_open || self.build_loading)
                && let Some(context) = &self.build_confirmation_context_label
            {
                ui.horizontal(|ui| {
                    ui.label("Selected engine source:");
                    ui.label(context);
                });
            }
            ui.label("Saved registry credentials are not read or sent; private base images may fail.");
            if let Some(context) = &self.build_context {
                let context = context.display().to_string();
                let safe = escape_untrusted_display_text(&context);
                ui.label(format!("Build context: {safe}"));
            } else {
                ui.label("Choose a local directory containing a regular root-level Dockerfile.");
            }
            if ui.add_enabled(allowed && !self.build_loading && !self.build_confirmation_open && !mutation_busy, egui::Button::new("Choose build context")).clicked()
                && let Some(context) = rfd::FileDialog::new().pick_folder()
            {
                self.build_context = Some(context);
            }
            ui.add_enabled_ui(!self.build_loading && !self.build_confirmation_open, |ui| {
                ui.horizontal(|ui| {
                    let label = ui.label("Build repository");
                    ui.text_edit_singleline(&mut self.build_repository).labelled_by(label.id);
                });
                ui.horizontal(|ui| {
                    let label = ui.label("Build tag");
                    ui.text_edit_singleline(&mut self.build_tag).labelled_by(label.id);
                });
            });
            let target_valid = self.build_target_valid();
            if !target_valid {
                ui.label("Choose a local build context and enter a valid repository/tag target (512-byte combined limit).");
            }
            if ui.add_enabled(
                allowed && target_valid && !self.build_loading && !mutation_busy && !self.build_confirmation_open,
                egui::Button::new("Build image"),
            ).clicked() {
                self.begin_build_confirmation(context_label);
            }
            if self.build_confirmation_open {
                let target = format!("{}:{}", self.build_repository, self.build_tag);
                let target = escape_untrusted_display_text(&target);
                ui.group(|ui| {
                    ui.heading("Confirm image build");
                    ui.label(format!(
                        "Build {target} from this context? Docker CLI sends the selected context to the local Docker engine after applying .dockerignore. Dockerfile commands can execute arbitrary code, and Docker may assign this tag to a different image. Confirm to run Buildx."
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Confirm build").clicked() { self.confirm_build(); }
                        if ui.button("Cancel").clicked() { self.cancel_build_confirmation(); }
                    });
                });
            }
            if self.build_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Building image…");
                    if ui.add_enabled(self.build_cancel_enabled, egui::Button::new("Cancel build")).clicked() {
                        self.request_cancel_build();
                    }
                });
            } else if self.build_complete {
                ui.label("Image build complete.");
            } else if let Some(error) = &self.build_error {
                ui.label(error);
            }
            if self.build_output.dropped_lines > 0 {
                ui.label(format!("{} older build output lines dropped.", self.build_output.dropped_lines));
            }
            egui::ScrollArea::vertical()
                .id_salt("image-build-output")
                .max_height(220.0)
                .show_rows(ui, 18.0, self.build_output.lines.len(), |ui, rows| {
                    for row in rows {
                        ui.add_sized(
                            [ui.available_width(), 18.0],
                            egui::Label::new(&self.build_output.lines[row]).truncate(),
                        );
                    }
                });
        });
    }
    pub(super) fn has_mutation_in_flight(&self) -> bool {
        let mut busy = self.active_pull_generation.is_some()
            || self.tag_loading
            || self.remove_loading
            || self.tag_confirmation_open
            || self.remove_confirmation_open;
        #[cfg(target_os = "linux")]
        {
            busy |= self.active_build_generation.is_some() || self.build_confirmation_open;
        }
        busy
    }
    pub(super) fn take_pull_requested(&mut self) -> bool {
        std::mem::take(&mut self.pull_requested)
    }
    pub(super) fn take_hide_progress_requested(&mut self) -> bool {
        std::mem::take(&mut self.hide_progress_requested)
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
        #[cfg(target_os = "linux")]
        {
            self.build_confirmation_open = false;
            self.build_confirmed = false;
            self.build_requested = false;
            self.build_cancel_requested = false;
            self.build_confirmation_context_label = None;
        }
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
            && !self.has_mutation_in_flight()
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
            && !self.has_mutation_in_flight()
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
    pub(super) fn complete_history<E: Display>(
        &mut self,
        list_generation: u64,
        inspection_generation: u64,
        id: &str,
        result: Result<Vec<ImageHistoryEntry>, E>,
    ) -> bool {
        if self.generation != list_generation
            || self.inspection_generation != inspection_generation
            || self.selected_id.as_deref() != Some(id)
            || !self.images.iter().any(|image| image.id == id)
        {
            return false;
        }
        self.history_loading = false;
        match result {
            Ok(history) => {
                self.history = history;
                self.history_error = None;
            }
            Err(error) => {
                self.history.clear();
                self.history_error = Some(error.to_string());
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
        build_context_label: &str,
    ) -> Option<ImageInspectionRequest> {
        ui.group(|ui| {
            ui.heading("Pull public image");
            ui.label("Anonymous pulls only; private registry authentication is not supported.");
            if mutation_busy && !self.pull_loading {
                ui.label("An image operation is in progress; wait for it to finish before starting another pull, tag, or removal.");
            }
            ui.horizontal(|ui| {
                let label = ui.label("Image reference");
                ui.add_enabled(
                    !mutation_busy || self.pull_loading,
                    egui::TextEdit::singleline(&mut self.pull_reference),
                ).labelled_by(label.id);
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
        #[cfg(target_os = "linux")]
        if operations_allowed || self.build_loading {
            self.draw_build_controls(ui, operations_allowed, mutation_busy, build_context_label);
        }
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

        let request = selected_id.and_then(|id| self.select_image(id));
        self.draw_details(ui);
        self.draw_history(ui);
        self.draw_tag_controls(ui, connected, operations_allowed, mutation_busy);
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
    fn draw_history(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();
        ui.heading("Image layer history");
        if self.selected_id.is_none() {
            ui.label("Select an image to load its read-only layer history.");
        } else if self.history_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading image history…");
            });
        } else if let Some(error) = &self.history_error {
            ui.colored_label(
                egui::Color32::from_rgb(227, 113, 102),
                "Image history unavailable",
            );
            ui.label(error);
        } else if self.history.is_empty() {
            ui.label("No safe image history fields were reported.");
        } else {
            let width = ui.available_width();
            let layer_width = width * 0.52;
            let created_width = width * 0.24;
            let size_width = width - layer_width - created_width;
            ui.horizontal(|ui| {
                ui.add_sized(
                    [layer_width, IMAGE_CELL_HEIGHT],
                    egui::Label::new("Layer ID"),
                );
                ui.add_sized(
                    [created_width, IMAGE_CELL_HEIGHT],
                    egui::Label::new("Created (Unix seconds)"),
                );
                ui.add_sized([size_width, IMAGE_CELL_HEIGHT], egui::Label::new("Size"));
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("image-history")
                .max_height(180.0)
                .show_rows(
                    ui,
                    IMAGE_HISTORY_ROW_HEIGHT,
                    self.history.len(),
                    |ui, rows| {
                        for row in rows {
                            let entry = &self.history[row];
                            let layer_id = entry.layer_id.as_deref().unwrap_or("Not reported");
                            let created = entry.created_unix_seconds.map_or_else(
                                || "Not reported".to_owned(),
                                |value| value.to_string(),
                            );
                            let size = format_size(entry.size_bytes);
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [layer_width, IMAGE_HISTORY_ROW_HEIGHT],
                                    egui::Label::new(layer_id)
                                        .truncate()
                                        .show_tooltip_when_elided(true),
                                );
                                ui.add_sized(
                                    [created_width, IMAGE_HISTORY_ROW_HEIGHT],
                                    egui::Label::new(created),
                                );
                                ui.add_sized(
                                    [size_width, IMAGE_HISTORY_ROW_HEIGHT],
                                    egui::Label::new(size),
                                );
                            });
                        }
                    },
                );
        }
    }

    fn select_image(&mut self, id: String) -> Option<ImageInspectionRequest> {
        if !self.images.iter().any(|image| image.id == id) {
            return None;
        }
        self.selected_id = Some(id.clone());
        self.details = None;
        self.detail_error = None;
        self.inspection_loading = true;
        self.history.clear();
        self.history_error = None;
        self.history_loading = true;
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        Some(ImageInspectionRequest {
            list_generation: self.generation,
            inspection_generation: self.inspection_generation,
            id,
        })
    }

    fn clear_selection(&mut self) {
        self.selected_id = None;
        self.details = None;
        self.detail_error = None;
        self.inspection_loading = false;
        self.history.clear();
        self.history_error = None;
        self.history_loading = false;
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
            ui.add_enabled_ui(!self.tag_loading && !self.tag_confirmation_open && !mutation_busy, |ui| {
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

pub(super) fn valid_docker_tag(tag: &str) -> bool {
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
                if let Some(request) =
                    state
                        .view
                        .show(ui, true, "", true, false, "Automatic local discovery")
                {
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

    #[cfg(target_os = "linux")]
    fn build_view() -> ImagesView {
        ImagesView {
            build_context: Some(std::path::PathBuf::from("/synthetic/context")),
            build_repository: "example/service".to_owned(),
            build_tag: "release-1".to_owned(),
            ..ImagesView::default()
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_target_is_bounded_and_rejects_invalid_controls() {
        let mut view = build_view();
        assert!(view.build_target_valid());
        view.build_tag = "bad tag".to_owned();
        assert!(!view.build_target_valid());
        view.build_tag = "x".repeat(129);
        assert!(!view.build_target_valid());
        view.build_tag = "latest".to_owned();
        view.build_repository = "example\nservice".to_owned();
        assert!(!view.build_target_valid());
        view.build_repository = "r".repeat(509);
        assert!(!view.build_target_valid());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_confirmation_requires_explicit_acceptance_and_cancel_submits_nothing() {
        let mut view = build_view();
        assert!(view.begin_build_confirmation("Automatic local discovery"));
        assert!(!view.take_build_requested());
        view.cancel_build_confirmation();
        assert!(!view.take_build_requested());
        assert!(view.begin_build().is_none());

        assert!(view.begin_build_confirmation("Automatic local discovery"));
        view.confirm_build();
        assert!(view.take_build_requested());
        assert!(view.begin_build().is_some());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn build_confirmation_names_the_selected_engine_source() {
        let mut view = build_view();
        assert!(view.begin_build_confirmation("development"));
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, true, "", true, false, "development");
            },
            view,
        );
        harness.run();
        let _heading = harness.get_by_role_and_label(Role::Label, "Selected engine source:");
        let _context = harness.get_by_role_and_label(Role::Label, "development");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_confirmation_gates_other_image_mutations() {
        let mut view = taggable_view();
        view.build_context = Some(std::path::PathBuf::from("/synthetic/context"));
        view.build_repository = "example/service".to_owned();
        view.build_tag = "release-1".to_owned();
        assert!(view.begin_build_confirmation("Automatic local discovery"));
        view.pull_reference = "alpine:3.20".to_owned();
        assert!(view.begin_pull().is_none());
        view.begin_tag_confirmation();
        assert!(!view.tag_confirmation_open);
        view.begin_remove_confirmation();
        assert!(!view.remove_confirmation_open);
        view.tag_confirmed = true;
        view.tag_confirmation_image_id = Some("sha256:image".to_owned());
        assert!(view.begin_tag().is_none());
        view.remove_confirmed = true;
        view.remove_confirmation_image_id = Some("sha256:image".to_owned());
        assert!(view.begin_remove().is_none());
        view.confirm_build();
        assert!(view.take_build_requested());
        let (generation, _request) = view.begin_build().expect("confirmed build starts");

        assert!(view.has_mutation_in_flight());
        view.pull_reference = "alpine:3.20".to_owned();
        assert!(view.begin_pull().is_none());
        view.tag_confirmed = true;
        view.tag_confirmation_image_id = Some("sha256:image".to_owned());
        assert!(view.begin_tag().is_none());
        view.remove_confirmed = true;
        view.remove_confirmation_image_id = Some("sha256:image".to_owned());
        assert!(view.begin_remove().is_none());

        assert!(view.finish_build(generation, Ok(())));
        assert!(!view.has_mutation_in_flight());
        assert!(view.begin_pull().is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_start_progress_success_error_and_cancel_have_terminal_transitions() {
        let mut view = build_view();
        assert!(view.begin_build_confirmation("Automatic local discovery"));
        view.confirm_build();
        assert!(view.take_build_requested());
        let (generation, _) = view.begin_build().expect("confirmed build starts");
        assert!(view.has_mutation_in_flight());
        assert!(view.update_build_output(generation, "Step 1/1 : FROM scratch".to_owned()));
        assert!(view.finish_build(generation, Ok(())));
        assert!(!view.has_mutation_in_flight());
        assert!(view.build_complete);

        assert!(view.begin_build_confirmation("Automatic local discovery"));
        view.confirm_build();
        assert!(view.take_build_requested());
        let (failed_generation, _) = view.begin_build().expect("next confirmed build starts");
        assert!(view.finish_build(
            failed_generation,
            Err(crate::image_build::ImageBuildError::Failed)
        ));
        assert!(!view.has_mutation_in_flight());
        assert!(view.build_error.is_some());

        assert!(view.begin_build_confirmation("Automatic local discovery"));
        view.confirm_build();
        assert!(view.take_build_requested());
        let (cancel_generation, _) = view.begin_build().expect("third confirmed build starts");
        view.request_cancel_build();
        assert!(view.take_build_cancel_requested());
        assert!(view.finish_build(
            cancel_generation,
            Err(crate::image_build::ImageBuildError::Cancelled)
        ));
        assert!(!view.has_mutation_in_flight());
        assert!(view.build_error.is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_output_is_bounded_and_escaped() {
        let mut view = build_view();
        assert!(view.begin_build_confirmation("Automatic local discovery"));
        view.confirm_build();
        assert!(view.take_build_requested());
        let (generation, _) = view.begin_build().expect("confirmed build starts");
        for _ in 0..10_050 {
            assert!(view.update_build_output(generation, "line".to_owned()));
        }
        assert!(view.build_output.lines.len() <= 10_000);
        let long_line = "x".repeat(512);
        for _ in 0..5_000 {
            assert!(view.update_build_output(generation, long_line.clone()));
        }
        assert!(view.update_build_output(generation, "escape\u{1b}[31m bidi\u{202e}".to_owned(),));
        let output = view
            .build_output
            .lines
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(view.build_output.retained_bytes <= 2 * 1024 * 1024);
        assert!(view.build_output.lines.len() <= 10_000);
        assert!(view.build_output.dropped_lines > 0);
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\u{202e}'));
        assert!(output.contains("\\x1b"));
        assert!(output.contains("\\u{202e}"));
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
                view.show(ui, true, "", true, true, "Automatic local discovery");
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
            "An image operation is in progress; wait for it to finish before starting another pull, tag, or removal.",
        );
    }

    #[test]
    fn an_in_flight_image_mutation_disables_competing_operations() {
        let mut view = taggable_view();
        view.pull_reference = "alpine:3.20".to_owned();
        let mut harness = Harness::new_ui_state(
            |ui, view: &mut ImagesView| {
                view.show(ui, true, "", true, true, "Automatic local discovery");
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
            "An image operation is in progress; wait for it to finish before starting another pull, tag, or removal.",
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
    fn navigation_opens_the_compose_page_through_an_accessible_action() {
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
            .get_by_role_and_label(Role::Button, "Compose")
            .click_accesskit();
        harness.run();
        assert_eq!(harness.state().page, AppPage::Compose);
        let _compose = harness.get_by_role_and_label(Role::Label, "Compose");
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
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading image details…");
        let request = harness.state_mut().inspection.take().unwrap();
        assert!(harness.state_mut().view.complete_inspection(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(details("sha256:alpine", "alpine:3.20")),
        ));
        assert!(harness.state_mut().view.complete_history(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(Vec::new()),
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
        harness.step();
        let request = harness.state_mut().inspection.take().unwrap();
        let mut details = details(&request.id, tag);
        details.author = Some("author\u{e0001}text\u{200b}".to_owned());
        assert!(harness.state_mut().view.complete_inspection(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(details),
        ));
        assert!(harness.state_mut().view.complete_history(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(Vec::new()),
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
        harness.step();
        let request = harness.state_mut().inspection.take().unwrap();
        assert!(harness.state_mut().view.complete_inspection(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Err::<ImageDetails, _>("inspection denied"),
        ));
        assert!(harness.state_mut().view.complete_history(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(Vec::new()),
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
        harness.step();
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
        harness.step();
        let current_inspection = harness.state_mut().inspection.take().unwrap();

        harness.state_mut().view.complete_inspection(
            stale_inspection.list_generation,
            stale_inspection.inspection_generation,
            &stale_inspection.id,
            Ok::<_, &str>(details("current", "current:tag")),
        );
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading image details…");

        harness.state_mut().view.complete_inspection(
            current_inspection.list_generation,
            current_inspection.inspection_generation,
            &current_inspection.id,
            Ok::<_, &str>(details("fresh", "fresh:tag")),
        );
        assert!(harness.state_mut().view.complete_history(
            current_inspection.list_generation,
            current_inspection.inspection_generation,
            &current_inspection.id,
            Ok::<_, &str>(Vec::new()),
        ));
        harness.run();
        let _fresh_tag = harness.get_by_role_and_label(Role::Label, "fresh:tag");
    }
    #[test]
    fn selected_image_history_shows_loading_empty_and_error_states() {
        let mut harness = harness_for(loaded_view(vec![image(
            "sha256:history-image",
            "history:latest",
            1024,
        )]));
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "history:latest")
            .click_accesskit();
        harness.step();
        let request = harness.state_mut().inspection.take().unwrap();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading image history…");

        assert!(harness.state_mut().view.complete_history(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Ok::<_, &str>(Vec::new()),
        ));
        harness.run();
        let _empty = harness
            .get_by_role_and_label(Role::Label, "No safe image history fields were reported.");

        harness
            .get_by_role_and_label(Role::Button, "history:latest")
            .click_accesskit();
        harness.step();
        let request = harness.state_mut().inspection.take().unwrap();
        assert!(harness.state_mut().view.complete_history(
            request.list_generation,
            request.inspection_generation,
            &request.id,
            Err::<Vec<ferrum_core::ImageHistoryEntry>, _>("history denied"),
        ));
        harness.run();
        let _error = harness.get_by_role_and_label(Role::Label, "Image history unavailable");
        let _message = harness.get_by_role_and_label(Role::Label, "history denied");
    }

    #[test]
    fn image_history_results_for_stale_or_noninventory_ids_are_ignored() {
        let mut harness = harness_for(loaded_view(vec![
            image("sha256:first", "first:latest", 1024),
            image("sha256:current", "current:latest", 2048),
        ]));
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "first:latest")
            .click_accesskit();
        harness.step();
        let stale = harness.state_mut().inspection.take().unwrap();
        harness
            .get_by_role_and_label(Role::Button, "current:latest")
            .click_accesskit();
        harness.step();
        let current = harness.state_mut().inspection.take().unwrap();

        assert!(!harness.state_mut().view.complete_history(
            stale.list_generation,
            stale.inspection_generation,
            &stale.id,
            Ok::<_, &str>(vec![ferrum_core::ImageHistoryEntry {
                layer_id: Some(format!("sha256:{}", "a".repeat(64))),
                created_unix_seconds: Some(1),
                size_bytes: Some(1),
            }]),
        ));
        assert!(!harness.state_mut().view.complete_history(
            current.list_generation,
            current.inspection_generation,
            "sha256:not-in-inventory",
            Ok::<_, &str>(Vec::new()),
        ));
        harness.step();
        let _loading = harness.get_by_role_and_label(Role::Label, "Loading image history…");

        assert!(harness.state_mut().view.complete_history(
            current.list_generation,
            current.inspection_generation,
            &current.id,
            Ok::<_, &str>(vec![ferrum_core::ImageHistoryEntry {
                layer_id: Some(format!("sha256:{}", "b".repeat(64))),
                created_unix_seconds: Some(1_720_000_000),
                size_bytes: Some(4096),
            }]),
        ));
        harness.run();
        let _layer =
            harness.get_by_role_and_label(Role::Label, &format!("sha256:{}", "b".repeat(64)));
        let _created = harness.get_by_role_and_label(Role::Label, "1720000000");
        let _size = harness.get_by_role_and_label(Role::Label, "4096 bytes");
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
                view.show(
                    ui,
                    false,
                    "offline",
                    false,
                    false,
                    "Automatic local discovery",
                );
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
                view.show(ui, true, "", true, false, "Automatic local discovery");
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
                view.show(ui, true, "", true, false, "Automatic local discovery");
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
                if let Some(request) = state.view.show(
                    ui,
                    true,
                    "",
                    true,
                    mutation_busy,
                    "Automatic local discovery",
                ) {
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
            "An image operation is in progress; wait for it to finish before starting another pull, tag, or removal.",
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
}
