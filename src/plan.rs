//! Turns each operation into transaction steps, and gives each step the command that
//! undoes it. A removal is undone by reinstalling the version that was installed. An
//! upgrade is undone by installing the previous version, where the manager allows
//! choosing one. Where it does not (Snap, Flatpak, Homebrew upgrades), the step is
//! reported as not reversible instead of being silently skipped.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::appimage;
use crate::managers::pip_command;
use crate::model::{Package, Source};
use crate::transaction::{Cmd, Lane, Step};

fn step(lane: Lane, title: String, forward: Cmd, undo: Option<Cmd>) -> Step {
    Step::new(lane, title, forward, undo)
}

/// A command run through the root helper.
fn system(program: &str, args: &[&str]) -> Cmd {
    Cmd::Exec {
        privileged: true,
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        fallback: None,
    }
}

/// A command run as the current user, for the manager `src`.
fn manager(src: Source, args: &[&str]) -> Cmd {
    let (program, prefix) = match src {
        Source::Pip => pip_command().unwrap_or_else(|| ("pip3".to_string(), Vec::new())),
        other => (other.command().unwrap_or_default().to_string(), Vec::new()),
    };
    let mut all = prefix;
    all.extend(args.iter().map(|a| a.to_string()));
    Cmd::Exec {
        privileged: false,
        program,
        args: all,
        fallback: None,
    }
}

/// Homebrew formulae and casks use different verbs in the same tool, so try the
/// formula form first and the cask form if that fails.
fn brew(formula: &[&str], cask: &[&str]) -> Cmd {
    let fallback = Some(cask.iter().map(|a| a.to_string()).collect());
    match manager(Source::Homebrew, formula) {
        Cmd::Exec {
            privileged,
            program,
            args,
            ..
        } => Cmd::Exec {
            privileged,
            program,
            args,
            fallback,
        },
        other => other,
    }
}

/// Flatpak command for the app's installation: system-wide (privileged) or per-user.
/// The scope flag goes right after the verb, where flatpak expects it.
fn flatpak(pkg: &Package, args: &[&str]) -> (bool, Cmd) {
    let is_system = pkg.scope != "user";
    let scope = if is_system { "--system" } else { "--user" };
    let (verb, rest) = args.split_first().unwrap_or((&"", &[]));
    let mut ordered = vec![verb.to_string(), "-y".to_string(), scope.to_string()];
    ordered.extend(rest.iter().map(|a| a.to_string()));
    (
        is_system,
        Cmd::Exec {
            privileged: is_system,
            program: "flatpak".to_string(),
            args: ordered,
            fallback: None,
        },
    )
}

fn lane_for(pkg: &Package) -> Lane {
    match pkg.source {
        Source::Apt | Source::Snap => Lane::Privileged,
        Source::Flatpak if pkg.scope != "user" => Lane::Privileged,
        other => Lane::Manager(other),
    }
}

/// Install a package from a catalogue (APT, Flatpak). Returns `None` for AppImages.
pub fn install(pkg: &Package) -> Option<Step> {
    let name = pkg.name.as_str();
    let title = format!("Install {}", pkg.title);
    let lane = lane_for(pkg);
    let (forward, undo) = match pkg.source {
        Source::Apt => (
            system("apt-get", &["install", name]),
            Some(system("apt-get", &["remove", name])),
        ),
        Source::Snap => (
            system("snap", &["install", name]),
            Some(system("snap", &["remove", name])),
        ),
        Source::Flatpak => {
            let origin = pkg.origin.as_str();
            let (_, forward) = flatpak(pkg, &["install", origin, name]);
            let (_, undo) = flatpak(pkg, &["uninstall", name]);
            (forward, Some(undo))
        }
        Source::Npm => (
            manager(Source::Npm, &["install", "-g", name]),
            Some(manager(Source::Npm, &["uninstall", "-g", name])),
        ),
        Source::Bun => (
            manager(Source::Bun, &["add", "-g", name]),
            Some(manager(Source::Bun, &["remove", "-g", name])),
        ),
        Source::Cargo => (
            manager(Source::Cargo, &["install", name]),
            Some(manager(Source::Cargo, &["uninstall", name])),
        ),
        Source::Pip => (
            manager(Source::Pip, &["install", "--user", name]),
            Some(manager(Source::Pip, &["uninstall", "-y", name])),
        ),
        Source::Uv => (
            manager(Source::Uv, &["tool", "install", name]),
            Some(manager(Source::Uv, &["tool", "uninstall", name])),
        ),
        Source::Homebrew => (
            brew(&["install", name], &["install", "--cask", name]),
            Some(brew(&["uninstall", name], &["uninstall", "--cask", name])),
        ),
        Source::AppImage => return None,
    };
    Some(step(lane, title, forward, undo))
}

