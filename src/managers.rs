//! Support for the package managers besides APT, Flatpak and AppImage: Snap, npm,
//! pip, Cargo, Bun and Homebrew. APT, Flatpak and AppImage have their own modules.
//!
//! Each manager is driven through its own command-line tool. Global installs
//! are used throughout. Where a manager's own tool needs root (Snap), the call
//! goes through `pkexec`. Everything else runs as the current user.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::model::{ActionResult, Package, Source};
use crate::{job, privileged};

/// Full path of the manager's command, or `None` when it is not installed.
/// AppImages have no command, so they return the folder they are read from.
pub fn locate(src: Source) -> Option<String> {
    match src.command() {
        None => Some(crate::appimage::directory().display().to_string()),
        Some(cmd) => {
            let out = Command::new("sh")
                .arg("-c")
                .arg(format!("command -v {cmd}"))
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!path.is_empty()).then_some(path)
        }
    }
}

/// Installed packages for the managers that have no APT/Flatpak-style catalogue.
pub fn load(src: Source) -> Result<Vec<Package>, String> {
    match src {
        Source::Snap => snap_list(),
        Source::Npm => npm_list(),
        Source::Pip => pip_list(),
        Source::Cargo => cargo_list(),
        Source::Bun => bun_list(),
        Source::Homebrew => brew_list(),
        _ => Ok(Vec::new()),
    }
}

/// Packages with a newer version available, mapped to that latest version
/// (empty when the manager does not report it).
pub fn outdated(src: Source) -> HashMap<String, String> {
    match src {
        Source::Snap => snap_outdated(),
        Source::Npm => npm_outdated(),
        Source::Pip => pip_outdated(),
        Source::Homebrew => brew_outdated(),
        _ => HashMap::new(),
    }
}

/// Install one package by name.
pub fn install(src: Source, name: &str) -> ActionResult {
    match src {
        Source::Snap => run("snap", &["install", name], true),
        Source::Npm => run("npm", &["install", "-g", name], false),
        Source::Pip => run("pip3", &["install", "--user", name], false),
        Source::Cargo => run("cargo", &["install", name], false),
        Source::Bun => run("bun", &["add", "-g", name], false),
        Source::Homebrew => {
            let formula = run("brew", &["install", name], false);
            if formula.success {
                formula
            } else {
                // Not a formula, so try it as a cask (a macOS-style app).
                run("brew", &["install", "--cask", name], false)
            }
        }
        _ => ActionResult::err("Install this one from its own tab."),
    }
}

/// Remove one installed package.
pub fn remove(src: Source, name: &str) -> ActionResult {
    match src {
        Source::Snap => run("snap", &["remove", name], true),
        Source::Npm => run("npm", &["uninstall", "-g", name], false),
        Source::Pip => run("pip3", &["uninstall", "-y", name], false),
        Source::Cargo => run("cargo", &["uninstall", name], false),
        Source::Bun => run("bun", &["remove", "-g", name], false),
        Source::Homebrew => {
            let formula = run("brew", &["uninstall", name], false);
            if formula.success {
                formula
            } else {
                run("brew", &["uninstall", "--cask", name], false)
            }
        }
        _ => ActionResult::err("Remove this one from its own tab."),
    }
}

/// Upgrade the given packages. An empty list upgrades everything that is outdated.
pub fn upgrade(src: Source, names: &[String]) -> ActionResult {
    let targets: Vec<String> = if names.is_empty() {
        outdated(src).into_keys().collect()
    } else {
        names.to_vec()
    };
    if targets.is_empty() {
        return ActionResult::ok("Nothing to update.");
    }
    let args: Vec<&str> = targets.iter().map(String::as_str).collect();
    match src {
        Source::Snap => {
            let mut a = vec!["refresh"];
            a.extend(args);
            run("snap", &a, true)
        }
        Source::Npm => {
            let mut a = vec!["update", "-g"];
            a.extend(args);
            run("npm", &a, false)
        }
        Source::Pip => {
            let mut a = vec!["install", "--user", "-U"];
            a.extend(args);
            run("pip3", &a, false)
        }
        Source::Homebrew => {
            let mut a = vec!["upgrade"];
            a.extend(args);
            run("brew", &a, false)
        }
        _ => ActionResult::err("Updates are not supported for this package manager."),
    }
}

/// Lines describing where this manager gets its packages, for the sources page.
pub fn sources(src: Source) -> Vec<String> {
    let lines: Vec<String> = match src {
        Source::Apt => crate::apt::sources(),
        Source::Flatpak => crate::flatpak::remotes(),
        Source::Npm => vec![format!(
            "registry: {}",
            capture("npm", &["config", "get", "registry"]).trim()
        )],
        Source::Pip => {
            let index = capture("pip3", &["config", "get", "global.index-url"]);
            let index = index.trim();
            vec![if index.is_empty() {
                "index: https://pypi.org/simple (default)".to_string()
            } else {
                format!("index: {index}")
            }]
        }
        Source::Homebrew => {
            let taps: Vec<String> = capture("brew", &["tap"])
                .lines()
                .map(|t| format!("tap: {}", t.trim()))
                .filter(|t| t.len() > 5)
                .collect();
            if taps.is_empty() {
                vec!["tap: homebrew/core (default)".into()]
            } else {
                taps
            }
        }
        Source::Cargo => vec!["registry: crates.io (default)".into()],
        Source::Bun => vec!["registry: npm registry (default)".into()],
        Source::Snap => vec!["store: Snap Store (default)".into()],
        Source::AppImage => vec![format!(
            "folder: {}",
            crate::appimage::directory().display()
        )],
    };
    lines
}

// ----- Snap ------------------------------------------------------------------

