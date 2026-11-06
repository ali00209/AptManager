//! Reading the package managers besides APT, Flatpak and AppImage: Snap, npm, pip,
//! Cargo, Bun and Homebrew. Changes (install, remove, upgrade) are planned in `plan.rs`
//! and run as transactions.

use crate::track::OutputTracked;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;

use crate::model::{Package, Source};

/// Full path of the manager's command, or `None` when it is not installed.
/// AppImages have no command, so they return the folder they are read from.
pub fn locate(src: Source) -> Option<String> {
    match src {
        Source::AppImage => Some(crate::appimage::directory().display().to_string()),
        Source::Pip => pip_command().map(|(program, prefix)| {
            if prefix.is_empty() {
                find_path(&program).unwrap_or(program)
            } else {
                format!("{} {}", find_path("python3").unwrap_or("python3".into()), prefix.join(" "))
            }
        }),
        other => other.command().and_then(find_path),
    }
}

/// Full path of a command on PATH.
fn find_path(cmd: &str) -> Option<String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd}"))
        .output_tracked()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// The command that runs pip, and any arguments that come before the pip arguments.
/// Tries `pip3`, then `pip`, then `python3 -m pip`. Found once, then reused.
pub fn pip_command() -> Option<(String, Vec<String>)> {
    static PIP: OnceLock<Option<(String, Vec<String>)>> = OnceLock::new();
    PIP.get_or_init(|| {
        for program in ["pip3", "pip"] {
            if find_path(program).is_some() {
                return Some((program.to_string(), Vec::new()));
            }
        }
        // The interpreter on PATH first, then the system Python, which often has pip
        // even when PATH does not reach it.
        for python in ["python3", "/usr/bin/python3"] {
            let works = Command::new(python)
                .args(["-m", "pip", "--version"])
                .output_tracked()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if works {
                return Some((python.to_string(), vec!["-m".to_string(), "pip".to_string()]));
            }
        }
        None
    })
    .clone()
}

/// Installed packages for the managers that have no APT/Flatpak-style catalogue.
pub fn load(src: Source) -> Result<Vec<Package>, String> {
    match src {
        Source::Snap => snap_list(),
        Source::Npm => npm_list(),
        Source::Pip => pip_list(),
        Source::Cargo => cargo_list(),
        Source::Bun => bun_list(),
        Source::Uv => uv_list(),
        Source::Homebrew => brew_list().map(|mut list| {
            let installed: HashSet<String> = list.iter().map(|p| p.name.clone()).collect();
            list.extend(brew_catalogue(&installed));
            list
        }),
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

/// Lines describing where this manager gets its packages, for the sources page.
pub fn sources(src: Source) -> Vec<String> {
    match src {
        Source::Apt => crate::apt::sources(),
        Source::Flatpak => crate::flatpak::remotes(),
        Source::Uv => vec![format!(
            "tool directory: {}",
            capture("uv", &["tool", "dir"]).trim()
        )],
        Source::Npm => vec![format!(
            "registry: {}",
            capture("npm", &["config", "get", "registry"]).trim()
        )],
        Source::Pip => {
            let index = pip_capture(&["config", "get", "global.index-url"]);
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
    }
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
    if pip_command().is_none() {
        return Ok(Vec::new());
    }
    let text = pip_capture(&["list", "--format=json"]);
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("pip: {e}"))?;
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
    let text = pip_capture_timed(&["list", "--outdated", "--format=json"]);
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

/// `uv tool list` prints `name vVERSION`, then `- binary` lines under each tool.
fn uv_list() -> Result<Vec<Package>, String> {
    Ok(parse_uv_tools(&capture("uv", &["tool", "list"])))
}

pub fn parse_uv_tools(text: &str) -> Vec<Package> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with(' ') && !l.starts_with('-'))
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let name = it.next()?;
            let version = it.next().unwrap_or("").trim_start_matches('v').to_string();
            let mut p = Package::new(Source::Uv, name);
            p.candidate = version.clone();
            p.installed = Some(version);
            p.description = "Python tool installed with uv tool".into();
            Some(p)
        })
        .collect()
}

/// What a manager says about one package right now, read after an action on it.
#[derive(Debug, Default, PartialEq)]
pub struct Check {
    pub installed: Option<String>,
    pub candidate: Option<String>,
}