/// Install a name typed by the user, for managers without a catalogue. Returns `None`
/// for the managers that have their own catalogue or no install at all.
pub fn install_by_name(src: Source, name: &str) -> Option<Step> {
    let title = format!("Install {name}");
    let (lane, forward, undo) = match src {
        Source::Snap => (
            Lane::Privileged,
            system("snap", &["install", name]),
            Some(system("snap", &["remove", name])),
        ),
        Source::Npm => (
            Lane::Manager(src),
            manager(src, &["install", "-g", name]),
            Some(manager(src, &["uninstall", "-g", name])),
        ),
        Source::Bun => (
            Lane::Manager(src),
            manager(src, &["add", "-g", name]),
            Some(manager(src, &["remove", "-g", name])),
        ),
        Source::Cargo => (
            Lane::Manager(src),
            manager(src, &["install", name]),
            Some(manager(src, &["uninstall", name])),
        ),
        Source::Pip => (
            Lane::Manager(src),
            manager(src, &["install", "--user", name]),
            Some(manager(src, &["uninstall", "-y", name])),
        ),
        Source::Uv => (
            Lane::Manager(src),
            manager(src, &["tool", "install", name]),
            Some(manager(src, &["tool", "uninstall", name])),
        ),
        Source::Homebrew => (
            Lane::Manager(src),
            brew(&["install", name], &["install", "--cask", name]),
            Some(brew(&["uninstall", name], &["uninstall", "--cask", name])),
        ),
        Source::Apt | Source::Flatpak | Source::AppImage => return None,
    };
    Some(step(lane, title, forward, undo))
}

/// Remove an installed package. The undo reinstalls the version that was installed.
/// Returns `None` for packages that cannot be removed this way.
pub fn remove(pkg: &Package) -> Option<Step> {
    let name = pkg.name.as_str();
    let version = pkg.installed.clone().unwrap_or_default();
    let title = format!("Remove {}", pkg.title);
    let lane = lane_for(pkg);
    let (forward, undo) = match pkg.source {
        Source::Apt => (
            system("apt-get", &["remove", name]),
            (!version.is_empty()).then(|| system("apt-get", &["install", &format!("{name}={version}")])),
        ),
        Source::Snap => (
            system("snap", &["remove", name]),
            Some(system("snap", &["install", name])),
        ),
        Source::Flatpak => {
            let (_, forward) = flatpak(pkg, &["uninstall", name]);
            let origin = pkg.origin.as_str();
            let (_, undo) = flatpak(pkg, &["install", origin, name]);
            (forward, (!origin.is_empty()).then_some(undo))
        }
        Source::AppImage => return Some(delete_appimage(pkg)),
        Source::Npm => (
            manager(Source::Npm, &["uninstall", "-g", name]),
            (!version.is_empty()).then(|| manager(Source::Npm, &["install", "-g", &format!("{name}@{version}")])),
        ),
        Source::Bun => (
            manager(Source::Bun, &["remove", "-g", name]),
            (!version.is_empty()).then(|| manager(Source::Bun, &["add", "-g", &format!("{name}@{version}")])),
        ),
        Source::Cargo => (
            manager(Source::Cargo, &["uninstall", name]),
            (!version.is_empty()).then(|| manager(Source::Cargo, &["install", name, "--version", &version])),
        ),
        Source::Pip => (
            manager(Source::Pip, &["uninstall", "-y", name]),
            (!version.is_empty()).then(|| manager(Source::Pip, &["install", "--user", &format!("{name}=={version}")])),
        ),
        Source::Uv => (
            manager(Source::Uv, &["tool", "uninstall", name]),
            (!version.is_empty()).then(|| manager(Source::Uv, &["tool", "install", &format!("{name}=={version}")])),
        ),
        Source::Homebrew => (
            brew(&["uninstall", name], &["uninstall", "--cask", name]),
            Some(brew(&["install", name], &["install", "--cask", name])),
        ),
    };
    Some(step(lane, title, forward, undo))
}

