//! The details panel on the right of the browser.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{human_size, Package, Source};
use crate::ui::app::AppView;
use crate::ui::theme::*;
use crate::ui::widgets::{
    appimage_actions, details_frame, icon_element, info_row, manager_actions,
};

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
                manager_actions(pkg, key.clone(), cx)
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
                flatpak_actions(pkg, key.clone(), cx)
            }
            Source::AppImage => {
                frame = frame
                    .child(info_row("File", pkg.name.clone()))
                    .child(info_row("Location", pkg.origin.clone()))
                    .child(info_row("Size", human_size(pkg.installed_size)));
                appimage_actions(key.clone(), cx)
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
                manager_actions(pkg, key.clone(), cx)
            }
        };

        frame.child(div().pt_2().child(action)).into_any_element()
    }
}

/// Flatpak apps: an installed app is removed; a new one is installed for the current user
/// or for all users, as the user picks.
pub(crate) fn flatpak_actions(pkg: &Package, key: String, cx: &mut Context<AppView>) -> AnyElement {
    if pkg.is_installed() {
        return manager_actions(pkg, key, cx);
    }
    let for_me = key.clone();
    let for_all = key;
    div()
        .flex()
        .gap_2()
        .child(
            Button::new("install-for-me")
                .primary()
                .label("Install for me")
                .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.install_as(for_me.clone(), false, cx);
                })),
        )
        .child(
            Button::new("install-for-all")
                .ghost()
                .label("Install for all users")
                .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.install_as(for_all.clone(), true, cx);
                })),
        )
        .into_any_element()
}
