use crate::search::CaseInsensitiveQuery;

use eframe::egui;
use ferrum_core::{ImageDetails, ImageSummary};
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
}

impl ImagesView {
    pub(super) fn is_loading(&self) -> bool {
        self.loading
    }
    pub(super) fn has_loaded(&self) -> bool {
        self.loaded
    }

    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.inspection_generation = self.inspection_generation.wrapping_add(1);
        self.loading = false;
        self.loaded = false;
        self.error = None;
        self.images.clear();
        self.visible.clear();
        self.clear_selection();
    }

    pub(super) fn begin_load(&mut self) -> u64 {
        self.invalidate();
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
    ) -> Option<ImageInspectionRequest> {
        if !connected {
            ui.heading("Image list unavailable");
            ui.label(connection_message);
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
    use egui_kittest::{Harness, kittest::Queryable};

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
                if let Some(request) = state.view.show(ui, true, "") {
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
        assert_eq!(harness.get_all_by_role(Role::Button).count(), 1);
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
        assert_eq!(harness.get_all_by_role(Role::Button).count(), 1);

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
        assert_eq!(harness.get_all_by_role(Role::Button).count(), 1);
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
}
