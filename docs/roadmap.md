# Roadmap

Hyprland first: new features land on Linux, while macOS and Windows build in CI and catch up
later. Releases are small and frequent. Every feature has to make the core loop (open, type,
Enter) faster; zunder isn't trying to be a Raycast clone.

**What makes zunder worth switching to**, and every release should sharpen at least one of these:
1. One tool for launching *and* ALT+TAB, with live window previews.
2. Plugins are any script that prints lines. No SDK, no Node.
3. A terminal look with CSS themes.
4. Deep Hyprland integration: real binds, workspaces, `hide_windows`.
5. One small native binary that opens instantly.

---

## 0.2: "five minutes from install to daily driver" (about 1 week)

The biggest hurdle today is setup: hand-editing Hyprland binds, autostart and window rules.

0. **One-line installer:**
   ```sh
   curl -fsSL https://raw.githubusercontent.com/elyesghazel/zunder/main/get.sh | sh
   ```
   Optionally a short URL later, e.g. `zunder.elyesghazel.ch/get` as a redirect, with no update
   server behind it. The new `get.sh` at the repo root:
   - Is POSIX `sh`, so it also works when your login shell is fish (`curl … | sh`).
   - Finds the newest version through the GitHub releases API.
   - **Arch:** downloads the `.pkg.tar.zst` and runs `sudo pacman -U` (the only sudo prompt).
   - **Other distros:**
     - checks for gtk4 4.14+ and gtk4-layer-shell with `pkg-config`, and prints the exact
       `apt`/`dnf`/`zypper` command if something is missing;
     - unpacks the tarball's binary to `~/.local/bin`.
   - Checks the sha256 against a `SHA256SUMS` file. The release workflow gets a step that
     generates and uploads it.
   - Then runs `zunder setup` with the terminal attached (`< /dev/tty`), so its questions work
     even though the script itself arrived through a pipe.
   - Re-running it updates. `get.sh --uninstall` removes zunder.
1. **`zunder setup`**, interactive, new in `src/setup.rs` and wired into `src/main.rs`. Plain
   terminal prompts with a default on Enter. `--yes` takes all defaults for scripts.
   - **Import:** detects vicinae, rofi, wofi and fuzzel.
     - Offers `zunder import vicinae` (favorites and launch counts, `src/import.rs`).
     - Offers to replace their autostart lines and binds in the Hyprland config, showing a diff
       first and keeping a `.bak`.
   - **Keys:** asks which keys you want: ALT+TAB switcher, ALT+SPACE or SUPER+SPACE launcher,
     SUPER+V clipboard. It checks `hyprctl binds -j` and says when a key is already taken.
   - **Clipboard:** if cliphist is installed but not running, offers to add its
     `wl-paste --watch` autostart lines.
   - **Theme:** a pick from the built-in themes, printed as colored swatches in the terminal.
   - **Example plugins:** offers to link `kill` and `ip`.
   - Then does the steps below.
   - Detects a Lua or classic Hyprland config. Reuse the detection in
     `platform::linux::install_binds` (`src/platform/linux.rs`).
   - Shows a diff of what it will add, then writes `zunder.lua` / `zunder.conf` with:
     - the autostart line;
     - ALT+TAB, ALT+SHIFT+TAB, ALT+SPACE, SUPER+V;
     - the settings window rule;
     - a `require` / `source` line in the main config, added only if missing.
   - Idempotent. Has `--dry-run`. Refuses to touch binds that are already taken; check with
     `hyprctl binds -j`.
   - Prints the next step.
2. **Update notice**, new in `src/update.rs`:
   - At most once a day, a background thread calls
     `https://api.github.com/repos/elyesghazel/zunder/releases/latest` with curl. Same pattern as
     `fetch_rates` in `src/providers/convert.rs`.
   - Caches the answer in `~/.cache/zunder/latest.json`.
   - If there's a newer version, the start screen gets an item "zunder 0.3 is out" that opens the
     release page.
   - Can be turned off in Settings → General (new `update_check` key in `src/config.rs`
     `SETTINGS`).
3. **ALT+TAB reliability** (`src/ui.rs`):
   - **Quick tap:** a fast Alt+Tab should flip to the previous window every time. The
     `alt_held` / `is_active` path needs a test hook and a fix if it misfires.
   - **Multiple monitors:** the overlay opens on the focused monitor; window previews show for
     every monitor.
   - **Fullscreen windows:** the overlay should show above them.
4. **Speed budget, measured:**
   - `zunder show` to first frame in under 30 ms.
   - Each keystroke searched in under 5 ms.
   - `Engine::search` gets a criterion bench in `benches/`.
   - Apps are cached instead of rescanned on every open: check the `applications/` dirs' mtimes
     in `providers/apps.rs` `reload`.
5. **First-run screen.** When `config.toml` doesn't exist yet, the start screen shows three
   items: "Run setup", "Pick a theme" and "Read the keys".
6. **Release:** `packaging/bump.sh 0.2.0`, a new demo GIF, then
   repost.

