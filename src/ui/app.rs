//! The window's state and actions, and the top-level layout that ties the screens together.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{ActionResult, Package, Source};
use crate::search;
use crate::transaction::{self, Step};
use crate::ui::loader::detect;
use crate::load::{self, Event, Hub};
use crate::track;
use crate::cache;
use crate::ui::theme::*;
use crate::{apt, appimage, flatpak, job, managers, plan, prefs};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Page {
    Browse,
    Preferences,
    Sources(Source),
}

/// Install states the list can be limited to. With neither ticked, the list shows
/// installed packages, or every match when there is a search.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StateFilter {
    Installed,
    NotInstalled,
}

/// Most rows the list shows for one search. The rest are counted, not drawn.
const LIST_LIMIT: usize = 500;

pub struct AppView {
    pub(crate) packages: Vec<Package>,
    /// Indices into `packages` that pass the current tab, filter and search.
    pub(crate) visible: Vec<usize>,
    pub(crate) search: Entity<InputState>,
    /// The search as last submitted with Enter. `None` until the first search.
    pub(crate) submitted: Option<String>,
    /// Registries ticked in the registry dropdown. Only these are searched.
    pub(crate) registries: HashSet<Source>,
    /// Whether the registry dropdown is open.
    pub(crate) registry_menu: bool,
    /// Whether the install-state dropdown is open.
    pub(crate) state_menu: bool,
    /// The install states ticked in the dropdown.
    pub(crate) state_filter: HashSet<StateFilter>,
    /// Flatpak installs go system-wide (asks for the password) instead of per user.
    pub(crate) flatpak_for_all: bool,
    /// Show only packages that have an update.
    pub(crate) updates_only: bool,
    /// How many packages matched before the list was capped.
    pub(crate) matched: usize,
    pub(crate) grid: bool,
    /// Keys (see `Package::key`) of ticked rows.
    pub(crate) checked: HashSet<String>,
    /// Key of the row shown in the details panel.
    pub(crate) selected: Option<String>,
    pub(crate) show_history: bool,
    pub(crate) history: Vec<apt::HistoryEntry>,
    pub(crate) status: String,
    pub(crate) loaded: bool,
    pub(crate) page: Page,
    /// Whether each manager's command was found on this machine.
    pub(crate) detected: HashMap<Source, bool>,
    /// Full path of each manager's command, where it was found.
    pub(crate) paths: HashMap<Source, String>,
    /// Managers the user has switched on.
    pub(crate) enabled: HashSet<Source>,
    pub(crate) prefs_search: Entity<InputState>,
    pub(crate) prefs_query: String,
    pub(crate) source_input: Entity<InputState>,
    /// Lines shown on the sources page for the manager that was opened.
    pub(crate) source_lines: Vec<String>,
    /// The job that is running or has just finished, shown above the status bar.
    /// Background jobs that are running or have finished and not yet been dismissed.
    pub(crate) jobs: Vec<JobUi>,
    /// Packages that a running job is changing. Another job cannot touch them.
    pub(crate) locked: HashSet<String>,
    pub(crate) next_job: u64,
    /// Where the registries' threads report their packages.
    pub(crate) hub: Hub,
    /// One load per registry: its progress, and its result once it has ended.
    pub(crate) loads: HashMap<Source, LoadUi>,
    _subscriptions: Vec<Subscription>,
}

/// One registry's load, as the window shows it.
pub(crate) struct LoadUi {
    pub(crate) started: Instant,
    /// What the thread is doing now.
    pub(crate) text: String,
    pub(crate) handle: load::Handle,
    /// Set once the thread has ended: whether it succeeded, what it says, and when.
    pub(crate) finished: Option<(bool, String, Instant)>,
}

/// One background job, as the window lists it.
pub(crate) struct JobUi {
    pub(crate) id: u64,
    pub(crate) handle: job::Handle,
}

/// What to do once a job succeeds. Only the packages the job touched are re-read, unless a
/// whole registry has to be read again.
pub(crate) enum After {
    Recheck(Vec<(Source, String)>),
    Refresh(Vec<Source>),
}

