//! Types shared by every package manager (APT, Flatpak, Snap, npm, pip, Cargo, Bun,
//! Homebrew, AppImage).

use std::path::PathBuf;

/// A package manager this app can manage. The declaration order is the order
/// the managers appear in the preferences list and the top tab bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Homebrew,
    Npm,
    Bun,
    Pip,
    Cargo,
    Apt,
    Snap,
    Flatpak,
    AppImage,
}

impl Source {
    pub const ALL: [Source; 9] = [
        Source::Homebrew,
        Source::Npm,
        Source::Bun,
        Source::Pip,
        Source::Cargo,
        Source::Apt,
        Source::Snap,
        Source::Flatpak,
        Source::AppImage,
    ];

    /// Short stable tag used in keys and the preferences file.
    pub fn tag(self) -> &'static str {
        match self {
            Source::Homebrew => "homebrew",
            Source::Npm => "npm",
            Source::Bun => "bun",
            Source::Pip => "pip",
            Source::Cargo => "cargo",
            Source::Apt => "apt",
            Source::Snap => "snap",
            Source::Flatpak => "flatpak",
            Source::AppImage => "appimage",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Source> {
        Source::ALL.iter().copied().find(|s| s.tag() == tag)
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Homebrew => "Homebrew",
            Source::Npm => "Npm",
            Source::Bun => "Bun",
            Source::Pip => "Pip",
            Source::Cargo => "Cargo",
            Source::Apt => "APT",
            Source::Snap => "Snap",
            Source::Flatpak => "Flatpak",
            Source::AppImage => "AppImage",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Source::Homebrew => "The Missing Package Manager for macOS (or Linux).",
            Source::Npm => "Node JS's package manager. Global packages for the JavaScript world.",
            Source::Bun => "Fast JavaScript runtime, bundler, and package manager.",
            Source::Pip => "Python's library manager. Global Python packages and tools.",
            Source::Cargo => "The Rust package manager. Binaries installed with cargo install.",
            Source::Apt => "The default package manager for Debian/Ubuntu-based Linux distributions.",
            Source::Snap => "The universal Linux package manager by Canonical.",
            Source::Flatpak => "The universal Linux package manager for desktop applications.",
            Source::AppImage => "Portable apps distributed as a single file, kept in ~/Applications.",
        }
    }

    /// Executable that must exist for this manager to be usable. `None` means always available.
    pub fn command(self) -> Option<&'static str> {
        match self {
            Source::Homebrew => Some("brew"),
            Source::Npm => Some("npm"),
            Source::Bun => Some("bun"),
            Source::Pip => Some("pip3"),
            Source::Cargo => Some("cargo"),
            Source::Apt => Some("apt-get"),
            Source::Snap => Some("snap"),
            Source::Flatpak => Some("flatpak"),
            Source::AppImage => None,
        }
    }

    /// True when the manager can list what is available, not only what is installed.
    /// The others are driven by name: type a package name and install it.
    pub fn has_catalogue(self) -> bool {
        matches!(self, Source::Apt | Source::Flatpak)
    }

    /// Letter shown on the default icon for this manager.
    pub fn glyph(self) -> &'static str {
        match self {
            Source::Homebrew => "H",
            Source::Npm => "N",
            Source::Bun => "B",
            Source::Pip => "Py",
            Source::Cargo => "Rs",
            Source::Apt => "A",
            Source::Snap => "S",
            Source::Flatpak => "F",
            Source::AppImage => "AI",
        }
    }

    /// Background colour of the default icon.
    pub fn color(self) -> u32 {
        match self {
            Source::Homebrew => 0xb8741a,
            Source::Npm => 0xb83a3a,
            Source::Bun => 0x8a6d3b,
            Source::Pip => 0x3775a9,
            Source::Cargo => 0x9a4a1e,
            Source::Apt => 0xc0502a,
            Source::Snap => 0x7d4fa0,
            Source::Flatpak => 0x3f72b8,
            Source::AppImage => 0x2a8a7f,
        }
    }
}

