//! Backend for Flatpak apps.
//!
//! Reads use `flatpak remote-ls` and `flatpak list` (no privileges needed for
//! listing). System-wide changes go through `pkexec flatpak`, so the desktop
//! polkit agent asks for a password. User-installation changes run as the
//! current user without any prompt.

use std::collections::{BTreeMap, HashSet};
use std::process::Command;

use crate::model::{parse_human_size, ActionResult, Package, Source};
use crate::{job, privileged};

const REMOTE_COLUMNS: &str = "--columns=application,name,version,origin,installed-size,download-size,description";
const INSTALLED_COLUMNS: &str = "--columns=application,name,version,origin,size,installation";

/// True when the `flatpak` command can be run at all.
pub fn is_available() -> bool {
    Command::new("flatpak")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Load every Flatpak app: the remote catalogue merged with what is installed
/// and what has an update waiting.
pub fn load() -> Result<Vec<Package>, String> {
    if !is_available() {
        return Err("Flatpak is not installed".into());
    }

    // `remote-ls` fails quietly with no remotes configured; that is just an empty catalogue.
    let remote = run_capture(&["remote-ls", "--app", REMOTE_COLUMNS]);
    let installed = run_capture(&["list", "--app", INSTALLED_COLUMNS]);
    let updates = run_capture(&["remote-ls", "--updates", "--app", "--columns=application"]);

    let mut by_id = parse_remote(&remote);
    merge_installed(&mut by_id, &installed);

    let upgradable: HashSet<String> = updates
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let mut packages: Vec<Package> = by_id.into_values().collect();
    for p in &mut packages {
        p.upgradable = upgradable.contains(&p.name);
        p.icon = find_icon(&p.origin, &p.name);
    }
    Ok(packages)
}

/// Icon for a Flatpak app: the installed copy if there is one, otherwise the copy
/// from the remote's appstream data. Looks in the system and per-user locations.
fn find_icon(origin: &str, id: &str) -> Option<std::path::PathBuf> {
    let arch = std::env::consts::ARCH;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let roots = [
        std::path::PathBuf::from("/var/lib/flatpak"),
        home.join(".local/share/flatpak"),
    ];

    let mut candidates = Vec::new();
    for root in &roots {
        candidates.push(root.join(format!(
            "app/{id}/current/active/export/share/icons/hicolor/64x64/apps/{id}.png"
        )));
    }
    if !origin.is_empty() {
        for root in &roots {
            candidates.push(root.join(format!(
                "appstream/{origin}/{arch}/active/icons/64x64/{id}.png"
            )));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Configured remotes as `name  url` lines. Used for the sources page.
pub fn remotes() -> Vec<String> {
    let text = run_capture(&["remotes", "--columns=name,url"]);
    let lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut parts = l.split('\t');
            let name = parts.next().unwrap_or("").trim();
            let url = parts.next().unwrap_or("").trim();
            format!("{name}  {url}")
        })
        .collect();
    if lines.is_empty() {
        vec!["No Flatpak remotes configured.".into()]
    } else {
        lines
    }
}

/// Add a system-wide remote, for example Flathub.
pub fn add_remote(name: &str, url: &str) -> ActionResult {
    run_privileged(&["remote-add", "--if-not-exists", "--system", name, url])
}

/// Remove a system-wide remote by name.
pub fn remove_remote(name: &str) -> ActionResult {
    run_privileged(&["remote-delete", "--system", name])
}

/// Parse `flatpak remote-ls --app --columns=...` output. When an app is offered by
/// several remotes or arches, the first record wins.
fn parse_remote(text: &str) -> BTreeMap<String, Package> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.splitn(7, '\t').collect();
        let id = f.first().copied().unwrap_or("").trim();
        if id.is_empty() || out.contains_key(id) {
            continue;
        }
        let mut p = base_package(id);
        p.title = non_empty(f.get(1).copied()).unwrap_or(id).to_string();
        p.candidate = f.get(2).copied().unwrap_or("").trim().to_string();
        p.origin = f.get(3).copied().unwrap_or("").trim().to_string();
        p.installed_size = parse_human_size(f.get(4).copied().unwrap_or(""));
        p.download_size = parse_human_size(f.get(5).copied().unwrap_or(""));
        p.description = f.get(6).copied().unwrap_or("").trim().to_string();
        out.insert(id.to_string(), p);
    }
    out
}

