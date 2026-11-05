//! The search toolbar, the registry dropdown, and the buttons on the right.

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    Disableable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::ui::app::{AppView, Page, StateFilter};
use crate::ui::theme::*;
use crate::ui::widgets::registry_chip;

impl AppView {
    /// One row: search, registry dropdown, Has updates and Refresh on the left;
    /// the view toggle, History, Upgrade All, Install Selected and Preferences on the right.
    pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_count = self.ticked_to_install().len();
        let install_label = if selected_count > 0 {
            format!("Install Selected ({selected_count})")
        } else {
            "Install Selected".to_string()
        };
        let updates = self.ticked_updates();
        let registry_label = format!(
            "Registries ({}/{}) ▾",
            self.registries.len(),
            self.active_registries().len()
        );

        let registries_btn = Button::new("registries")
            .ghost()
            .label(registry_label)
            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.toggle_registry_menu(cx);
            }));

        let state_label = {
            let installed = self.state_filter.contains(&StateFilter::Installed);
            let missing = self.state_filter.contains(&StateFilter::NotInstalled);
            match (installed, missing) {
                (false, false) => "Status: default ▾",
                (true, false) => "Status: installed ▾",
                (false, true) => "Status: not installed ▾",
                (true, true) => "Status: all ▾",
            }
        };
        let state_btn = Button::new("state-filter")
            .ghost()
            .label(state_label)
            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.toggle_state_menu(cx);
            }));

        let scope_btn = Button::new("flatpak-scope")
            .ghost()
            .label(if self.flatpak_for_all { "Flatpak: all users" } else { "Flatpak: me only" })
            .tooltip("Where Flatpak apps are installed: for you only (no password) or for all users (asks for the password)")
            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.toggle_flatpak_scope(cx);
            }));

        let updates_btn = Button::new("has-updates").label("⬆ Has updates");
        let updates_btn = if self.updates_only { updates_btn.primary() } else { updates_btn.ghost() };
        let updates_btn = updates_btn.on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
            this.toggle_updates_only(cx);
        }));

        let list_btn = Button::new("view-list").label("☰").tooltip("List view");
        let list_btn = if self.grid { list_btn.ghost() } else { list_btn.primary() };
        let grid_btn = Button::new("view-grid").label("▦").tooltip("Grid view");
        let grid_btn = if self.grid { grid_btn.primary() } else { grid_btn.ghost() };

        let history_btn = Button::new("history").label("History");
        let history_btn = if self.show_history { history_btn.primary() } else { history_btn.ghost() };

        let mut row = div()
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
            .child(registries_btn)
            .child(state_btn)
            .child(updates_btn)
            .child(
                Button::new("refresh")
                    .ghost()
                    .label("⟳ Refresh")
                    .tooltip("Refresh the package lists of the ticked registries")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.refresh(cx);
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
                history_btn.on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.toggle_history(cx);
                })),
            );

        if updates > 0 {
            row = row.child(
                Button::new("upgrade-all")
                    .ghost()
                    .label(format!("Upgrade All ({updates})"))
                    .disabled(self.any_loading())
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.upgrade_all(cx);
                    })),
            );
        }

        row.child(scope_btn)
        .child(
            Button::new("install-selected")
                .primary()
                .label(install_label)
                .disabled(selected_count == 0)
                .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.install_selected(cx);
                })),
        )
        .child(
            Button::new("open-prefs")
                .ghost()
                .label("⚙ Preferences")
                .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.page = Page::Preferences;
                    cx.notify();
                })),
        )
    }

    /// The install-state dropdown: Installed and Not installed, each ticked on or off.
    pub(crate) fn render_state_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let states = [
            (StateFilter::Installed, "Installed", "state-installed"),
            (StateFilter::NotInstalled, "Not installed", "state-missing"),
        ];
        let mut row = div().flex().flex_wrap().gap_2().items_center();
        for (state, label, id) in states {
            let ticked = self.state_filter.contains(&state);
            row = row.child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .hover(|s| s.bg(rgb(HOVER)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut AppView, _: &MouseDownEvent, _window, cx| {
                            this.toggle_state(state, cx);
                        }),
                    )
                    .child(
                        div()
                            .size(px(14.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(rgb(if ticked { ACCENT } else { MUTED }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(ticked, |d| d.bg(rgb(ACCENT)).child("✓")),
                    )
                    .child(label),
            );
        }

        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                Button::new("state-all")
                    .ghost()
                    .label("All")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.set_all_states(true, cx);
                    })),
            )
            .child(
                Button::new("state-none")
                    .ghost()
                    .label("None")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.set_all_states(false, cx);
                    })),
            )
            .child(row)
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("None ticked: installed packages, or every match when you search."),
            )
    }

    /// The registry dropdown: All, None, and one tick box per registry that is on.
    pub(crate) fn render_registry_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut items = div().flex().flex_wrap().gap_2().items_center();
        for (i, src) in self.active_registries().into_iter().enumerate() {
            let ticked = self.registries.contains(&src);
            items = items.child(
                div()
                    .id(("registry-item", i))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .hover(|s| s.bg(rgb(HOVER)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut AppView, _: &MouseDownEvent, _window, cx| {
                            this.toggle_registry(src, cx);
                        }),
                    )
                    .child(
                        div()
                            .size(px(14.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(rgb(if ticked { ACCENT } else { MUTED }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(ticked, |d| d.bg(rgb(ACCENT)).child("✓")),
                    )
                    .child(registry_chip(src)),
            );
        }

        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                Button::new("registry-all")
                    .ghost()
                    .label("All")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.set_all_registries(true, cx);
                    })),
            )
            .child(
                Button::new("registry-none")
                    .ghost()
                    .label("None")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.set_all_registries(false, cx);
                    })),
            )
            .child(items)
    }
}