/// One entry in the package list, from any manager.
#[derive(Clone, Debug)]
pub struct Package {
    pub source: Source,
    /// Identifier unique within its manager: package name, Flatpak application ID,
    /// AppImage file name, and so on.
    pub name: String,
    /// Human-readable name shown in the list. Equal to `name` where there is none.
    pub title: String,
    /// Version that would be installed (or the latest known version).
    pub candidate: String,
    /// Version currently installed, if any.
    pub installed: Option<String>,
    /// Bytes to download (0 when unknown).
    pub download_size: u64,
    /// Bytes on disk once installed (0 when unknown).
    pub installed_size: u64,
    /// APT archive section such as `utils` or `net`. Empty for other managers.
    pub section: String,
    /// Flatpak remote, AppImage file path, Snap publisher, and so on.
    pub origin: String,
    /// Flatpak installation the app lives in: `system` or `user`. Empty otherwise.
    pub scope: String,
    pub description: String,
    pub homepage: String,
    /// True when the manager reports a newer version than the installed one.
    pub upgradable: bool,
    /// Icon file for this package, when one can be found on disk.
    pub icon: Option<PathBuf>,
}

impl Package {
    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    /// Key that is unique across all managers.
    pub fn key(&self) -> String {
        format!("{}:{}", self.source.tag(), self.name)
    }

    /// A package with empty manager-specific fields.
    pub fn new(source: Source, name: &str) -> Package {
        Package {
            source,
            title: name.to_string(),
            name: name.to_string(),
            candidate: String::new(),
            installed: None,
            download_size: 0,
            installed_size: 0,
            section: String::new(),
            origin: String::new(),
            scope: String::new(),
            description: String::new(),
            homepage: String::new(),
            upgradable: false,
            icon: None,
        }
    }

    /// Plain APT package with empty source-specific fields.
    pub fn apt_base(name: String) -> Package {
        Package::new(Source::Apt, &name)
    }
}

/// Outcome of an action that changes the system.
#[derive(Clone, Debug)]
pub struct ActionResult {
    pub success: bool,
    pub summary: String,
}

impl ActionResult {
    pub fn ok(summary: impl Into<String>) -> Self {
        Self { success: true, summary: summary.into() }
    }

    pub fn err(summary: impl Into<String>) -> Self {
        Self { success: false, summary: summary.into() }
    }
}

/// Human-readable size using decimal units, matching what apt and flatpak print.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "—".into();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Parse a size string such as `55.5 MB`, `152.4 kB` or `1.2 GB` into bytes.
pub fn parse_human_size(text: &str) -> u64 {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let Ok(value) = number.parse::<f64>() else {
        return 0;
    };
    let multiplier: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" | "k" => 1e3,
        "mb" | "m" => 1e6,
        "gb" | "g" => 1e9,
        "tb" | "t" => 1e12,
        _ => return 0,
    };
    (value * multiplier).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flatpak_style_sizes() {
        assert_eq!(parse_human_size("152.4 kB"), 152_400);
        assert_eq!(parse_human_size("1.2 GB"), 1_200_000_000);
        assert_eq!(parse_human_size("512 B"), 512);
        assert_eq!(parse_human_size("garbage"), 0);
        assert_eq!(parse_human_size(""), 0);
    }

    #[test]
    fn keys_are_unique_across_sources() {
        let mut a = Package::apt_base("gimp".into());
        let b = Package::new(Source::Flatpak, "gimp");
        assert_ne!(a.key(), b.key());
        a.installed = Some("1".into());
        assert!(a.is_installed());
    }

    #[test]
    fn source_tags_round_trip() {
        for s in Source::ALL {
            assert_eq!(Source::from_tag(s.tag()), Some(s));
        }
        assert_eq!(Source::from_tag("nope"), None);
    }
}
