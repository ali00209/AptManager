//! Backend for talking to APT / dpkg.
//!
//! Everything here is plain Rust with no UI dependencies. Read operations use
//! `apt-cache` and `dpkg-query` (no privileges needed). Write operations go
//! through `pkexec apt-get` so the desktop's polkit agent prompts for a password.

use crate::track::OutputTracked;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use crate::model::{ActionResult, Package};
use crate::privileged;

/// One entry from /var/log/apt/history.log.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub date: String,
    pub command: String,
}

/// Load the full APT catalogue: available packages merged with install state
/// and the upgradable set.
pub fn load_catalog() -> Result<Vec<Package>, String> {
    let available = run_capture("apt-cache", &["dumpavail"])?;
    let installed = run_capture(
        "dpkg-query",
        &["-W", "-f=${Package}\t${Version}\t${Status}\n"],
    )?;
    let upgradable = load_upgradable_names();

    let mut by_name = parse_dumpavail(&available);

    // Merge in installed state. Packages installed from a source that is no
    // longer configured still need to appear, so we add them as well.
    for line in installed.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(name), Some(version), Some(status)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // dpkg status looks like "install ok installed"; we only want installed ones.
        if !status.ends_with(" installed") {
            continue;
        }
        let name = name.trim().to_string();
        let version = version.trim().to_string();
        by_name
            .entry(name.clone())
            .and_modify(|p| p.installed = Some(version.clone()))
            .or_insert_with(|| {
                let mut p = Package::apt_base(name);
                p.candidate = version.clone();
                p.installed = Some(version);
                p
            });
    }

    let mut packages: Vec<Package> = by_name.into_values().collect();
    for p in &mut packages {
        p.upgradable = upgradable.contains(&p.name);
        p.icon = icon_for(&p.name);
    }
    Ok(packages)
}

/// Icon for a package, found by name in the icon theme or by the `Icon=` entry of its
/// `.desktop` file. Icons exist on disk only for installed packages, so available
/// packages normally return `None`.
fn icon_for(name: &str) -> Option<PathBuf> {
    icon_index().get(name).cloned()
}

/// Builds the icon lookup once. Earlier entries win, so larger PNG sizes come first.
fn icon_index() -> &'static HashMap<String, PathBuf> {
    static INDEX: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map: HashMap<String, PathBuf> = HashMap::new();

        let mut dirs: Vec<PathBuf> = ["64x64", "48x48", "128x128", "256x256", "96x96", "32x32", "scalable"]
            .iter()
            .map(|size| PathBuf::from(format!("/usr/share/icons/hicolor/{size}/apps")))
            .collect();
        dirs.push(PathBuf::from("/usr/share/pixmaps"));
        for dir in dirs {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let usable = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "svg"))
                    .unwrap_or(false);
                if !usable {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    map.entry(stem.to_string()).or_insert(path);
                }
            }
        }

        // Desktop entries name their icon, often differently from the file name.
        if let Ok(entries) = fs::read_dir("/usr/share/applications") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                    continue;
                };
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let Some(icon) = text
                    .lines()
                    .find_map(|l| l.strip_prefix("Icon="))
                    .map(|s| s.trim().to_string())
                else {
                    continue;
                };
                let target = if icon.starts_with('/') {
                    let p = PathBuf::from(&icon);
                    p.is_file().then_some(p)
                } else {
                    map.get(&icon).cloned()
                };
                if let Some(target) = target {
                    map.entry(stem).or_insert(target);
                }
            }
        }
        map
    })
}

/// Configured APT sources, one line per entry, read from the sources files. Read-only.
pub fn sources() -> Vec<String> {
    let mut files = vec![std::path::PathBuf::from("/etc/apt/sources.list")];
    if let Ok(dir) = fs::read_dir("/etc/apt/sources.list.d") {
        files.extend(dir.flatten().map(|e| e.path()));
    }
    let mut lines = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("deb ") || line.starts_with("deb-src ") || line.starts_with("URIs:") || line.starts_with("Suites:") {
                lines.push(line.to_string());
            }
        }
    }
    if lines.is_empty() {
        lines.push("No repositories found in /etc/apt.".into());
    }
    lines
}

/// Parse `apt-cache dumpavail` output. Records are separated by blank lines.
/// Multi-arch duplicates are collapsed to the first record per package name.
fn parse_dumpavail(text: &str) -> BTreeMap<String, Package> {
    let mut out: BTreeMap<String, Package> = BTreeMap::new();
    let mut cur: BTreeMap<&'static str, String> = BTreeMap::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            flush_record(&mut cur, &mut out);
            continue;
        }
        // Continuation lines of multi-line fields start with whitespace.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some((key, value)) = line.split_once(": ") {
            let key: &'static str = match key {
                "Package" => "Package",
                "Version" => "Version",
                "Installed-Size" => "Installed-Size",
                "Size" => "Size",
                "Section" => "Section",
                "Description" => "Description",
                "Homepage" => "Homepage",
                _ => continue,
            };
            // Keep only the first occurrence (the short one-line description).
            cur.entry(key).or_insert_with(|| value.trim().to_string());
        }
    }
    flush_record(&mut cur, &mut out);
    out
}

