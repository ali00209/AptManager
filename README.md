# Apt Manager

A graphical package manager for Debian and Ubuntu that manages packages from
several package managers in one window: APT, Flatpak, Snap, npm, pip, Cargo,
Bun, Homebrew and AppImage. It is written in Rust with
[GPUI](https://www.gpui.rs) through [gpui-kit](https://github.com/longbridge/gpui-kit).
The layout follows the Shelly reference UI: a top tab bar, sub-tabs, a search
toolbar, a package list (or grid), a details panel, and a bottom status bar.

## Package manager preferences

Click **⚙ Preferences** in the top bar to open the list of managers.

- Each manager has a switch. Switched-off managers are hidden from the top bar
  and are not loaded.
- Each row has a status badge:
  - **Ready**: the manager is installed and switched on.
  - **Disabled**: the manager is installed but switched off.
  - **Not found**: the manager's command isn't on this machine. The switch is locked.
- The search box at the top filters the list.
- The **›** button opens that manager's sources. Flatpak remotes can be added
  and removed there (type `name url`). The other managers show their sources
  read-only.
- Choices are saved to `~/.config/apt-manager/managers.conf`. On the first run,
  every manager that was found is switched on.

## Features

### Per-manager tabs

Each switched-on manager that is installed gets a tab. Tabs for managers that
have no catalogue (npm, pip, Cargo, Bun, Snap, Homebrew) list the packages
already installed. To install one of those, type its name in the search box and
click **Install '<name>' with <manager>**.

| Manager  | Lists installed | Install / remove | Updates                        |
|----------|-----------------|------------------|--------------------------------|
| APT      | yes (catalogue) | yes (pkexec)     | yes (Upgrade All)              |
| Flatpak  | yes (catalogue) | yes (pkexec for system apps) | yes           |
| Snap     | yes             | yes (pkexec)     | yes (`snap refresh`)           |
| npm      | yes (global)    | yes (`npm -g`)   | yes (`npm update -g`)          |
| pip      | yes (`pip3 list`) | yes (`pip3 --user`) | yes (`pip3 install -U`)    |
| Cargo    | yes (`cargo install --list`) | yes      | no (see limitations)           |
| Bun      | yes (global)    | yes (`bun -g`)   | no (see limitations)           |
| Homebrew | yes (formulae and casks) | yes (formula, then cask) | yes (`brew upgrade`) |
| AppImage | yes (`~/Applications`) | delete file | none                       |

### Views and icons

- **List** (☰) and **Grid** (▦) buttons switch the package view. The grid shows
  up to 300 cards; filter or search to see the rest.
- Packages show their own icon when one is found on disk: Flatpak appstream and
  export icons, and APT icons in `/usr/share/icons/hicolor/64x64/apps` and
  `/usr/share/pixmaps`. Otherwise a coloured box shows the letter of the
  manager (for example **Py** for pip, **N** for npm).

### APT

- Browse all packages from the configured repositories (about 88k on Ubuntu 24.04).
- Search by name or description; filter by Any / Installed / Not installed.
- Tick packages and install them together, or install/remove from the details panel.
- Updates sub-tab, plus **Upgrade All**.
- **Refresh** runs `apt-get update` on the APT tabs and reloads the others.
- **History** shows the operations recorded in `/var/log/apt/history.log`.

### Flatpak

- Lists apps from every configured remote, with installed state.
- Install, remove and update apps. Changes to system-wide installs go through `pkexec`.
  Apps installed for the current user are changed without a password prompt.

### AppImage

- Lists every `.AppImage` file in `~/Applications`.
- **Launch** starts the app (the file is made executable first if needed).
- **Delete file** removes the file.
- AppImages have no central catalogue, so to add one copy the file into `~/Applications`.

### Upgrade All and Updates

**Upgrade All** runs the upgrade for every manager that has updates waiting. It
stops at the first failure. **Updates (N)** in the status bar jumps to the first
manager with updates.

## Requirements

- Debian or Ubuntu with `apt`, `dpkg-query`, `apt-cache` and `pkexec` (polkit).
- Optional, for the managers that you use:
  - `flatpak`. Add Flathub with
    `flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`.
  - `snap`
  - `npm`, `pip3`, `cargo`, `bun`, `brew`
- A running polkit authentication agent (GNOME, KDE and most desktops provide one).
- Build dependencies for GPUI on Linux, for example on Ubuntu:

  ```sh
  sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libxcb-render-util0-dev \
    libxcb-shape0-dev libxcb-xfixes0-dev libxcb-icccm4-dev libxcb-image0-dev \
    libxcb-keysyms1-dev libxcb-randr0-dev libxcb-xinput-dev libfontconfig1-dev \
    libasound2-dev libssl-dev pkg-config libvulkan-dev
  ```

Install, remove and upgrade that need system rights (APT, system-wide Flatpak,
Snap) run through a privileged helper. The first such action asks for your password
once, through the desktop's polkit agent. Later actions in the same session use the
same helper and don't ask again. The helper accepts only fixed subcommands
(apt-get install/remove/upgrade/update, flatpak install/uninstall/update/remote-add/remote-delete
with `--system`, snap install/remove/refresh) and refuses any argument that starts with `-`
or contains unusual characters. Listing needs no privileges.

Every action shows a progress panel above the status bar: the title, a progress bar
(apt reports real percentages; other tools show a moving bar), the latest line of
output, and on failure the last lines of output and the error. Click ✕ to dismiss it.

## Build and run

```sh
cargo run
cargo test
```

Set `RUST_LOG=info` to see GPUI's renderer logs.

## Dependency notes

- Uses [`gpui-kit`](https://github.com/longbridge/gpui-kit) 0.9.0, which bundles
  GPUI (`gpui-pre` 0.3.11), `gpui-component` and the rest of the kit. It is the
  only GUI dependency.
- `serde_json` parses the JSON output of npm and pip.

## Limitations

- No "Install Local" (.deb file) button yet.
- No vcpkg, .NET tool or PowerShell managers.
- Sources can be edited only for Flatpak. APT, npm, pip, Homebrew and Snap sources
  are read-only in the app.
- Cargo and Bun packages show no update status, so **Upgrade All** does not
  touch them. Update them with `cargo install <name>` or `bun add -g <name>`.
- pip packages are installed with `--user`, so they never need root.
- Snap installs, removals and refreshes need the password once per session.
- The privileged helper is this same program started with `--helper`. For a
  system-wide install, copy the binary to a root-owned location (for example
  `/usr/local/bin`). A binary that a normal user can overwrite must not run as root.
- Package icons appear only when the package's icon is already on disk, so an
  uninstalled APT package shows the default box.
- Update checks (npm, pip, Homebrew, Snap) give up after 25 seconds, so an offline
  machine shows the installed packages without update information.
- AppImages can't be downloaded or installed from inside the app. Copy them into `~/Applications`.
- The Flatpak parser expects the tab-separated output that `flatpak --columns` prints
  when not attached to a terminal. It is covered by unit tests with fixture data, but
  it has not been checked against a live Flathub, which the build environment could not reach.
- The "Section / Source" column shows the APT section, the Flatpak remote, the
  Snap publisher, or the manager name. APT's repository name isn't in
  `apt-cache dumpavail` output, so the section stands in for it.
- Package icons come only from files already on disk. Packages that don't
  ship an icon show the default box.

## Project layout

- `src/main.rs`: startup (PATH setup, root-helper entry point) and the window.
- `src/ui/app.rs`: window state, actions, and the top-level layout.
- `src/ui/toolbar.rs`: top tab bar, sub-tabs, search toolbar.
- `src/ui/list.rs`: package list, grid, history, and the browse layout.
- `src/ui/details.rs`: details panel.
- `src/ui/preferences.rs`: Package manager preferences and sources pages.
- `src/ui/progress.rs`: progress panel for running and finished jobs.
- `src/ui/bottom_bar.rs`: status bar.
- `src/ui/widgets.rs`, `src/ui/theme.rs`: shared buttons, icons, colours and sizes.
- `src/ui/loader.rs`: background loading and batch installs.
- `src/apt.rs`, `src/flatpak.rs`, `src/appimage.rs`, `src/managers.rs`: package-manager backends.
- `src/privileged.rs`, `src/job.rs`: root helper and job progress; `src/prefs.rs`: saved choices.
