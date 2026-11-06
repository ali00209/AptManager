//! The details panel on the right of the browser.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::actions::{self, Action};
use crate::model::{human_size, Package, Source};
use crate::ui::app::AppView;
use crate::ui::theme::*;
use crate::ui::widgets::{details_frame, icon_element, info_row};

impl AppView {
    pub(crate) fn render_details(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(pkg) = self.selected.as_deref().and_then(|k| self.find_package(k)) else {
            return details_frame()
                .items_center()
                .justify_center()
                .text_color(rgb(MUTED))
                .child("Select a package to see its details.")
                .into_any_element();
        };

        let key = pkg.key();
        let description = if pkg.description.is_empty() {
            "No description available.".to_string()
        } else {
            pkg.description.clone()
        };

        let mut frame = details_frame()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon_element(pkg, 44.))
                    .child(div().flex_1().overflow_hidden().text_lg().child(pkg.title.clone())),
            )
            .child(div().text_color(rgb(MUTED)).child(description))
            .child(div().h(px(1.)).w_full().bg(rgb(BORDER)));

        let action = match pkg.source {
            Source::Apt => {
                frame = frame
                    .child(info_row("Version", pkg.candidate.clone()))
                    .child(info_row(
                        "Installed",
                        pkg.installed.clone().unwrap_or_else(|| "No".into()),
                    ))
                    .child(info_row("Section", pkg.section.clone()))
                    .child(info_row("Download", human_size(pkg.download_size)))
                    .child(info_row("Installed size", human_size(pkg.installed_size)))
                    .child(info_row(
                        "Homepage",
                        if pkg.homepage.is_empty() { "—".into() } else { pkg.homepage.clone() },
                    ));
                action_buttons(pkg, key.clone(), cx)
            }
            Source::Flatpak => {
                frame = frame
                    .child(info_row("App ID", pkg.name.clone()))
                    .child(info_row("Version", pkg.candidate.clone()))
                    .child(info_row(
                        "Installed",
                        pkg.installed.clone().unwrap_or_else(|| "No".into()),
                    ))
                    .child(info_row("Remote", pkg.origin.clone()))
                    .child(info_row(
                        "Installation",
                        if pkg.scope.is_empty() { "—".into() } else { pkg.scope.clone() },
                    ))
                    .child(info_row("Download", human_size(pkg.download_size)))
                    .child(info_row("Installed size", human_size(pkg.installed_size)));
                action_buttons(pkg, key.clone(), cx)
            }
            Source::AppImage => {
                frame = frame
                    .child(info_row("File", pkg.name.clone()))
                    .child(info_row("Location", pkg.origin.clone()))
                    .child(info_row("Size", human_size(pkg.installed_size)));
                action_buttons(pkg, key.clone(), cx)
            }
            other => {
                frame = frame
                    .child(info_row("Manager", other.label().to_string()))
                    .child(info_row("Version", pkg.candidate.clone()))
                    .child(info_row(
                        "Installed",
                        pkg.installed.clone().unwrap_or_else(|| "No".into()),
                    ))
                    .child(info_row(
                        "Source",
                        if pkg.origin.is_empty() { "—".into() } else { pkg.origin.clone() },
                    ));
                action_buttons(pkg, key.clone(), cx)
            }
        };

        frame.child(div().pt_2().child(action)).into_any_element()
    }
}

/// The actions for a package, from the same list the ⋯ menu uses. A Flatpak app that is not
/// installed gets two install buttons, for you or for all users, so the user picks the scope.
pub(crate) fn action_buttons(pkg: &Package, key: String, cx: &mut Context<AppView>) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap_2();
    for (i, action) in actions::available(pkg).into_iter().enumerate() {
        if action == Action::Install && pkg.source == Source::Flatpak {
            for (id, label, for_all) in [
                ("install-for-me", "Install for me", false),
                ("install-for-all", "Install for all users", true),
            ] {
                let k = key.clone();
                let button = Button::new(id)
                    .primary()
                    .label(label)
                    .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                        this.install_as(k.clone(), for_all, cx);
                    }));
                row = row.child(button);
            }
            continue;
        }
        let k = key.clone();
        let button = Button::new(format!("detail-action-{i}")).label(action.label());
        let button = if action.is_danger() {
            button.danger()
        } else if matches!(action, Action::Install | Action::Update | Action::Launch) {
            button.primary()
        } else {
            button.ghost()
        };
        row = row.child(button.on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
            this.run_action(k.clone(), action, cx);
        })));
    }
    row.into_any_element()
}
