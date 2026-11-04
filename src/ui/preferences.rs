//! The Package manager preferences screen and each manager's sources page.

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    switch::Switch,
    Disableable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::Source;
use crate::ui::app::{AppView, Page};
use crate::ui::theme::*;
use crate::ui::widgets::glyph_box;

impl AppView {
    /// The preferences list: one row per manager with a switch, a status badge and
    /// a chevron that opens its sources.
    pub(crate) fn render_preferences(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.prefs_query.trim().to_lowercase();
        let rows: Vec<AnyElement> = Source::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, src)| {
                query.is_empty()
                    || src.label().to_lowercase().contains(&query)
                    || src.description().to_lowercase().contains(&query)
            })
            .map(|(ix, src)| self.render_pref_row(ix, src, cx))
            .collect();

        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(52.))
                    .w_full()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .bg(rgb(PANEL))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        Button::new("prefs-back")
                            .ghost()
                            .label("←")
                            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                                this.page = Page::Browse;
                                cx.notify();
                            })),
                    )
                    .child(div().text_lg().child("Package manager preferences"))
                    .child(div().flex_1())
                    .child(div().w(px(300.)).child(Input::new(&self.prefs_search))),
            )
            .child(
                div()
                    .id("prefs-scroll")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    pub(crate) fn render_pref_row(&self, ix: usize, src: Source, cx: &mut Context<Self>) -> AnyElement {
        let detected = self.detected.get(&src).copied().unwrap_or(false);
        let on = self.enabled.contains(&src);
        let (badge, badge_color) = if !detected {
            ("Not found", AMBER)
        } else if on {
            ("Ready", SUCCESS)
        } else {
            ("Disabled", MUTED)
        };
        let path_text = match self.paths.get(&src) {
            Some(path) => format!("Path: {path}"),
            None => "Not found on this machine".to_string(),
        };

        div()
            .id(("pref", ix))
            .h(px(72.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .rounded(px(8.))
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .child(glyph_box(src.glyph(), src.color(), 36.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .overflow_hidden()
                    .child(div().child(src.label()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(src.description()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(if detected { TEXT } else { AMBER }))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(path_text),
                    ),
            )
            .child(
                div()
                    .w(px(90.))
                    .flex_none()
                    .text_xs()
                    .text_color(rgb(badge_color))
                    .child(badge),
            )
            .child(
                Switch::new(("switch", ix))
                    .checked(on)
                    .disabled(!detected)
                    .on_click(cx.listener(move |this: &mut AppView, checked: &bool, _window, cx| {
                        this.set_enabled(src, *checked, cx);
                    })),
            )
            .child(
                Button::new(("sources", ix))
                    .ghost()
                    .label("›")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.open_sources(src, cx);
                    })),
            )
            .into_any_element()
    }

    /// Sources page for one manager. Flatpak remotes can be added and removed here;
    /// the other managers show their sources read-only.
    pub(crate) fn render_sources(&self, src: Source, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div().flex().flex_col().gap_2();
        if src == Source::Flatpak {
            for (ix, line) in self.source_lines.iter().enumerate() {
                let mut row = div()
                    .id(("source-row", ix))
                    .h(px(44.))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .rounded(px(8.))
                    .bg(rgb(PANEL))
                    .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(line.clone()));
                if !line.starts_with("No ") {
                    let name = line.split_whitespace().next().unwrap_or("").to_string();
                    row = row.child(
                        Button::new(("rm-remote", ix))
                            .danger()
                            .label("Remove")
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                                this.remove_remote(name.clone(), cx);
                            })),
                    );
                }
                body = body.child(row);
            }
            body = body.child(
                div()
                    .pt_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.source_input)))
                    .child(
                        Button::new("add-remote")
                            .primary()
                            .label("Add remote")
                            .disabled(self.busy)
                            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                                this.add_remote(cx);
                            })),
                    ),
            );
        } else {
            for (ix, line) in self.source_lines.iter().enumerate() {
                body = body.child(
                    div()
                        .id(("source-ro", ix))
                        .h(px(40.))
                        .w_full()
                        .flex()
                        .items_center()
                        .px_3()
                        .rounded(px(8.))
                        .bg(rgb(PANEL))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(line.clone()),
                );
            }
            body = body.child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Sources for this manager are read-only here. Change them with the manager's own tools."),
            );
        }

        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(52.))
                    .w_full()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .bg(rgb(PANEL))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        Button::new("sources-back")
                            .ghost()
                            .label("←")
                            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                                this.page = Page::Preferences;
                                cx.notify();
                            })),
                    )
                    .child(div().text_lg().child(format!("{} sources", src.label()))),
            )
            .child(
                div()
                    .id("sources-scroll")
                    .flex_1()
                    .p_4()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }
}
