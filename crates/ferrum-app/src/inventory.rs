use crate::search::CaseInsensitiveQuery;

use eframe::egui;
use ferrum_core::ContainerSummary;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SortField {
    #[default]
    Name,
    Image,
    State,
    Status,
}

#[derive(Default)]
pub(super) struct InventoryView {
    query: String,
    matcher: CaseInsensitiveQuery,
    sort: SortField,
    descending: bool,
    visible: Vec<usize>,
}

impl InventoryView {
    pub(super) fn show_controls(&mut self, ui: &mut egui::Ui, inventory: &[ContainerSummary]) {
        ui.horizontal(|ui| {
            ui.label("Search");
            if ui.text_edit_singleline(&mut self.query).changed() {
                self.rebuild(inventory);
            }
            let mut sort = self.sort;
            egui::ComboBox::from_id_salt("container-sort-field")
                .selected_text(match sort {
                    SortField::Name => "Name",
                    SortField::Image => "Image",
                    SortField::State => "State",
                    SortField::Status => "Status",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sort, SortField::Name, "Name");
                    ui.selectable_value(&mut sort, SortField::Image, "Image");
                    ui.selectable_value(&mut sort, SortField::State, "State");
                    ui.selectable_value(&mut sort, SortField::Status, "Status");
                });
            let mut descending = self.descending;
            ui.checkbox(&mut descending, "Descending");
            self.set_sort(sort, descending, inventory);
        });
    }

    fn set_sort(&mut self, sort: SortField, descending: bool, inventory: &[ContainerSummary]) {
        if self.sort != sort || self.descending != descending {
            self.sort = sort;
            self.descending = descending;
            self.rebuild(inventory);
        }
    }

    pub(super) fn inventory_changed(&mut self, inventory: &[ContainerSummary]) {
        self.rebuild(inventory);
    }

    pub(super) fn visible(&self) -> &[usize] {
        &self.visible
    }

    fn rebuild(&mut self, inventory: &[ContainerSummary]) {
        self.matcher.set(&self.query);
        self.visible.clear();
        self.visible
            .extend(inventory.iter().enumerate().filter_map(|(index, item)| {
                let matches = self.matcher.matches(&item.name)
                    || self.matcher.matches(&item.id)
                    || item
                        .image
                        .as_deref()
                        .is_some_and(|v| self.matcher.matches(v))
                    || item
                        .state
                        .as_ref()
                        .is_some_and(|v| self.matcher.matches(v.label()))
                    || item
                        .status
                        .as_deref()
                        .is_some_and(|v| self.matcher.matches(v));
                matches.then_some(index)
            }));
        let sort = self.sort;
        let descending = self.descending;
        self.visible.sort_by(|left, right| {
            let a = sort_value(&inventory[*left], sort);
            let b = sort_value(&inventory[*right], sort);
            match (a, b) {
                (None, None) => left.cmp(right),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => {
                    let order = compare_case_insensitive(a, b);
                    let order = if descending { order.reverse() } else { order };
                    order.then_with(|| left.cmp(right))
                }
            }
        });
    }
}

fn sort_value(item: &ContainerSummary, field: SortField) -> Option<&str> {
    match field {
        SortField::Name => Some(item.name.as_str()),
        SortField::Image => item.image.as_deref(),
        SortField::State => item.state.as_ref().map(|v| v.label()),
        SortField::Status => item.status.as_deref(),
    }
}

