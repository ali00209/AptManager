//! Startup detection: which managers are installed, and which are switched on.
//! The package lists themselves are loaded in the background (see `load.rs`).

use std::collections::{HashMap, HashSet};

use crate::model::Source;
use crate::{managers, prefs};

/// What was found on this machine at startup.
pub(crate) struct Detection {
    pub detected: HashMap<Source, bool>,
    pub paths: HashMap<Source, String>,
    pub enabled: HashSet<Source>,
}

/// Find every manager's command. A manager is on when its saved choice says so, or when
/// it was found and no choice is saved.
pub(crate) fn detect() -> Detection {
    let paths: HashMap<Source, String> = Source::ALL
        .into_iter()
        .filter_map(|s| managers::locate(s).map(|p| (s, p)))
        .collect();
    let detected: HashMap<Source, bool> = Source::ALL
        .into_iter()
        .map(|s| (s, paths.contains_key(&s)))
        .collect();
    let choices = prefs::load();
    let enabled: HashSet<Source> = Source::ALL
        .into_iter()
        .filter(|s| choices.get(s).copied().unwrap_or(detected[s]))
        .collect();
    Detection {
        detected,
        paths,
        enabled,
    }
}
