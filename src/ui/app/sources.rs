//! sources for the main window (part of `AppView`).

use super::*;
use crate::model::Source;
use crate::ui::app::confirm::Confirm;
use crate::{flatpak, managers};

impl AppView {
    pub(crate) fn open_sources(&mut self, src: Source, cx: &mut Context<Self>) {
        self.source_lines = managers::sources(src);
        self.page = Page::Sources(src);
        cx.notify();
    }

    /// Add a Flatpak remote from the text box, written as `name url`, after confirming.
    pub(crate) fn add_remote(&mut self, cx: &mut Context<Self>) {
        let text = self.source_input.read(cx).value().to_string();
        let mut parts = text.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
            self.status = "Type a remote name and a URL, separated by a space.".into();
            cx.notify();
            return;
        };
        let (name, url) = (name.to_string(), url.to_string());
        let confirm = Confirm {
            title: format!("Add Flatpak remote {name}?"),
            lines: vec![format!("{name} · {url}")],
            note: None,
            yes: "Add".into(),
            danger: false,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.add_remote_now(name, url, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    fn add_remote_now(&mut self, name: String, url: String, cx: &mut Context<Self>) {
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

    /// Remove a Flatpak remote after confirming.
    pub(crate) fn remove_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let confirm = Confirm {
            title: format!("Remove Flatpak remote {name}?"),
            lines: vec![name.clone()],
            note: None,
            yes: "Remove".into(),
            danger: true,
            run: Box::new(move |this: &mut AppView, cx: &mut Context<AppView>| {
                this.remove_remote_now(name, cx);
            }),
        };
        self.ask(confirm, cx);
    }

    fn remove_remote_now(&mut self, name: String, cx: &mut Context<Self>) {
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
}
