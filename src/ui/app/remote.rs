//! remote for the main window (part of `AppView`).

use super::*;
use std::collections::HashSet;
use crate::model::Source;
use crate::search;

impl AppView {
    /// Ask the npm, Bun and Cargo registries for the submitted search, in the background.
    /// Their results replace the previous online results. Results that come back after
    /// the search has changed are dropped.
    pub(crate) fn start_remote_search(&mut self, cx: &mut Context<Self>) {
        let query = self.submitted.clone().unwrap_or_default().trim().to_string();
        self.packages.retain(|p| !p.online);
        let sources: Vec<Source> = [Source::Npm, Source::Bun, Source::Cargo]
            .into_iter()
            .filter(|s| self.is_on(*s) && self.registries.contains(s))
            .collect();
        if query.is_empty() || sources.is_empty() {
            self.refilter();
            cx.notify();
            return;
        }
        let labels: Vec<&str> = sources.iter().map(|s| s.label()).collect();
        self.status = format!("Searching {} for '{query}'…", labels.join(", "));
        self.refilter();
        cx.notify();

        let asked = query.clone();
        cx.spawn(async move |this, cx| {
            let (results, problems) = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::new();
                    let mut problems = Vec::new();
                    for src in sources {
                        match search::search(src, &query) {
                            Ok(list) => results.extend(list),
                            Err(e) => problems.push(format!("{}: {e}", src.label())),
                        }
                    }
                    (results, problems)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.submitted.clone().unwrap_or_default().trim() != asked {
                    return;
                }
                let known: HashSet<String> = this.packages.iter().map(|p| p.key()).collect();
                let mut added = HashSet::new();
                for p in results {
                    if !known.contains(&p.key()) && added.insert(p.key()) {
                        this.packages.push(p);
                    }
                }
                let found = this.packages.iter().filter(|p| p.online).count();
                this.status = if problems.is_empty() {
                    format!("{found} online result(s) for '{asked}'")
                } else {
                    format!("{found} online result(s); {}", problems.join("; "))
                };
                this.refilter();
                cx.notify();
            });
        })
        .detach();
    }
}
