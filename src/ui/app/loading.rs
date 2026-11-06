//! loading for the main window (part of `AppView`).

use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Instant;
use crate::model::Source;
use crate::load::{self, Event};
use crate::track;

impl AppView {
    /// Read the ticked registries in the background, each on its own thread. With
    /// `show_cache`, the cached packages appear at once.
    pub(crate) fn start_loading(&mut self, show_cache: bool, cx: &mut Context<Self>) {
        self.status = "Reading packages…".into();
        for src in self.active_registries() {
            self.begin_load(src, show_cache);
        }
        cx.notify();
    }

    /// Read the given registries again in the background. Each one reports its changes
    /// as it finishes, and the others are not affected.
    pub(crate) fn refresh_registries(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        for src in sources {
            if self.is_on(src) {
                self.begin_load(src, false);
            }
        }
        cx.notify();
    }

    /// Take the events the registry threads have reported and apply them to the list.
    /// Returns true when the list changed.
    pub(crate) fn drain_events(&mut self) -> bool {
        let events: Vec<Event> = match self.hub.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            Err(_) => return false,
        };
        if events.is_empty() {
            return false;
        }
        for event in events {
            match event {
                Event::Cached { source, packages } => {
                    if !self.is_on(source) {
                        continue;
                    }
                    self.packages.retain(|p| p.source != source || p.online);
                    self.packages.extend(packages);
                    self.loaded = true;
                }
                Event::Changed { source, changed, removed } => {
                    if !self.is_on(source) {
                        continue;
                    }
                    let removed: HashSet<String> = removed.into_iter().collect();
                    self.packages
                        .retain(|p| p.source != source || p.online || !removed.contains(&p.key()));
                    let mut at: HashMap<String, usize> = self
                        .packages
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.source == source && !p.online)
                        .map(|(i, p)| (p.key(), i))
                        .collect();
                    for p in changed {
                        match at.get(&p.key()) {
                            Some(&i) => self.packages[i] = p,
                            None => {
                                at.insert(p.key(), self.packages.len());
                                self.packages.push(p);
                            }
                        }
                    }
                    self.loaded = true;
                }
                Event::Progress { source, text } => {
                    if let Some(load) = self.loads.get_mut(&source) {
                        load.text = text;
                    }
                }
                Event::Finished { source, ok, text } => {
                    self.loaded = true;
                    if let Some(load) = self.loads.get_mut(&source) {
                        load.finished = Some((ok, text, Instant::now()));
                    }
                    if !self.any_loading() {
                        self.status = "Up to date".into();
                    }
                }
            }
        }
        true
    }

    /// Start one registry's load on its own thread, unless it is already loading.
    pub(crate) fn begin_load(&mut self, src: Source, show_cache: bool) {
        if self.is_loading(src) {
            return;
        }
        let handle = load::start(src, self.hub.clone(), show_cache);
        self.loads.insert(
            src,
            LoadUi {
                started: Instant::now(),
                text: "Starting…".into(),
                handle,
                finished: None,
            },
        );
    }

    pub(crate) fn is_loading(&self, src: Source) -> bool {
        self.loads.get(&src).is_some_and(|l| l.finished.is_none())
    }

    pub(crate) fn any_loading(&self) -> bool {
        self.loads.values().any(|l| l.finished.is_none())
    }

    /// Stop one registry's load. Its commands are terminated; the cache is left as it was.
    pub(crate) fn stop_load(&mut self, src: Source) {
        if let Some(load) = self.loads.get_mut(&src) {
            if load.finished.is_none() {
                load.handle.cancel.store(true, Ordering::SeqCst);
                track::kill_all(&load.handle.sink);
                load.text = "Cancelling…".into();
            }
        }
    }

    pub(crate) fn cancel_load(&mut self, src: Source, cx: &mut Context<Self>) {
        self.stop_load(src);
        cx.notify();
    }

    /// Remove a finished load's row from the panel.
    pub(crate) fn dismiss_load(&mut self, src: Source, cx: &mut Context<Self>) {
        if self.loads.get(&src).is_some_and(|l| l.finished.is_some()) {
            self.loads.remove(&src);
        }
        cx.notify();
    }
}
