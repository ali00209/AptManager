# Apt Manager

A graphical package manager for Debian and Ubuntu that manages packages from
several package managers in one window: APT, Flatpak, Snap, npm, pip, Cargo,
Bun, Homebrew and AppImage. It is written in Rust with
[GPUI](https://www.gpui.rs) through [gpui-kit](https://github.com/longbridge/gpui-kit).
The layout follows the Shelly reference UI: a search toolbar, a package list (or grid),
a details panel, and a bottom status bar. All the package managers share one list.

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

### Startup, cache and background loading

- The app opens at once from a cache in `~/.cache/apt-manager/`, one folder per registry
  (`<tag>.base.jsonl` is a full snapshot, `<tag>.log.jsonl` holds changes since then).
- Every registry is checked on its own thread, in parallel. Each one adds its packages to the
  list as soon as it finishes. Nothing waits for the others.
- A check compares the fresh list with the cache, keyed by `source:name`, in one pass. Only the
  packages that are new, changed or gone are written, as lines appended to the log. The log is
  compacted into a new snapshot when it grows larger than the snapshot.
- Each registry has its own row above the status bar while it loads: its phase (for example
  "Checking installed packages…"), elapsed time, a progress bar, and its own **Cancel** button.
  Cancel stops that registry's commands and leaves its cache as it was. A finished row says
  what changed and hides after a few seconds if it succeeded; a failed or cancelled row stays
  until you press ✕.
- A check that returns nothing for a registry that has cached packages keeps the cache, so a
  failed command cannot wipe it.
- You can install, remove or update packages while the checks run. Only **Upgrade All** waits.
- After an action, only the packages it touched are read again, with their manager's own query,
  and their rows update at once. Managers without such a query (AppImage) are read again as a
  whole, but only that registry.
- Switching a registry off hides it and keeps its cache on disk.
- **uv** is a registry: it lists tools installed with `uv tool`, and installs, removes and
  upgrades them with `uv tool install`, `uninstall` and `upgrade`. It has no online search.
- **AppImages** get a desktop entry in `~/.local/share/applications` (named
  `apt-manager-appimage-<file>.desktop`), so they appear in the application menu. Entries are
  created when an AppImage is added and on each check, and removed when its file is deleted.
  They have no icon.

### One list for every registry

All switched-on managers share one list. Each row and card shows a chip with the
manager's name (APT, Flatpak, npm, pip, and so on), and the list has a Registry column.

- **Search runs on Enter.** Typing alone does nothing, and the list stays empty until you
  press Enter. Clearing the box and pressing Enter returns to the empty screen.
- **Registries dropdown** (beside the search) ticks the registries to search. All and None
  are shortcuts. Changing the ticks re-runs the last search at once. With no registry
  ticked, the screen says so.
- **Has updates** shows every package with an update in the ticked registries, without a
  search. Typed text narrows it.
- Results are ordered by relevance (exact name, then prefix, then contains, then
  description), then by title. The list shows at most 500 rows and the grid 300 cards, and
  says how many matched.
- npm, Bun and Cargo: installed packages match as you type, and **Enter** also searches
  the online registries (npm for npm and Bun, crates.io for Cargo). Online results are not
  installed and show no update status.
- Homebrew: the catalogue is every formula in homebrew-core and every cask in homebrew-cask
  (from `brew formulae` and `brew casks`), with installed state. A missing tap leaves its part out.
- pip and Snap: installed packages only. When a search finds nothing, a button offers to
  install the typed name with each ticked registry that supports it.
- Installed packages show an **Installed** badge.
- **Status** dropdown: tick **Installed**, **Not installed**, or both (All). With none ticked,
  the list shows installed packages, or every match when you search. Clearing the search box
  goes back to the installed list at once; Enter is only needed to search.
- **Flatpak: me only / all users** (toolbar) chooses where Flatpak apps are installed:
  for you only (no password) or system-wide (asks for the password). Installed Flatpak
  apps stay in the scope they were installed with. The details panel also has both
  install buttons.
- **At startup** the list shows the installed packages from the ticked registries. Enter
  searches the full catalogues, installed packages, and online results. Each ticked package
  installs as its own job.
- **Refresh** refreshes the ticked registries. APT runs `apt-get update`; the others reload.

### Views and icons

- **List** (☰) and **Grid** (▦) buttons switch the package view. The grid shows
  up to 300 cards, and the list up to 500 rows; refine the search to see the rest.
- Packages show their own icon when one is found on disk: Flatpak appstream and
  export icons, and APT icons in `/usr/share/icons/hicolor/64x64/apps` and
  `/usr/share/pixmaps`. Otherwise a coloured box shows the letter of the
  manager (for example **Py** for pip, **N** for npm).

### APT

- Search the configured repositories (about 88k packages on Ubuntu 24.04) by name or description, on Enter.
- Tick packages and install them together, or install/remove from the details panel.
- **Has updates** and **Upgrade All** (shown when a ticked registry has updates).
- **Refresh** runs `apt-get update` when APT is ticked, and reloads the other registries.
- **History** shows the operations recorded in `/var/log/apt/history.log`.

### Flatpak

- Lists apps from every configured remote, with installed state.
- Install, remove and update apps. Changes to system-wide installs go through `pkexec`.
  Apps installed for the current user are changed without a password prompt.

### AppImage, .deb and Flatpak files

- Lists every `.AppImage` file in `~/Applications`.
- **Drop to install**: drag files from your file manager onto the app. Each file is one job:
  - `.AppImage` is copied into `~/Applications` and made executable.
  - `.deb` is installed with `apt-get install` (asks for the password). A package that is
    already installed is refused. Undo removes it again.
  - `.flatpak` (bundle) and `.flatpakref` are installed with `flatpak install`, for you or
    for all users, as chosen in the toolbar. Undo uninstalls the app.
  - The file is first copied into `~/.cache/apt-manager/staging`, so the install reads a
    copy with a safe name. The copy is moved to the trash when the job finishes.
  - If any dropped file is refused, nothing is installed.
- **Launch** starts the app (the file is made executable first if needed).
- **Delete file** moves the file aside. It is deleted for real only when the operation
  finishes, so Cancel or a failure can put it back.

### Upgrade All and Updates

**Upgrade All** starts one upgrade job per package that has an update. Each job has its own
row, Cancel and rollback. **Updates (N)** in the status bar jumps to the first
manager with updates.

## Requirements

- Debian or Ubuntu with `apt`, `dpkg-query`, `apt-cache` and `pkexec` (polkit).
- Optional, for the managers that you use:
  - `flatpak`. Add Flathub with
    `flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`.
  - `snap`
  - `npm`, `pip` (or `pip3`), `cargo`, `bun`, `brew`
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
(apt-get install/remove/upgrade/update, `apt-get install --only-upgrade`, flatpak install/uninstall/update/remote-add/remote-delete
with `--system`, snap install/remove/refresh) and refuses any argument that starts with `-`
or contains unusual characters. Listing needs no privileges.

Every action shows a progress panel above the status bar: the title, a progress bar
(apt reports real percentages; other tools show a moving bar), the latest line of
output, and on failure the last lines of output and the error. Click ✕ to dismiss it.

### Transactions, lanes, Cancel and rollback

Each action is a transaction (a job) made of steps. Every step has the command that undoes it:

- Removing a package is undone by reinstalling the version that was installed.
- Installing is undone by removing the package again.
- Upgrading is undone by installing the previous version, where the manager allows
  choosing one (APT, npm, pip). Snap, Flatpak and Homebrew upgrades have no automatic
  undo; the app says so when it has to roll back past one.
- AppImage deletes are undone by moving the file back.

Steps run in lanes. Steps in one lane run one after another; different lanes run at the
same time. The system lane (APT, system-wide Flatpak and Snap) is one lane because those
tools share the package database and the root helper. Each other manager (npm, pip,
Cargo, Bun, Homebrew, AppImage) has its own lane, so for example npm and pip can run
together while APT waits.

If a step fails, no new step starts and every finished step is undone in reverse order.
**Cancel** in the progress panel does the same thing: unstarted steps are skipped,
unprivileged commands that are running are stopped at once, and finished steps are undone.
A system step that is already running (for example an `apt-get install`) is allowed to
finish, because stopping `dpkg` part way can leave the package database broken. The
rollback follows after it. The result says what was undone and what could not be.

### Parallel jobs and package locks

Installs, removals and upgrades run as jobs. You can start several at once; each job has
its own progress row with a bar, the latest output line and its own **Cancel** button.
A finished job shows its result. **⧉** copies its headline, result and full log to the clipboard
(the last 5,000 lines), and **✕** removes its row.

- A package can be in only one job at a time. If you start a job that includes a package
  another job is already changing, the new job refuses at once and says which job holds it.
  Other packages in that selection are not held up by this.
- **Install Selected** and **Upgrade All** start one job per package, not one job for the whole set,
  so each package has its own row, Cancel and rollback.
- Privileged commands (APT, system-wide Flatpak and Snap) still run one at a time through
  the root helper, so a second system job waits its turn. A queued job that is cancelled
  while waiting gives up at once and does not run.
- Unprivileged work of different jobs runs side by side in the manager lanes.
- Each job rolls back only its own finished steps when it fails or is cancelled.

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
- The single-package checks after an action are written for APT, Flatpak, npm, pip, uv, Cargo,
  Bun, Homebrew and Snap. Only APT and npm were run against a live system while this was built.
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
- AppImages can't be downloaded from inside the app. Drop a file you already have onto the AppImage page.
- Cancel does not interrupt an APT, system Flatpak or Snap step that is already running.
  The step finishes, then the operation is rolled back.
- Rollback of a Snap, Flatpak or Homebrew upgrade is not automatic. An APT downgrade
  needs the old version to still be in the repositories.
- pip on a system Python marked "externally managed" (PEP 668, the default on Ubuntu
  24.04) refuses installs unless `--break-system-packages` is used. The app does not
  pass that flag. Use a virtual environment or pipx for those tools.
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
- `src/ui/toolbar.rs`: search toolbar, registry dropdown, and the buttons on the right.
- `src/ui/list.rs`: package list, grid, history, and the browse layout.
- `src/ui/details.rs`: details panel.
- `src/ui/preferences.rs`: Package manager preferences and sources pages.
- `src/ui/progress.rs`: progress panel for running and finished jobs.
- `src/ui/bottom_bar.rs`: status bar.
- `src/ui/widgets.rs`, `src/ui/theme.rs`: shared buttons, icons, colours and sizes.
- `src/ui/loader.rs`: startup detection of installed managers.
- `src/load.rs`: one thread per registry, sending events to the window.
- `src/track.rs`: records the commands a load runs, so Cancel can stop them.
- `src/ui/loads.rs`: the per-registry progress rows.
- `src/cache.rs`: the on-disk cache, and the comparison with a fresh list.
- `src/search.rs`: online search for npm, Bun and Cargo.
- `src/plan.rs`: turns each operation into transaction steps, with the undo for each.
- `src/transaction.rs`: runs steps in lanes, cancels, and rolls back.
- `src/apt.rs`, `src/flatpak.rs`, `src/appimage.rs`, `src/managers.rs`: package-manager backends.
- `src/privileged.rs`, `src/job.rs`: root helper and job progress; `src/prefs.rs`: saved choices.
- `src/managers.rs` finds each manager's command. For pip it tries `pip3`, then `pip`,
  then `python3 -m pip`, and uses the first that works.
