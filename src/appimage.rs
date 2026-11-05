//! Backend for AppImages.
//!
//! AppImages have no central repository, so this manages the files in
//! `~/Applications`: list them, launch them, and delete them. Nothing here needs
//! privileges. To add one, copy the `.AppImage` file into `~/Applications`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{Package, Source};

/// Folder that AppImages are read from.
pub fn directory() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join("Applications")
}

/// List every `.AppImage` file in the AppImage folder. A missing folder is just empty.
pub fn load() -> Vec<Package> {
    let dir = directory();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut packages: Vec<Package> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path().is_file()
                && entry
                    .path()
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("appimage"))
                    .unwrap_or(false)
        })
        .map(|entry| {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file_name.clone());
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            Package {
                source: Source::AppImage,
                name: file_name,
                title: stem,
                candidate: String::new(),
                installed: Some("on disk".into()),
                download_size: 0,
                installed_size: size,
                section: String::new(),
                origin: path.display().to_string(),
                scope: String::new(),
                description: format!("AppImage in {}", dir.display()),
                homepage: String::new(),
                upgradable: false,
                icon: None,
                online: false,
            }
        })
        .collect();

    packages.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    packages
}
/// Start an AppImage. Makes it executable first if it isn't already.
pub fn launch(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    let meta = fs::metadata(path).map_err(|e| format!("Cannot read file: {e}"))?;
    if meta.permissions().mode() & 0o111 == 0 {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).map_err(|e| format!("Cannot make executable: {e}"))?;
    }
    Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start {}: {e}", path.display()))
}

/// Folder where desktop entries are written, so the application menu lists AppImages.
fn desktop_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/share/applications")
}

const ENTRY_PREFIX: &str = "apt-manager-appimage-";

/// Make one `.desktop` entry per AppImage in `packages`, and remove the entries of
/// AppImages that are gone. Only files this app wrote (with `ENTRY_PREFIX`) are touched.
pub fn sync_desktop_entries(packages: &[Package]) {
    let dir = desktop_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut wanted: Vec<String> = Vec::new();
    for pkg in packages.iter().filter(|p| p.source == Source::AppImage) {
        let safe: String = pkg
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
            .collect();
        let file = format!("{ENTRY_PREFIX}{safe}.desktop");
        let exec = pkg.origin.replace('\\', "\\\\").replace('"', "\\\"");
        let text = format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec=\"{exec}\"\nTerminal=false\nCategories=Utility;\nX-Managed-By=apt-manager\n",
            pkg.title
        );
        let path = dir.join(&file);
        if fs::read_to_string(&path).map(|old| old != text).unwrap_or(true) {
            let _ = fs::write(&path, text);
        }
        wanted.push(file);
    }
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(ENTRY_PREFIX) && !wanted.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
