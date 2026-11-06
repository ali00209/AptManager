//! checks for the main window (part of `AppView`).

use super::*;
use std::collections::{HashMap, HashSet};
use crate::model::{Package, Source};
use crate::cache;
use crate::managers;

impl AppView {
    /// Read the state of only the packages an action touched, in the background. Their
    /// rows and the cache are updated; no other package is read again.
    pub(crate) fn recheck(&mut self, keys: Vec<(Source, String)>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    keys.into_iter()
                        .map(|(src, name)| {
                            let check = managers::check_one(src, &name);
                            (src, name, check)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| this.apply_checks(results, cx));
        })
        .detach();
    }

    /// Put the re-read state of each touched package into the list and the cache.
    /// A manager with no single-package check is read again as a whole, but only that one.
    pub(crate) fn apply_checks(
        &mut self,
        results: Vec<(Source, String, Option<managers::Check>)>,
        cx: &mut Context<Self>,
    ) {
        let mut at: HashMap<String, usize> = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.online)
            .map(|(i, p)| (p.key(), i))
            .collect();
        let mut drop_keys: HashSet<String> = HashSet::new();
        let mut cached: HashMap<Source, (Vec<Package>, Vec<String>)> = HashMap::new();
        let mut needs_read: Vec<Source> = Vec::new();
        for (src, name, check) in results {
            let Some(check) = check else {
                if !needs_read.contains(&src) {
                    needs_read.push(src);
                }
                continue;
            };
            let key = format!("{}:{name}", src.tag());
            match check.installed {
                Some(version) => {
                    let candidate = check.candidate.unwrap_or_else(|| version.clone());
                    let mut p = match at.get(&key) {
                        Some(&i) => self.packages[i].clone(),
                        None => Package::new(src, &name),
                    };
                    p.installed = Some(version);
                    p.candidate = candidate;
                    p.upgradable = false;
                    p.online = false;
                    match at.get(&key) {
                        Some(&i) => self.packages[i] = p.clone(),
                        None => {
                            at.insert(key.clone(), self.packages.len());
                            self.packages.push(p.clone());
                        }
                    }
                    cached.entry(src).or_default().0.push(p);
                }
                None if src.has_catalogue() => {
                    if let Some(&i) = at.get(&key) {
                        let p = &mut self.packages[i];
                        p.installed = None;
                        p.upgradable = false;
                        if let Some(candidate) = check.candidate {
                            p.candidate = candidate;
                        }
                        cached.entry(src).or_default().0.push(p.clone());
                    }
                }
                None => {
                    // Not installed, and there is no catalogue to keep it in: it leaves the list.
                    drop_keys.insert(key.clone());
                    cached.entry(src).or_default().1.push(key);
                }
            }
        }
        self.packages
            .retain(|p| p.online || !drop_keys.contains(&p.key()));
        for (src, (changed, removed)) in cached {
            let _ = cache::append(src, &changed, &removed);
        }
        self.refilter();
        if !needs_read.is_empty() {
            self.refresh_registries(needs_read, cx);
        }
        cx.notify();
    }
}