/// Check one package with its own manager's query. `None` means this manager has no
/// single-package query, so the caller falls back to reloading that registry.
pub fn check_one(src: Source, name: &str) -> Option<Check> {
    let installed_version: Option<String> = match src {
        Source::Apt => {
            let out = stdout_if_ok("dpkg-query", &["-W", "-f=${db:Status-Status}|${Version}", name])?;
            let (status, version) = out.trim().split_once('|')?;
            return Some(Check {
                installed: (status == "installed").then(|| version.to_string()),
                candidate: candidate_from_policy(name),
            });
        }
        Source::Flatpak => capture("flatpak", &["list", "--app", "--columns=application,version"])
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .find(|(app, _)| *app == name)
            .map(|(_, v)| v.trim().to_string()),
        Source::Npm => serde_json::from_str::<Value>(&capture("npm", &["ls", "-g", "--depth=0", "--json"]))
            .ok()
            .and_then(|v| v.get("dependencies")?.get(name)?.get("version")?.as_str().map(str::to_string)),
        Source::Pip => {
            let (program, prefix) = pip_command()?;
            let mut args = prefix;
            args.extend(["show".to_string(), name.to_string()]);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            stdout_if_ok(&program, &refs)?
                .lines()
                .find_map(|l| l.strip_prefix("Version:").map(|v| v.trim().to_string()))
        }
        Source::Uv => capture("uv", &["tool", "list"])
            .lines()
            .find_map(|l| {
                let mut it = l.split_whitespace();
                (it.next()? == name).then(|| it.next().unwrap_or("").trim_start_matches('v').to_string())
            }),
        Source::Cargo => capture("cargo", &["install", "--list"])
            .lines()
            .filter(|l| !l.starts_with(' ') && l.trim_end().ends_with(':'))
            .find_map(|l| {
                let mut it = l.split_whitespace();
                (it.next()? == name).then(|| it.next().unwrap_or("").trim_start_matches('v').trim_end_matches(':').to_string())
            }),
        Source::Bun => {
            let manifest = home().join(".bun/install/global/node_modules").join(name).join("package.json");
            fs::read_to_string(manifest)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|v| v.get("version")?.as_str().map(str::to_string))
        }
        Source::Homebrew => stdout_if_ok("brew", &["list", "--versions", name])?
            .split_whitespace()
            .last()
            .map(str::to_string),
        Source::Snap => stdout_if_ok("snap", &["list", name])?
            .lines()
            .nth(1)
            .and_then(|l| l.split_whitespace().nth(1))
            .map(str::to_string),
        Source::AppImage => return None,
    };
    Some(Check {
        candidate: installed_version.clone(),
        installed: installed_version,
    })
}

/// Stdout of a command, only when it exits successfully.
fn stdout_if_ok(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output_tracked()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The `Candidate:` version from `apt-cache policy`, or `None` when APT has none.
fn candidate_from_policy(name: &str) -> Option<String> {
    capture("apt-cache", &["policy", name])
        .lines()
        .find_map(|l| l.trim().strip_prefix("Candidate:").map(|v| v.trim().to_string()))
        .filter(|v| v != "(none)" && !v.is_empty())
}

/// Every formula in homebrew-core and every cask in homebrew-cask that is not installed.
/// A missing tap just leaves its part out.
fn brew_catalogue(installed: &HashSet<String>) -> Vec<Package> {
    let mut out: Vec<Package> = Vec::new();
    let mut seen: HashSet<String> = installed.clone();
    for (args, kind, origin) in [
        (&["formulae"][..], "formula", "homebrew-core"),
        (&["casks"][..], "cask", "homebrew-cask"),
    ] {
        for line in capture_timed("brew", args).lines() {
            let name = line.trim();
            if name.is_empty() || !seen.insert(name.to_string()) {
                continue;
            }
            let mut p = Package::new(Source::Homebrew, name);
            p.description = format!("Homebrew {kind}, not installed");
            p.origin = origin.to_string();
            out.push(p);
        }
    }
    out
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

/// Run pip through the detected command, with extra arguments.
fn pip_capture(args: &[&str]) -> String {
    match pip_command() {
        Some((program, prefix)) => {
            let mut all: Vec<&str> = prefix.iter().map(String::as_str).collect();
            all.extend_from_slice(args);
            capture(&program, &all)
        }
        None => String::new(),
    }
}

fn pip_capture_timed(args: &[&str]) -> String {
    match pip_command() {
        Some((program, prefix)) => {
            let mut all: Vec<&str> = prefix.iter().map(String::as_str).collect();
            all.extend_from_slice(args);
            capture_timed(&program, &all)
        }
        None => String::new(),
    }
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
        .output_tracked()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// stdout of a command, or an empty string if it cannot run. Exit status is ignored
/// on purpose: npm and brew exit non-zero when they have something to report.
fn capture(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output_tracked()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod uv_tests {
    use super::*;

    #[test]
    fn installed_uv_tools_are_listed_when_uv_is_present() {
        if find_path("uv").is_none() {
            return; // uv is not installed here
        }
        let list = load(Source::Uv).expect("uv tool list runs");
        println!("uv packages found: {}", list.len());
        for p in &list {
            println!("  {} {:?}", p.name, p.installed);
        }
        assert!(!list.is_empty(), "uv has tools installed but none were listed");
    }
}

#[cfg(test)]
mod uv_cache_tests {
    use super::*;

    #[test]
    fn a_uv_package_survives_the_cache_round_trip() {
        let packages = parse_uv_tools("black v26.5.1\n- black\n- blackd\n");
        assert_eq!(packages.len(), 1);
        let json = serde_json::to_string(&packages[0]).expect("serialises");
        let back: crate::model::Package = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back.source, Source::Uv);
        assert_eq!(back.key(), "uv:black");
        assert_eq!(back.installed.as_deref(), Some("26.5.1"));
    }
}
