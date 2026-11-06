//! registries for the main window (part of `AppView`).

use super::*;
use std::collections::HashSet;
use crate::model::Source;
use crate::prefs;

impl AppView {
    // ----- state helpers ----------------------------------------------------

    /// True when a manager is switched on and its command was found.
    pub(crate) fn is_on(&self, src: Source) -> bool {
        self.enabled.contains(&src) && self.detected.get(&src).copied().unwrap_or(false)
    }

    /// Managers that can be searched: switched on and found on this machine.
    pub(crate) fn active_registries(&self) -> Vec<Source> {
        Source::ALL.into_iter().filter(|&s| self.is_on(s)).collect()
    }

    /// Keep the ticked registries in step with the managers that are on. On the first
    /// load every registry starts ticked.
    pub(crate) fn sync_registries(&mut self, first_load: bool) {
        let on: HashSet<Source> = self.active_registries().into_iter().collect();
        self.registries.retain(|s| on.contains(s));
        if first_load {
            self.registries = on;
        }
    }

    pub(crate) fn toggle_registry(&mut self, src: Source, cx: &mut Context<Self>) {
        if !self.registries.remove(&src) {
            self.registries.insert(src);
            if is_online(src) {
                self.start_remote_search(cx);
                return;
            }
        }
        self.refilter();
        cx.notify();
    }

    pub(crate) fn set_all_registries(&mut self, all: bool, cx: &mut Context<Self>) {
        self.registries = if all {
            self.active_registries().into_iter().collect()
        } else {
            HashSet::new()
        };
        if all {
            self.start_remote_search(cx);
        } else {
            self.refilter();
            cx.notify();
        }
    }

    pub(crate) fn toggle_registry_menu(&mut self, cx: &mut Context<Self>) {
        self.registry_menu = !self.registry_menu;
        self.state_menu = false;
        cx.notify();
    }

    /// Switch a manager on or off and save the choice. Switching on reads that registry
    /// from its cache first. Switching off hides its packages; its cache stays on disk.
    pub(crate) fn set_enabled(&mut self, src: Source, on: bool, cx: &mut Context<Self>) {
        if on {
            self.enabled.insert(src);
            self.registries.insert(src);
            if self.detected.get(&src).copied().unwrap_or(false) {
                self.begin_load(src, true);
            }
        } else {
            self.enabled.remove(&src);
            self.registries.remove(&src);
            self.packages.retain(|p| p.source != src || p.online);
            self.stop_load(src);
            self.loads.remove(&src);
            self.refilter();
        }
        if let Err(e) = prefs::save(&self.enabled, &self.detected) {
            self.status = e;
        }
        self.show_history = false;
        cx.notify();
    }
}
