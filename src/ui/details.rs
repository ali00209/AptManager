//! The details panel on the right of the browser.

use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{human_size, Source};
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
                manager_actions(pkg, key.clone(), self.busy, cx)
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
                manager_actions(pkg, key.clone(), self.busy, cx)
            }
            Source::AppImage => {
                frame = frame
                    .child(info_row("File", pkg.name.clone()))
                    .child(info_row("Location", pkg.origin.clone()))
                    .child(info_row("Size", human_size(pkg.installed_size)));
                appimage_actions(key.clone(), self.busy, cx)
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
                manager_actions(pkg, key.clone(), self.busy, cx)
            }
        };

        frame.child(div().pt_2().child(action)).into_any_element()
    }
}
