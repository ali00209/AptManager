//! Commands that need root (APT, Flatpak system installs, Snap), run through one helper.
//!
//! The first time a privileged action is needed, this app starts the helper with
//! `pkexec`. That is the only password prompt for the session. The helper is this same
//! binary started with `--helper`. Later actions are written to its stdin, and its
//! output comes back on stdout, so no further prompts appear.
//!
//! The helper runs only the whitelisted subcommands below, checks every argument,
//! and never starts a shell. It exits when its stdin closes, which happens when the app quits.
//!
//! Protocol (one line per message, fields separated by tabs):
//!   app -> helper:  `<program>\t<arg>\t<arg>...`
//!   helper -> app:  `READY`, then `O\t<line>` / `E\t<line>` for output, then `X\t<exit code>`

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::model::ActionResult;
use crate::job;

enum Msg {
    Ready,
    Out(String),
    Err(String),
    Exit(i32),
    Closed,
}

struct Helper {
    _child: Child,
    stdin: ChildStdin,
    rx: Receiver<Msg>,
}

static HELPER: Mutex<Option<Helper>> = Mutex::new(None);

/// Run `program` with `args` as root. Output goes to the current job.
pub fn run(program: &str, args: &[&str]) -> ActionResult {
    // Commands take turns on the helper. While waiting, a cancelled job gives up at once.
    let mut guard = loop {
        match HELPER.try_lock() {
            Ok(guard) => break guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                if job::current_cancelled() {
                    return ActionResult::err("Cancelled before it started.");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return ActionResult::err("The privileged helper is unavailable.");
            }
        }
    };
    if guard.is_none() {
        match start_helper() {
            Ok(helper) => *guard = Some(helper),
            Err(e) => return ActionResult::err(e),
        }
    }

    let Some(helper) = guard.as_mut() else {
        return ActionResult::err("The privileged helper is unavailable.");
    };
    let line = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join("\t");
    if writeln!(helper.stdin, "{line}").and_then(|_| helper.stdin.flush()).is_err() {
        *guard = None;
        return ActionResult::err("Lost the privileged helper. Try again.");
    }

    let mut last_out = String::new();
    let mut last_err = String::new();
    let outcome = loop {
        match helper.rx.recv() {
            Ok(Msg::Out(l)) => {
                job::log(&l);
                if !l.trim().is_empty() {
                    last_out = l;
                }
            }
            Ok(Msg::Err(l)) => {
                job::log(&l);
                if !l.trim().is_empty() {
                    last_err = l;
                }
            }
            Ok(Msg::Exit(code)) => break Some(code),
            Ok(Msg::Closed) | Err(_) => break None,
            Ok(Msg::Ready) => {}
        }
    };

    match outcome {
        Some(0) => ActionResult::ok(if last_out.trim().is_empty() {
            "Done.".to_string()
        } else {
            last_out.trim().to_string()
        }),
        Some(code) => {
            let reason = if last_err.trim().is_empty() {
                last_out.trim().to_string()
            } else {
                last_err.trim().to_string()
            };
            ActionResult::err(format!("{program} exited with {code}: {reason}"))
        }
        None => {
            *guard = None;
            ActionResult::err("The privileged helper stopped unexpectedly.")
        }
    }
}

/// Start the helper through pkexec and wait until it reports READY.
fn start_helper() -> Result<Helper, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find the app binary: {e}"))?;
    let mut child = Command::new("pkexec")
        .arg(&exe)
        .arg("--helper")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Could not start pkexec: {e}"))?;

    let stdin = child.stdin.take().ok_or("Could not open the helper's input")?;
    let stdout = child.stdout.take().ok_or("Could not open the helper's output")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let msg = if line == "READY" {
                Msg::Ready
            } else if let Some(l) = line.strip_prefix("O\t") {
                Msg::Out(l.to_string())
            } else if let Some(l) = line.strip_prefix("E\t") {
                Msg::Err(l.to_string())
            } else if let Some(c) = line.strip_prefix("X\t") {
                Msg::Exit(c.trim().parse().unwrap_or(-1))
            } else {
                continue;
            };
            if tx.send(msg).is_err() {
                return;
            }
        }
        let _ = tx.send(Msg::Closed);
    });

    match rx.recv() {
        Ok(Msg::Ready) => Ok(Helper {
            _child: child,
            stdin,
            rx,
        }),
        _ => {
            let code = child.wait().ok().and_then(|s| s.code());
            if code == Some(126) {
                // pkexec returns 126 when the password prompt is dismissed.
                Err("Authentication cancelled.".into())
            } else {
                Err(format!(
                    "Could not start the privileged helper (pkexec exit {}).",
                    code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into())
                ))
            }
        }
    }
}

// ----- helper side ------------------------------------------------------------

/// Entry point when the binary is started with `--helper`. Returns the exit code.
pub fn serve() -> i32 {
    std::env::set_var(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    let out = Arc::new(Mutex::new(std::io::stdout()));
    emit(&out, "READY");

    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let mut parts = line.split('\t');
        let program = parts.next().unwrap_or("");
        if program == "exit" {
            break;
        }
        let args: Vec<&str> = parts.collect();
        let code = match validate(program, &args) {
            Ok(argv) => execute(program, &argv, &out),
            Err(e) => {
                emit(&out, &format!("E\t{e}"));
                2
            }
        };
        emit(&out, &format!("X\t{code}"));
    }
    0
}

