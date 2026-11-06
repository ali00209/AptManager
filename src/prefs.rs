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

fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        });
    base.join("apt-manager")
}

fn path() -> PathBuf {
    config_dir().join("managers.conf")
}

fn ui_path() -> PathBuf {
    config_dir().join("ui.conf")
}

/// Whether the details panel is shown. It is shown unless the saved choice says `details=off`.
pub fn details_shown() -> bool {
    parse_details(&fs::read_to_string(ui_path()).unwrap_or_default())
}

pub fn parse_details(text: &str) -> bool {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("details="))
        .map(|v| v.trim() != "off")
        .unwrap_or(true)
}

/// Save whether the details panel is shown.
pub fn save_details_shown(shown: bool) -> Result<(), String> {
    let file = ui_path();
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    }
    let text = format!(
        "# apt-manager: interface settings\ndetails={}\n",
        if shown { "on" } else { "off" }
    );
    fs::write(&file, text).map_err(|e| format!("Cannot save {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::parse_details;

    #[test]
    fn details_are_shown_by_default_and_when_saved_on() {
        assert!(parse_details(""));
        assert!(parse_details("details=on\n"));
        assert!(parse_details("# note\n  details=on  \n"));
    }

    #[test]
    fn details_stay_hidden_when_saved_off() {
        assert!(!parse_details("details=off\n"));
        assert!(!parse_details("# apt-manager: interface settings\ndetails=off\n"));
    }
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