/// Registries whose results come from an online search.
fn job_done(handle: &job::Handle) -> bool {
    handle.lock().map(|p| p.done).unwrap_or(true)
}

fn is_online(src: Source) -> bool {
    matches!(src, Source::Npm | Source::Bun | Source::Cargo)
}

/// How well a package matches the search: 0 exact, 1 prefix, 2 contains, 3 description.
/// `None` when it does not match. An empty query matches everything at 2.
fn relevance(pkg: &Package, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(2);
    }
    let name = pkg.name.to_lowercase();
    let title = pkg.title.to_lowercase();
    if name == query || title == query {
        Some(0)
    } else if name.starts_with(query) || title.starts_with(query) {
        Some(1)
    } else if name.contains(query) || title.contains(query) {
        Some(2)
    } else if pkg.description.to_lowercase().contains(query) {
        Some(3)
    } else {
        None
    }
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search packages..."));
        let search_sub = cx.subscribe(
            &search,
            |this: &mut AppView, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<AppView>| {
                match event {
                    // Search runs on Enter, not on every key stroke.
                    InputEvent::PressEnter { .. } => {
                        this.submitted = Some(state.read(cx).value().trim().to_string());
                        this.start_remote_search(cx);
                    }
                    // Clearing the box goes back to the installed list at once.
                    InputEvent::Change
                        if state.read(cx).value().trim().is_empty()
                            && this.submitted.as_deref().is_some_and(|s| !s.is_empty()) =>
                    {
                        this.submitted = Some(String::new());
                        this.start_remote_search(cx);
                        cx.notify();
                    }
                    _ => {}
                }
            },
        );

        let prefs_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search package managers"));
        let prefs_sub = cx.subscribe(
            &prefs_search,
            |this: &mut AppView, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<AppView>| {
                if let InputEvent::Change = event {
                    this.prefs_query = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );

        let source_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Remote name and URL, e.g. flathub https://dl.flathub.org/repo/flathub.flatpakrepo")
        });

        let mut this = AppView {
            packages: Vec::new(),
            visible: Vec::new(),
            search,
            submitted: None,
            registries: HashSet::new(),
            registry_menu: false,
            state_menu: false,
            state_filter: HashSet::new(),
            flatpak_for_all: false,
            updates_only: false,
            matched: 0,
            grid: false,
            checked: HashSet::new(),
            selected: None,
            show_history: false,
            history: Vec::new(),
            status: "Loading packages…".into(),
            loaded: false,
            page: Page::Browse,
            detected: HashMap::new(),
            paths: HashMap::new(),
            enabled: HashSet::new(),
            prefs_search,
            prefs_query: String::new(),
            source_input,
            source_lines: Vec::new(),
            jobs: Vec::new(),
            locked: HashSet::new(),
            next_job: 0,
            hub: load::new_hub(),
            loads: HashMap::new(),
            _subscriptions: vec![search_sub, prefs_sub],
        };
        let detection = detect();
        this.detected = detection.detected;
        this.paths = detection.paths;
        this.enabled = detection.enabled;
        this.sync_registries(true);
        this.start_loading(true, cx);
        this.start_ticker(cx);
        this
    }

    /// Read the ticked registries in the background, each on its own thread. With
    /// `show_cache`, the cached packages appear at once.
    fn start_loading(&mut self, show_cache: bool, cx: &mut Context<Self>) {
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
    fn drain_events(&mut self) -> bool {
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

    /// Apply the reported events every 150 ms. The list only redraws when something changed,
    /// or while a load is running, so its elapsed time keeps moving.
    fn start_ticker(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let alive = this.update(cx, |this, cx| {
                let changed = this.drain_events();
                if changed {
                    this.refilter();
                }
                if changed || this.any_loading() || this.any_job_running() {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// Run a job in the background, alongside any others. `keys` are the packages it changes;
    /// a package can be in only one job at a time. `job_fn` gets the job's handle, so a
    /// transaction can report progress and honour its own Cancel. When the job succeeds,
    /// `after` says which packages to read again.
    pub(crate) fn run_job<F>(
        &mut self,
        title: String,
        clear: Vec<String>,
        keys: Vec<String>,
        job_fn: F,
        after: After,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(&job::Handle) -> ActionResult + Send + 'static,
    {
        if let Some(key) = keys.iter().find(|k| self.locked.contains(*k)) {
            let name = self
                .find_package(key)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| key.clone());
            self.status = format!("{name} is already being changed by another job.");
            cx.notify();
            return;
        }
        self.locked.extend(keys.iter().cloned());
        let handle = job::start(&title);
        self.next_job += 1;
        self.jobs.push(JobUi {
            id: self.next_job,
            handle: handle.clone(),
        });
        self.status = format!("{title}…");
        cx.notify();

        let worker = handle.clone();
        let result_title = title.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    job::bind(&worker);
                    let r = job_fn(&worker);
                    job::finish(&worker, &r);
                    r
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for key in &keys {
                    this.locked.remove(key);
                }
                if result.success {
                    for key in &clear {
                        this.checked.remove(key);
                    }
                    this.status = format!("{result_title}: {}", result.summary);
                    match after {
                        After::Recheck(list) => this.recheck(list, cx),
                        After::Refresh(sources) => this.refresh_registries(sources, cx),
                    }
                } else {
                    this.status = format!("{result_title} failed: {}", result.summary);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Run a list of steps as one transaction. See `transaction::run`.
    fn run_steps(
        &mut self,
        title: String,
        clear: Vec<String>,
        steps: Vec<Step>,
        after: After,
        cx: &mut Context<Self>,
    ) {
        if steps.is_empty() {
            self.status = "Nothing to do.".into();
            cx.notify();
            return;
        }
        let keys: Vec<String> = match &after {
            After::Recheck(list) => list.iter().map(|(src, name)| format!("{}:{name}", src.tag())).collect(),
            After::Refresh(_) => Vec::new(),
        };
        self.run_job(title, clear, keys, move |handle| transaction::run(handle, &steps), after, cx);
    }

    /// Ask one job to stop. Its finished steps are undone; nothing new starts.
    pub(crate) fn cancel_job(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(job) = self.jobs.iter().find(|j| j.id == id) {
            job::request_cancel(&job.handle);
            self.status = "Cancelling…".into();
        }
        cx.notify();
    }

    /// Remove a finished job's row from the panel.
    pub(crate) fn dismiss_job(&mut self, id: u64, cx: &mut Context<Self>) {
        self.jobs.retain(|j| j.id != id || !job_done(&j.handle));
        cx.notify();
    }

    pub(crate) fn any_job_running(&self) -> bool {
        self.jobs.iter().any(|j| !job_done(&j.handle))
    }

    /// Read the state of only the packages an action touched, in the background. Their
    /// rows and the cache are updated; no other package is read again.
    fn recheck(&mut self, keys: Vec<(Source, String)>, cx: &mut Context<Self>) {
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
    fn apply_checks(
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
            let want_installed = self.state_filter.contains(&StateFilter::Installed);
            let want_missing = self.state_filter.contains(&StateFilter::NotInstalled);
            let state_ok = if want_installed || want_missing {
                (want_installed && pkg.is_installed()) || (want_missing && !pkg.is_installed())
            } else {
                // Without a search or the updates filter, the list shows what is installed.
                searching || self.updates_only || pkg.is_installed()
            };
            if !state_ok {
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
    }

    /// Packages with an update that are in a ticked registry.
    pub(crate) fn ticked_updates(&self) -> usize {
        self.packages
            .iter()
            .filter(|p| p.upgradable && p.is_installed() && self.registries.contains(&p.source))
            .count()
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

    pub(crate) fn toggle_registry_menu(&mut self, cx: &mut Context<Self>) {
        self.registry_menu = !self.registry_menu;
        self.state_menu = false;
        cx.notify();
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

    /// Packages ticked and not yet installed. AppImages are never installed from here.
    pub(crate) fn ticked_to_install(&self) -> Vec<&Package> {
        self.packages
            .iter()
            .filter(|p| {
                self.checked.contains(&p.key()) && !p.is_installed() && p.source != Source::AppImage
            })
            .collect()
    }

    // ----- actions ----------------------------------------------------------

    /// Install every ticked package, one job per package.
    pub(crate) fn install_selected(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.ticked_to_install().iter().map(|p| p.key()).collect();
        if keys.is_empty() {
            self.status = "Tick one or more packages to install them.".into();
            cx.notify();
            return;
        }
        self.status = format!("Started {} install job(s).", keys.len());
        cx.notify();
        let for_all = self.flatpak_for_all;
        for key in keys {
            self.install_as(key, for_all, cx);
        }
    }

    /// Install one package. Flatpak apps use the scope chosen in the toolbar.
    pub(crate) fn install_one(&mut self, key: String, cx: &mut Context<Self>) {
        let for_all = self.flatpak_for_all;
        self.install_as(key, for_all, cx);
    }

    /// Install one package as its own job. For Flatpak, `for_all` picks a system-wide
    /// install (asks for the password) instead of one for the current user.
    pub(crate) fn install_as(&mut self, key: String, for_all: bool, cx: &mut Context<Self>) {
        let Some(mut pkg) = self.find_package(&key).cloned() else {
            return;
        };
        if pkg.source == Source::Flatpak {
            pkg.scope = plan::scope_name(for_all).to_string();
        }
        match plan::install(&pkg) {
            Some(step) => {
                let title = format!("Installing {}", pkg.title);
                self.run_steps(title, vec![key], vec![step], After::Recheck(vec![(pkg.source, pkg.name.clone())]), cx);
            }
            None => {
                self.status = format!("{} is added from the AppImage folder, not installed.", pkg.title);
                cx.notify();
            }
        }
    }

    /// Install a name typed in the search box, for managers with no catalogue.
    pub(crate) fn install_by_name(&mut self, src: Source, name: String, cx: &mut Context<Self>) {
        let title = format!("Installing {name} with {}", src.label());
        match plan::install_by_name(src, &name) {
            Some(step) => self.run_steps(title, Vec::new(), vec![step], After::Recheck(vec![(src, name.clone())]), cx),
            None => {
                self.status = format!("{} has no install by name.", src.label());
                cx.notify();
            }
        }
    }

    pub(crate) fn remove_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let verb = if pkg.source == Source::AppImage { "Deleting" } else { "Removing" };
        let title = format!("{verb} {}", pkg.title);
        match plan::remove(&pkg) {
            Some(step) => self.run_steps(title, vec![key.clone()], vec![step], After::Recheck(vec![(pkg.source, pkg.name.clone())]), cx),
            None => {
                self.status = format!("{} can't be removed from here.", pkg.title);
                cx.notify();
            }
        }
    }

    pub(crate) fn update_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        match plan::upgrade(&pkg) {
            Some(step) => {
                let title = format!("Updating {}", pkg.title);
                self.run_steps(title, vec![key.clone()], vec![step], After::Recheck(vec![(pkg.source, pkg.name.clone())]), cx);
            }
            None => {
                self.status = format!("{} has no update that can be applied here.", pkg.title);
                cx.notify();
            }
        }
    }

    pub(crate) fn launch_appimage(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(key) else {
            return;
        };
        self.status = match appimage::launch(&pkg.origin) {
            Ok(()) => format!("Started {}", pkg.title),
            Err(e) => e,
        };
        cx.notify();
    }

    /// Upgrade every package that has an update. Each package is its own job, so each has
    /// its own progress row and Cancel, and they run side by side.
    pub(crate) fn upgrade_all(&mut self, cx: &mut Context<Self>) {
        let pkgs: Vec<Package> = self
            .packages
            .iter()
            .filter(|p| p.upgradable && p.is_installed() && self.registries.contains(&p.source))
            .cloned()
            .collect();
        let jobs: Vec<(Package, Step)> = pkgs
            .into_iter()
            .filter_map(|p| plan::upgrade(&p).map(|step| (p, step)))
            .collect();
        if jobs.is_empty() {
            self.status = "Nothing to update.".into();
            cx.notify();
            return;
        }
        self.status = format!("Started {} update job(s).", jobs.len());
        cx.notify();
        for (pkg, step) in jobs {
            let title = format!("Updating {}", pkg.title);
            let touched = vec![(pkg.source, pkg.name.clone())];
            self.run_steps(title, Vec::new(), vec![step], After::Recheck(touched), cx);
        }
    }

    /// Install each file dropped on the app (AppImage, .deb, .flatpak, .flatpakref), one
    /// job per file. If any file is refused, nothing starts.
    pub(crate) fn install_dropped(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut jobs = Vec::new();
        let mut problems = Vec::new();
        for path in &paths {
            match plan::file_job(path, self.flatpak_for_all) {
                Ok(job) => jobs.push(job),
                Err(e) => problems.push(e),
            }
        }
        if !problems.is_empty() {
            self.status = problems.join("; ");
            cx.notify();
            return;
        }
        for job in jobs {
            self.run_steps(job.title, Vec::new(), job.steps, After::Refresh(vec![job.source]), cx);
        }
    }

    /// Refresh: APT re-reads its package lists first (as a job, with progress), then every
    /// ticked registry is checked again in the background. Only changed packages are updated.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let ticked: Vec<Source> = self.active_registries().into_iter().filter(|s| self.registries.contains(s)).collect();
        if self.registries.contains(&Source::Apt) && self.is_on(Source::Apt) {
            self.run_job(
                "Refreshing APT package lists".into(),
                Vec::new(),
                Vec::new(),
                |_| apt::refresh_lists(),
                After::Refresh(ticked),
                cx,
            );
        } else {
            self.refresh_registries(ticked, cx);
        }
    }

    pub(crate) fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.show_history = !self.show_history;
        if self.show_history {
            self.history = apt::load_history(200);
        }
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

    pub(crate) fn open_sources(&mut self, src: Source, cx: &mut Context<Self>) {
        self.source_lines = managers::sources(src);
        self.page = Page::Sources(src);
        cx.notify();
    }

    /// Add a Flatpak remote from the text box, written as `name url`.
    pub(crate) fn add_remote(&mut self, cx: &mut Context<Self>) {
        let text = self.source_input.read(cx).value().to_string();
        let mut parts = text.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
            self.status = "Type a remote name and a URL, separated by a space.".into();
            cx.notify();
            return;
        };
        let (name, url) = (name.to_string(), url.to_string());
        let title = format!("Adding Flatpak remote {name}");
        self.run_job(
            title,
            Vec::new(),
            Vec::new(),
            move |_| flatpak::add_remote(&name, &url),
            After::Refresh(vec![Source::Flatpak]),
            cx,
        );
    }

    pub(crate) fn remove_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let title = format!("Removing Flatpak remote {name}");
        self.run_job(
            title,
            Vec::new(),
            Vec::new(),
            move |_| flatpak::remove_remote(&name),
            After::Refresh(vec![Source::Flatpak]),
            cx,
        );
    }

    // ----- layout -----------------------------------------------------------

    pub(crate) fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.page {
            Page::Browse => self.render_browse(cx),
            Page::Preferences => self.render_preferences(cx),
            Page::Sources(src) => self.render_sources(src, cx),
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let browsing = self.page == Page::Browse;
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_sm();

        if browsing {
            root = root.child(self.render_toolbar(cx));
            if self.registry_menu {
                root = root.child(self.render_registry_menu(cx));
            }
            if self.state_menu {
                root = root.child(self.render_state_menu(cx));
            }
        }
        root = root.child(self.render_body(cx));
        if let Some(panel) = self.render_loads(cx) {
            root = root.child(panel);
        }
        if let Some(panel) = self.render_jobs(cx) {
            root = root.child(panel);
        }
        if browsing {
            root = root.child(self.render_bottom_bar(cx));
        }
        root
    }
}