/// Accept only the subcommands this app uses, and only plain arguments.
fn validate(program: &str, args: &[&str]) -> Result<Vec<String>, String> {
    let allowed_char = |c: char| c.is_ascii_alphanumeric() || "._+-:/=@".contains(c);
    for arg in args {
        if arg.is_empty() || !arg.chars().all(allowed_char) {
            return Err(format!("Refused argument: {arg:?}"));
        }
    }
    let sub = args.first().copied().unwrap_or("");
    let names = &args[1.min(args.len())..];

    match program {
        "apt-get" => {
            // "--only-upgrade" stops apt-get from installing new packages; "--reinstall" reinstalls
            // an installed package. These are the only flags allowed.
            let flags_ok = args
                .iter()
                .filter(|a| a.starts_with('-'))
                .all(|a| *a == "--only-upgrade" || *a == "--reinstall");
            if !["update", "install", "remove", "upgrade"].contains(&sub)
                || !flags_ok
            {
                return Err(format!("Refused apt-get {sub}"));
            }
            let mut argv = vec!["-y".to_string(), "-o".into(), "APT::Status-Fd=1".into()];
            argv.extend(args.iter().map(|s| s.to_string()));
            Ok(argv)
        }
        "flatpak" => {
            let flags_ok = args
                .iter()
                .filter(|a| a.starts_with('-'))
                .all(|a| ["-y", "--system", "--if-not-exists", "--bundle", "--reinstall"].contains(a));
            if !["install", "uninstall", "update", "remote-add", "remote-delete"].contains(&sub)
                || !args.contains(&"--system")
                || !flags_ok
            {
                return Err(format!("Refused flatpak {sub}"));
            }
            Ok(args.iter().map(|s| s.to_string()).collect())
        }
        "snap" => {
            if !["install", "remove", "refresh"].contains(&sub)
                || names.iter().any(|a| a.starts_with('-'))
            {
                return Err(format!("Refused snap {sub}"));
            }
            Ok(args.iter().map(|s| s.to_string()).collect())
        }
        other => Err(format!("Refused program {other:?}")),
    }
}

/// Run the validated command and forward its output. Returns its exit code.
fn execute(program: &str, argv: &[String], out: &Arc<Mutex<std::io::Stdout>>) -> i32 {
    let spawned = Command::new(program)
        .args(argv)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            emit(out, &format!("E\tCould not run {program}: {e}"));
            return 1;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let out1 = out.clone();
    let t1 = thread::spawn(move || {
        if let Some(s) = stdout {
            forward(s, "O", &out1);
        }
    });
    let out2 = out.clone();
    let t2 = thread::spawn(move || {
        if let Some(s) = stderr {
            forward(s, "E", &out2);
        }
    });

    let status = child.wait();
    let _ = t1.join();
    let _ = t2.join();
    status.ok().and_then(|s| s.code()).unwrap_or(1)
}

fn forward<R: Read>(stream: R, tag: &str, out: &Arc<Mutex<std::io::Stdout>>) {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        emit(out, &format!("{tag}\t{line}"));
    }
}

fn emit(out: &Arc<Mutex<std::io::Stdout>>, line: &str) {
    if let Ok(mut o) = out.lock() {
        let _ = writeln!(o, "{line}");
        let _ = o.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn apt_install_remove_upgrade_and_update_are_allowed() {
        for sub in ["install", "remove", "upgrade", "update"] {
            let argv = validate("apt-get", &[sub, "nano"]).expect(sub);
            assert_eq!(&argv[..3], ["-y", "-o", "APT::Status-Fd=1"]);
            assert_eq!(argv.last().unwrap(), "nano");
        }
    }

    #[test]
    fn apt_only_upgrade_is_the_one_allowed_flag() {
        assert!(validate("apt-get", &["install", "--only-upgrade", "nano"]).is_ok());
        assert!(validate("apt-get", &["install", "--reinstall", "nano"]).is_ok());
        assert!(validate("apt-get", &["install", "--allow-unauthenticated", "nano"]).is_err());
        assert!(validate("apt-get", &["install", "-o", "Dir=/", "nano"]).is_err());
    }

    #[test]
    fn other_apt_subcommands_are_refused() {
        assert!(validate("apt-get", &["purge", "nano"]).is_err());
        assert!(validate("apt-get", &["autoremove"]).is_err());
    }

    #[test]
    fn flatpak_needs_the_system_flag_and_a_known_verb() {
        assert!(validate("flatpak", &["install", "--system", "-y", "flathub", "org.gimp.GIMP"]).is_ok());
        assert!(validate("flatpak", &["install", "--bundle", "--system", "/tmp/a.flatpak"]).is_ok());
        assert!(validate("flatpak", &["install", "flathub", "org.gimp.GIMP"]).is_err());
        assert!(validate("flatpak", &["override", "--system", "org.gimp.GIMP"]).is_err());
        assert!(validate("flatpak", &["install", "--user", "--system", "x"]).is_err());
    }

    #[test]
    fn snap_flags_are_refused() {
        assert!(validate("snap", &["install", "hello"]).is_ok());
        assert!(validate("snap", &["install", "--classic", "code"]).is_err());
        assert!(validate("snap", &["run", "hello"]).is_err());
    }

    #[test]
    fn other_programs_are_refused() {
        assert!(validate("rm", &["-rf", "/"]).is_err());
        assert!(validate("sh", &["-c", "id"]).is_err());
    }

    #[test]
    fn arguments_outside_the_allowed_characters_are_refused() {
        assert!(validate("apt-get", &["install", "nano;id"]).is_err());
        assert!(validate("apt-get", &["install", "$(id)"]).is_err());
        assert!(validate("apt-get", &["install", "a b"]).is_err());
        assert!(validate("apt-get", &["install", ""]).is_err());
    }
}
