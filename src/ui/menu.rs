//! The ⋯ button on each package row and card, and the menu it opens. The menu lists the
//! actions that apply to the package right now (see `actions::available`).
//!
//! The menu closes on Escape, on a click outside it, or after an item is chosen. Only one
//! menu is open at a time.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::actions;
use crate::model::Package;
use crate::ui::app::AppView;
use crate::ui::theme::*;

impl AppView {
    /// Open the menu for `key`, or close it if it is already open.
    pub(crate) fn toggle_menu(&mut self, key: String, cx: &mut Context<Self>) {
        self.menu_for = if self.menu_for.as_deref() == Some(key.as_str()) { None } else { Some(key) };
        cx.notify();
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu_for.is_some() {
            self.menu_for = None;
            cx.notify();
        }
    }

    /// The ⋯ button and, when it is open, its menu. Place this at the right of a row or card.
    pub(crate) fn more_menu(&self, pkg: &Package, cx: &mut Context<Self>) -> AnyElement {
        let key = pkg.key();
        let click_key = key.clone();
        let button = Button::new(format!("more:{key}"))
            .ghost()
            .label("⋯")
            .tooltip("Actions")
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, window, cx| {
                // The row underneath selects the package on click; the ⋯ button must not.
                cx.stop_propagation();
                this.toggle_menu(click_key.clone(), cx);
                if this.menu_for.is_some() {
                    window.focus(&this.menu_focus, cx);
                }
            }));

        let mut wrapper = div().relative().flex_none().child(button);
        if self.menu_for.as_deref() == Some(key.as_str()) {
            wrapper = wrapper.child(self.menu_panel(pkg, cx));
        }
        wrapper.into_any_element()
    }

    /// The menu itself, anchored under its ⋯ button.
    fn menu_panel(&self, pkg: &Package, cx: &mut Context<Self>) -> AnyElement {
        let key = pkg.key();
        let mut panel = div()
            .absolute()
            .top_full()
            .right_0()
            .mt_1()
            .w(px(180.))
            .p_1()
            .flex()
            .flex_col()
            .gap_px()
            .rounded(px(6.))
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .track_focus(&self.menu_focus)
            .on_key_down(cx.listener(|this: &mut AppView, ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "escape" {
                    this.close_menu(cx);
                }
            }))
            .on_mouse_down_out(cx.listener(|this: &mut AppView, _: &MouseDownEvent, _window, cx| {
                this.close_menu(cx);
            }));

        for (i, action) in actions::available(pkg).into_iter().enumerate() {
            let item_key = key.clone();
            let colour = if action.is_danger() { ERROR_TEXT } else { TEXT };
            panel = panel.child(
                div()
                    .id(("menu-item", i))
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .text_color(rgb(colour))
                    .hover(|s| s.bg(rgb(HOVER)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this: &mut AppView, _: &MouseDownEvent, _window, cx| {
                            this.close_menu(cx);
                            this.run_action(item_key.clone(), action, cx);
                        }),
                    )
                    .child(action.label()),
            );
        }
        panel.into_any_element()
    }
}
