//! One row per background job: its title, progress bar, latest line, and its own Cancel
//! button while it runs. A failed job keeps its last lines until its ✕ is pressed.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::ui::app::AppView;
use crate::ui::theme::*;

impl AppView {
    /// The panel of job rows above the status bar. Nothing is shown when no job exists.
    pub(crate) fn render_jobs(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.jobs.is_empty() {
            return None;
        }
        let rows: Vec<AnyElement> = self.jobs.iter().map(|job| job_row(job.id, &job.handle, cx)).collect();
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

fn job_row(id: u64, handle: &crate::job::Handle, cx: &mut Context<AppView>) -> AnyElement {
    let Ok(p) = handle.lock() else {
        return div().into_any_element();
    };
    let title = p.title.clone();
    let percent = p.percent;
    let tail: Vec<String> = p.tail.iter().cloned().collect();
    let done = p.done;
    let ok = p.ok;
    let summary = p.summary.clone();
    let cancelling = p.cancel_requested;
    drop(p);

    let cancelled = done && ok && summary.starts_with("Cancelled");
    let failed = done && !ok;
    let color = if !done {
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
        if cancelling {
            format!("{title}: cancelling…")
        } else {
            title.clone()
        }
    } else if cancelled {
        format!("{title}: cancelled")
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

    let action = if !done {
        if cancelling {
            div().flex_none().text_xs().text_color(rgb(MUTED)).child("Cancelling…").into_any_element()
        } else {
            Button::new(format!("job-cancel-{id}"))
                .ghost()
                .label("Cancel")
                .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.cancel_job(id, cx);
                }))
                .into_any_element()
        }
    } else {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(
                Button::new(format!("job-copy-{id}"))
                    .ghost()
                    .label("⧉")
                    .tooltip("Copy the log to the clipboard")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.copy_job_log(id, cx);
                    })),
            )
            .child(
                Button::new(format!("job-dismiss-{id}"))
                    .ghost()
                    .label("✕")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.dismiss_job(id, cx);
                    })),
            )
            .into_any_element()
    };

    let mut row = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .text_color(rgb(if failed { ERROR } else { TEXT }))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(headline),
                )
                .child(action),
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
                        .bg(rgb(color)),
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
        let mut log = div().flex().flex_col().gap_1();
        for line in tail.iter().rev().take(4).rev() {
            log = log.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(line.clone()),
            );
        }
        row = row.child(log);
    }
    row.into_any_element()
}
