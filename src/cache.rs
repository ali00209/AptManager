//! The on-disk package cache, one per registry.
//!
//! A registry has two files in `~/.cache/apt-manager/`:
//! - `<tag>.base.jsonl`: a full snapshot, one package per line.
//! - `<tag>.log.jsonl`: changes made since that snapshot, one per line. A refresh appends
//!   only the packages that changed, so the snapshot is not rewritten each time.
//!
//! Loading replays the log over the snapshot. When the log grows larger than the snapshot,
//! the next full check writes a new snapshot and empties the log.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Package, Source};

#[derive(Serialize, Deserialize)]
enum Entry {
    Put(Package),
    Del(String),
}

fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".cache")
        });
    base.join("apt-manager")
}

fn base_path(src: Source) -> PathBuf {
    dir().join(format!("{}.base.jsonl", src.tag()))
}

fn log_path(src: Source) -> PathBuf {
    dir().join(format!("{}.log.jsonl", src.tag()))
}

/// Everything cached for one registry, keyed by `Package::key`.
pub fn load(src: Source) -> HashMap<String, Package> {
    let mut map = HashMap::new();
    for line in read_lines(&base_path(src)) {
        if let Ok(p) = serde_json::from_str::<Package>(&line) {
            map.insert(p.key(), p);
        }
    }
    for line in read_lines(&log_path(src)) {
        match serde_json::from_str::<Entry>(&line) {
            Ok(Entry::Put(p)) => {
                map.insert(p.key(), p);
            }
            Ok(Entry::Del(key)) => {
                map.remove(&key);
            }
            Err(_) => {}
        }
    }
    map
}

fn read_lines(path: &PathBuf) -> Vec<String> {
    fs::read_to_string(path)
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Replace the whole cache for one registry with `all`, and empty its log.
pub fn save_snapshot(src: Source, all: &[Package]) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let tmp = dir().join(format!("{}.base.tmp", src.tag()));
    let mut text = String::new();
    for p in all {
        if let Ok(line) = serde_json::to_string(p) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, base_path(src)).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(log_path(src));
    Ok(())
}

/// Record changed and removed packages in the log. Only these lines are written.
pub fn append(src: Source, changed: &[Package], removed: &[String]) -> Result<(), String> {
    if changed.is_empty() && removed.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(src))
        .map_err(|e| e.to_string())?;
    let mut text = String::new();
    for p in changed {
        if let Ok(line) = serde_json::to_string(&Entry::Put(p.clone())) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    for key in removed {
        if let Ok(line) = serde_json::to_string(&Entry::Del(key.clone())) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

/// True when the log has grown larger than the snapshot, so the snapshot should be rewritten.
pub fn log_is_large(src: Source) -> bool {
    let log = fs::metadata(log_path(src)).map(|m| m.len()).unwrap_or(0);
    let base = fs::metadata(base_path(src)).map(|m| m.len()).unwrap_or(0);
    log > base.max(1)
}

/// Compare a fresh list against the cache. Returns the packages that are new or changed,
/// and the keys of the packages that have gone. Each key is looked up once, so this takes
/// time in proportion to the number of packages.
pub fn diff(old: &HashMap<String, Package>, fresh: &[Package]) -> (Vec<Package>, Vec<String>) {
    let mut seen: HashSet<String> = HashSet::with_capacity(fresh.len());
    let mut changed = Vec::new();
    for p in fresh {
        let key = p.key();
        if !seen.insert(key.clone()) {
            continue;
        }
        match old.get(&key) {
            Some(o) if same(o, p) => {}
            _ => changed.push(p.clone()),
        }
    }
    let removed = old.keys().filter(|k| !seen.contains(*k)).cloned().collect();
    (changed, removed)
}

/// True when nothing a user can see differs between the two packages.
fn same(a: &Package, b: &Package) -> bool {
    a.title == b.title
        && a.candidate == b.candidate
        && a.installed == b.installed
        && a.download_size == b.download_size
        && a.installed_size == b.installed_size
        && a.section == b.section
        && a.origin == b.origin
        && a.scope == b.scope
        && a.description == b.description
        && a.upgradable == b.upgradable
        && a.icon == b.icon
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, candidate: &str) -> Package {
        let mut p = Package::new(Source::Npm, name);
        p.candidate = candidate.to_string();
        p
    }

    #[test]
    fn diff_finds_only_new_changed_and_removed() {
        let old: HashMap<String, Package> = [pkg("a", "1"), pkg("b", "1"), pkg("c", "1")]
            .into_iter()
            .map(|p| (p.key(), p))
            .collect();
        let fresh = vec![pkg("a", "1"), pkg("b", "2"), pkg("d", "1")];
        let (changed, removed) = diff(&old, &fresh);
        let names: Vec<&str> = changed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["b", "d"]);
        assert_eq!(removed, vec!["npm:c".to_string()]);
    }

    #[test]
    fn diff_ignores_duplicate_keys_in_fresh_list() {
        let (changed, removed) = diff(&HashMap::new(), &[pkg("x", "1"), pkg("x", "2")]);
        assert_eq!(changed.len(), 1);
        assert!(removed.is_empty());
    }
}
