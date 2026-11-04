//! The package list and grid, the history view, and the browse layout that joins them
//! with the details panel.

use std::ops::Range;

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    Disableable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{human_size, Source};
use crate::ui::app::{AppView, SubTab};
use crate::ui::theme::*;
use crate::ui::widgets::{centered_message, icon_element, location_label};

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
            .child(div().w(px(150.)).flex_none().child("Section / Source"))
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
            .child(
                div()
                    .w(px(150.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(rgb(MUTED))
                    .child(location_label(pkg)),
            )
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
            .child(div().text_xs().text_color(rgb(state.1)).child(state.0))
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

    /// Banner shown for managers with no catalogue: offers to install the typed name.
    pub(crate) fn install_hint(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let src = self.top;
        if src.has_catalogue() || src == Source::AppImage || self.sub != SubTab::Install {
            return None;
        }
        let name = self.query.trim().to_string();
        if name.is_empty() {
            return None;
        }
        if self
            .packages
            .iter()
            .any(|p| p.source == src && p.name.eq_ignore_ascii_case(&name))
        {
            return None;
        }
        let label = format!("Install '{name}' with {}", src.label());
        Some(
            div()
                .h(px(44.))
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .bg(rgb(PANEL))
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(div().text_color(rgb(MUTED)).child(format!("'{name}' is not installed yet.")))
                .child(
                    Button::new("install-by-name")
                        .primary()
                        .label(label)
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                            this.install_by_name(src, name.clone(), cx);
                        })),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.loaded {
            return centered_message("Loading packages…");
        }
        let hint = self.install_hint(cx);
        if self.visible.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .overflow_hidden()
                .children(hint)
                .child(centered_message(self.empty_text()))
                .into_any_element();
        }

        let view = cx.entity().clone();
        let list = uniform_list("package-rows", self.visible.len(), move |range, _window, cx| {
            view.update(cx, |this, cx| this.render_rows(range, cx))
        })
        .size_full();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .children(hint)
            .child(self.render_column_header())
            .child(div().flex_1().w_full().child(list))
            .into_any_element()
    }

    pub(crate) fn render_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.loaded {
            return centered_message("Loading packages…");
        }
        if self.visible.is_empty() {
            return centered_message(self.empty_text());
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
        if self.visible.len() > shown {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(format!(
                        "Showing the first {shown} of {}. Filter or search to see the rest.",
                        self.visible.len()
                    )),
            );
        }
        body.into_any_element()
    }

    pub(crate) fn empty_text(&self) -> &'static str {
        match self.top {
            Source::AppImage => "No AppImages found. Copy .AppImage files into ~/Applications.",
            Source::Flatpak => "No Flatpak apps match. Is a Flatpak remote configured?",
            src if !src.has_catalogue() => {
                "Nothing installed here yet. Type a package name above to install it."
            }
            _ => "No packages match your search.",
        }
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
