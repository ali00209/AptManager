//! Progress for the background job that is running now.
//!
//! The job's output is streamed into a shared `Progress`. The window reads it about
//! every 150 ms, so the progress bar and the last lines of output stay current.
//! Only one job runs at a time, so a single global "current job" is enough.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::model::ActionResult;

const TAIL_LINES: usize = 8;

#[derive(Default)]
pub struct Progress {
    pub title: String,
    /// Known percentage, or `None` when the tool does not report one.
    pub percent: Option<f32>,
    /// The most recent lines of output.
    pub tail: VecDeque<String>,
    pub done: bool,
    pub ok: bool,
    pub summary: String,
}

pub type Handle = Arc<Mutex<Progress>>;

static CURRENT: Mutex<Option<Handle>> = Mutex::new(None);

/// Start tracking a new job. The window keeps the returned handle to draw it.
pub fn start(title: &str) -> Handle {
    let handle = Arc::new(Mutex::new(Progress {
        title: title.to_string(),
        ..Default::default()
    }));
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some(handle.clone());
    }
    handle
}

/// Record one line of output for the current job.
pub fn log(line: &str) {
    let Some(handle) = current() else {
        return;
    };
    let Ok(mut p) = handle.lock() else {
        return;
    };
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    // apt's machine-readable status lines (enabled with APT::Status-Fd):
    //   pmstatus:<package>:<percent>:<message>
    //   dlstatus:<url>:<percent>:<message>
    if let Some(rest) = line
        .strip_prefix("pmstatus:")
        .or_else(|| line.strip_prefix("dlstatus:"))
    {
        let parts: Vec<&str> = rest.split(':').collect();
        if let Some(i) = parts.iter().position(|s| s.trim().parse::<f32>().is_ok()) {
            let percent: f32 = parts[i].trim().parse().unwrap_or(0.0);
            p.percent = Some(percent.clamp(0.0, 100.0));
            let message = parts[i + 1..].join(":");
            if !message.trim().is_empty() {
                push(&mut p.tail, message.trim().to_string());
            }
        }
        return;
    }
    push(&mut p.tail, line.to_string());
}

fn push(tail: &mut VecDeque<String>, line: String) {
    tail.push_back(line);
    while tail.len() > TAIL_LINES {
        tail.pop_front();
    }
}

fn current() -> Option<Handle> {
    CURRENT.lock().ok()?.clone()
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

/// Run an ordinary command (no privileges) and stream its output into the current job.
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
    let out = child.stdout.take().map(pump);
    let err = child.stderr.take().map(pump);
    let status = child.wait();

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

/// Read a stream line by line on a thread, logging each line as it arrives.
fn pump<R: Read + Send + 'static>(stream: R) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            log(&line);
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
