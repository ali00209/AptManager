//! Which package managers the user wants the app to manage.
//!
//! Stored as plain text in `~/.config/apt-manager/managers.conf`, one `tag=on` or
//! `tag=off` per line. Only managers that were found on the machine are written, so
//! a manager that is installed later is on by default. Managers with no line follow
//! whether they were found.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::model::Source;

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        });
    base.join("apt-manager").join("managers.conf")
}

/// The explicit on/off choices saved so far. Managers without an entry are not in the map.
pub fn load() -> HashMap<Source, bool> {
    let Ok(text) = fs::read_to_string(path()) else {
        return HashMap::new();
    };
    let mut choices = HashMap::new();
    for line in text.lines() {
        let Some((tag, state)) = line.trim().split_once('=') else {
            continue;
        };
        if let Some(src) = Source::from_tag(tag.trim()) {
            choices.insert(src, state.trim() == "on");
        }
    }
    choices
}

/// Save the choices for every manager that was found, plus any that are switched on.
pub fn save(enabled: &HashSet<Source>, detected: &HashMap<Source, bool>) -> Result<(), String> {
    let file = path();
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    }
    let mut text = String::from("# apt-manager: managers to show (on) or hide (off)\n");
    for src in Source::ALL {
        if !(detected.get(&src).copied().unwrap_or(false) || enabled.contains(&src)) {
            continue;
        }
        let state = if enabled.contains(&src) { "on" } else { "off" };
        text.push_str(&format!("{}={state}\n", src.tag()));
    }
    fs::write(&file, text).map_err(|e| format!("Cannot save {}: {e}", file.display()))
}
