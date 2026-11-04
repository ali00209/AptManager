//! Small pieces shared by several screens: icons, action buttons and the details frame.

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    Disableable,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::model::{Package, Source};
use crate::ui::app::AppView;
use crate::ui::theme::*;

/// Buttons for a package from a manager: Install when absent, Remove when present,
/// and Update when a newer version is waiting.
pub(crate) fn manager_actions(pkg: &Package, key: String, busy: bool, cx: &mut Context<AppView>) -> AnyElement {
    let mut col = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(install_or_remove(pkg, key.clone(), busy, cx));
    if pkg.upgradable && !matches!(pkg.source, Source::Apt | Source::Flatpak) {
        let update_key = key.clone();
        col = col.child(
            Button::new("update-one")
                .primary()
                .label("Update")
                .disabled(busy)
                .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.update_one(update_key.clone(), cx);
                })),
        );
    }
    col.into_any_element()
}

/// Install or Remove button for a package.
pub(crate) fn install_or_remove(pkg: &Package, key: String, busy: bool, cx: &mut Context<AppView>) -> AnyElement {
    if pkg.is_installed() {
        Button::new("remove-one")
            .danger()
            .label("Remove")
            .disabled(busy)
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.remove_one(key.clone(), cx);
            }))
            .into_any_element()
    } else {
        Button::new("install-one")
            .primary()
            .label("Install")
            .disabled(busy)
            .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                this.install_one(key.clone(), cx);
            }))
            .into_any_element()
    }
}

/// Launch and Delete buttons for an AppImage.
pub(crate) fn appimage_actions(key: String, busy: bool, cx: &mut Context<AppView>) -> AnyElement {
    let launch_key = key.clone();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            Button::new("launch-appimage")
                .primary()
                .label("Launch")
                .disabled(busy)
                .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.launch_appimage(&launch_key, cx);
                })),
        )
        .child(
            Button::new("remove-appimage")
                .danger()
                .label("Delete file")
                .disabled(busy)
                .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _window, cx| {
                    this.remove_one(key.clone(), cx);
                })),
        )
        .into_any_element()
}

/// The icon for a package: its own icon file when one exists, otherwise a coloured
/// box with the glyph of its manager (the "default icon by type").
pub(crate) fn icon_element(pkg: &Package, size: f32) -> AnyElement {
    match &pkg.icon {
        Some(path) => img(path.clone())
            .size(px(size))
            .flex_none()
            .into_any_element(),
        None => glyph_box(pkg.source.glyph(), pkg.source.color(), size),
    }
}

/// Coloured rounded box with a short label, used as the default icon.
pub(crate) fn glyph_box(glyph: &'static str, color: u32, size: f32) -> AnyElement {
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(size * 0.22))
        .bg(rgb(color))
        .text_color(rgb(0xffffff))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .child(glyph)
        .into_any_element()
}

/// Right-hand details panel container.
pub(crate) fn details_frame() -> Div {
    div()
        .w(px(DETAILS_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .bg(rgb(PANEL))
        .border_l_1()
        .border_color(rgb(BORDER))
}

/// What the "Section / Source" column shows for each manager.
pub(crate) fn location_label(pkg: &Package) -> String {
    match pkg.source {
        Source::Apt => pkg.section.clone(),
        Source::Flatpak => pkg.origin.clone(),
        Source::AppImage => "AppImage".to_string(),
        other => {
            if pkg.origin.is_empty() {
                other.label().to_string()
            } else {
                pkg.origin.clone()
            }
        }
    }
}

pub(crate) fn info_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .gap_3()
        .child(div().text_color(rgb(MUTED)).flex_none().child(label))
        .child(div().flex_1().overflow_hidden().child(value))
}

pub(crate) fn centered_message(text: &str) -> AnyElement {
    div()
        .flex_1()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(MUTED))
        .child(text.to_string())
        .into_any_element()
}