**Done when:**
- A fresh Arch user runs the one curl line, answers a few questions, and has working ALT+TAB
  in under 2 minutes.
- Re-running `get.sh` updates zunder.
- `get.sh` is tested in a clean `archlinux` and a clean `debian:trixie` container.
- `zunder setup` is tested against a real vicinae-era Hyprland config.
- The README's Install section starts with the curl line.
- CI is green and the bench numbers are in the README.

---

## 0.2.x: Windows, lightweight and installable (side track, in parallel)

**Goal:** a ~10 MB `zunder-setup.exe` that installs and starts like any other app.

1. **Centered window** (`src/platform/windows.rs` `init_overlay` and `src/ui.rs` `show`):
   - Get the HWND through `gdk4-win32` (`Win32Surface::handle`).
   - After `present()`, center it on the monitor under the cursor with `SetWindowPos` /
     `MonitorFromPoint` (`windows-sys`).
   - Set the window topmost.
2. **Glyphs and icons:**
   - Glyphs: bundle `JetBrainsMono Nerd Font` (~2 MB). The installer installs it per user, or the
     app loads it privately with `AddFontResourceExW(FR_PRIVATE)` at startup.
   - App icons: extract them from the `.lnk` targets with `SHGetFileInfoW` / `IShellItemImageFactory`
     into `%LOCALAPPDATA%\zunder\icons\*.png` once, then point `App.icon` at that path.
     `ui::icon()` already takes absolute paths.
3. **Smaller bundle** (`.github/workflows/release.yml`, Windows job):
   - Ship only the Adwaita icons we actually use (folder, generic file and image types, a few
     symbolic ones) instead of the whole theme.
   - Drop unused DLLs, checked with `ntldd -R`.
   - Expect roughly 22 MB → ~10 MB compressed.
