//! The window's state and actions, and the top-level layout that ties the screens together.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{ActionResult, Package, Source};
use crate::ui::loader::{group_for_install, install_batch, load_all};
use crate::ui::theme::*;
use crate::{apt, appimage, flatpak, job, managers, prefs};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Page {
    Browse,
    Preferences,
    Sources(Source),
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SubTab {
    Install,
    Updates,
    Manage,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Filter {
    Any,
    Installed,
    NotInstalled,
}

impl Filter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Filter::Any => "Any",
            Filter::Installed => "Installed",
            Filter::NotInstalled => "Not installed",
        }
    }

    pub(crate) fn next(self) -> Filter {
        match self {
            Filter::Any => Filter::Installed,
            Filter::Installed => Filter::NotInstalled,
            Filter::NotInstalled => Filter::Any,
        }
    }
}

pub struct AppView {
    pub(crate) packages: Vec<Package>,
    /// Indices into `packages` that pass the current tab, filter and search.
    pub(crate) visible: Vec<usize>,
    pub(crate) search: Entity<InputState>,
    pub(crate) query: String,
    /// The manager whose tab is showing.
    pub(crate) top: Source,
    pub(crate) sub: SubTab,
    pub(crate) filter: Filter,
    pub(crate) grid: bool,
    /// Keys (see `Package::key`) of ticked rows.
    pub(crate) checked: HashSet<String>,
    /// Key of the row shown in the details panel.
    pub(crate) selected: Option<String>,
    pub(crate) show_history: bool,
    pub(crate) history: Vec<apt::HistoryEntry>,
    pub(crate) status: String,
    pub(crate) busy: bool,
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
    pub(crate) job: Option<job::Handle>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search packages..."));
        let search_sub = cx.subscribe(
            &search,
            |this: &mut AppView, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<AppView>| {
                if let InputEvent::Change = event {
                    this.query = state.read(cx).value().to_string();
                    this.refilter();
                    cx.notify();
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
            query: String::new(),
            top: Source::Apt,
            sub: SubTab::Install,
            filter: Filter::Any,
            grid: false,
            checked: HashSet::new(),
            selected: None,
            show_history: false,
            history: Vec::new(),
            status: "Loading packages…".into(),
            busy: false,
            loaded: false,
            page: Page::Browse,
            detected: HashMap::new(),
            paths: HashMap::new(),
            enabled: HashSet::new(),
            prefs_search,
            prefs_query: String::new(),
            source_input,
            source_lines: Vec::new(),
            job: None,
            _subscriptions: vec![search_sub, prefs_sub],
        };
        this.reload(cx);
        this
    }

    /// Reload every enabled manager on a background thread. The first load uses the
    /// saved preferences; later loads use the switches as they are now.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.status = "Loading packages…".into();
        cx.notify();

        let override_enabled = if self.loaded { Some(self.enabled.clone()) } else { None };
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { load_all(override_enabled) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.loaded = true;
                this.packages = loaded.packages;
                this.detected = loaded.detected;
                this.paths = loaded.paths;
                this.enabled = loaded.enabled;
                this.status = loaded.status;
                this.fix_top();
                this.refilter();
                cx.notify();
            });
        })
        .detach();
    }

    /// Run a job in the background with live progress, then reload on success.
    /// `title` describes the job, for example "Installing gimp".
    pub(crate) fn run_job<F>(&mut self, title: String, clear: Vec<String>, job_fn: F, cx: &mut Context<Self>)
    where
        F: FnOnce() -> ActionResult + Send + 'static,
    {
        if self.busy {
            return;
        }
        self.busy = true;
        let handle = job::start(&title);
        self.job = Some(handle.clone());
        self.status = format!("{title}…");
        cx.notify();

        // Redraw while the job runs so the progress bar moves.
        let ticker = handle.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let done = ticker.lock().map(|p| p.done).unwrap_or(true);
            if this.update(cx, |_, cx| cx.notify()).is_err() || done {
                break;
            }
        })
        .detach();

        let worker = handle.clone();
        let result_title = title.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let r = job_fn();
                    job::finish(&worker, &r);
                    r
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if result.success {
                    for key in &clear {
                        this.checked.remove(key);
                    }
                    this.status = format!("{result_title}: {}", result.summary);
                    this.reload(cx);
                } else {
                    this.status = format!("{result_title} failed: {}", result.summary);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ----- state helpers ----------------------------------------------------

    /// True when a manager is switched on and its command was found.
    pub(crate) fn is_on(&self, src: Source) -> bool {
        self.enabled.contains(&src) && self.detected.get(&src).copied().unwrap_or(false)
    }

    /// Managers shown as top tabs: each one that is switched on and found.
    pub(crate) fn tabs(&self) -> Vec<Source> {
        Source::ALL.into_iter().filter(|&s| self.is_on(s)).collect()
    }

    /// Move to a visible tab when the current one was switched off.
    pub(crate) fn fix_top(&mut self) {
        let tabs = self.tabs();
        if !tabs.contains(&self.top) {
            self.top = tabs.first().copied().unwrap_or(Source::Apt);
        }
    }

    /// Whether a package passes the sub-tab (Install / Updates / Manage) rules.
    pub(crate) fn sub_ok(&self, pkg: &Package) -> bool {
        match self.sub {
            SubTab::Install => true,
            SubTab::Updates => pkg.upgradable,
            SubTab::Manage => pkg.is_installed(),
        }
    }

    pub(crate) fn matches(&self, pkg: &Package, query: &str) -> bool {
        let tab_ok = pkg.source == self.top
            && (self.top == Source::AppImage || self.sub_ok(pkg));
        let filter_ok = match self.filter {
            Filter::Any => true,
            Filter::Installed => pkg.is_installed(),
            Filter::NotInstalled => !pkg.is_installed(),
        };
        let query_ok = query.is_empty()
            || pkg.name.to_lowercase().contains(query)
            || pkg.title.to_lowercase().contains(query)
            || pkg.description.to_lowercase().contains(query);
        tab_ok && filter_ok && query_ok
    }

    pub(crate) fn refilter(&mut self) {
        let query = self.query.trim().to_lowercase();
        let visible: Vec<usize> = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, pkg)| self.matches(pkg, &query))
            .map(|(idx, _)| idx)
            .collect();
        self.visible = visible;
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

    pub(crate) fn install_selected(&mut self, cx: &mut Context<Self>) {
        let picks = self.ticked_to_install();
        if picks.is_empty() {
            self.status = "Tick one or more packages to install them.".into();
            cx.notify();
            return;
        }
        let clear: Vec<String> = picks.iter().map(|p| p.key()).collect();
        let title = format!("Installing {} package(s)", picks.len());
        let (apt_names, flatpak_groups, others) = group_for_install(&picks);
        self.run_job(
            title,
            clear,
            move || install_batch(apt_names, flatpak_groups, others),
            cx,
        );
    }

    pub(crate) fn install_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let title = format!("Installing {}", pkg.title);
        let picks = vec![&pkg];
        let (apt_names, flatpak_groups, others) = group_for_install(&picks);
        self.run_job(
            title,
            vec![key],
            move || install_batch(apt_names, flatpak_groups, others),
            cx,
        );
    }

    /// Install a name typed in the search box, for managers with no catalogue.
    pub(crate) fn install_by_name(&mut self, src: Source, name: String, cx: &mut Context<Self>) {
        let title = format!("Installing {name} with {}", src.label());
        self.run_job(title, Vec::new(), move || managers::install(src, &name), cx);
    }

    pub(crate) fn remove_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let verb = if pkg.source == Source::AppImage { "Deleting" } else { "Removing" };
        let title = format!("{verb} {}", pkg.title);
        let job_fn = move || match pkg.source {
            Source::Apt => apt::remove(&[pkg.name.clone()]),
            Source::Flatpak => flatpak::remove(&pkg.name, &pkg.scope),
            Source::AppImage => appimage::remove(&pkg.origin),
            other => managers::remove(other, &pkg.name),
        };
        self.run_job(title, vec![key], job_fn, cx);
    }

    pub(crate) fn update_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let src = pkg.source;
        let name = pkg.name.clone();
        let title = format!("Updating {}", pkg.title);
        self.run_job(
            title,
            vec![key],
            move || managers::upgrade(src, &[name]),
            cx,
        );
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

    pub(crate) fn upgrade_all(&mut self, cx: &mut Context<Self>) {
        let sources: Vec<Source> = Source::ALL
            .into_iter()
            .filter(|&s| self.updates_for(s) > 0)
            .collect();
        let has_user_flatpak = self
            .packages
            .iter()
            .any(|p| p.source == Source::Flatpak && p.is_installed() && p.scope == "user");
        let has_system_flatpak = self
            .packages
            .iter()
            .any(|p| p.source == Source::Flatpak && p.is_installed() && p.scope != "user");
        let title = format!("Upgrading {} manager(s)", sources.len());

        self.run_job(
            title,
            Vec::new(),
            move || {
                for src in sources {
                    let r = match src {
                        Source::Apt => apt::upgrade_all(),
                        Source::Flatpak => flatpak::update_all(has_user_flatpak, has_system_flatpak),
                        other => managers::upgrade(other, &[]),
                    };
                    if !r.success {
                        return r;
                    }
                }
                ActionResult::ok("Upgrade finished.")
            },
            cx,
        );
    }

    /// Refresh: APT re-reads its package lists, everything else reloads.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.top == Source::Apt {
            self.run_job(
                "Refreshing APT package lists".into(),
                Vec::new(),
                apt::refresh_lists,
                cx,
            );
        } else {
            self.reload(cx);
        }
    }

    pub(crate) fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.show_history = !self.show_history;
        if self.show_history {
            self.history = apt::load_history(200);
        }
        cx.notify();
    }

    /// Switch a manager on or off, save the choice and reload.
    pub(crate) fn set_enabled(&mut self, src: Source, on: bool, cx: &mut Context<Self>) {
        if on {
            self.enabled.insert(src);
        } else {
            self.enabled.remove(&src);
        }
        if let Err(e) = prefs::save(&self.enabled, &self.detected) {
            self.status = e;
        }
        self.show_history = false;
        self.reload(cx);
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
            move || flatpak::add_remote(&name, &url),
            cx,
        );
    }

    pub(crate) fn remove_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let title = format!("Removing Flatpak remote {name}");
        self.run_job(
            title,
            Vec::new(),
            move || flatpak::remove_remote(&name),
            cx,
        );
    }

    /// Jump to the first manager with updates waiting, on its Updates tab.
    pub(crate) fn show_updates(&mut self, cx: &mut Context<Self>) {
        if let Some(src) = Source::ALL
            .into_iter()
            .find(|&s| self.updates_for(s) > 0 && self.is_on(s))
        {
            self.top = src;
            self.sub = SubTab::Updates;
            self.page = Page::Browse;
            self.show_history = false;
            self.refilter();
            cx.notify();
        }
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
            .text_sm()
            .child(self.render_top_bar(cx));

        if browsing && self.top != Source::AppImage {
            root = root.child(self.render_sub_bar(self.top, cx));
        }
        if browsing {
            root = root.child(self.render_toolbar(cx));
        }
        root = root.child(self.render_body(cx));
        if let Some(panel) = self.render_job(cx) {
            root = root.child(panel);
        }
        if browsing {
            root = root.child(self.render_bottom_bar(cx));
        }
        root
    }
}
