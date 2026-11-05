//! Transactions: a list of steps that either all succeed, or are all undone.
//!
//! Steps are grouped into lanes. Steps in one lane run in order. Different lanes run at
//! the same time. The system lane (APT, system Flatpak, Snap) is one lane because those
//! tools share the package database and the root helper. Each other manager has its own
//! lane, so, for example, npm and pip can run together.
//!
//! When a step fails, or the user cancels, no new step starts. Every completed step
//! is undone in reverse order. A step that has no undo is reported, not silently skipped.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;

use crate::job::{self, Handle};
use crate::model::{ActionResult, Source};
use crate::privileged;

/// Where a step runs. Steps in the same lane run one after another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    /// APT, system-wide Flatpak and Snap. They run through the root helper, one at a time.
    Privileged,
    /// Everything else, one lane per manager.
    Manager(Source),
}

impl Lane {
    fn label(self) -> &'static str {
        match self {
            Lane::Privileged => "system",
            Lane::Manager(src) => src.label(),
        }
    }
}

/// One command or file operation.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// Run a program. If it fails and `fallback` is set, that is tried next.
    Exec {
        privileged: bool,
        program: String,
        args: Vec<String>,
        fallback: Option<Vec<String>>,
    },
    /// Copy a file into place (made executable). Fails if the target exists.
    AddFile { from: PathBuf, to: PathBuf },
    /// Delete a file that was added.
    RemoveFile { path: PathBuf },
    /// Move a file out of the way. It is deleted for real only when the transaction commits.
    Trash { path: PathBuf, trash: PathBuf },
    /// Move a file back to where it was.
    Rename { from: PathBuf, to: PathBuf },
}

/// One action in a transaction, with the action that undoes it.
#[derive(Clone, Debug)]
pub struct Step {
    pub lane: Lane,
    pub title: String,
    pub forward: Cmd,
    pub undo: Option<Cmd>,
}

impl Step {
    pub fn new(lane: Lane, title: impl Into<String>, forward: Cmd, undo: Option<Cmd>) -> Step {
        Step {
            lane,
            title: title.into(),
            forward,
            undo,
        }
    }
}

/// Run every step. On failure or cancel, undo what finished. Returns the overall result.
pub fn run(handle: &Handle, steps: &[Step]) -> ActionResult {
    if steps.is_empty() {
        return ActionResult::ok("Nothing to do.");
    }
    let total = steps.len();
    let completed: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let abort = AtomicBool::new(false);

    // Group step indices by lane, keeping each lane's order.
    let mut lanes: Vec<(Lane, Vec<usize>)> = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        match lanes.iter_mut().find(|(lane, _)| *lane == step.lane) {
            Some((_, indices)) => indices.push(i),
            None => lanes.push((step.lane, vec![i])),
        }
    }

    thread::scope(|scope| {
        for (lane, indices) in &lanes {
            let completed = &completed;
            let failure = &failure;
            let abort = &abort;
            scope.spawn(move || {
                job::bind(handle);
                job::set_lane(lane.label());
                for &i in indices {
                    if abort.load(Ordering::SeqCst) || job::cancel_requested(handle) {
                        break;
                    }
                    let step = &steps[i];
                    job::log(&format!("▶ {}", step.title));
                    let result = execute(&step.forward);
                    if result.success {
                        if let Ok(mut done) = completed.lock() {
                            done.push(i);
                            job::set_percent(done.len() as f32 / total as f32 * 100.0);
                        }
                    } else {
                        // A step that was stopped by Cancel is not a failure: the job is cancelled.
                        if !job::cancel_requested(handle) {
                            if let Ok(mut f) = failure.lock() {
                                *f = Some(format!("{}: {}", step.title, result.summary));
                            }
                        }
                        abort.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            });
        }
    });

    let done = completed.into_inner().unwrap_or_default();
    let failed = failure.into_inner().unwrap_or_default();
    let cancelled = job::cancel_requested(handle);

    if failed.is_none() && !cancelled {
        // Commit: the trashed files are no longer needed.
        for &i in &done {
            if let Cmd::Trash { trash, .. } = &steps[i].forward {
                let _ = fs::remove_file(trash);
            }
        }
        return ActionResult::ok(format!("{total} step(s) completed."));
    }

    // Roll back, newest first.
    let cancel_only = failed.is_none();
    let reason = failed.unwrap_or_else(|| "Cancelled".to_string());
    job::log(&format!("Rolling back {} step(s)…", done.len()));
    let mut problems = Vec::new();
    let mut not_reversible = Vec::new();
    let mut rolled_back = 0;
    for &i in done.iter().rev() {
        let step = &steps[i];
        match &step.undo {
            Some(undo) => {
                let r = execute(undo);
                if r.success {
                    rolled_back += 1;
                    job::log(&format!("↩ undone: {}", step.title));
                } else {
                    problems.push(format!("{}: {}", step.title, r.summary));
                }
            }
            None => not_reversible.push(step.title.clone()),
        }
    }

    if !problems.is_empty() {
        return ActionResult::err(format!(
            "{reason}. Rollback incomplete: {}",
            problems.join("; ")
        ));
    }
    let mut summary = if cancel_only {
        format!("Cancelled. Rolled back {rolled_back} step(s); nothing was changed.")
    } else {
        format!("{reason}. Rolled back {rolled_back} step(s).")
    };
    if !not_reversible.is_empty() {
        summary.push_str(&format!(
            " Could not undo automatically: {}.",
            not_reversible.join(", ")
        ));
        return ActionResult::err(summary);
    }
    if cancel_only {
        ActionResult::ok(summary)
    } else {
        ActionResult::err(summary)
    }
}

/// Run one command or file operation.
fn execute(cmd: &Cmd) -> ActionResult {
    match cmd {
        Cmd::Exec {
            privileged: is_privileged,
            program,
            args,
            fallback,
        } => {
            let first = run_exec(*is_privileged, program, args);
            match fallback {
                Some(alt) if !first.success => run_exec(*is_privileged, program, alt),
                _ => first,
            }
        }
        Cmd::AddFile { from, to } => add_file(from, to),
        Cmd::RemoveFile { path } => match fs::remove_file(path) {
            Ok(()) => ActionResult::ok(format!("Removed {}", path.display())),
            Err(e) => ActionResult::err(format!("Could not remove {}: {e}", path.display())),
        },
        Cmd::Trash { path, trash } => move_file(path, trash),
        Cmd::Rename { from, to } => move_file(from, to),
    }
}

fn run_exec(privileged_cmd: bool, program: &str, args: &[String]) -> ActionResult {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    if privileged_cmd {
        privileged::run(program, &refs)
    } else {
        job::run_streamed(program, &refs)
    }
}

fn add_file(from: &PathBuf, to: &PathBuf) -> ActionResult {
    if to.exists() {
        return ActionResult::err(format!("{} already exists", to.display()));
    }
    if let Some(dir) = to.parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            return ActionResult::err(format!("Cannot create {}: {e}", dir.display()));
        }
    }
    job::log(&format!("Copying {} to {}", from.display(), to.display()));
    if let Err(e) = fs::copy(from, to) {
        return ActionResult::err(format!("Could not copy {}: {e}", from.display()));
    }
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = fs::set_permissions(to, fs::Permissions::from_mode(0o755)) {
        return ActionResult::err(format!("Could not make {} executable: {e}", to.display()));
    }
    ActionResult::ok(format!("Added {}", to.display()))
}

