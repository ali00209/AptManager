//! Backend for AppImages.
//!
//! AppImages have no central repository, so this manages the files in
//! `~/Applications`: list them, launch them, and delete them. Nothing here needs
//! privileges. To add one, copy the `.AppImage` file into `~/Applications`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{ActionResult, Package, Source};

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
            }
        })
        .collect();

    packages.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    packages
}

/// Delete an AppImage file.
pub fn remove(path: &str) -> ActionResult {
    match fs::remove_file(path) {
        Ok(()) => ActionResult::ok(format!("Removed {}", file_label(path))),
        Err(e) => ActionResult::err(format!("Could not remove {}: {e}", file_label(path))),
    }
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

fn file_label(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}
