//! Progress for background jobs. Each job has its own `Progress`, which the window reads
//! about every 150 ms. Several jobs can run at once. A thread reports to the job it is
//! bound to (see `bind`), and cancelling a job stops only that job's commands.
//! Lines from parallel lanes are prefixed with the lane's name.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::model::ActionResult;
use crate::track::{self, Sink};

const TAIL_LINES: usize = 8;

#[derive(Default)]
pub struct Progress {
    pub title: String,
    /// Known percentage, or `None` when the job does not report one.
    pub percent: Option<f32>,
    /// The most recent lines of output.
    pub tail: VecDeque<String>,
    pub done: bool,
    pub ok: bool,
    pub summary: String,
    /// Set by the job's Cancel button. Steps that have not started are skipped.
    pub cancel_requested: bool,
    /// Process ids of this job's commands that are running now.
    pub sink: Sink,
}

pub type Handle = Arc<Mutex<Progress>>;

thread_local! {
    static CURRENT: RefCell<Option<Handle>> = const { RefCell::new(None) };
    static LANE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Start a new job. The window keeps the returned handle to draw it.
pub fn start(title: &str) -> Handle {
    Arc::new(Mutex::new(Progress {
        title: title.to_string(),
        sink: track::new_sink(),
        ..Default::default()
    }))
}

/// Make the current thread report to `handle`, and record its commands in the job's sink.
pub fn bind(handle: &Handle) {
    CURRENT.with(|c| *c.borrow_mut() = Some(handle.clone()));
    if let Ok(p) = handle.lock() {
        track::bind(p.sink.clone());
    }
}

fn current() -> Option<Handle> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Name this thread's lane. Output logged from the thread is prefixed with it.
pub fn set_lane(label: &str) {
    LANE.with(|l| *l.borrow_mut() = label.to_string());
}

fn lane() -> String {
    LANE.with(|l| l.borrow().clone())
}

/// Record one line of output for this thread's job, tagged with this thread's lane.
pub fn log(line: &str) {
    if let Some(handle) = current() {
        log_to(&handle, &lane(), line);
    }
}

/// Record one line in `handle`, tagged with `lane`.
pub fn log_to(handle: &Handle, lane: &str, line: &str) {
    let Ok(mut p) = handle.lock() else {
        return;
    };
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    // apt's machine-readable status lines (enabled with APT::Status-Fd) are shown as their message.
    if let Some(rest) = line
        .strip_prefix("pmstatus:")
        .or_else(|| line.strip_prefix("dlstatus:"))
    {
        let parts: Vec<&str> = rest.split(':').collect();
        if let Some(i) = parts.iter().position(|s| s.trim().parse::<f32>().is_ok()) {
            let message = parts[i + 1..].join(":");
            if !message.trim().is_empty() {
                push(&mut p.tail, format!("{}{}", prefix(lane), message.trim()));
            }
        }
        return;
    }
    let text = if lane.is_empty() {
        line.to_string()
    } else {
        format!("[{lane}] {line}")
    };
    push(&mut p.tail, text);
}

fn prefix(lane: &str) -> String {
    if lane.is_empty() {
        String::new()
    } else {
        format!("[{lane}] ")
    }
}

/// Set this thread's job's progress bar from completed steps.
pub fn set_percent(percent: f32) {
    if let Some(handle) = current() {
        if let Ok(mut p) = handle.lock() {
            p.percent = Some(percent.clamp(0.0, 100.0));
        }
    }
}

fn push(tail: &mut VecDeque<String>, line: String) {
    tail.push_back(line);
    while tail.len() > TAIL_LINES {
        tail.pop_front();
    }
}

/// True once the user has pressed Cancel for this job.
pub fn cancel_requested(handle: &Handle) -> bool {
    handle.lock().map(|p| p.cancel_requested).unwrap_or(false)
}

/// True when the job this thread is bound to has been cancelled.
pub fn current_cancelled() -> bool {
    current().map(|h| cancel_requested(&h)).unwrap_or(false)
}

/// Ask a job to stop. Its commands in flight are terminated now. A system command that is
/// already running finishes first, then the job rolls back.
pub fn request_cancel(handle: &Handle) {
    let sink = match handle.lock() {
        Ok(mut p) => {
            p.cancel_requested = true;
            push(&mut p.tail, "Cancel requested. Stopping after the current step…".into());
            p.sink.clone()
        }
        Err(_) => return,
    };
    track::kill_all(&sink);
}

/// Mark the job as finished with its result.
pub fn finish(handle: &Handle, result: &ActionResult) {
    if let Ok(mut p) = handle.lock() {
        p.done = true;
        p.ok = result.success;
        p.summary = result.summary.clone();
        if result.success {
            p.percent = Some(100.0);
        }
    }
}

/// Run an ordinary command (no privileges) and stream its output into this thread's job.
pub fn run_streamed(program: &str, args: &[&str]) -> ActionResult {
    let spawned = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return ActionResult::err(format!("Could not run {program}: {e}")),
    };
    let pid = child.id();
    track::register(pid);

    let handle = current();
    let lane = lane();
    let out = child.stdout.take().map(|s| pump(s, handle.clone(), lane.clone()));
    let err = child.stderr.take().map(|s| pump(s, handle, lane));
    let status = child.wait();
    track::unregister(pid);

    let out_lines = out.and_then(|t| t.join().ok()).unwrap_or_default();
    let err_lines = err.and_then(|t| t.join().ok()).unwrap_or_default();

    match status {
        Ok(s) if s.success() => {
            ActionResult::ok(last_nonempty(&out_lines).unwrap_or_else(|| "Done.".into()))
        }
        Ok(s) => ActionResult::err(
            last_nonempty(&err_lines)
                .or_else(|| last_nonempty(&out_lines))
                .unwrap_or_else(|| format!("{program} exited with {}", s.code().unwrap_or(-1))),
        ),
        Err(e) => ActionResult::err(format!("{program} failed: {e}")),
    }
}

/// Read a stream line by line on a thread, logging each line to the job as it arrives.
fn pump<R: Read + Send + 'static>(
    stream: R,
    handle: Option<Handle>,
    lane: String,
) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if let Some(h) = &handle {
                log_to(h, &lane, &line);
            }
            lines.push(line);
        }
        lines
    })
}

fn last_nonempty(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .map(str::to_string)
}
