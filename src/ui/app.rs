//! The window's state and actions, and the top-level layout that ties the screens together.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{Package, Source};
use crate::ui::loader::detect;
use crate::load::{self, Hub};
use crate::ui::theme::*;
use crate::{apt, job};

mod actions;
mod checks;
pub(crate) mod confirm;
mod filter;
mod jobs;
mod loading;
mod registries;
mod remote;
mod sources;

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
    /// The action waiting for confirmation, if any.
    pub(crate) confirm: Option<confirm::Confirm>,
    /// Whether the dialog should take the keyboard focus on the next frame.
    pub(crate) focus_confirm: bool,
    pub(crate) confirm_focus: FocusHandle,
    /// The package whose ⋯ menu is open, if any.
    pub(crate) menu_for: Option<String>,
    pub(crate) menu_focus: FocusHandle,
    /// Whether the details panel is shown.
    pub(crate) show_details: bool,
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
    /// Updates waiting for a free job slot. See `jobs::MAX_PARALLEL_JOBS`.
    pub(crate) queue: VecDeque<jobs::QueuedJob>,
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search packages… (Enter to search)"));
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
            confirm: None,
            focus_confirm: false,
            confirm_focus: cx.focus_handle(),
            menu_for: None,
            menu_focus: cx.focus_handle(),
            show_details: crate::prefs::details_shown(),
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
            queue: VecDeque::new(),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_confirm && self.confirm.is_some() {
            self.focus_confirm = false;
            window.focus(&self.confirm_focus, cx);
        }
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
        // Last, so the dialog sits above everything else.
        if let Some(dialog) = self.render_confirm(cx) {
            root = root.child(dialog);
        }
        root
    }
}
