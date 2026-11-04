//! The status bar at the bottom: status text, History, Updates and Upgrade All.

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    Disableable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::ui::app::AppView;
use crate::ui::theme::*;

impl AppView {
    pub(crate) fn render_bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let updates = self.total_updates();
        let history_label = if self.show_history { "Hide History" } else { "History" };

        div()
            .h(px(46.))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(div().size(px(22.)).rounded(px(6.)).bg(rgb(ACCENT)).flex_none())
            .child(
                div()
                    .text_color(rgb(MUTED))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(self.status.clone()),
            )
            .child(div().flex_1())
            .child(
                Button::new("history-toggle")
                    .ghost()
                    .label(history_label)
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.toggle_history(cx);
                    })),
            )
            .child(
                Button::new("updates-jump")
                    .ghost()
                    .label(format!("Updates ({updates})"))
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.show_updates(cx);
                    })),
            )
            .child(
                Button::new("upgrade-all")
                    .primary()
                    .label("Upgrade All")
                    .disabled(self.busy || updates == 0)
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.upgrade_all(cx);
                    })),
            )
    }
}
