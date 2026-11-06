//! The job panel above the status bar: one row per background job, a summary line with the
//! queue, and at most `MAX_SHOWN_JOBS` rows so a large batch cannot take over the window.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::ui::app::AppView;
use crate::ui::theme::*;

/// Rows shown before the rest are summarised as "and N more".
const MAX_SHOWN_JOBS: usize = 4;

impl AppView {
    /// The panel of job rows above the status bar. Nothing is shown when no job or queued update exists.
    pub(crate) fn render_jobs(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.jobs.is_empty() && self.queue.is_empty() {
            return None;
        }
        let running = self.running_jobs();
        let queued = self.queue.len();
        let mut summary = format!("{running} running");
        if queued > 0 {
            summary.push_str(&format!(" · {queued} queued"));
        }

        let mut header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(div().text_xs().text_color(rgb(MUTED)).child(summary));
        if queued > 0 {
            header = header.child(
                Button::new("queue-clear")
                    .ghost()
                    .label("Cancel queued")
                    .tooltip("Drop the updates that have not started yet")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.clear_queue(cx);
                    })),
            );
        }

        let rows: Vec<AnyElement> = self
            .jobs
            .iter()
            .take(MAX_SHOWN_JOBS)
            .map(|job| job_row(job.id, &job.handle, cx))
            .collect();
        let hidden = self.jobs.len().saturating_sub(MAX_SHOWN_JOBS);

        let mut panel = div()
            .w_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(header)
            .children(rows);
        if hidden > 0 {
            panel = panel.child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(format!("and {hidden} more job(s), each with its own progress in the list above")),
            );
        }
        Some(panel.into_any_element())
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
    // Amber for cancelled: it is neither a success nor a failure.
    let color = if !done {
        ACCENT
    } else if cancelled {
        AMBER
    } else if ok {
        SUCCESS
    } else {
        ERROR
    };
    // Known progress fills the bar. Unknown progress leaves it empty and says "Working…"
    // rather than showing a fixed fraction that looks like real progress.
    let fill = percent.map(|p| (p / 100.0).clamp(0.02, 1.0));
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
                    .label("Copy log")
                    .tooltip("Copy the log to the clipboard")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.copy_job_log(id, cx);
                    })),
            )
            .child(
                Button::new(format!("job-dismiss-{id}"))
                    .ghost()
                    .label("Dismiss")
                    .tooltip("Remove this row from the panel")
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.dismiss_job(id, cx);
                    })),
            )
            .into_any_element()
    };

    let mut track = div()
        .w_full()
        .h(px(6.))
        .rounded(px(3.))
        .bg(rgb(CHIP));
    if let Some(f) = fill {
        track = track.child(div().h_full().w(relative(f)).rounded(px(3.)).bg(rgb(color)));
    }

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
                        .text_color(rgb(if failed { ERROR_TEXT } else { TEXT }))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(headline),
                )
                .child(action),
        )
        .child(track)
        .child(
            div()
                .text_xs()
                .text_color(rgb(if failed { ERROR_TEXT } else { MUTED }))
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
