//! actions for the main window (part of `AppView`).
//!
//! Each action that changes packages asks first (see `confirm.rs`). The `*_now` functions do
//! the work once the user has pressed Yes.

use super::*;
use std::path::PathBuf;
use crate::actions::{self as rules, Action};
use crate::model::{Package, Source};
use crate::transaction::Step;
use crate::ui::app::confirm::Confirm;
use crate::ui::app::jobs::QueuedJob;
use crate::{apt, appimage, plan};

impl AppView {
    // ----- asking first -------------------------------------------------------

    /// Install every ticked package after one confirmation, one job per package.
    pub(crate) fn install_selected(&mut self, cx: &mut Context<Self>) {
        let picks: Vec<Package> = self.ticked_to_install().into_iter().cloned().collect();
        if picks.is_empty() {
            self.status = "Tick one or more packages to install them.".into();
            cx.notify();
            return;
        }
        let for_all = self.flatpak_for_all;
        let keys: Vec<String> = picks.iter().map(|p| p.key()).collect();
        let confirm = Confirm {
            title: format!("Install {} package(s)?", picks.len()),
            lines: picks.iter().map(|p| rules::describe(p, for_all)).collect(),
            note: None,
            yes: "Install".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.status = format!("Started {} install job(s).", keys.len());
                cx.notify();
                for key in keys {
                    this.install_as_now(key, for_all, cx);
                }
            }),
        };
        self.ask(confirm, cx);
    }

    /// Install one package. Flatpak apps use the scope chosen in the toolbar.
    pub(crate) fn install_one(&mut self, key: String, cx: &mut Context<Self>) {
        let for_all = self.flatpak_for_all;
        self.install_as(key, for_all, cx);
    }

    /// Install one package as its own job after confirming. For Flatpak, `for_all` picks a
    /// system-wide install (asks for the password) instead of one for the current user.
    pub(crate) fn install_as(&mut self, key: String, for_all: bool, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        if pkg.source == Source::AppImage {
            // Nothing to confirm: the AppImage is only added from its folder.
            self.install_as_now(key, for_all, cx);
            return;
        }
        let confirm = Confirm {
            title: format!("Install {}?", pkg.title),
            lines: vec![rules::describe(&pkg, for_all)],
            note: None,
            yes: "Install".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.install_as_now(key, for_all, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    /// Install a name typed in the search box, for managers with no catalogue.
    pub(crate) fn install_by_name(&mut self, src: Source, name: String, cx: &mut Context<Self>) {
        let confirm = Confirm {
            title: format!("Install {name}?"),
            lines: vec![format!("{name} · {}", src.label())],
            note: None,
            yes: "Install".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.install_by_name_now(src, name, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    /// Remove a package, or delete an AppImage file, after confirming.
    pub(crate) fn remove_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let (title, yes, note) = if pkg.source == Source::AppImage {
            (
                format!("Delete {}?", pkg.title),
                "Delete",
                "The file is moved out of ~/Applications when the job finishes.",
            )
        } else {
            (
                format!("Remove {}?", pkg.title),
                "Remove",
                "This uninstalls the package from this computer.",
            )
        };
        let confirm = Confirm {
            title,
            lines: vec![rules::describe(&pkg, pkg.scope != "user")],
            note: Some(note.into()),
            yes: yes.into(),
            danger: true,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.remove_one_now(key, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    /// Update one package after confirming.
    pub(crate) fn update_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let confirm = Confirm {
            title: format!("Update {}?", pkg.title),
            lines: vec![rules::describe(&pkg, pkg.scope != "user")],
            note: None,
            yes: "Update".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.update_one_now(key, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    /// Reinstall an installed package after confirming. Its scope is the one it was installed with.
    pub(crate) fn reinstall_one(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        let confirm = Confirm {
            title: format!("Reinstall {}?", pkg.title),
            lines: vec![rules::describe(&pkg, pkg.scope != "user")],
            note: Some(rules::reinstall_note(&pkg).into()),
            yes: "Reinstall".into(),
            danger: true,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.reinstall_one_now(key, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    /// Upgrade every package that has an update, after one confirmation. Each package is its
    /// own job with its own progress row and Cancel. At most `MAX_PARALLEL_JOBS` run at once;
    /// the rest wait in the queue.
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
        self.ask_update_jobs(jobs, false, cx);
    }

    /// Update every ticked package that has an update, after one confirmation.
    pub(crate) fn update_selected(&mut self, cx: &mut Context<Self>) {
        let jobs: Vec<(Package, Step)> = self
            .ticked_to_update()
            .into_iter()
            .cloned()
            .filter_map(|p| plan::upgrade(&p).map(|step| (p, step)))
            .collect();
        if jobs.is_empty() {
            self.status = "Tick one or more packages that have an update.".into();
            cx.notify();
            return;
        }
        self.ask_update_jobs(jobs, true, cx);
    }

    /// Confirm a batch of updates, then queue them. When `ticked`, each package's tick is
    /// cleared once its update succeeds.
    fn ask_update_jobs(&mut self, jobs: Vec<(Package, Step)>, ticked: bool, cx: &mut Context<Self>) {
        if jobs.is_empty() {
            self.status = "Nothing to update.".into();
            cx.notify();
            return;
        }
        let confirm = Confirm {
            title: format!("Update {} package(s)?", jobs.len()),
            lines: jobs.iter().map(|(p, _)| rules::describe_update(p)).collect(),
            note: None,
            yes: "Update".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                let count = jobs.len();
                for (pkg, step) in jobs {
                    this.queue.push_back(QueuedJob {
                        title: format!("Updating {}", pkg.title),
                        clear: if ticked { vec![pkg.key()] } else { Vec::new() },
                        touched: vec![(pkg.source, pkg.name.clone())],
                        step,
                    });
                }
                this.status = format!("Queued {count} update(s).");
                this.start_queued(cx);
            }),
        };
        self.ask(confirm, cx);
    }

    /// Install each file dropped on the app (AppImage, .deb, .flatpak, .flatpakref), one job
    /// per file, after one confirmation. If any file is refused, nothing is asked for or started.
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
        let confirm = Confirm {
            title: format!("Install {} file(s)?", jobs.len()),
            lines: jobs.iter().map(|j| j.title.clone()).collect(),
            note: None,
            yes: "Install".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                for job in jobs {
                    this.run_steps(job.title, Vec::new(), job.steps, After::Refresh(vec![job.source]), cx);
                }
            }),
        };
        self.ask(confirm, cx);
    }

    // ----- the work, after Yes -----------------------------------------------

    /// Install one package as its own job. Called after the user confirms.
    pub(crate) fn install_as_now(&mut self, key: String, for_all: bool, cx: &mut Context<Self>) {
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

    pub(crate) fn install_by_name_now(&mut self, src: Source, name: String, cx: &mut Context<Self>) {
        let title = format!("Installing {name} with {}", src.label());
        match plan::install_by_name(src, &name) {
            Some(step) => self.run_steps(title, Vec::new(), vec![step], After::Recheck(vec![(src, name.clone())]), cx),
            None => {
                self.status = format!("{} has no install by name.", src.label());
                cx.notify();
            }
        }
    }

    pub(crate) fn remove_one_now(&mut self, key: String, cx: &mut Context<Self>) {
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

    pub(crate) fn update_one_now(&mut self, key: String, cx: &mut Context<Self>) {
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

    pub(crate) fn reinstall_one_now(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(&key).cloned() else {
            return;
        };
        match plan::reinstall(&pkg) {
            Some(steps) => {
                let title = format!("Reinstalling {}", pkg.title);
                self.run_steps(title, vec![key], steps, After::Recheck(vec![(pkg.source, pkg.name.clone())]), cx);
            }
            None => {
                self.status = format!("{} can't be reinstalled from here.", pkg.title);
                cx.notify();
            }
        }
    }

    // ----- actions without a confirmation ------------------------------------

    /// Run one action from the ⋯ menu or the details panel.
    pub(crate) fn run_action(&mut self, key: String, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Install => self.install_one(key, cx),
            Action::Update => self.update_one(key, cx),
            Action::Remove | Action::DeleteFile => self.remove_one(key, cx),
            Action::Reinstall => self.reinstall_one(key, cx),
            Action::OpenHomepage => self.open_homepage(&key, cx),
            Action::CopyName => self.copy_name(&key, cx),
            Action::Launch => self.launch_appimage(&key, cx),
        }
    }

    pub(crate) fn open_homepage(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(key) else {
            return;
        };
        let url = pkg.homepage.trim().to_string();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            self.status = format!("{} has no web homepage.", pkg.title);
            cx.notify();
            return;
        }
        // Run the opener on its own thread, so the window never waits for it.
        std::thread::spawn(move || {
            let _ = std::process::Command::new("xdg-open").arg(url).status();
        });
        self.status = "Opening the homepage…".into();
        cx.notify();
    }

    pub(crate) fn copy_name(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(pkg) = self.find_package(key) else {
            return;
        };
        let name = pkg.name.clone();
        cx.write_to_clipboard(ClipboardItem::new_string(name.clone()));
        self.status = format!("Copied {name}.");
        cx.notify();
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

    /// Show or hide the details panel, and remember the choice for the next start.
    pub(crate) fn toggle_details(&mut self, cx: &mut Context<Self>) {
        self.show_details = !self.show_details;
        if let Err(e) = crate::prefs::save_details_shown(self.show_details) {
            self.status = e;
        }
        cx.notify();
    }

    pub(crate) fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.show_history = !self.show_history;
        if self.show_history {
            self.history = apt::load_history(200);
        }
        cx.notify();
    }
}