/// Turn one collected dumpavail record into a `Package`, keeping the first
/// record seen for each name (multi-arch duplicates share a name).
fn flush_record(cur: &mut BTreeMap<&'static str, String>, out: &mut BTreeMap<String, Package>) {
    if let Some(name) = cur.get("Package").cloned() {
        if !out.contains_key(&name) {
            let installed_kib: u64 = cur
                .get("Installed-Size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let mut p = Package::apt_base(name.clone());
            p.candidate = cur.get("Version").cloned().unwrap_or_default();
            p.download_size = cur.get("Size").and_then(|s| s.parse().ok()).unwrap_or(0);
            p.installed_size = installed_kib * 1024;
            p.section = cur.get("Section").cloned().unwrap_or_default();
            p.description = cur.get("Description").cloned().unwrap_or_default();
            p.homepage = cur.get("Homepage").cloned().unwrap_or_default();
            out.insert(name, p);
        }
    }
    cur.clear();
}

/// Names of packages with a newer version available (`apt list --upgradable`).
fn load_upgradable_names() -> HashSet<String> {
    let Ok(text) = run_capture("apt", &["list", "--upgradable"]) else {
        return HashSet::new();
    };
    text.lines()
        .filter(|l| l.contains("[upgradable"))
        .filter_map(|l| l.split('/').next())
        .map(|s| s.trim().to_string())
        .collect()
}

/// Read /var/log/apt/history.log and return the most recent operations, newest first.
pub fn load_history(limit: usize) -> Vec<HistoryEntry> {
    let Ok(text) = fs::read_to_string("/var/log/apt/history.log") else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let mut date = String::new();
    let mut command = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Start-Date: ") {
            date = v.trim().to_string();
            command.clear();
        } else if let Some(v) = line.strip_prefix("Commandline: ") {
            command = v.trim().to_string();
        } else if line.trim().is_empty() && !date.is_empty() {
            entries.push(HistoryEntry {
                date: std::mem::take(&mut date),
                command: std::mem::take(&mut command),
            });
        }
    }
    if !date.is_empty() {
        entries.push(HistoryEntry { date, command });
    }
    entries.reverse();
    entries.truncate(limit);
    entries
}

/// `apt-get update` — refresh the package lists from the repositories.
pub fn refresh_lists() -> ActionResult {
    privileged::run("apt-get", &["update"])
}

fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output_tracked()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    // apt-cache may exit non-zero on partial data; keep whatever stdout we got.
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Package: 0ad
Architecture: amd64
Version: 0.0.26-3
Installed-Size: 59000
Size: 55530000
Section: games
Description: Cross-platform, 3D and historically-based real-time strategy game
 A longer description continuation line.
Homepage: http://play0ad.com/

Package: 0ad
Architecture: i386
Version: 0.0.25-1
Installed-Size: 1
Size: 1
Section: games
Description: duplicate arch record that must be ignored

Package: git
Version: 1:2.43.0-1ubuntu7
Installed-Size: 20000
Size: 9000000
Section: vcs
Description: distributed version control system
";

    #[test]
    fn parses_records_and_collapses_multiarch() {
        let map = parse_dumpavail(SAMPLE);
        assert_eq!(map.len(), 2);
        let ad = &map["0ad"];
        assert_eq!(ad.candidate, "0.0.26-3");
        assert_eq!(ad.download_size, 55_530_000);
        assert_eq!(ad.installed_size, 59_000 * 1024);
        assert_eq!(ad.section, "games");
        assert_eq!(
            ad.description,
            "Cross-platform, 3D and historically-based real-time strategy game"
        );
        assert_eq!(ad.homepage, "http://play0ad.com/");
        assert_eq!(map["git"].candidate, "1:2.43.0-1ubuntu7");
    }

    #[test]
    fn live_catalog_loads_on_this_machine() {
        // Smoke test against the real apt database. Skipped quietly if apt is absent.
        if let Ok(packages) = load_catalog() {
            let installed = packages.iter().filter(|p| p.is_installed()).count();
            println!(
                "live catalogue: {} packages, installed: {installed}, upgradable: {}",
                packages.len(),
                packages.iter().filter(|p| p.upgradable).count()
            );
        }
    }
}
