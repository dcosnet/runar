# runar

**A hybrid Thunar/PCManFM + ROX-Filer file manager, built in Rust with iced.**

[![license: GPL-2.0-only](https://img.shields.io/badge/license-GPL--2.0--only-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.html)
[![language: Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![toolkit: iced 0.13](https://img.shields.io/badge/toolkit-iced%200.13-blue.svg)](https://github.com/iced-rs/iced)
[![tests: 21 passing](https://img.shields.io/badge/tests-83%20passing-brightgreen.svg)](#testing)
<!-- TODO: CI badge, crates.io badge once published -->

runar is a keyboard-first, statically-linkable Linux file manager written from scratch in pure Rust. It borrows its visual language from the dual-pane, breadcrumb-driven design of Thunar and PCManFM, while adopting ROX-Filer's AppDir paradigm and instantaneous, daemon-free MIME action hooks. The result is a single self-contained binary with no system library dependencies and no requirement for a desktop environment to be running. It targets power users who want a fast, predictable, scriptable file manager that respects the filesystem as the source of truth. The sidebar intentionally drops the conventional pile of hardcoded XDG user directories (Music, Pictures, Videos, Public, Templates) in favor of filesystem mounts, a small fixed set of power-user locations, and user-defined bookmarks — giving you a clean slate rather than a vendor-curated set of folders.

---

<!-- TODO: screenshots -->

Screenshots are coming soon. The interface is composed of three primary regions: a breadcrumb pathbar at the top that can be toggled into a text input field, a three-section sidebar on the left (DEVICES, LOCATIONS, BOOKMARKS), and a scrolling file list on the right with icon, name, size, modified, and type columns. The look is intentionally minimal — inline SVG glyphs replace any system icon theme, so the same UI renders identically on a stock Arch install, a minimal Sway session, or a headless recovery image booted into a TTY with `xinit`. Until real screenshots land, the best way to see runar is to `cargo run` it.

---

## Why runar?

runar combines the clean dual-pane/tree-and-grid layout of Thunar/PCManFM with the raw speed, AppDir capability, and lightweight MIME action hooks of ROX-Filer (the backbone of Puppy Linux and DSL). The sidebar strips out the conventional hardcoded XDG user directory sprawl — no Music, Pictures, Videos, Public, or Templates cluttering the view — in favor of filesystem mounts, a small fixed set of power-user locations (including `$HOME` and `$HOME/Downloads` as deliberate exceptions), and a fully user-editable Bookmarks section.

Conventional GTK-based file managers tend to bundle a pile of assumptions about the surrounding desktop: they expect a settings daemon, a volume monitor service, an icon theme, and a curated set of XDG user directories like Music, Pictures, and Public. runar throws those assumptions out. The only thing it assumes is a mounted `/proc` filesystem and a working Linux kernel, which means it runs equally well on a workstation, a server, a Raspberry Pi, or a recovery live USB. This design choice is deliberate: the file manager should reflect what is actually on disk, not what a desktop integration layer believes ought to be there.

The ROX-Filer lineage shows up most clearly in AppDir support and in the `actions.toml` MIME-hook system. AppDirs — directories that bundle an application together with its icons and resources, launched by executing an `AppRun` script or a same-named binary — let you install software by dropping a folder anywhere on the filesystem, no package manager required. The `actions.toml` file lets you bind any file extension, MIME glob, or exact filename to an arbitrary shell command, bypassing `xdg-open` entirely when you want finer control. Together these features restore the Unix philosophy of small, composable tools that the modern desktop has largely abandoned.

---

## Features

runar is built around a small, deliberate feature set that prioritizes speed, predictability, and scriptability over visual flash.

- **Pure-Rust, statically linkable binary** — a single `target/release/runar` executable with no shared library dependencies and no system icon theme requirement.
- **Keyboard-first navigation** — Vim-style `H`/`J`/`K`/`L` plus arrow keys, with `/` or Ctrl+L to toggle the pathbar into edit mode and Enter to activate the selection.
- **Dual-pane visual language** — breadcrumb pathbar on top, three-section sidebar (DEVICES, LOCATIONS, BOOKMARKS) on the left, and a scrolling file list on the right.
- **ROX-Filer AppDir support** — directories containing an `AppRun` script or a same-named executable are detected and can be launched as applications; Shift+Enter overrides this to enter them as directories.
- **`actions.toml` MIME hooks** — bind any file extension, MIME glob (e.g. `image/*`), or exact filename to an arbitrary shell command, taking precedence over `xdg-open`.
- **Pure-Rust mount polling** — replaces `gio::VolumeMonitor` with a direct `/proc/mounts` reader that filters out pseudo-filesystems and unescapes octal sequences in mount points.
- **Daemon-free sidebar** — no settings daemon, no volume monitor service, no icon theme; the sidebar reflects actual filesystem mounts and user-defined locations only.
- **User-editable bookmarks** — `bookmarks.toml` is purely user-added, with no injected XDG defaults; the Bookmarks section starts empty.
- **Hardcoded power-user LOCATIONS** — `/mnt`, `/var/run/media`, `/opt`, `/usr/src`, `$HOME`, and `$HOME/Downloads` are always present and complement the user bookmarks.
- **Inline SVG glyph set** — every icon lives in `icons.rs` as inline SVG, so the UI renders identically on any Linux install without depending on a system icon theme.
- **Atomic config saves** — `bookmarks.toml` and `actions.toml` are written via a temp-file-plus-rename pattern so a crash mid-write can never corrupt your configuration.
- **Async non-blocking VFS** — directory scanning runs on a tokio runtime and streams results back to the UI, so even a directory with tens of thousands of entries never freezes the interface.
- **`notify`-based filesystem watcher** — inotify events are bridged into a tokio channel and reflected in the grid automatically as files are created, modified, or deleted.

---

## Why iced, not GTK4?

The project manifest originally proposed `gtk4-rs`. We switched to **iced** for the static-binary ethos: pure-Rust toolchain, no system lib deps (`libgtk-4-dev`, `libglib2.0-dev`, hicolor icon theme, etc.). A release build produces a single self-contained binary that runs on a fresh minimal Linux install.

Trade-offs:
- ✅ True static binary — no shared lib deps, no system icon theme required
- ✅ Pure Rust — `cargo build` just works, no `pkg-config` gymnastics
- ✅ Async-native Elm architecture — clean state/update/view separation
- ❌ No native cross-process DND (XDS) — in-app DND works, cross-process does not
- ❌ No `gio::VolumeMonitor` — replaced with pure-Rust `/proc/mounts` poller
- ❌ Immature at file-manager scale — untested at Nautilus/Thunar traffic levels

For runar's use case (power-user file manager, Linux-native, security-conscious), the static-binary win outweighs the ecosystem gaps. Shipping one binary that runs anywhere — from a fully loaded KDE Plasma desktop to a 50 MB SliTaz live image to a headless server reached over SSH with X forwarding — is worth more than native drag-and-drop into GIMP. The Elm-style state/update/view architecture that iced provides also maps cleanly onto a file manager's event loop: filesystem events, keyboard input, and UI clicks all funnel through a single `update()` function that mutates a single `AppState` struct, which makes the code easy to reason about and easy to test. The gaps that remain — cross-process DND, mature high-throughput list rendering, thumbnail pipelines — are explicitly tracked in the roadmap below and are the active focus of ongoing work.

---

## Installation

runar builds with a stock Rust toolchain and no system dependencies beyond a working Linux installation with `/proc` mounted. You need Rust 1.70 or later on the stable channel; everything else is pulled in by Cargo as crate dependencies.

```sh
cargo run                          # debug build, opens at CWD
cargo run -- /some/path            # debug build, opens at /some/path
cargo build --release              # optimized static binary at target/release/runar
```

The release profile is already configured in `Cargo.toml` for a tight, stripped binary: `opt-level=3`, `lto=fat`, `codegen-units=1`, `strip=symbols`, and `panic=abort`. A release build typically takes one to three minutes on a modern laptop, since fat LTO and a single codegen unit trade compile time for a smaller, faster binary. The resulting `target/release/runar` is a single executable you can drop into `/usr/local/bin`, copy onto a USB stick, or ship inside a container image without any runtime library dependencies. For a fully static musl binary that has no dynamic linker requirement at all, add the musl target and rebuild:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The musl variant is what you want for recovery images, initramfs payloads, and any environment where glibc may not be present or where you want the binary to run unchanged across distributions. Both the glibc and musl release binaries behave identically at runtime; only the libc ABI linkage differs.

---

## Quick start

```sh
cargo run --release                # opens at your home directory
cargo run --release -- /etc        # opens at /etc
cargo run --release -- /mnt/data   # opens at a specific mount point
```

Once the window is up, press `/` or Ctrl+L to edit the path inline, type a new path, hit Enter to navigate, and use `H`/`J`/`K`/`L` or the arrow keys to move around the file grid. Drop a `[[bookmarks]]` stanza into `~/.config/runar/bookmarks.toml` to pin frequently visited directories in the sidebar, and an `[[actions]]` stanza into `~/.config/runar/actions.toml` to bind file types to your preferred editors and viewers. For a more detailed walkthrough — including how to set up AppDir shortcuts, how to write MIME-glob action rules, and how to integrate runar with a tiling window manager — see [QUICKSTART.md](QUICKSTART.md).

---

## Key bindings

runar is driven from the keyboard. Every navigation action has both an arrow-key binding and a Vim-style equivalent, so you can keep your hands in one place regardless of input preference.

| Key | Action |
|-----|--------|
| Arrow Up / `K` | move selection up |
| Arrow Down / `J` | move selection down |
| `H` / Backspace | go up one directory |
| `L` / Enter | activate selection (open / launch / navigate) |
| Shift+Enter | on an AppDir, enter it as a directory (overrides launch) |
| `/` or Ctrl+L | toggle pathbar edit mode |
| Escape | exit pathbar edit mode |

The global keyboard handler lives in `src/main.rs` and dispatches into `App::update()` for state mutation. When the pathbar is in edit mode, the file grid ignores navigation keys so that typing a path does not move the selection. Shift+Enter on a directory that runar has detected as an AppDir will descend into it as a normal directory rather than executing its `AppRun` script, which is the escape hatch you need when you want to inspect an AppDir's contents rather than launch the application it bundles.

---

## Configuration

Both of runar's configuration files live under `~/.config/runar/` and are created empty on first save. Neither file ever injects XDG defaults — the Bookmarks section starts empty, and the actions table starts empty, so `xdg-open` is the fallback for every file until you explicitly override it. The path is resolved through the standard `XDG_CONFIG_HOME` environment variable, falling back to `~/.config` when unset, exactly as the XDG Base Directory Specification requires.

### `bookmarks.toml`

Bookmarks are purely user-added entries that appear in the BOOKMARKS section of the sidebar, below the hardcoded LOCATIONS. The `label` field is optional; when omitted, runar falls back to the basename of the path.

```toml
[[bookmarks]]
path = "/mnt/data"
label = "Data"  # optional
```

### `actions.toml`

Actions bind a pattern to a shell command (or a chain of fallback commands). The `pattern` may be a bare file extension (with or without a leading dot), a MIME glob such as `image/*`, or an exact filename like `Makefile`. The `{}` placeholder in `command` is substituted with the shell-quoted absolute path of the matched file, and the resulting command line is executed via `sh -c`. The first matching action wins, so order your rules from most specific to least specific.

**Single-command form** (simplest):

```toml
[[actions]]
pattern = "txt"
command = "foot -e vim {}"

[[actions]]
pattern = "image/*"
command = "feh {}"

[[actions]]
pattern = "Makefile"
command = "make -C $(dirname {})"
```

**Fallback-chain form** — try each command in order, succeed on the first whose binary is on `$PATH`. Useful when you want a preferred tool but a graceful fallback for systems where it isn't installed:

```toml
[[actions]]
pattern = "txt"
commands = ["scitano {}", "scite {}", "geany {}", "nano {}"]

[[actions]]
pattern = "image/*"
commands = ["feh {}", "sxiv {}", "xdg-open {}"]
```

If every command in the chain fails the `$PATH` check, runar falls through to the built-in MIME table, then to `xdg-open`. The chain is checked statically (we parse the leading binary name from each command string and verify it exists on `$PATH` before spawning) so a missing binary doesn't pay the cost of a failed `sh -c` invocation.

Both files are written atomically on save: runar writes to a sibling temp file and then renames it over the original, so a crash or power loss mid-write cannot leave a half-written config behind. The config engine has unit tests covering roundtrip serialization, the fail-soft behavior when a config file does not exist yet, and the precedence rules for `actions.toml` patterns.

---

## Default locations

The LOCATIONS section of the sidebar contains a small, hardcoded set of power-user directories chosen by the author. These are always present and cannot be removed by editing `bookmarks.toml`; they complement the user-editable Bookmarks section rather than competing with it.

- `/mnt` — traditional mount point
- `/var/run/media` — udisks2 auto-mount point (systemd systems)
- `/opt` — optional software packages
- `/usr/src` — kernel sources / build trees
- `$HOME` — user's home directory
- `$HOME/Downloads` — downloads folder

This list reflects a deliberate bias toward systems administration and power-user workflows: `/mnt` and `/var/run/media` are where removable media and network mounts actually appear on a modern Linux box, `/opt` and `/usr/src` are where hand-installed software and kernel build trees live, and `$HOME` plus `$HOME/Downloads` cover the day-to-day user directories without forcing Music, Pictures, Videos, Public, and Templates into your sidebar. If you want additional shortcuts — a project directory, a media library, a network share — add them to `bookmarks.toml` and they will appear in the BOOKMARKS section right below these defaults. The split between hardcoded LOCATIONS and user-editable BOOKMARKS keeps the sidebar visually stable across config changes while still letting you customize it freely.

---

## Architecture

runar is organized into a thin UI layer over an async VFS core, with configuration and launch dispatch factored out into standalone modules. The split makes it straightforward to unit-test the filesystem, config, and launch logic without ever instantiating the iced runtime.

```
src/
  main.rs              iced entry point + key-press bridge (raw Key/Modifiers → Message)
  date.rs              Howard Hinnant civil_from_days algorithm (leap-year-correct date math, no chrono)
  vfs/
    mod.rs             async scan_directory (Phase 1 — non-blocking tokio directory reader)
    appdir.rs          ROX AppDir detection (AppRun or same-name executable)
    watcher.rs         notify-based inotify watcher, bridged to tokio channel
  config/
    mod.rs             Config struct, atomic save, XDG path resolution (~/.config/runar/)
    bookmarks.rs       bookmarks.toml (purely user-added, no XDG defaults)
    actions.rs         actions.toml (extension/MIME → shell command overrides)
    defaults.rs        Hardcoded default locations shown in sidebar LOCATIONS section
  mounts.rs            /proc/mounts poller + pure parse_mounts() (replaces gio::VolumeMonitor)
  icons.rs             Inline SVG glyph set with cached LazyLock<Handle> per variant
  mime.rs              Built-in default MIME action table (text→$EDITOR, image→$IMAGE_VIEWER, etc.)
  launch.rs            Launch dispatcher: AppDir exec → actions.toml → built-in MIME → xdg-open
  ui/
    mod.rs             App state, update(), view(), subscription(), handle_key()
    pathbar.rs         Breadcrumb view, toggleable to text input via / or Ctrl+L
    sidebar.rs         Three sections: DEVICES, LOCATIONS, BOOKMARKS
    grid.rs            Scrolling file list with icon/name/size/modified/type columns (mouse_area for right-click)
    menubar.rs         Top menubar (File/Edit/View/Go/Bookmarks/Help) + dropdown panels
    context_menu.rs    Right-click context menu with type-aware entries per file kind
    about.rs           About dialog (Help → About runar)
```

The `vfs` module is the heart of the project: `scan_directory` runs inside a tokio runtime and streams `Entry` records back to the iced subscription loop, so the UI thread never blocks even when scanning a directory containing tens of thousands of files. The `appdir` submodule classifies each directory entry as either a plain directory, an AppDir (containing an `AppRun` script or a same-named executable), or a regular file, which the launch dispatcher uses to decide what to do when you press Enter. `watcher.rs` wraps the `notify` crate's inotify backend and bridges events into the same tokio channel, so changes from other processes — `git pull`, a download finishing, a USB stick being mounted — show up in the grid automatically. Configuration is split across `mod.rs`, `bookmarks.rs`, `actions.toml`, and `defaults.rs`, each responsible for one file or concept, and `mounts.rs` handles `/proc/mounts` parsing independently of any desktop volume monitor. The `ui` module is intentionally thin: `mod.rs` holds the `AppState` struct and the `update`/`view`/`subscription` trinity, while `pathbar.rs`, `sidebar.rs`, and `grid.rs` are pure view functions that turn slices of state into iced widgets.

---

## Testing

runar ships with **83 unit tests, all passing**. Run them with:

```sh
cargo test --bin runar
```

The test suite covers every non-UI module and is structured to mirror the source tree, so each module's tests live alongside its implementation in a `#[cfg(test)] mod tests` block. Tests are deterministic, do not mutate process-global state (no `std::env::set_var` calls — overrides are threaded through function parameters instead, which avoids both parallel-test flakiness and the `unsafe` requirement that Rust 1.82+ imposes on env mutation), and do not touch the real filesystem for writes: where a temp directory is needed, the tests use `std::env::temp_dir()` and clean up after themselves. The full breakdown:

- **date::tests::\*** — Howard Hinnant `civil_from_days` algorithm: epoch anchors, leap-year handling (1972 leap, 1973 non-leap, 2000 leap despite div-by-100, 1900 and 2100 non-leap), pre-epoch dates, Y2K rollover, `format_date` shape (14 tests)
- **mime::tests::\*** — built-in default MIME action table: text→editor chain (scitano→scite→geany→nano, $EDITOR first), image→$IMAGE_VIEWER, video/audio/PDF→xdg-open-with-env-override, HTML→$BROWSER (checked before text/ rule), archives→`ls -l`, JSON/YAML/TOML/XML→editor, shellscript→editor, editor chain ordering, $EDITOR precedence, unknown MIME returns None, charset-suffix stripping, path-based resolution (17 tests)
- **config::tests::\*** — config load/save roundtrip, missing-config fail-soft, `config_dir_with` path composition (3 tests)
- **config::bookmarks::tests::\*** — `bookmarks.toml` roundtrip (1 test)
- **config::actions::tests::\*** — pattern matching (extension, dot-prefix, MIME glob, exact filename), single-command form, fallback-chain form, `commands_to_try` ordering (chain wins over single), empty action, TOML roundtrip for both forms, TOML parsing of both forms (12 tests)
- **config::defaults::tests::\*** — hardcoded locations include `/mnt` `/opt` `/usr/src` `/var/run/media`, `$HOME` and `$HOME/Downloads` resolution when home provided, omission when home is `None`, total count (4 tests)
- **launch::tests::\*** — `shell_quote` (simple paths, paths with spaces, paths with single-quotes), `extract_leading_binary` (simple, env assignment, env-var expansion returns None, empty, `sh -c`), `binary_on_path` (finds sh, rejects nonexistent, handles absolute path) (11 tests)
- **vfs::tests::\*** — directory scan reports done (1 test)
- **vfs::appdir::tests::\*** — `AppRun` detection, same-name executable detection, plain dir rejection (3 tests)
- **mounts::tests::\*** — `/proc/mounts` parsing (sample input, pseudo-FS filtering, network FS tagging, dedup, empty input, malformed lines), octal escape unescaping (spaces, tabs, backslashes), UTF-8 preservation in mount labels (9 tests)
- **icons::tests::\*** — all variants have distinct SVG markup, widget construction doesn't panic (2 tests)

The `shell_quote` tests in `launch.rs` are particularly important because they are the only line of defense between user-controlled filenames and `sh -c`. They assert that paths containing spaces are wrapped in single quotes, that paths containing single quotes themselves are escaped correctly using the `'\''` idiom, and that simple paths without special characters are passed through unchanged. The `actions.toml` tests verify the four pattern-matching strategies — bare extension, dotted extension, MIME glob, and exact filename — so that the precedence rules documented in the Configuration section above are enforced by code rather than by convention. The mounts tests cover both the happy path of parsing a typical `/proc/mounts` line and the edge case of mount points containing octal-escaped characters like `\040` for a space, which the kernel uses to disambiguate the whitespace-separated mount table format.

---

## Roadmap

runar is feature-complete for daily power-user use, but several capabilities that desktop users expect from a Thunar-class file manager are still on the way. The phases below track what is done and what is queued for the next several release cycles.

- [x] Phase 1: Core engine & async VFS (scanner, watcher, AppDir detector)
- [x] Phase 2: Shell layout (pathbar, sidebar, file grid, keyboard nav)
- [x] Phase 3: Launch dispatcher (AppDir exec, actions.toml, built-in MIME table, xdg-open fallback)
- [x] Phase 4: Config engine (bookmarks.toml, actions.toml, atomic save)
- [ ] Background copy/move/delete with progress toasts
- [ ] True multi-column icon grid with spatial H/J/K/L navigation
- [ ] Virtualized file list (iced 0.13's stock `scrollable` lays out all children; page or custom-widget the entry list for 10k+ entries)
- [ ] Cross-process drag-and-drop (XDS — currently limited to in-app)
- [ ] Thumbnail rendering (will need to add the `image` crate as a dependency)
- [ ] File permissions dialog
- [ ] Bulk rename tool
- [ ] Bookmark editor UI (currently edit bookmarks.toml by hand)

The completed phases cover everything you need to browse, navigate, and launch files. The remaining items fall into two buckets: filling in file-management operations that every file manager needs (background copy/move/delete, permissions dialog, bulk rename) and pushing the UI closer to parity with Thunar and ROX-Filer (spatial icon grid, thumbnails, cross-process DND). The thumbnail pipeline is the lowest-hanging fruit in concept but will require adding the `image` crate as a dependency (it is not currently in `Cargo.toml`) and wiring a thumbnail cache into the grid renderer alongside the existing `rayon` pool. The cross-process DND item is the hardest, because XDS support in pure Rust effectively does not exist today and will likely require either an FFI shim to libX11 or a from-scratch implementation of the XDS protocol on top of a pure-Rust X11 client. Pull requests targeting any of these items are very welcome.

---

## Credits and inspiration

### Design inspiration (no code copied)

- **Thunar** (Xfce) — dual-pane layout, breadcrumb pathbar, keyboard-first navigation
- **PCManFM** (LXDE) — lightweight GTK file manager philosophy
- **ROX-Filer** — AppDir paradigm, instant MIME action hooks (no DE daemon dependency)
- **Puppy Linux / DSL** — ROX-Filer as desktop backbone, AppDir application bundles
- **SliTaz** — minimal-footprint philosophy, fast boot from read-only media

### Code reuse

No code was copied from any of the above projects. runar is a clean-room implementation. The Rust crates used (iced, tokio, notify, mime_guess, serde, toml, rayon, walkdir, open) are each credited in `Cargo.toml` and remain under their respective licenses.

### Author

**Jeremy Anderson** <jeremy@dcos.net> — https://dcos.net/runar

---

## Contributing

Patches welcome at https://dcos.net/runar (see repository for issue tracker and merge request workflow).

Building from source requires Rust 1.70+ (stable). No system dependencies beyond a working Linux installation with /proc mounted.

The contribution workflow is intentionally lightweight: fork the repository, branch off `main`, write code that passes `cargo test --bin runar` and `cargo clippy -- -D warnings`, and open a merge request with a clear description of the change and the motivation behind it. New features should ship with unit tests in the same `#[cfg(test)] mod tests` style as the existing code, and any new user-visible string should be plain ASCII (Unicode is fine where it makes sense, but avoid emoji in the UI itself). If you are adding a new keyboard binding, document it in the Key bindings table in this README and update the global keyboard handler in `src/main.rs` in the same commit. If you are touching the launch dispatcher or the `actions.toml` parser, add a test case to `launch::tests` or `config::actions::tests` that exercises the new behavior, since those code paths are the boundary between user-controlled input and `sh -c` and need the most rigorous coverage.

---

## License

GPL-2.0-only — see the [LICENSE](LICENSE) file for details.
