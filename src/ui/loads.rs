//! One row per registry that is loading: its progress, elapsed time, and its own Cancel
//! button. Each row is independent, so one registry can be stopped while the others run.
//! A finished load stays until dismissed, except a success, which hides after a few seconds.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::Source;
use crate::ui::app::AppView;
use crate::ui::theme::*;
use crate::ui::widgets::registry_chip;

const SUCCESS_SHOWN_FOR: Duration = Duration::from_secs(4);

impl AppView {
    pub(crate) fn render_loads(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mut rows = Vec::new();
        for src in Source::ALL {
            let Some(load) = self.loads.get(&src) else {
                continue;
            };
            if let Some((ok, _, at)) = &load.finished {
                if *ok && at.elapsed() > SUCCESS_SHOWN_FOR {
                    continue;
                }
            }
            rows.push(load_row(src, load, cx));
        }
        if rows.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .bg(rgb(PANEL))
                .border_t_1()
                .border_color(rgb(BORDER))
                .children(rows)
                .into_any_element(),
        )
    }
}

fn load_row(src: Source, load: &crate::ui::app::LoadUi, cx: &mut Context<AppView>) -> AnyElement {
    let (text, fraction, color, action) = match &load.finished {
        None => {
            let elapsed = load.started.elapsed().as_secs();
            (
                format!("{} · {elapsed}s", load.text),
                0.3,
                ACCENT,
                Button::new(format!("load-cancel-{}", src.tag()))
                    .ghost()
                    .label("Cancel")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.cancel_load(src, cx);
                    }))
                    .into_any_element(),
            )
        }
        Some((ok, summary, _)) => {
            let (text, color) = if *ok {
                (summary.clone(), SUCCESS)
            } else {
                (summary.clone(), ERROR)
            };
            (
                text,
                1.0,
                color,
                Button::new(format!("load-dismiss-{}", src.tag()))
                    .ghost()
                    .label("✕")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.dismiss_load(src, cx);
                    }))
                    .into_any_element(),
            )
        }
    };
    div()
        .h(px(38.))
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(registry_chip(src))
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(MUTED))
                .child(text),
        )
        .child(
            div()
                .w(px(160.))
                .h(px(6.))
                .flex_none()
                .rounded(px(3.))
                .bg(rgb(CHIP))
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded(px(3.))
                        .bg(rgb(color)),
                ),
        )
        .child(action)
        .into_any_element()
}