/// Upgrade a package that has a newer version. The undo installs the old version where
/// the manager allows it. Returns `None` when the package has no upgrade.
pub fn upgrade(pkg: &Package) -> Option<Step> {
    if !pkg.upgradable {
        return None;
    }
    let name = pkg.name.as_str();
    let previous = pkg.installed.clone().unwrap_or_default();
    let title = format!("Upgrade {}", pkg.title);
    let lane = lane_for(pkg);
    let (forward, undo) = match pkg.source {
        Source::Apt => (
            system("apt-get", &["install", "--only-upgrade", name]),
            (!previous.is_empty()).then(|| system("apt-get", &["install", &format!("{name}={previous}")])),
        ),
        Source::Snap => (system("snap", &["refresh", name]), None),
        Source::Flatpak => {
            let (_, forward) = flatpak(pkg, &["update", name]);
            (forward, None)
        }
        Source::Npm => (
            manager(Source::Npm, &["install", "-g", &format!("{name}@latest")]),
            (!previous.is_empty()).then(|| manager(Source::Npm, &["install", "-g", &format!("{name}@{previous}")])),
        ),
        Source::Pip => (
            manager(Source::Pip, &["install", "--user", "-U", name]),
            (!previous.is_empty()).then(|| manager(Source::Pip, &["install", "--user", &format!("{name}=={previous}")])),
        ),
        Source::Homebrew => (brew(&["upgrade", name], &["upgrade", "--cask", name]), None),
        Source::Uv => (
            manager(Source::Uv, &["tool", "upgrade", name]),
            (!previous.is_empty()).then(|| manager(Source::Uv, &["tool", "install", &format!("{name}=={previous}")])),
        ),
        Source::Bun | Source::Cargo | Source::AppImage => return None,
    };
    Some(step(lane, title, forward, undo))
}

/// Copy an AppImage into the AppImage folder. Fails for other files, and when a file
/// with the same name is already there.
pub fn add_appimage(source: &Path) -> Result<Step, String> {
    let is_appimage = source
        .extension()
        .map(|e| e.eq_ignore_ascii_case("appimage"))
        .unwrap_or(false);
    if !is_appimage {
        return Err(format!("{} is not an .AppImage file", source.display()));
    }
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} has no file name", source.display()))?;
    let to = appimage::directory().join(&file_name);
    if to.exists() {
        return Err(format!("{file_name} is already in ~/Applications"));
    }
    Ok(step(
        Lane::Manager(Source::AppImage),
        format!("Add {file_name}"),
        Cmd::AddFile {
            from: source.to_path_buf(),
            to: to.clone(),
        },
        Some(Cmd::RemoveFile { path: to }),
    ))
}

/// Delete an AppImage. The file is moved aside and only deleted when the transaction commits.
fn delete_appimage(pkg: &Package) -> Step {
    let path = PathBuf::from(&pkg.origin);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let trash = trash_dir().join(format!("{nanos}-{}", pkg.name));
    step(
        Lane::Manager(Source::AppImage),
        format!("Delete {}", pkg.title),
        Cmd::Trash {
            path: path.clone(),
            trash: trash.clone(),
        },
        Some(Cmd::Rename { from: trash, to: path }),
    )
}

