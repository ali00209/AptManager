//! Loading the package lists in the background, and running a batch of installs.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::{ActionResult, Package, Source};
use crate::{apt, appimage, flatpak, managers, prefs};

/// Everything a background load produces.
pub(crate) struct Loaded {
    pub packages: Vec<Package>,
    pub detected: HashMap<Source, bool>,
    pub paths: HashMap<Source, String>,
    pub enabled: HashSet<Source>,
    pub status: String,
}

pub(crate) type FlatpakGroups = Vec<(String, Vec<String>)>;

/// Load every enabled and found manager. A manager that fails is reported in the
/// status line and the others still load.
pub(crate) fn load_all(override_enabled: Option<HashSet<Source>>) -> Loaded {
    let paths: HashMap<Source, String> = Source::ALL
        .into_iter()
        .filter_map(|s| managers::locate(s).map(|p| (s, p)))
        .collect();
    let detected: HashMap<Source, bool> = Source::ALL
        .into_iter()
        .map(|s| (s, paths.contains_key(&s)))
        .collect();

    // A manager with a saved choice uses it. Otherwise it is on when it was found.
    let enabled: HashSet<Source> = match override_enabled {
        Some(set) => set,
        None => {
            let choices = prefs::load();
            Source::ALL
                .into_iter()
                .filter(|s| choices.get(s).copied().unwrap_or(detected[s]))
                .collect()
        }
    };

    let mut packages = Vec::new();
    let mut notes = Vec::new();
    for src in Source::ALL {
        if !enabled.contains(&src) || !detected[&src] {
            continue;
        }
        let result = match src {
            Source::Apt => apt::load_catalog(),
            Source::Flatpak => flatpak::load(),
            Source::AppImage => Ok(appimage::load()),
            other => managers::load(other).map(|mut list| {
                let newer = managers::outdated(other);
                for p in &mut list {
                    if let Some(latest) = newer.get(&p.name) {
                        p.upgradable = true;
                        if !latest.is_empty() {
                            p.candidate = latest.clone();
                        }
                    }
                }
                list
            }),
        };
        match result {
            Ok(list) => packages.extend(list),
            Err(e) => notes.push(format!("{}: {e}", src.label())),
        }
    }

    packages.sort_by(|a, b| {
        (a.source, a.title.to_lowercase()).cmp(&(b.source, b.title.to_lowercase()))
    });

    let parts: Vec<String> = Source::ALL
        .into_iter()
        .filter(|s| enabled.contains(s) && detected[s])
        .map(|s| {
            let n = packages.iter().filter(|p| p.source == s).count();
            format!("{n} {}", s.label())
        })
        .collect();
    let mut status = if parts.is_empty() {
        "No package managers are on. Open Preferences to turn some on.".to_string()
    } else {
        parts.join(" · ")
    };
    if !notes.is_empty() {
        status.push_str(" — ");
        status.push_str(&notes.join("; "));
    }

    Loaded {
        packages,
        detected,
        paths,
        enabled,
        status,
    }
}

/// Split a batch of picks into APT names, Flatpak IDs grouped by remote, and
/// (manager, name) pairs for everything else.
pub(crate) fn group_for_install(picks: &[&Package]) -> (Vec<String>, FlatpakGroups, Vec<(Source, String)>) {
    let mut apt_names = Vec::new();
    let mut flatpak: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut others = Vec::new();
    for pkg in picks {
        match pkg.source {
            Source::Apt => apt_names.push(pkg.name.clone()),
            Source::Flatpak => flatpak
                .entry(pkg.origin.clone())
                .or_default()
                .push(pkg.name.clone()),
            Source::AppImage => {}
            other => others.push((other, pkg.name.clone())),
        }
    }
    (apt_names, flatpak.into_iter().collect(), others)
}

/// Run APT installs, then one Flatpak install per remote, then the other managers.
/// Stops at the first failure.
pub(crate) fn install_batch(
    apt_names: Vec<String>,
    flatpak_groups: FlatpakGroups,
    others: Vec<(Source, String)>,
) -> ActionResult {
    if !apt_names.is_empty() {
        let r = apt::install(&apt_names);
        if !r.success {
            return r;
        }
    }
    for (origin, ids) in flatpak_groups {
        let r = flatpak::install(&origin, &ids);
        if !r.success {
            return r;
        }
    }
    for (src, name) in others {
        let r = managers::install(src, &name);
        if !r.success {
            return r;
        }
    }
    ActionResult::ok("Selected packages installed.")
}
