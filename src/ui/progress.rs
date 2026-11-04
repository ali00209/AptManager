//! The progress panel shown above the status bar while a job runs, and after it finishes.

use gpui_kit::component::{button::{Button, ButtonVariants}};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::ui::app::AppView;
use crate::ui::theme::*;

impl AppView {
    /// Title, progress bar, latest output line, and the result. Failures stay on screen
    /// with the last lines of output until dismissed.
    pub(crate) fn render_job(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let handle = self.job.as_ref()?;
        let (title, percent, tail, done, ok, summary) = {
            let p = handle.lock().ok()?;
            (
                p.title.clone(),
                p.percent,
                p.tail.iter().cloned().collect::<Vec<String>>(),
                p.done,
                p.ok,
                p.summary.clone(),
            )
        };

        let failed = done && !ok;
        let bar_color = if !done {
            ACCENT
        } else if ok {
            SUCCESS
        } else {
            ERROR
        };
        // Unknown progress shows a short bar that reads as "working".
        let fraction = match percent {
            Some(p) => (p / 100.0).clamp(0.02, 1.0),
            None => 0.3,
        };
        let headline = if !done {
            title.clone()
        } else if ok {
            format!("{title}: done")
        } else {
            format!("{title}: failed")
        };
        let latest = if done {
            summary.clone()
        } else {
            tail.last().cloned().unwrap_or_else(|| "Working…".to_string())
        };

        let mut panel = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_color(rgb(if failed { ERROR } else { TEXT }))
                            .child(headline),
                    )
                    .when(done, |d| {
                        d.child(
                            Button::new("job-dismiss")
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                                    this.job = None;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .w_full()
                    .h(px(6.))
                    .rounded(px(3.))
                    .bg(rgb(CHIP))
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .rounded(px(3.))
                            .bg(rgb(bar_color)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(if failed { ERROR } else { MUTED }))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(latest),
            );

        if failed {
            let log_lines: Vec<String> = tail.iter().rev().take(4).rev().cloned().collect();
            let mut log = div().flex().flex_col().gap_1();
            for line in log_lines {
                log = log.child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(line),
                );
            }
            panel = panel.child(log);
        }
        Some(panel.into_any_element())
    }
}
