//! The status bar at the bottom: the latest status and the update count. Each registry's
//! progress is in its own row above (see `loads.rs`).

use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::ui::app::AppView;
use crate::ui::theme::*;

impl AppView {
    pub(crate) fn render_bottom_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let text = format!("{} · {} update(s)", self.status, self.total_updates());
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
                    .child(text),
            )
    }
}