fn trash_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".cache/apt-manager/trash")
}

// ----- files dropped on the app ------------------------------------------------

/// One dropped file, planned as one job.
pub struct FileJob {
    pub title: String,
    pub steps: Vec<Step>,
    pub source: Source,
}

/// Plan the install for a file dropped on the app: an AppImage, a .deb, a .flatpak bundle
/// or a .flatpakref. Nothing runs until the job starts. `for_all` installs a Flatpak
/// system-wide (asks for the password) instead of for the current user only.
pub fn file_job(path: &Path, for_all: bool) -> Result<FileJob, String> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "appimage" => Ok(FileJob {
            title: format!("Adding {file_name}"),
            steps: vec![add_appimage(path)?],
            source: Source::AppImage,
        }),
        "deb" => deb_job(path, &file_name),
        "flatpak" | "flatpakref" => flatpak_job(path, &file_name, &ext, for_all),
        _ => Err(format!(
            "{file_name} is not an .AppImage, .deb, .flatpak or .flatpakref file"
        )),
    }
}

/// Where dropped installer files are copied first. It is inside the user's own cache
/// folder, so only the user can change the copy before the install reads it.
fn staging_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".cache/apt-manager/staging")
}

/// The file name with only letters, digits, '.' and '-', so it is safe as a path argument.
fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' })
        .collect()
}

/// The steps around one install: copy the file into the staging folder, run the install,
/// then move the copy to the trash (deleted when the job commits). A rollback removes the copy.
fn staged_steps(
    lane: Lane,
    file_name: &str,
    source: &Path,
    staged: &Path,
    title: String,
    install: Cmd,
    undo: Option<Cmd>,
) -> Vec<Step> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let trash = trash_dir().join(format!("{nanos}-{}", safe_name(file_name)));
    vec![
        step(
            lane,
            format!("Copy {file_name}"),
            Cmd::AddFile { from: source.to_path_buf(), to: staged.to_path_buf() },
            Some(Cmd::RemoveFile { path: staged.to_path_buf() }),
        ),
        step(lane, title, install, undo),
        step(
            lane,
            "Clean up the copy".to_string(),
            Cmd::Trash { path: staged.to_path_buf(), trash: trash.clone() },
            Some(Cmd::Rename { from: trash, to: staged.to_path_buf() }),
        ),
    ]
}

/// Read one control field of a .deb file. Reads only; nothing is installed.
fn dpkg_field(path: &Path, field: &str) -> Result<String, String> {
    let out = std::process::Command::new("dpkg-deb")
        .arg("-f")
        .arg(path)
        .arg(field)
        .output()
        .map_err(|e| format!("Could not run dpkg-deb: {e}"))?;
    if !out.status.success() {
        return Err(format!("{} is not a valid .deb file", path.display()));
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let value = text
        .strip_prefix(&format!("{field}:"))
        .unwrap_or(&text)
        .trim()
        .to_string();
    if value.is_empty() {
        return Err(format!("{} has no {field} field", path.display()));
    }
    Ok(value)
}

fn deb_job(path: &Path, file_name: &str) -> Result<FileJob, String> {
    let package = dpkg_field(path, "Package")?;
    let version = dpkg_field(path, "Version")?;
    let status = std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", package.as_str()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if status == "install ok installed" {
        return Err(format!("{package} is already installed. Remove it first, then drop the file again."));
    }
    let staged = staging_dir().join(safe_name(file_name));
    let staged_str = staged.to_string_lossy().into_owned();
    let title = format!("Installing {package} {version}");
    let steps = staged_steps(
        Lane::Privileged,
        file_name,
        path,
        &staged,
        title.clone(),
        system("apt-get", &["install", staged_str.as_str()]),
        Some(system("apt-get", &["remove", package.as_str()])),
    );
    Ok(FileJob { title, steps, source: Source::Apt })
}