fn compare_case_insensitive(left: &str, right: &str) -> std::cmp::Ordering {
    left.chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrum_core::ContainerState;

    fn item(
        id: &str,
        name: &str,
        image: Option<&str>,
        state: Option<ContainerState>,
        status: Option<&str>,
    ) -> ContainerSummary {
        ContainerSummary {
            id: id.into(),
            name: name.into(),
            image: image.map(str::to_owned),
            state,
            status: status.map(str::to_owned),
        }
    }

    fn ids<'a>(view: &InventoryView, rows: &'a [ContainerSummary]) -> Vec<&'a str> {
        view.visible()
            .iter()
            .map(|index| rows[*index].id.as_str())
            .collect()
    }

    #[test]
    fn searches_supported_fields_case_insensitively_without_matching_placeholders() {
        let rows = vec![
            item(
                "ID-Alpha",
                "Web",
                Some("nginx"),
                Some(ContainerState::Running),
                Some("Up 2 minutes"),
            ),
            item(
                "b",
                "Worker",
                Some("redis"),
                Some(ContainerState::Paused),
                Some("Exited"),
            ),
            item("empty", "Empty", None, None, None),
        ];
        let mut view = InventoryView::default();
        for query in ["web", "id-alpha", "NGINX", "running", "UP 2"] {
            view.query = query.to_owned();
            view.inventory_changed(&rows);
            assert_eq!(ids(&view, &rows), vec!["ID-Alpha"], "query: {query}");
        }
        view.query = "no match".to_owned();
        view.inventory_changed(&rows);
        assert!(view.visible().is_empty());
        view.query = "not reported".to_owned();
        view.inventory_changed(&rows);
        assert!(view.visible().is_empty());
    }

    #[test]
    fn searches_unicode_case_insensitively_with_lowercase_expansions() {
        let rows = vec![
            item("dotted-i", "İstanbul", None, None, None),
            item("accent", "café", None, None, None),
            item("other", "Berlin", None, None, None),
        ];
        let mut view = InventoryView::default();

        for (query, expected) in [("i\u{307}STAN", vec!["dotted-i"]), ("CAFÉ", vec!["accent"])] {
            view.query = query.to_owned();
            view.inventory_changed(&rows);
            assert_eq!(ids(&view, &rows), expected, "query: {query}");
        }
    }

    #[test]
    fn sorts_ascending_and_descending_with_unknowns_last() {
        let rows = vec![
            item("missing", "z", None, None, None),
            item(
                "b",
                "beta",
                Some("z-image"),
                Some(ContainerState::Paused),
                Some("z-status"),
            ),
            item(
                "a",
                "Alpha",
                Some("a-image"),
                Some(ContainerState::Running),
                Some("a-status"),
            ),
        ];
        let mut view = InventoryView::default();
        view.set_sort(SortField::Image, false, &rows);
        assert_eq!(ids(&view, &rows), vec!["a", "b", "missing"]);
        view.set_sort(SortField::Image, true, &rows);
        assert_eq!(ids(&view, &rows), vec!["b", "a", "missing"]);
        view.set_sort(SortField::Name, false, &rows);
        assert_eq!(ids(&view, &rows), vec!["a", "b", "missing"]);
        view.set_sort(SortField::Name, true, &rows);
        assert_eq!(ids(&view, &rows), vec!["missing", "b", "a"]);
    }

    #[test]
    fn equal_sort_keys_preserve_original_inventory_order() {
        let rows = vec![
            item("b", "Beta", Some("same-image"), None, None),
            item("a", "Alpha", Some("same-image"), None, None),
        ];
        let mut view = InventoryView::default();

        view.set_sort(SortField::Image, false, &rows);

        assert_eq!(ids(&view, &rows), vec!["b", "a"]);
    }

    #[test]
    fn unknown_state_and_status_values_sort_last() {
        let rows = vec![
            item("u", "Unknown", Some("image"), None, None),
            item(
                "r",
                "Known",
                Some("image"),
                Some(ContainerState::Running),
                Some("z-up"),
            ),
            item(
                "p",
                "Paused",
                Some("image"),
                Some(ContainerState::Paused),
                Some("a-paused"),
            ),
        ];
        let mut view = InventoryView::default();
        view.set_sort(SortField::State, false, &rows);
        assert_eq!(ids(&view, &rows), vec!["p", "r", "u"]);
        view.set_sort(SortField::State, true, &rows);
        assert_eq!(ids(&view, &rows), vec!["r", "p", "u"]);
        view.set_sort(SortField::Status, false, &rows);
        assert_eq!(ids(&view, &rows), vec!["p", "r", "u"]);
        view.set_sort(SortField::Status, true, &rows);
        assert_eq!(ids(&view, &rows), vec!["r", "p", "u"]);
    }
}
