//! filter for the main window (part of `AppView`).

use super::*;
use std::collections::HashSet;
use crate::model::{Package, Source};

impl AppView {
    /// Work out which packages to list. Nothing is listed until a search is submitted,
    /// or until "Has updates" is on. Results come only from the ticked registries and are
    /// ordered by relevance, then title.
    pub(crate) fn refilter(&mut self) {
        let submitted = self.submitted.clone().unwrap_or_default();
        let query = submitted.trim().to_lowercase();
        let searching = !query.is_empty();
        let mut hits: Vec<(u8, String, usize)> = Vec::new();
        for (idx, pkg) in self.packages.iter().enumerate() {
            if !self.registries.contains(&pkg.source) {
                continue;
            }
            if self.updates_only && !pkg.upgradable {
                continue;
            }
            if !state_allows(pkg.is_installed(), &self.state_filter, searching, self.updates_only) {
                continue;
            }
            if let Some(score) = relevance(pkg, &query) {
                hits.push((score, pkg.title.to_lowercase(), idx));
            }
        }
        hits.sort();
        self.matched = hits.len();
        hits.truncate(LIST_LIMIT);
        self.visible = hits.into_iter().map(|(_, _, idx)| idx).collect();
        // Ticks on packages the filter hides are dropped, so Install and Update Selected only
        // act on what the list shows.
        let listed: HashSet<String> = self.visible.iter().map(|&i| self.packages[i].key()).collect();
        self.checked.retain(|key| listed.contains(key));
    }

    /// Packages with an update that are in a ticked registry.
    pub(crate) fn ticked_updates(&self) -> usize {
        self.packages
            .iter()
            .filter(|p| p.upgradable && p.is_installed() && self.registries.contains(&p.source))
            .count()
    }

    pub(crate) fn toggle_state_menu(&mut self, cx: &mut Context<Self>) {
        self.state_menu = !self.state_menu;
        self.registry_menu = false;
        cx.notify();
    }

    /// Tick or untick one install state, then filter the list again.
    pub(crate) fn toggle_state(&mut self, state: StateFilter, cx: &mut Context<Self>) {
        if !self.state_filter.remove(&state) {
            self.state_filter.insert(state);
        }
        self.refilter();
        cx.notify();
    }

    /// Tick both install states (All) or neither (None).
    pub(crate) fn set_all_states(&mut self, all: bool, cx: &mut Context<Self>) {
        self.state_filter = if all {
            [StateFilter::Installed, StateFilter::NotInstalled].into_iter().collect()
        } else {
            HashSet::new()
        };
        self.refilter();
        cx.notify();
    }

    pub(crate) fn toggle_flatpak_scope(&mut self, cx: &mut Context<Self>) {
        self.flatpak_for_all = !self.flatpak_for_all;
        cx.notify();
    }

    pub(crate) fn toggle_updates_only(&mut self, cx: &mut Context<Self>) {
        self.updates_only = !self.updates_only;
        self.refilter();
        cx.notify();
    }

    /// Packages with a newer version waiting, from one source.
    pub(crate) fn updates_for(&self, source: Source) -> usize {
        self.packages
            .iter()
            .filter(|p| p.source == source && p.upgradable)
            .count()
    }

    pub(crate) fn total_updates(&self) -> usize {
        Source::ALL.into_iter().map(|s| self.updates_for(s)).sum()
    }

    pub(crate) fn find_package(&self, key: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.key() == key)
    }

    pub(crate) fn toggle_checked(&mut self, key: &str) {
        if !self.checked.remove(key) {
            self.checked.insert(key.to_string());
        }
    }

    /// Ticked packages that have an update waiting.
    pub(crate) fn ticked_to_update(&self) -> Vec<&Package> {
        self.packages
            .iter()
            .filter(|p| self.checked.contains(&p.key()) && p.upgradable && p.is_installed())
            .collect()
    }

    /// Packages ticked and not yet installed. AppImages are never installed from here.
    pub(crate) fn ticked_to_install(&self) -> Vec<&Package> {
        self.packages
            .iter()
            .filter(|p| {
                self.checked.contains(&p.key()) && !p.is_installed() && p.source != Source::AppImage
            })
            .collect()
    }
}

/// Whether a package is listed for its install state. With states ticked, only those states
/// are listed. With none ticked, installed packages are listed, and every package when there
/// is a search or the updates filter.
pub(crate) fn state_allows(
    is_installed: bool,
    states: &HashSet<StateFilter>,
    searching: bool,
    updates_only: bool,
) -> bool {
    let want_installed = states.contains(&StateFilter::Installed);
    let want_missing = states.contains(&StateFilter::NotInstalled);
    if want_installed || want_missing {
        (want_installed && is_installed) || (want_missing && !is_installed)
    } else {
        searching || updates_only || is_installed
    }
}

#[cfg(test)]
mod tests {
    use super::super::relevance;
    use super::{state_allows, StateFilter};
    use crate::model::{Package, Source};
    use std::collections::HashSet;

    fn states(list: &[StateFilter]) -> HashSet<StateFilter> {
        list.iter().copied().collect()
    }

    #[test]
    fn with_no_state_ticked_the_list_shows_installed_packages_only() {
        let none = states(&[]);
        assert!(state_allows(true, &none, false, false));
        assert!(!state_allows(false, &none, false, false));
    }

    #[test]
    fn with_no_state_ticked_a_search_or_the_updates_filter_shows_everything() {
        let none = states(&[]);
        assert!(state_allows(false, &none, true, false));
        assert!(state_allows(false, &none, false, true));
    }

    #[test]
    fn a_ticked_state_is_a_strict_filter_even_during_a_search() {
        let installed = states(&[StateFilter::Installed]);
        assert!(state_allows(true, &installed, true, false));
        assert!(!state_allows(false, &installed, true, false));

        let missing = states(&[StateFilter::NotInstalled]);
        assert!(!state_allows(true, &missing, true, false));
        assert!(state_allows(false, &missing, true, false));
    }

    #[test]
    fn ticking_both_states_shows_everything() {
        let both = states(&[StateFilter::Installed, StateFilter::NotInstalled]);
        assert!(state_allows(true, &both, false, false));
        assert!(state_allows(false, &both, false, false));
    }

    #[test]
    fn exact_matches_rank_before_prefixes_contains_and_description() {
        let mut exact = Package::new(Source::Apt, "gimp");
        let mut prefix = Package::new(Source::Apt, "gimp-data");
        let mut contains = Package::new(Source::Apt, "xgimpx");
        let mut described = Package::new(Source::Apt, "paint");
        for p in [&mut exact, &mut prefix, &mut contains, &mut described] {
            p.title = p.name.clone();
        }
        described.description = "an image editor called gimp".into();

        let q = "gimp";
        assert_eq!(relevance(&exact, q), Some(0));
        assert_eq!(relevance(&prefix, q), Some(1));
        assert_eq!(relevance(&contains, q), Some(2));
        assert_eq!(relevance(&described, q), Some(3));
        assert!(relevance(&Package::new(Source::Apt, "vim"), q).is_none());
    }
}
