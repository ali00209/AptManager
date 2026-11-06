mod actions;
mod apt;
mod cache;
mod appimage;
mod flatpak;
mod job;
mod load;
mod managers;
mod model;
mod prefs;
mod transaction;
mod plan;
mod privileged;
mod search;
mod track;
mod ui;

use std::path::Path;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::prelude::*;
use gpui_kit::*;

use ui::app::AppView;

/// Put the folders where package managers usually live on PATH. A window started from
/// the desktop does not get the shell's PATH, so Homebrew and similar tools would
/// otherwise look missing.
fn extend_path() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let mut extra: Vec<std::path::PathBuf> = vec![
        "/home/linuxbrew/.linuxbrew/bin".into(),
        "/home/linuxbrew/.linuxbrew/sbin".into(),
        "/snap/bin".into(),
        "/var/lib/flatpak/exports/bin".into(),
        "/usr/local/bin".into(),
    ];
    for rel in [
        ".linuxbrew/bin",
        ".cargo/bin",
        ".bun/bin",
        ".local/bin",
        ".npm-global/bin",
        ".local/share/flatpak/exports/bin",
    ] {
        extra.push(home.join(rel));
    }

    let mut parts: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    for dir in extra {
        let text = dir.display().to_string();
        if Path::new(&dir).is_dir() && !parts.contains(&text) {
            parts.push(text);
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

fn main() {
    // The same binary is the root helper when started with --helper. It must not open a window.
    if std::env::args().any(|a| a == "--helper") {
        std::process::exit(privileged::serve());
    }

    let _ = env_logger::try_init();
    extend_path();

    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Apt Manager".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| AppView::new(window, cx)))
            .expect("failed to open the main window");
    });
}