4. **Inno Setup installer**, new `packaging/windows/zunder.iss`:
   - Built with `iscc` on the GitHub Windows runner (it's preinstalled there).
   - Per-user install to `%LOCALAPPDATA%\Programs\zunder`, so no admin prompt.
   - Pages with checkboxes: start with Windows (Startup shortcut with `server`), desktop icon,
     Start menu entry, default hotkey (Ctrl+Alt+Space set as the shortcut key).
   - Uninstaller included. Optionally opens zunder at the end.
   - Becomes a release asset, `zunder-<v>-setup.exe`. The zip stays for portable use.
5. **Later:** a winget manifest pointing at the GitHub release, so `winget install zunder` works.
   No update server of our own; the 0.2 update notice covers Windows too.

**Done when:** download `setup.exe` → Next, Next → zunder opens centered, with glyphs and app
icons, and comes back after a reboot. Tested on real Windows.

---

## 0.3: the switcher nobody else has (about 1–2 weeks)

All in `src/providers/windows.rs`, `src/ui.rs` and `src/platform/linux.rs`.

1. **Window actions in ALT+TAB and search:**
   - `Ctrl+W` close
   - `Ctrl+1…9` move to that workspace
   - `Ctrl+F` fullscreen
   - `Ctrl+M` send to the minimized special workspace
   - `Ctrl+Shift+Enter` focus and pull to the current workspace

   Implementation:
   - New `platform` functions: `close_window`, `move_window(ws)`, `fullscreen_window`. On Linux
     they use hyprctl Lua dispatch; on the other systems they're no-ops for now.
   - The footer shows the keys when a window is selected.
2. **An action menu (`Ctrl+K`)** for any item:
   - Windows: the actions above.
   - Files: open, open folder, copy path, open in terminal.
   - Apps: launch, launch new window, show the desktop file.
   - Plugins can declare extra actions with a new `"actions": [...]` field in their JSON lines
     (`providers/plugins.rs` `Line`).
3. **Switcher options** in Settings → Switcher:
   - Group by workspace.
   - Current workspace only.
   - Windows of the same app next to each other.

**Done when:** you can close or move any window without leaving the keyboard, and there's a GIF of
it for the 0.3 post.

---

## 0.4: plugins people can install (about 2 weeks)

1. **A registry repo, `elyesghazel/zunder-plugins`:**
   - One folder per plugin: script, README, screenshot.
   - An `index.json` built by CI in that repo.
2. **CLI:**
   - `zunder plugin search|install|update|remove <name>`
   - Downloads into `~/.config/zunder/plugins/<name>`, pinned to a commit.
   - Settings → Plugins gets install and remove buttons.
3. **Plugin settings:**
   - Header `@zunder.setting key "help"`, stored in `config.toml` under `[plugins.<name>]`.
   - Passed to the script as environment variables (`ZUNDER_<KEY>`).
4. **Starter set, generic:**
   - github, quicklinks, kill, ip
   - wifi (nmcli), bluetooth (bluetoothctl), audio output (wpctl)
   - pass / gopass
   - hyprpicker color, screenshot (grim + slurp), systemd units
   - an `agenda` that uses the ICS URL from settings 
5. **Streaming results:** a slow plugin can print lines as it goes, and they appear live.
   Extends the `deferred` job channel in `src/ui.rs`.

**Done when:** `zunder plugin install wifi` works, and the README links the registry with at least
10 plugins.

---

## 0.5: sync through GitHub (about 1 week)

**Needs:** a GitHub OAuth app with Device Flow turned on.

1. **Login:** `zunder login` (also a button in Settings → Account). Uses the GitHub device flow:
   zunder shows a code, you confirm on github.com.
2. **Where the token lives:** the system keyring through `oo7` / `secret-service`; the file
   `~/.local/share/zunder/token` with 0600 permissions as fallback.
3. **What syncs:**
   - A private gist "zunder-sync" with `config.toml`, `favorites.json`, `themes/*.css` and the
     plugin list (names and commits, not the scripts).
   - `binds.json` per system, because keybinds don't carry over between OSes.
4. **When it syncs:**
   - Pulls when the daemon starts and every hour.
   - Pushes whenever any of these files changes.
   - Last write wins, with the previous version kept as `*.bak`.

**Done when:** you log in on two machines (one headless, with `--no-ui`) and favorites,
themes and plugins match on both.

---

## 0.6: macOS and Windows catch up (when testers are around regularly)

Everything goes behind the existing `src/platform/{macos,windows}.rs` functions.

**macOS:**
- Window list and focus: `CGWindowListCopyWindowInfo` plus the AX API.
- Previews: `ScreenCaptureKit`.
- Hotkeys: the `global-hotkey` crate.
- Clipboard history: our own watcher with `arboard` polling.
- A centered `NSPanel`, through the `objc2` crates.
- Login item.
- Signed and notarized dmg, once there is an Apple Developer account. Without it the
  first launch stays right-click → Open.

**Windows:**
- Window list: `EnumWindows`.
- Previews: `DwmRegisterThumbnail` or `PrintWindow`.
- Hotkeys: `RegisterHotKey`. ALT+TAB itself needs a low-level keyboard hook, opt-in.
- Clipboard history: `AddClipboardFormatListener`.

---

## Later and exploratory (0.7+)

1. **Raycast extension bridge.** An optional `zunder-raycast` package: a small Node runtime that
   implements part of `@raycast/api` (`List`, `Detail`, `ActionPanel`, `preferences`,
   `LocalStorage`, `Clipboard`, `open`). It talks to zunder over the plugin JSON protocol.
   - Start with List-type extensions only.
   - vicinae does something similar, so it's proven possible. It's also big: weeks of work and a
     moving API.
   - Only if 0.2–0.5 show real demand.
2. **A lighter UI on Windows and macOS.** If the bundled GTK keeps the installers above ~10 MB or
   feels slow, a native UI for those two (Slint, or AppKit / Win32) reusing the same engine.
   Decide after 0.6, based on numbers.
3. **1.0 means:**
   - The `config.toml` and plugin protocol are frozen and documented.
   - No known crashers.
   - Packages for Arch (AUR), Windows (winget) and macOS (Homebrew cask).
   - People besides you use it daily.

**Not doing:** an AI chat, a notes app, snippets/text expansion, a calendar UI, an update server of
our own. Plugins and the GitHub release page cover these.

---

## Getting users (runs alongside every release)

- **Who it's for:** Hyprland users who rice their setup and live in the terminal. That's a small
  group, but it talks a lot, and it's where launchers like rofi, wofi and vicinae spread.
- **Each release:** a 20–30 s GIF of the one new thing.
  - Post on r/unixporn (tag [Hyprland], with a rice details comment) and r/hyprland.
  - Share in the Hyprland Discord #showcase.
- **Once:**
  - A pull request adding zunder to the app launcher list on the Hyprland wiki
    (`hyprwm/hyprland-wiki`).
  - A pull request to `hyprland-community/awesome-hyprland`.
  - Post the AUR package once registration reopens.
  - Show HN at 0.3, when the switcher actions make a strong demo.
- **Listen:** issue templates for bugs, ideas and plugin requests, and a pinned "what should
  zunder do next?" discussion. Build what several people ask for.
- **Watch:** GitHub stars, issues from people other than you, AUR votes and release download
  counts (`gh release view`).

---

## Verification for every release

1. CI green on Linux, macOS and Windows (`.github/workflows/ci.yml`), with clippy on strict on
   Linux.
2. A release dry run (`gh workflow run release.yml`) succeeds, and the downloaded files contain the
   binary or bundle. The same check I did for 0.1.
3. A manual checklist on Hyprland, with a demo daemon on a private socket:
   - ALT+TAB with previews
   - search and files
   - clipboard text and image previews
   - time, currency, theme switch
   - the settings window
4. Windows releases: install `setup.exe` on the Windows laptop and check that it's centered, has
   glyphs and icons, and autostarts. macOS releases: someone opens the dmg on a real Mac.
5. Then `packaging/bump.sh <version>`, check the release page, and post the GIF.
