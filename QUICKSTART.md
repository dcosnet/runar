# runar Quickstart

A focused guide to building, launching, and customizing **runar** — a pure-Rust file manager built with iced 0.13. In under ten minutes you will build the binary, tour the interface, navigate by keyboard, wire up a file association, and produce a release build you can install system-wide.

## 1. Prerequisites

You need Rust 1.70 or later on the stable channel. If you do not already have it, install it from [https://rustup.rs](https://rustup.rs). runar targets Linux on any modern distribution — Ubuntu, Fedora, Arch, Alpine, and others all work — and requires a working X11 or Wayland session to display its window.

runar is pure Rust with no system library dependencies. On Debian and Ubuntu you do **not** need `libgtk-4-dev` or any other `-dev` packages; nothing pulls in a C toolchain beyond what cargo already uses. On Alpine, musl builds work out of the box with no extra setup, which makes runar a natural fit for that distribution.

## 2. Build & Run

Clone the repository and launch a release build in three commands:

```sh
git clone https://dcos.net/runar.git
cd runar
cargo run --release
```

The first build takes roughly three to five minutes because the release profile uses fat LTO and a single codegen unit to squeeze out runtime performance. Subsequent builds finish in under thirty seconds thanks to incremental compilation, so the long initial wait is a one-time cost.

If you are iterating on code or just want to start quickly, `cargo run` (a debug build) compiles much faster but runs slower and produces a larger, unstripped binary. For day-to-day use, prefer `--release`.

You can open runar at a specific directory by passing it as an argument:

```sh
cargo run --release -- /etc
```

This is handy for poking around system configuration trees without navigating from your home directory.

## 3. First Launch Tour

When the window opens you are presented with four regions. The **top bar** shows a breadcrumb of your current location, for example `/ > etc`. Click any segment to jump up to that ancestor directory. Click the pencil icon — or press `/` — to switch the pathbar into edit mode and type a path manually. Press Enter to navigate, or Escape to return to breadcrumb mode without changing location.

The **left sidebar** is divided into three sections. **DEVICES** lists mounts polled from `/proc/mounts`, filtered to show only real storage: pseudo-filesystems such as `proc`, `sysfs`, and `tmpfs` are excluded. Network mounts (`nfs`, `cifs`, `sshfs`) are shown with a purple globe icon so you can spot them at a glance. **LOCATIONS** holds hardcoded defaults that are always present and not removable: `/mnt`, `/var/run/media`, `/opt`, `/usr/src`, your `$HOME`, and `$HOME/Downloads`. **BOOKMARKS** starts empty; you populate it by editing `~/.config/runar/bookmarks.toml`, which is covered later in this guide.

The **main grid** is the file list with columns for icon, name, size, modified date, and type. Hidden files (dotfiles) appear in muted gray so they stay visible without dominating the view. AppDirs — directories that contain an `AppRun` file or an executable with the same name as the directory — get a teal icon with a play arrow, marking them as launchable applications.

The **status bar** at the bottom shows item count and the current path. It also surfaces launch results and watch errors, so if an action fails or an inotify watch drops, you will see it there rather than having to check a separate log.

## 4. Keyboard Navigation

runar is keyboard-first. The full binding table:

| Key | Action |
|-----|--------|
| Arrow Up / `K` | move selection up |
| Arrow Down / `J` | move selection down |
| `H` / Backspace | go up one directory |
| `L` / Enter | activate selection (open / launch / navigate) |
| Shift+Enter | on an AppDir, enter it as a directory |
| `/` or Ctrl+L | toggle pathbar edit mode |
| Escape | exit pathbar edit mode |

Try it now. Open `/usr/bin`, hold `J` to scroll down the long listing, press `L` to launch a binary such as `htop` or `less`, then press `H` to climb back up to `/usr`. The vim-style `H`/`J`/`K`/`L` bindings make one-handed navigation natural once you are used to them, and the arrow keys remain available for anyone who prefers them.

## 5. Opening Files (Built-in MIME Defaults)

Double-click any file (or select it and press Enter) and runar launches it through a three-layer fallback:

1. **Your `actions.toml` overrides** (covered in the next section) — highest priority.
2. **Built-in default MIME table** — sensible power-user defaults for common types, no config required.
3. **Direct execution** — if the file itself carries an executable bit, run it (the ROX-Filer convention). Otherwise runar reports "no handler" — there is no `xdg-open` and no system MIME database to defer to; the fix is a line in `actions.toml`.

The built-in table honors standard environment variables ($EDITOR, $BROWSER, $IMAGE_VIEWER, $VIDEO_PLAYER, $AUDIO_PLAYER, $PDF_VIEWER) and then walks a chain of known binaries, lightest first — so it works on a minimal SliTaz-style install as well as a full desktop, with no xdg anywhere:

| MIME category | Default action |
|---|---|
| `text/*` (plain, markdown, csv, source code) | `$EDITOR` if set, else walks: `scitano` → `scite` → `geany` → `kate` → `mousepad` |
| `text/html` | `$BROWSER` if set, else: `firefox` → `librewolf` → `zen` → `mullvad-browser` → `swell` → `ladybird` → `gnomeweb`/`epiphany` → `netsurf` |
| `image/*` | `$IMAGE_VIEWER` if set, else: `marten` → `gpicview` → `gphoto2` |
| `video/*` | `$VIDEO_PLAYER` if set, else: `ferret` → `smplayer` → `mpv` → `vlc` |
| `audio/*` | `$AUDIO_PLAYER` if set, else: `mpv` → `mplayer` → `audacious` → `deadbeef` |
| `application/pdf` | `$PDF_VIEWER` if set, else: `evince` → `atril` |
| `application/json`, `/yaml`, `/toml`, `/xml` | Same editor chain as `text/*` |
| `application/x-shellscript` | Same editor chain as `text/*` |
| `application/zip`, `/x-tar`, `/gzip`, etc. | `ls -l {}` (list contents, don't extract) |

**Editor fallback chain:** for any text-type file, runar tries (in order) `$EDITOR` (if set in your shell), then `scitano`, `scite`, `geany`, `kate`, and finally `mousepad`. The first one found on `$PATH` wins. This means on a bare install with only `mousepad` present, double-clicking a `.txt` opens it in `mousepad`; install `scite` and it becomes the default; set `EDITOR=code` in your shell and VS Code wins. No config file required.

**Terminal emulator** (used by "Open in Terminal" in the right-click menu): `$TERMINAL` if set, else walks `rs-mrxvt` → `mrxvt` → `urxvt` → `ghostty` → `alacritty` → `tabby` → `guake`. The terminal opens with its shell already in the folder you clicked — right-clicking a directory/AppDir opens that folder, right-clicking a file opens the folder containing it.

**Where MIME types come from:** extension → MIME resolution is self-contained, in two layers. If `/etc/mime.types` exists, runar parses that simple flat file itself (once per process, then cached) and honors it; otherwise — or for extensions it doesn't cover — runar falls back to a table compiled into the binary. There is no `shared-mime-info`, no `xdg-mime`, and no desktop database anywhere in the pipeline.

To override per-extension with your own chain: see the next section.

## 6. Add a Custom File Association

The killer feature inherited from ROX-Filer is instant MIME action hooks with no desktop environment daemon required. Edit `~/.config/runar/actions.toml`, creating the file if it does not exist. Two forms are supported:

**Single-command form** — one command, always tries to run it:

```toml
[[actions]]
pattern = "txt"
command = "foot -e vim {}"

[[actions]]
pattern = "image/*"
command = "feh {}"

[[actions]]
pattern = ".pdf"
command = "zathura {}"

[[actions]]
pattern = "Makefile"
command = "make -C $(dirname {})"
```

The `pattern` field accepts four forms. A bare extension like `txt` or a dotted extension like `.txt` matches by file extension and is case-insensitive. A MIME glob such as `image/*` matches any MIME type under that category, so `image/png` and `image/jpeg` both hit the rule. An exact MIME string like `image/png` matches only that type. A plain filename like `Makefile` matches by exact name, which is how you catch extensionless files.

The `{}` token is substituted with the shell-quoted file path. Commands run via `sh -c`, so pipes, environment variables, and command substitution all work as they would in a terminal — note the `$(dirname {})` call in the Makefile example.

You do not need to restart runar for changes to take effect; `actions.toml` is re-read on each launch action, so simply navigate away and back. Double-click a `.txt` file and it opens in `vim` inside `foot`. No `xdg-mime` queries, no `.desktop` files, no daemon.

**Fallback-chain form** — try each command in order, succeed on the first whose binary is on `$PATH`. Useful when you want a preferred tool but a graceful fallback for systems where it isn't installed:

```toml
[[actions]]
pattern = "txt"
commands = ["scitano {}", "scite {}", "geany {}", "kate {}"]
```

If every command in the chain fails the `$PATH` check, runar falls through to the built-in MIME table, and finally runs the file directly if it carries an executable bit (the ROX-Filer convention). The chain is checked statically (runar parses the leading binary name and verifies it exists on `$PATH` before spawning) so a missing binary doesn't pay the cost of a failed `sh -c` invocation.

## 7. Top Menubar and Right-Click Context Menu

runar ships with a traditional top menubar — **File / Edit / View / Go / Bookmarks / Help** — inspired by Thunar and PCManFM. Click any menu button to open its entries as a horizontal strip of buttons directly below the bar (the same interaction as the Scitano editor); long menus scroll sideways with a hidden scrollbar. Click again or press Escape to close. Menu entries that aren't wired yet (Rename, Delete, Cut, Copy, Paste, Properties) are listed but emit a "not implemented yet" status-bar message so you can see the click registered. The wired entries include:

- **File → Open** (Enter) — activates the selected entry
- **File → Reload** (Ctrl+R) — rescan the current directory
- **Go → Up** (Alt+Up) — navigate to the parent directory
- **Go → Edit Path…** (Ctrl+L) — toggle the pathbar into edit mode
- **Go → LOCATIONS** — jump straight to any hardcoded default location
- **Bookmarks → Add Current Directory** (Ctrl+D) — pin the current dir
- **Bookmarks → (entry)** — jump to any user bookmark
- **Help → About runar** — show the About dialog (click anywhere or press Escape to close)

**Right-click any row** in the file grid to open a context menu with type-aware entries:

- **Directories**: Open, Open in Terminal, Cut, Copy, Paste, Rename, Delete, Properties
- **AppDirs**: Launch, Enter Directory (the Shift+Enter equivalent), Cut, Copy, Rename, Delete, Properties
- **Files**: Open (with detected MIME type shown), Open With…, Copy Path, Open in Terminal, Cut, Copy, Rename, Delete, Properties
- **Symlinks**: Follow Link, Edit Target…, Copy Path, Cut, Copy, Rename, Delete, Properties

The context menu selects the row underneath the cursor when you right-click, so subsequent keyboard actions (Enter, Delete, etc.) apply to it. Press Escape to close the menu without acting.

## 8. Drag Folders to the Bookmarks Sidebar

You can pin a folder to the sidebar by dragging it from the file grid into the BOOKMARKS section. The folder is inserted at the position where you drop it — between the two bookmarks your cursor lands on, or at the end if you drop below the last entry.

**How it works:**

1. **Press and hold** the left mouse button on a folder (or AppDir) row in the file grid.
2. **Move the cursor** slightly — runar detects the movement and promotes the click to a drag. The status bar shows `dragging /path/to/folder… drop on a bookmark slot`.
3. **Drag over the sidebar's BOOKMARKS section.** As the cursor enters each existing bookmark row, that row highlights in blue — this is where the folder will be inserted (before the highlighted entry). If you drag below the last bookmark, the empty append zone highlights instead.
4. **Release the mouse button.** The folder is inserted at the highlighted position, `bookmarks.toml` is saved atomically, and the status bar confirms `bookmarked /path/to/folder at position N`.

**Rules:**

- Only **directories and AppDirs** can be dragged to bookmarks — files are silently ignored (you can't navigate into a file, so bookmarking one is meaningless).
- If you drop a folder that's **already bookmarked**, it's **moved** to the new position rather than duplicated.
- If you drop **outside the BOOKMARKS section** (on DEVICES, LOCATIONS, the grid, or empty space), the drag is cancelled with a status-bar message.
- A plain **click without movement** still works as before (single-click selects, double-click opens). The drag only activates when the cursor moves while the button is held.

**Reordering:** you can also drag an existing bookmark row to a new position within the BOOKMARKS section — the same insert-at-index logic applies, so dropping bookmark #3 between bookmarks #1 and #2 moves it there.

## 9. Build a Release Binary

For a daily-driver binary you can drop into `/usr/local/bin`:

```sh
cargo build --release
sudo cp target/release/runar /usr/local/bin/
```

The release profile uses `opt-level=3`, `lto=fat`, `codegen-units=1`, `strip=symbols`, and `panic=abort`, producing a roughly 5-8 MB stripped binary. It has zero shared library dependencies beyond glibc (or musl) and libgcc, so it will run on any distribution with a compatible ABI.

For a fully static musl binary — ideal for Alpine, distro-hopping, or chroot environments — add the target and rebuild:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The resulting binary runs on any Linux kernel 3.2 or newer with no userspace dependencies whatsoever.

## 10. Where to Go Next

Read the full `README.md` for architecture details and the complete configuration schema. Edit `~/.config/runar/bookmarks.toml` to pin frequently used directories — no XDG defaults are injected, so it is a clean slate you control entirely. File bugs and feature requests at [https://dcos.net/runar](https://dcos.net/runar). The `BLOG.md` file explores the design philosophy behind runar and the GTK4-to-iced pivot that shaped its current form.
