//! The actions offered for one package, shown in the ⋯ menu and the details panel. The
//! rules here are plain functions, so they can be tested without a window.

use crate::model::{Package, Source};
use crate::plan;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Install,
    Update,
    Remove,
    Reinstall,
    OpenHomepage,
    CopyName,
    Launch,
    DeleteFile,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Install => "Install",
            Action::Update => "Update",
            Action::Remove => "Remove",
            Action::Reinstall => "Reinstall",
            Action::OpenHomepage => "Open homepage",
            Action::CopyName => "Copy name",
            Action::Launch => "Launch",
            Action::DeleteFile => "Delete file",
        }
    }

    /// Actions that remove or replace something, shown with the danger style.
    pub fn is_danger(self) -> bool {
        matches!(self, Action::Remove | Action::Reinstall | Action::DeleteFile)
    }
}

/// The actions that apply to a package right now, in menu order. Actions that do not apply
/// are left out.
pub fn available(pkg: &Package) -> Vec<Action> {
    if pkg.source == Source::AppImage {
        return vec![Action::Launch, Action::DeleteFile, Action::CopyName];
    }
    let mut out = Vec::new();
    if pkg.is_installed() {
        if pkg.upgradable {
            out.push(Action::Update);
        }
        if plan::can_reinstall(pkg) {
            out.push(Action::Reinstall);
        }
        out.push(Action::Remove);
    } else {
        out.push(Action::Install);
    }
    if !pkg.homepage.trim().is_empty() {
        out.push(Action::OpenHomepage);
    }
    out.push(Action::CopyName);
    out
}

/// One line naming a package and where it comes from. Flatpak apps also say whether they go
/// to the current user or to all users.
pub fn describe(pkg: &Package, for_all: bool) -> String {
    if pkg.source == Source::Flatpak {
        let scope = if for_all { "for all users" } else { "for you only" };
        format!("{} · {} · {scope}", pkg.title, pkg.source.label())
    } else {
        format!("{} · {}", pkg.title, pkg.source.label())
    }
}

/// One line for an update: name, registry, and the version change.
pub fn describe_update(pkg: &Package) -> String {
    let from = pkg.installed.clone().unwrap_or_default();
    format!("{} · {} · {from} → {}", pkg.title, pkg.source.label(), pkg.candidate)
}

/// What a reinstall does, for the confirmation message.
pub fn reinstall_note(pkg: &Package) -> &'static str {
    match pkg.source {
        Source::Flatpak => "This uninstalls the app first, then installs it again.",
        _ => "The same version is installed again.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(source: Source, name: &str) -> Package {
        let mut p = Package::new(source, name);
        p.installed = Some("1.0".into());
        p
    }

    #[test]
    fn an_installed_package_offers_reinstall_and_remove_not_install() {
        let got = available(&installed(Source::Apt, "nano"));
        assert_eq!(got, [Action::Reinstall, Action::Remove, Action::CopyName]);
    }

    #[test]
    fn an_update_is_offered_only_when_one_is_waiting() {
        let mut p = installed(Source::Npm, "left-pad");
        assert!(!available(&p).contains(&Action::Update));
        p.upgradable = true;
        p.candidate = "1.1".into();
        assert!(available(&p).contains(&Action::Update));
    }

    #[test]
    fn a_package_not_installed_offers_install() {
        let got = available(&Package::new(Source::Apt, "nano"));
        assert_eq!(got, [Action::Install, Action::CopyName]);
    }

    #[test]
    fn open_homepage_appears_only_with_a_homepage() {
        let mut p = Package::new(Source::Apt, "nano");
        assert!(!available(&p).contains(&Action::OpenHomepage));
        p.homepage = "https://nano-editor.org".into();
        assert!(available(&p).contains(&Action::OpenHomepage));
    }

    #[test]
    fn snap_has_no_reinstall() {
        let got = available(&installed(Source::Snap, "hello"));
        assert!(!got.contains(&Action::Reinstall));
        assert!(got.contains(&Action::Remove));
    }

    #[test]
    fn an_appimage_offers_launch_and_delete_file() {
        let got = available(&Package::new(Source::AppImage, "x.AppImage"));
        assert_eq!(got, [Action::Launch, Action::DeleteFile, Action::CopyName]);
    }

    #[test]
    fn remove_reinstall_and_delete_use_the_danger_style() {
        assert!(Action::Remove.is_danger());
        assert!(Action::Reinstall.is_danger());
        assert!(Action::DeleteFile.is_danger());
        assert!(!Action::Install.is_danger());
    }

    #[test]
    fn flatpak_descriptions_say_who_the_app_is_for() {
        let p = Package::new(Source::Flatpak, "org.gimp.GIMP");
        assert!(describe(&p, false).ends_with("for you only"));
        assert!(describe(&p, true).ends_with("for all users"));
        let apt = Package::new(Source::Apt, "nano");
        assert_eq!(describe(&apt, false), "nano · APT");
    }

    #[test]
    fn describe_update_shows_the_version_change() {
        let mut p = Package::new(Source::Apt, "nano");
        p.installed = Some("7.2-2".into());
        p.candidate = "7.2-2ubuntu0.2".into();
        p.upgradable = true;
        assert_eq!(describe_update(&p), "nano · APT · 7.2-2 → 7.2-2ubuntu0.2");
    }

    #[test]
    fn tick_boxes_only_on_actionable_rows() {
        let mut installed = Package::new(Source::Apt, "git");
        installed.installed = Some("1".into());
        assert!(!installed.tickable(), "installed and current: no tick box");
        installed.upgradable = true;
        assert!(installed.tickable(), "installed with an update: tick to update");
        let missing = Package::new(Source::Apt, "gimp");
        assert!(missing.tickable(), "not installed: tick to install");
        let appimage = Package::new(Source::AppImage, "tool.AppImage");
        assert!(!appimage.tickable(), "AppImages are never ticked");
    }
}
