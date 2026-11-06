//! jobs for the main window (part of `AppView`).

use super::*;
use crate::model::ActionResult;
use crate::transaction::{self, Step};
use crate::job;
use std::collections::VecDeque;
use crate::model::Source;

/// Background jobs that may run at once. Further updates wait in `AppView::queue`.
pub(crate) const MAX_PARALLEL_JOBS: usize = 3;

/// An update waiting for a free job slot.
pub(crate) struct QueuedJob {
    pub(crate) title: String,
    /// Ticks cleared when the job succeeds.
    pub(crate) clear: Vec<String>,
    /// Packages the job changes, re-read after it succeeds.
    pub(crate) touched: Vec<(Source, String)>,
    pub(crate) step: Step,
}

impl AppView {
    /// Run a job in the background, alongside any others. `keys` are the packages it changes;
    /// a package can be in only one job at a time. `job_fn` gets the job's handle, so a
    /// transaction can report progress and honour its own Cancel. When the job succeeds,
    /// `after` says which packages to read again.
    pub(crate) fn run_job<F>(
        &mut self,
        title: String,
        clear: Vec<String>,
        keys: Vec<String>,
        job_fn: F,
        after: After,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(&job::Handle) -> ActionResult + Send + 'static,
    {
        if let Some(key) = keys.iter().find(|k| self.locked.contains(*k)) {
            let name = self
                .find_package(key)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| key.clone());
            self.status = format!("{name} is already being changed by another job.");
            cx.notify();
            return;
        }
        self.locked.extend(keys.iter().cloned());
        let handle = job::start(&title);
        self.next_job += 1;
        self.jobs.push(JobUi {
            id: self.next_job,
            handle: handle.clone(),
        });
        self.status = format!("{title}…");
        cx.notify();

        let worker = handle.clone();
        let result_title = title.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    job::bind(&worker);
                    let r = job_fn(&worker);
                    job::finish(&worker, &r);
                    r
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for key in &keys {
                    this.locked.remove(key);
                }
                if result.success {
                    for key in &clear {
                        this.checked.remove(key);
                    }
                    this.status = format!("{result_title}: {}", result.summary);
                    match after {
                        After::Recheck(list) => this.recheck(list, cx),
                        After::Refresh(sources) => this.refresh_registries(sources, cx),
                    }
                } else {
                    this.status = format!("{result_title} failed: {}", result.summary);
                }
                this.start_queued(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Run a list of steps as one transaction. See `transaction::run`.
    pub(crate) fn run_steps(
        &mut self,
        title: String,
        clear: Vec<String>,
        steps: Vec<Step>,
        after: After,
        cx: &mut Context<Self>,
    ) {
        if steps.is_empty() {
            self.status = "Nothing to do.".into();
            cx.notify();
            return;
        }
        let keys: Vec<String> = match &after {
            After::Recheck(list) => list.iter().map(|(src, name)| format!("{}:{name}", src.tag())).collect(),
            After::Refresh(_) => Vec::new(),
        };
        self.run_job(title, clear, keys, move |handle| transaction::run(handle, &steps), after, cx);
    }

    /// Ask one job to stop. Its finished steps are undone; nothing new starts.
    pub(crate) fn cancel_job(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(job) = self.jobs.iter().find(|j| j.id == id) {
            job::request_cancel(&job.handle);
            self.status = "Cancelling…".into();
        }
        cx.notify();
    }

    /// Copy a finished job's headline, result and full log to the clipboard.
    pub(crate) fn copy_job_log(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(job) = self.jobs.iter().find(|j| j.id == id) else {
            return;
        };
        let Ok(p) = job.handle.lock() else {
            return;
        };
        let text = job_log_text(&p.title, p.ok, &p.summary, &p.log);
        drop(p);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.status = "Job log copied to the clipboard.".into();
        cx.notify();
    }

    /// Remove a finished job's row from the panel.
    pub(crate) fn dismiss_job(&mut self, id: u64, cx: &mut Context<Self>) {
        self.jobs.retain(|j| j.id != id || !job_done(&j.handle));
        cx.notify();
    }

    pub(crate) fn any_job_running(&self) -> bool {
        self.jobs.iter().any(|j| !job_done(&j.handle))
    }

    /// Jobs that have not finished yet.
    pub(crate) fn running_jobs(&self) -> usize {
        self.jobs.iter().filter(|j| !job_done(&j.handle)).count()
    }

    /// Start queued updates while there is a free job slot.
    pub(crate) fn start_queued(&mut self, cx: &mut Context<Self>) {
        while self.running_jobs() < MAX_PARALLEL_JOBS {
            let Some(job) = self.queue.pop_front() else {
                break;
            };
            self.run_steps(job.title, job.clear, vec![job.step], After::Recheck(job.touched), cx);
        }
        cx.notify();
    }

    /// Drop every queued update that has not started. Running jobs are not affected.
    pub(crate) fn clear_queue(&mut self, cx: &mut Context<Self>) {
        let n = self.queue.len();
        self.queue.clear();
        self.status = format!("Cancelled {n} queued update(s).");
        cx.notify();
    }
}

/// The text copied for a finished job: its headline and result, its summary, then its full log.
pub(crate) fn job_log_text(title: &str, ok: bool, summary: &str, log: &VecDeque<String>) -> String {
    let result = if !ok {
        "failed"
    } else if summary.starts_with("Cancelled") {
        "cancelled"
    } else {
        "done"
    };
    let mut text = format!("{title}: {result}\n");
    if !summary.is_empty() {
        text.push_str(summary);
        text.push('\n');
    }
    text.push('\n');
    for line in log {
        text.push_str(line);
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::job_log_text;
    use std::collections::VecDeque;

    fn log(lines: &[&str]) -> VecDeque<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_done_job_copies_headline_summary_and_every_line() {
        let text = job_log_text("Installing nano", true, "Done.", &log(&["one", "two"]));
        assert_eq!(text, "Installing nano: done\nDone.\n\none\ntwo\n");
    }

    #[test]
    fn a_failed_job_without_a_summary_still_copies_its_log() {
        let text = job_log_text("Removing x", false, "", &log(&["E: locked"]));
        assert_eq!(text, "Removing x: failed\n\nE: locked\n");
    }

    #[test]
    fn a_cancelled_job_is_labelled_cancelled_not_done() {
        let text = job_log_text("Upgrading y", true, "Cancelled. Rolled back 1 step(s).", &log(&[]));
        assert!(text.starts_with("Upgrading y: cancelled\n"));
    }
}
