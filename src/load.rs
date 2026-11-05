//! Background loading. Each registry is read on its own thread, so registries run in
//! parallel and never wait for each other. Each thread reports its phase as it goes, and
//! can be cancelled on its own. The cache is shown first. A live check then finds what
//! changed, and only those packages are sent to the window and written to the cache.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::model::{Package, Source};
use crate::track::{self, Sink};
use crate::{appimage, apt, cache, flatpak, managers};

/// What a registry's thread reports to the window.
pub enum Event {
    /// The cached packages, shown at once. Replaces that registry's installed packages.
    Cached { source: Source, packages: Vec<Package> },
    /// The packages that are new or changed, and the keys of those that have gone.
    Changed { source: Source, changed: Vec<Package>, removed: Vec<String> },
    /// What the registry's thread is doing now.
    Progress { source: Source, text: String },
    /// The thread has ended. `ok` is false when it failed or was cancelled.
    Finished { source: Source, ok: bool, text: String },
}

/// Events waiting for the window. Each registry's thread adds to it; the window takes them all.
pub type Hub = Arc<Mutex<VecDeque<Event>>>;

pub fn new_hub() -> Hub {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// How the window stops one registry's thread.
pub struct Handle {
    pub cancel: Arc<AtomicBool>,
    pub sink: Sink,
}

fn push(hub: &Hub, event: Event) {
    if let Ok(mut queue) = hub.lock() {
        queue.push_back(event);
    }
}

/// Start reading one registry on its own thread. With `show_cache`, the cached packages
/// are sent first, so the list has something to show while the live check runs.
pub fn start(src: Source, hub: Hub, show_cache: bool) -> Handle {
    let cancel = Arc::new(AtomicBool::new(false));
    let sink = track::new_sink();
    let handle = Handle {
        cancel: cancel.clone(),
        sink: sink.clone(),
    };
    thread::spawn(move || {
        track::bind(sink);
        let report = |text: &str| push(&hub, Event::Progress { source: src, text: text.to_string() });
        let end = |ok: bool, text: String| push(&hub, Event::Finished { source: src, ok, text });
        let stopped = || cancel.load(Ordering::SeqCst);
        const CANCELLED: &str = "Cancelled. The cache is unchanged.";

        report("Reading cache…");
        let cached = cache::load(src);
        if show_cache && !cached.is_empty() {
            let packages = cached.values().cloned().collect();
            push(&hub, Event::Cached { source: src, packages });
        }
        if stopped() {
            return end(false, CANCELLED.into());
        }

        report("Checking installed packages…");
        let fresh = match fetch(src) {
            Ok(list) => list,
            Err(message) => return end(false, message),
        };
        if stopped() {
            return end(false, CANCELLED.into());
        }
        // A command that printed nothing must not wipe a cache that has packages in it.
        if fresh.is_empty() && !cached.is_empty() {
            return end(false, "Nothing was returned. The cache is kept.".into());
        }

        report(&format!("Comparing {} packages…", fresh.len()));
        let (changed, removed) = cache::diff(&cached, &fresh);
        let summary = format!(
            "{} packages · {} changed · {} removed",
            fresh.len(),
            changed.len(),
            removed.len()
        );
        if stopped() {
            return end(false, CANCELLED.into());
        }
        report("Saving changes…");
        if cached.is_empty() || cache::log_is_large(src) {
            let _ = cache::save_snapshot(src, &fresh);
        } else {
            let _ = cache::append(src, &changed, &removed);
        }
        if !changed.is_empty() || !removed.is_empty() {
            push(&hub, Event::Changed { source: src, changed, removed });
        }
        end(true, summary);
    });
    handle
}

/// Read every package one registry has right now.
pub fn fetch(src: Source) -> Result<Vec<Package>, String> {
    match src {
        Source::Apt => apt::load_catalog(),
        Source::Flatpak => flatpak::load(),
        Source::AppImage => {
            let list = appimage::load();
            appimage::sync_desktop_entries(&list);
            Ok(list)
        }
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
    }
}
