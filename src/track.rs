//! Child processes started by a background load, so that Cancel can stop them.
//!
//! A loader thread binds a `Sink` to itself. Every command it runs through
//! `output_tracked` records its process id in that sink while the command runs. Cancel
//! sends TERM to the ids in the sink.

use std::cell::RefCell;
use std::io;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

/// Process ids of the commands that a load is running now.
pub type Sink = Arc<Mutex<Vec<u32>>>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

pub fn new_sink() -> Sink {
    Arc::new(Mutex::new(Vec::new()))
}

/// Make the current thread record its commands in `sink`.
pub fn bind(sink: Sink) {
    SINK.with(|s| *s.borrow_mut() = Some(sink));
}

/// Record a command that this thread has started, so a Cancel can stop it.
pub fn register(pid: u32) {
    SINK.with(|s| {
        if let Some(sink) = s.borrow().as_ref() {
            if let Ok(mut pids) = sink.lock() {
                pids.push(pid);
            }
        }
    });
}

/// Forget a command that has ended.
pub fn unregister(pid: u32) {
    SINK.with(|s| {
        if let Some(sink) = s.borrow().as_ref() {
            if let Ok(mut pids) = sink.lock() {
                pids.retain(|&p| p != pid);
            }
        }
    });
}

/// Stop every command recorded in `sink`.
pub fn kill_all(sink: &Sink) {
    let pids: Vec<u32> = sink.lock().map(|p| p.clone()).unwrap_or_default();
    for pid in pids {
        let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
    }
}

/// Like `Command::output`, but records the process while it runs.
pub trait OutputTracked {
    fn output_tracked(&mut self) -> io::Result<Output>;
}

impl OutputTracked for Command {
    fn output_tracked(&mut self) -> io::Result<Output> {
        let child = self
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        register(pid);
        let out = child.wait_with_output();
        unregister(pid);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn cancel_stops_a_running_tracked_command() {
        let sink = new_sink();
        let worker_sink = sink.clone();
        let started = Instant::now();
        let worker = std::thread::spawn(move || {
            bind(worker_sink);
            Command::new("sleep").arg("20").output_tracked().is_ok()
        });
        // Wait until the command is recorded, then cancel it.
        for _ in 0..200 {
            if !sink.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!sink.lock().unwrap().is_empty(), "command was not tracked");
        kill_all(&sink);
        worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(10), "cancel did not stop the command");
        assert!(sink.lock().unwrap().is_empty(), "finished command should be unregistered");
    }
}