fn snap_list() -> Result<Vec<Package>, String> {
    let text = capture("snap", &["list"]);
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let name = *f.first()?;
            let version = *f.get(1)?;
            let mut p = Package::new(Source::Snap, name);
            p.candidate = version.to_string();
            p.installed = Some(version.to_string());
            p.origin = f.get(4).unwrap_or(&"").to_string();
            p.description = format!("Snap package from {}", p.origin);
            Some(p)
        })
        .collect())
}

fn snap_outdated() -> HashMap<String, String> {
    capture_timed("snap", &["refresh", "--list"])
        .lines()
        .skip(1)
        .filter(|l| !l.starts_with("All "))
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let name = it.next()?.to_string();
            let version = it.next().unwrap_or("").to_string();
            Some((name, version))
        })
        .collect()
}

// ----- npm -------------------------------------------------------------------

fn npm_list() -> Result<Vec<Package>, String> {
    let text = capture("npm", &["ls", "-g", "--depth=0", "--json"]);
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("npm: {e}"))?;
    let deps = value
        .get("dependencies")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(deps
        .iter()
        .map(|(name, info)| {
            let version = info.get("version").and_then(Value::as_str).unwrap_or("").to_string();
            let mut p = Package::new(Source::Npm, name);
            p.candidate = version.clone();
            p.installed = Some(version);
            p.description = "Global npm package".into();
            p
        })
        .collect())
}

fn npm_outdated() -> HashMap<String, String> {
    let text = capture_timed("npm", &["outdated", "-g", "--json"]);
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    value
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(name, info)| {
                    let latest = info.get("latest").and_then(Value::as_str).unwrap_or("").to_string();
                    (name.clone(), latest)
                })
                .collect()
        })
        .unwrap_or_default()
}

// ----- pip -------------------------------------------------------------------

fn pip_list() -> Result<Vec<Package>, String> {
    let text = capture("pip3", &["list", "--format=json"]);
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("pip3: {e}"))?;
    Ok(value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?;
                    let version = item.get("version")?.as_str()?;
                    let mut p = Package::new(Source::Pip, name);
                    p.candidate = version.to_string();
                    p.installed = Some(version.to_string());
                    p.description = "Python package".into();
                    Some(p)
                })
                .collect()
        })
        .unwrap_or_default())
}

fn pip_outdated() -> HashMap<String, String> {
    let text = capture_timed("pip3", &["list", "--outdated", "--format=json"]);
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.to_string();
                    let latest = item.get("latest_version")?.as_str()?.to_string();
                    Some((name, latest))
                })
                .collect()
        })
        .unwrap_or_default()
}

// ----- Cargo -----------------------------------------------------------------

fn cargo_list() -> Result<Vec<Package>, String> {
    let text = capture("cargo", &["install", "--list"]);
    Ok(text
        .lines()
        .filter(|l| !l.starts_with(' ') && l.trim_end().ends_with(':'))
        .filter_map(|line| {
            // Lines look like `ripgrep v14.1.0:`.
            let mut it = line.trim_end_matches(':').split_whitespace();
            let name = it.next()?;
            let version = it.next()?.trim_start_matches('v');
            let mut p = Package::new(Source::Cargo, name);
            p.candidate = version.to_string();
            p.installed = Some(version.to_string());
            p.description = "Binary installed with cargo install".into();
            Some(p)
        })
        .collect())
}

// ----- Bun -------------------------------------------------------------------

fn bun_list() -> Result<Vec<Package>, String> {
    let dir = home().join(".bun/install/global/node_modules");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Skip hidden folders and scoped (@scope) folders, which are not single packages.
        if name.starts_with('.') || name.starts_with('@') {
            continue;
        }
        let version = fs::read_to_string(entry.path().join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("version").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        let mut p = Package::new(Source::Bun, &name);
        p.candidate = version.clone();
        p.installed = Some(version);
        p.description = "Global Bun package".into();
        out.push(p);
    }
    Ok(out)
}

// ----- Homebrew --------------------------------------------------------------

fn brew_list() -> Result<Vec<Package>, String> {
    let mut out = Vec::new();
    for (flag, kind) in [("--formula", "formula"), ("--cask", "cask")] {
        let text = capture("brew", &["list", flag, "--versions"]);
        for line in text.lines() {
            let mut it = line.split_whitespace();
            let Some(name) = it.next() else {
                continue;
            };
            // `brew list --versions` prints `name v1 v2`; the last version is the current one.
            let version = it.last().unwrap_or("").to_string();
            let mut p = Package::new(Source::Homebrew, name);
            p.candidate = version.clone();
            p.installed = Some(version);
            p.description = format!("Homebrew {kind}");
            out.push(p);
        }
    }
    Ok(out)
}

fn brew_outdated() -> HashMap<String, String> {
    capture_timed("brew", &["outdated", "--quiet"])
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| (l.to_string(), String::new()))
        .collect()
}

// ----- helpers ---------------------------------------------------------------

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Like `capture`, but gives up after 25 seconds. The update checks reach the network
/// (npm, pip, Homebrew, Snap), and an unreachable network must not block the window.
fn capture_timed(program: &str, args: &[&str]) -> String {
    Command::new("timeout")
        .arg("25")
        .arg(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("NO_UPDATE_NOTIFIER", "1")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// stdout of a command, or an empty string if it cannot run. Exit status is ignored
/// on purpose: npm and brew exit non-zero when they have something to report.
fn capture(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Run a command that changes something. Output streams into the current job. System
/// commands (Snap) go through the privileged helper, so the password is asked once.
fn run(program: &str, args: &[&str], privileged: bool) -> ActionResult {
    if privileged {
        privileged::run(program, args)
    } else {
        job::run_streamed(program, args)
    }
}