/// Fold `flatpak list --app --columns=...` output into the catalogue. Installed
/// apps that the remotes no longer offer are added as well.
fn merge_installed(by_id: &mut BTreeMap<String, Package>, text: &str) {
    for line in text.lines() {
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        let id = f.first().copied().unwrap_or("").trim();
        if id.is_empty() {
            continue;
        }
        let version = f.get(2).copied().unwrap_or("").trim().to_string();
        let origin = f.get(3).copied().unwrap_or("").trim().to_string();
        let size = parse_human_size(f.get(4).copied().unwrap_or(""));
        let scope = f.get(5).copied().unwrap_or("").trim().to_string();

        let entry = by_id.entry(id.to_string()).or_insert_with(|| {
            let mut p = base_package(id);
            p.title = non_empty(f.get(1).copied()).unwrap_or(id).to_string();
            p.candidate = version.clone();
            p.origin = origin.clone();
            p.installed_size = size;
            p
        });
        entry.installed = Some(version);
        entry.scope = scope;
        if entry.origin.is_empty() {
            entry.origin = origin;
        }
    }
}

fn base_package(id: &str) -> Package {
    Package {
        source: Source::Flatpak,
        name: id.to_string(),
        title: id.to_string(),
        candidate: String::new(),
        installed: None,
        download_size: 0,
        installed_size: 0,
        section: String::new(),
        origin: String::new(),
        scope: String::new(),
        description: String::new(),
        homepage: String::new(),
        upgradable: false,
        icon: None,
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

/// Install Flatpak apps from one remote, system-wide.
pub fn install(origin: &str, ids: &[String]) -> ActionResult {
    if ids.is_empty() {
        return ActionResult::err("Nothing selected.");
    }
    let mut args = vec!["install", "-y", "--system"];
    if !origin.is_empty() {
        args.push(origin);
    }
    args.extend(ids.iter().map(String::as_str));
    run_privileged(&args)
}

/// Remove one Flatpak app from the installation it lives in.
pub fn remove(id: &str, scope: &str) -> ActionResult {
    if scope == "user" {
        run_plain(&["uninstall", "-y", "--user", id])
    } else {
        run_privileged(&["uninstall", "-y", "--system", id])
    }
}

/// Update every Flatpak app. `has_user` / `has_system` say which installations have apps.
pub fn update_all(has_user: bool, has_system: bool) -> ActionResult {
    let mut messages = Vec::new();
    if has_user {
        let r = run_plain(&["update", "-y", "--user"]);
        if !r.success {
            return r;
        }
        messages.push(r.summary);
    }
    if has_system {
        let r = run_privileged(&["update", "-y", "--system"]);
        if !r.success {
            return r;
        }
        messages.push(r.summary);
    }
    if messages.is_empty() {
        ActionResult::ok("No Flatpak apps to update.")
    } else {
        ActionResult::ok(messages.join(" · "))
    }
}

/// System-wide Flatpak change, run through the privileged helper.
fn run_privileged(args: &[&str]) -> ActionResult {
    privileged::run("flatpak", args)
}

/// Per-user Flatpak change, streamed into the current job. Needs no password.
fn run_plain(args: &[&str]) -> ActionResult {
    job::run_streamed("flatpak", args)
}

fn run_capture(args: &[&str]) -> String {
    Command::new("flatpak")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tab-separated, as `flatpak --columns` prints when not attached to a terminal.
    const REMOTE: &str = "\
org.gimp.GIMP\tGIMP\t2.10.38\tflathub\t400.0 MB\t120.5 MB\tImage editor
org.gimp.GIMP\tGIMP\t2.10.38\tflathub-beta\t1 GB\t1 GB\tduplicate, must be ignored
org.mozilla.firefox\tFirefox\t140.0\tflathub\t300.0 MB\t90.0 MB\tWeb browser
";
    const INSTALLED: &str = "\
org.gimp.GIMP\tGIMP\t2.10.36\tflathub\t400.0 MB\tsystem
org.local.Tool\tLocal Tool\t1.0\t\t12.0 MB\tuser
";

    #[test]
    fn parses_remote_catalogue_and_dedupes() {
        let map = parse_remote(REMOTE);
        assert_eq!(map.len(), 2);
        let gimp = &map["org.gimp.GIMP"];
        assert_eq!(gimp.title, "GIMP");
        assert_eq!(gimp.candidate, "2.10.38");
        assert_eq!(gimp.origin, "flathub");
        assert_eq!(gimp.download_size, 120_500_000);
        assert_eq!(gimp.description, "Image editor");
        assert_eq!(gimp.source, Source::Flatpak);
    }

    #[test]
    fn merges_installed_state_and_adds_unknown_apps() {
        let mut map = parse_remote(REMOTE);
        merge_installed(&mut map, INSTALLED);
        assert_eq!(map["org.gimp.GIMP"].installed.as_deref(), Some("2.10.36"));
        assert_eq!(map["org.gimp.GIMP"].scope, "system");
        assert!(!map["org.mozilla.firefox"].is_installed());

        let local = &map["org.local.Tool"];
        assert!(local.is_installed());
        assert_eq!(local.scope, "user");
        assert_eq!(local.title, "Local Tool");
        assert_eq!(local.installed_size, 12_000_000);
    }
}
