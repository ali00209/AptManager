//! The package list and grid, the history view, and the browse layout that joins them
//! with the details panel.

use std::ops::Range;

use gpui_kit::component::{
    button::{Button, ButtonVariants},
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{human_size, Source};
use crate::ui::app::AppView;
use crate::ui::theme::*;
use crate::ui::widgets::{centered_message, icon_element, registry_chip};

impl AppView {
    pub(crate) fn render_browse(&self, cx: &mut Context<Self>) -> AnyElement {
        let main = if self.show_history {
            self.render_history()
        } else if self.grid {
            self.render_grid(cx)
        } else {
            self.render_list(cx)
        };
        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .child(main)
            .child(self.render_details(cx))
            .into_any_element()
    }

    pub(crate) fn render_column_header(&self) -> impl IntoElement {
        div()
            .h(px(28.))
            .w_full()
            .flex()
            .items_center()
            .px_3()
            .gap_3()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(MUTED))
            .text_xs()
            .child(div().w(px(16.)).flex_none())
            .child(div().w(px(20.)).flex_none())
            .child(div().flex_1().min_w(px(180.)).child("Name"))
            .child(div().w(px(200.)).flex_none().child("Version"))
            .child(div().w(px(100.)).flex_none().child("Size"))
            .child(div().w(px(150.)).flex_none().child("Registry"))
    }

    pub(crate) fn render_row(&self, row: usize, idx: usize, cx: &mut Context<Self>) -> AnyElement {
        let pkg = &self.packages[idx];
        let key = pkg.key();
        let is_checked = self.checked.contains(&key);
        let is_selected = self.selected.as_deref() == Some(key.as_str());

        let check_key = key.clone();
        let select_key = key.clone();

        let checkbox = div()
            .id(("chk", row))
            .size(px(16.))
            .flex_none()
            .rounded(px(3.))
            .border_1()
            .border_color(rgb(if is_checked { ACCENT } else { MUTED }))
            .flex()
            .items_center()
            .justify_center()
            .when(is_checked, |d| d.bg(rgb(ACCENT)).child("✓"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this: &mut AppView, _: &MouseDownEvent, _window, cx| {
                    this.toggle_checked(&check_key);
                    cx.notify();
                }),
            );

        let mut name_cell = div()
            .flex_1()
            .min_w(px(180.))
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .whitespace_nowrap();
        if pkg.is_installed() {
            name_cell = name_cell.child(div().text_color(rgb(SUCCESS)).child("✓"));
        }
        if pkg.upgradable {
            name_cell = name_cell.child(div().text_color(rgb(ACCENT)).child("⬆"));
        }
        name_cell = name_cell.child(div().child(pkg.title.clone()));

        let version = if pkg.candidate.is_empty() {
            "—".to_string()
        } else {
            pkg.candidate.clone()
        };
        let size = if pkg.download_size > 0 {
            pkg.download_size
        } else {
            pkg.installed_size
        };

        div()
            .id(("row", row))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .px_3()
            .gap_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .when(is_selected, |d| d.bg(rgb(SELECTED)))
            .hover(|s| s.bg(rgb(HOVER)))
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.selected = Some(select_key.clone());
                cx.notify();
            }))
            .child(checkbox)
            .child(icon_element(pkg, 20.))
            .child(name_cell)
            .child(
                div()
                    .w(px(200.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(version),
            )
            .child(div().w(px(100.)).flex_none().child(human_size(size)))
            .child(div().w(px(150.)).flex_none().child(registry_chip(pkg.source)))
            .into_any_element()
    }

    /// One card for grid view: icon, name, version, state and a checkbox.
    pub(crate) fn render_card(&self, row: usize, idx: usize, cx: &mut Context<Self>) -> AnyElement {
        let pkg = &self.packages[idx];
        let key = pkg.key();
        let is_checked = self.checked.contains(&key);
        let is_selected = self.selected.as_deref() == Some(key.as_str());

        let check_key = key.clone();
        let select_key = key.clone();

        let checkbox = div()
            .id(("card-chk", row))
            .size(px(16.))
            .flex_none()
            .rounded(px(3.))
            .border_1()
            .border_color(rgb(if is_checked { ACCENT } else { MUTED }))
            .flex()
            .items_center()
            .justify_center()
            .when(is_checked, |d| d.bg(rgb(ACCENT)).child("✓"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this: &mut AppView, _: &MouseDownEvent, _window, cx| {
                    this.toggle_checked(&check_key);
                    cx.notify();
                }),
            );

        let state = if pkg.upgradable {
            ("Update available", ACCENT)
        } else if pkg.is_installed() {
            ("Installed", SUCCESS)
        } else {
            ("Not installed", MUTED)
        };
        let blurb: String = pkg.description.chars().take(80).collect();

        div()
            .id(("card", row))
            .w(px(220.))
            .h(px(132.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(8.))
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(if is_selected { ACCENT } else { BORDER }))
            .hover(|s| s.bg(rgb(HOVER)))
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.selected = Some(select_key.clone());
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon_element(pkg, 36.))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(pkg.title.clone()),
                    )
                    .child(checkbox),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(if pkg.candidate.is_empty() { "—".to_string() } else { pkg.candidate.clone() }),
            )
            .child(div().flex_1().text_xs().text_color(rgb(MUTED)).overflow_hidden().child(blurb))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(registry_chip(pkg.source))
                    .child(div().text_xs().text_color(rgb(state.1)).child(state.0)),
            )
            .into_any_element()
    }

    /// Rows for the virtualized list. Called with the visible row range only.
    pub(crate) fn render_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::with_capacity(range.len());
        for row in range {
            if let Some(&idx) = self.visible.get(row) {
                rows.push(self.render_row(row, idx, cx));
            }
        }
        rows
    }

    /// Buttons offered when a search finds nothing: install the typed name with each
    /// ticked registry that has no catalogue and supports install by name.
    pub(crate) fn install_hints(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let name = self.submitted.clone().unwrap_or_default().trim().to_string();
        if name.is_empty() {
            return Vec::new();
        }
        let options: Vec<Source> = self
            .active_registries()
            .into_iter()
            .filter(|s| self.registries.contains(s) && !s.has_catalogue() && *s != Source::AppImage)
            .collect();
        if options.is_empty() {
            return Vec::new();
        }
        let mut hints = Vec::new();
        for src in options {
            let typed = name.clone();
            hints.push(
                Button::new(src.label())
                    .primary()
                    .label(format!("Install '{name}' with {}", src.label()))
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.install_by_name(src, typed.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        hints
    }

    /// What fills the list when there is nothing to show.
    pub(crate) fn empty_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .flex_1()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(rgb(MUTED))
            .child(self.empty_text());
        for hint in self.install_hints(cx) {
            col = col.child(hint);
        }
        col.child(
            div()
                .text_xs()
                .child("AppImages: drop .AppImage files anywhere on this list to add them to ~/Applications."),
        )
        .into_any_element()
    }

    /// A line that says how many matches the limit hides.
    pub(crate) fn cap_line(&self, shown: usize) -> Option<AnyElement> {
        (self.matched > shown).then(|| {
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(format!(
                    "Showing the first {shown} of {}. Refine your search to see the rest.",
                    self.matched
                ))
                .into_any_element()
        })
    }

    /// The package list. The whole list is also a drop target: AppImages, .deb and Flatpak files
    /// dropped on it are installed.
    pub(crate) fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = if !self.loaded {
            centered_message("Reading packages…")
        } else if self.visible.is_empty() {
            self.empty_view(cx)
        } else {
            self.render_list_body(cx)
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(body)
            .on_drop(cx.listener(|this: &mut AppView, paths: &ExternalPaths, _window, cx| {
                this.install_dropped(paths.paths().to_vec(), cx);
            }))
            .into_any_element()
    }

    fn render_list_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity().clone();
        let list = uniform_list("package-rows", self.visible.len(), move |range, _window, cx| {
            view.update(cx, |this, cx| this.render_rows(range, cx))
        })
        .size_full();

        let mut col = div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.render_column_header());
        if let Some(line) = self.cap_line(self.visible.len()) {
            col = col.child(line);
        }
        col.child(div().flex_1().w_full().child(list)).into_any_element()
    }

    pub(crate) fn render_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.loaded {
            return centered_message("Loading packages…");
        }
        if self.visible.is_empty() {
            return self.empty_view(cx);
        }
        let shown = self.visible.len().min(GRID_LIMIT);
        let cards: Vec<AnyElement> = (0..shown)
            .map(|row| {
                let idx = self.visible[row];
                self.render_card(row, idx, cx)
            })
            .collect();

        let mut body = div()
            .id("grid-scroll")
            .flex_1()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .overflow_y_scroll()
            .child(div().flex().flex_wrap().gap_3().children(cards));
        if let Some(line) = self.cap_line(shown) {
            body = body.child(line);
        }
        body.into_any_element()
    }

    pub(crate) fn empty_text(&self) -> String {
        if self.registries.is_empty() {
            return "Choose at least one registry.".into();
        }
        let query = self.submitted.clone().unwrap_or_default();
        let query = query.trim();
        if query.is_empty() {
            return if self.updates_only {
                "No updates in the chosen registries.".into()
            } else {
                "Nothing installed in the chosen registries. Type a name and press Enter to search the catalogues.".into()
            };
        }
        format!("No packages match '{query}' in the chosen registries.")
    }

    pub(crate) fn render_history(&self) -> AnyElement {
        if self.history.is_empty() {
            return centered_message("No apt history found in /var/log/apt/history.log.");
        }
        let rows: Vec<AnyElement> = self
            .history
            .iter()
            .enumerate()
            .map(|(ix, entry)| {
                div()
                    .id(("history", ix))
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .w(px(170.))
                            .flex_none()
                            .text_color(rgb(MUTED))
                            .child(entry.date.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(entry.command.clone()),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .id("history-list")
            .flex_1()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }
}