fn move_file(from: &PathBuf, to: &PathBuf) -> ActionResult {
    if let Some(dir) = to.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match fs::rename(from, to) {
        Ok(()) => ActionResult::ok(format!("Moved {}", from.display())),
        Err(e) => ActionResult::err(format!("Could not move {}: {e}", from.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Cmd {
        Cmd::Exec {
            privileged: false,
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            fallback: None,
        }
    }

    #[test]
    fn all_steps_succeed_and_nothing_is_undone() {
        let handle = job::start("ok");
        let steps = vec![
            Step::new(Lane::Manager(Source::Npm), "a", sh("true"), Some(sh("false"))),
            Step::new(Lane::Manager(Source::Pip), "b", sh("true"), Some(sh("false"))),
        ];
        let r = run(&handle, &steps);
        assert!(r.success, "{}", r.summary);
    }

    #[test]
    fn failure_rolls_back_finished_steps_in_their_lane() {
        let handle = job::start("fail");
        let steps = vec![
            Step::new(Lane::Manager(Source::Npm), "first", sh("true"), Some(sh("true"))),
            Step::new(Lane::Manager(Source::Npm), "second", sh("false"), None),
            // Runs in another lane; it may or may not start before the abort, but must never be left half-done.
            Step::new(Lane::Manager(Source::Pip), "other", sh("true"), Some(sh("true"))),
        ];
        let r = run(&handle, &steps);
        assert!(!r.success);
        assert!(r.summary.contains("second"), "{}", r.summary);
    }

    #[test]
    fn failed_undo_is_reported() {
        let handle = job::start("undo-fail");
        let steps = vec![
            Step::new(Lane::Manager(Source::Npm), "first", sh("true"), Some(sh("false"))),
            Step::new(Lane::Manager(Source::Npm), "second", sh("false"), None),
        ];
        let r = run(&handle, &steps);
        assert!(!r.success);
        assert!(r.summary.contains("Rollback incomplete"), "{}", r.summary);
    }

    #[test]
    fn add_file_is_undone_by_removing_it() {
        let dir = std::env::temp_dir().join(format!("apt-manager-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let from = dir.join("src.AppImage");
        let to = dir.join("dest.AppImage");
        fs::write(&from, b"x").unwrap();
        let handle = job::start("add");
        let steps = vec![
            Step::new(
                Lane::Manager(Source::AppImage),
                "add",
                Cmd::AddFile { from: from.clone(), to: to.clone() },
                Some(Cmd::RemoveFile { path: to.clone() }),
            ),
            Step::new(Lane::Manager(Source::Npm), "fail", sh("false"), None),
        ];
        let r = run(&handle, &steps);
        assert!(!r.success);
        assert!(!to.exists(), "copied file should be removed on rollback");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_jobs_run_side_by_side_and_cancelling_one_leaves_the_other() {
        use std::time::{Duration, Instant};
        let slow = job::start("slow");
        let quick = job::start("quick");
        let (slow_worker, quick_worker) = (slow.clone(), quick.clone());
        let slow_thread = std::thread::spawn(move || {
            job::bind(&slow_worker);
            run(
                &slow_worker,
                &[Step::new(Lane::Manager(Source::Npm), "slow", sh("exec sleep 20"), None)],
            )
        });
        let quick_thread = std::thread::spawn(move || {
            job::bind(&quick_worker);
            run(
                &quick_worker,
                &[Step::new(Lane::Manager(Source::Pip), "quick", sh("exec sleep 1"), None)],
            )
        });
        std::thread::sleep(Duration::from_millis(400));
        job::request_cancel(&slow);
        let started = Instant::now();
        let slow_result = slow_thread.join().unwrap();
        let quick_result = quick_thread.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(10), "cancel did not stop the slow job");
        assert!(slow_result.summary.starts_with("Cancelled"), "{}", slow_result.summary);
        assert!(quick_result.success, "{}", quick_result.summary);
    }
}
