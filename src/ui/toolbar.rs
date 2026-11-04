//! The top tab bar, the sub-tabs under it, and the search toolbar.

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    Disableable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::Source;
use crate::ui::app::{AppView, Page, SubTab};
use crate::ui::theme::*;

impl AppView {
    pub(crate) fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tabs = div()
            .flex()
            .gap_1()
            .p_1()
            .rounded(px(8.))
            .bg(rgb(CHIP));
        for src in self.tabs() {
            tabs = tabs.child(self.top_tab_button(src, cx));
        }

        div()
            .h(px(46.))
            .w_full()
            .flex()
            .items_center()
            .px_3()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(div().flex_1())
            .child(tabs)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .child(
                        Button::new("open-prefs")
                            .ghost()
                            .label("⚙ Preferences")
                            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                                this.page = Page::Preferences;
                                cx.notify();
                            })),
                    ),
            )
    }

    pub(crate) fn top_tab_button(&self, src: Source, cx: &mut Context<Self>) -> AnyElement {
        let label = src.label();
        let active = self.top == src && self.page == Page::Browse;
        div()
            .id(label)
            .px_3()
            .py_1()
            .rounded(px(6.))
            .when(active, |d| d.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
            .when(!active, |d| d.text_color(rgb(MUTED)).hover(|s| s.bg(rgb(HOVER))))
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.top = src;
                this.page = Page::Browse;
                this.show_history = false;
                this.refilter();
                cx.notify();
            }))
            .child(label)
            .into_any_element()
    }

    pub(crate) fn render_sub_bar(&self, src: Source, cx: &mut Context<Self>) -> impl IntoElement {
        let updates = self.updates_for(src);
        let updates_label = if updates > 0 {
            format!("Updates ({updates})")
        } else {
            "Updates".to_string()
        };
        div()
            .h(px(38.))
            .w_full()
            .flex()
            .items_end()
            .gap_6()
            .px_4()
            .bg(rgb(BG))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(self.sub_tab_button("Install", SubTab::Install, "Install".to_string(), cx))
            .child(self.sub_tab_button("Updates", SubTab::Updates, updates_label, cx))
            .child(self.sub_tab_button("Manage", SubTab::Manage, "Manage".to_string(), cx))
    }

    pub(crate) fn sub_tab_button(
        &self,
        id: &'static str,
        tab: SubTab,
        label: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.sub == tab;
        div()
            .id(id)
            .pt_2()
            .pb_2()
            .border_b_2()
            .border_color(if active { rgb(ACCENT) } else { rgba(0x00000000) })
            .text_color(if active { rgb(TEXT) } else { rgb(MUTED) })
            .hover(|s| s.text_color(rgb(TEXT)))
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.sub = tab;
                this.show_history = false;
                this.refilter();
                cx.notify();
            }))
            .child(label)
    }

    pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_count = self.ticked_to_install().len();
        let install_label = if selected_count > 0 {
            format!("Install Selected ({selected_count})")
        } else {
            "Install Selected".to_string()
        };
        let appimage_tab = self.top == Source::AppImage;
        let refresh_tip = if self.top == Source::Apt {
            "Refresh APT package lists"
        } else {
            "Reload packages"
        };

        let list_btn = Button::new("view-list").label("☰").tooltip("List view");
        let list_btn = if self.grid { list_btn.ghost() } else { list_btn.primary() };
        let grid_btn = Button::new("view-grid").label("▦").tooltip("Grid view");
        let grid_btn = if self.grid { grid_btn.primary() } else { grid_btn.ghost() };

        div()
            .h(px(50.))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(BG))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(div().w(px(320.)).child(Input::new(&self.search)))
            .child(
                Button::new("filter")
                    .ghost()
                    .label(format!("{} ▾", self.filter.label()))
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.filter = this.filter.next();
                        this.refilter();
                        cx.notify();
                    })),
            )
            .child(
                list_btn.on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.grid = false;
                    cx.notify();
                })),
            )
            .child(
                grid_btn.on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.grid = true;
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(
                Button::new("refresh")
                    .ghost()
                    .label("⟳ Refresh")
                    .tooltip(refresh_tip)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.refresh(cx);
                    })),
            )
            .child(
                Button::new("install-selected")
                    .primary()
                    .label(install_label)
                    .disabled(self.busy || selected_count == 0 || appimage_tab)
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.install_selected(cx);
                    })),
            )
    }
}