/// A Flatpak command for a dropped file: system-wide (privileged) or for the current user.
fn flatpak_cmd(for_all: bool, verb: &str, rest: &[&str]) -> Cmd {
    let scope = if for_all { "--system" } else { "--user" };
    let mut args = vec![verb.to_string(), "-y".to_string(), scope.to_string()];
    args.extend(rest.iter().map(|a| a.to_string()));
    Cmd::Exec { privileged: for_all, program: "flatpak".to_string(), args, fallback: None }
}

/// The app ID inside a .flatpak bundle, read by flatpak itself.
fn bundle_app_id(path: &Path) -> Result<String, String> {
    let out = std::process::Command::new("flatpak")
        .args(["info", "--show-ref"])
        .arg(path)
        .output()
        .map_err(|e| format!("Could not run flatpak: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // A ref looks like app/<id>/<arch>/<branch>.
    text.split('/')
        .nth(1)
        .filter(|id| out.status.success() && !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("Could not read the app ID from {}", path.display()))
}

/// The app ID named in a .flatpakref file (its Name= line).
fn flatpakref_app_id(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Name=").map(|v| v.trim().to_string()))
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{} has no Name= line", path.display()))
}

fn flatpak_job(path: &Path, file_name: &str, ext: &str, for_all: bool) -> Result<FileJob, String> {
    let app_id = if ext == "flatpakref" {
        flatpakref_app_id(path)?
    } else {
        bundle_app_id(path)?
    };
    let staged = staging_dir().join(safe_name(file_name));
    let staged_str = staged.to_string_lossy().into_owned();
    let install_args: Vec<&str> = if ext == "flatpakref" {
        vec![staged_str.as_str()]
    } else {
        vec!["--bundle", staged_str.as_str()]
    };
    let lane = if for_all { Lane::Privileged } else { Lane::Manager(Source::Flatpak) };
    let title = format!("Installing {app_id}");
    let steps = staged_steps(
        lane,
        file_name,
        path,
        &staged,
        title.clone(),
        flatpak_cmd(for_all, "install", &install_args),
        Some(flatpak_cmd(for_all, "uninstall", &[app_id.as_str()])),
    );
    Ok(FileJob { title, steps, source: Source::Flatpak })
}

/// The Flatpak scope name for the Package field: "system" or "user".
pub fn scope_name(for_all: bool) -> &'static str {
    if for_all { "system" } else { "user" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_deb_plans_copy_install_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("apt-manager-deb-{}", std::process::id()));
        let pkg = dir.join("pkg");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(pkg.join("DEBIAN")).unwrap();
        fs::write(
            pkg.join("DEBIAN/control"),
            "Package: apt-manager-test-pkg\nVersion: 1.2\nArchitecture: all\nMaintainer: test <t@example.com>\nDescription: test\n",
        )
        .unwrap();
        let deb = dir.join("apt-manager-test-pkg_1.2_all.deb");
        let built = std::process::Command::new("dpkg-deb")
            .arg("--build")
            .arg(&pkg)
            .arg(&deb)
            .output();
        if !matches!(built, Ok(ref o) if o.status.success()) {
            return; // dpkg-deb is not available here
        }
        let job = file_job(&deb, false).expect("a valid .deb is planned");
        assert_eq!(job.source, Source::Apt);
        assert_eq!(job.steps.len(), 3);
        assert_eq!(job.title, "Installing apt-manager-test-pkg 1.2");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_files_are_refused() {
        let err = file_job(Path::new("/tmp/notes.txt"), false).err().unwrap();
        assert!(err.contains("not an .AppImage, .deb, .flatpak or .flatpakref"));
    }

    #[test]
    fn flatpakref_names_its_app() {
        let path = std::env::temp_dir().join(format!("apt-manager-test-{}.flatpakref", std::process::id()));
        fs::write(&path, "[Flatpak Ref]\nName=org.example.App\nUrl=https://example.org/repo\n").unwrap();
        assert_eq!(flatpakref_app_id(&path).unwrap(), "org.example.App");
        let _ = fs::remove_file(&path);
    }
}
