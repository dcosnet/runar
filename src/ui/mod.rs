//! iced UI shell — Phase 2 layout + Phase 4 config wiring.
//!
//! Layout (top to bottom, left to right):
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │ [↑] [/  home  user  documents]                    [✎ edit]   │  ← pathbar
//! ├──────────────┬──────────────────────────────────────────────┤
//! │ DEVICES      │                                              │
//! │   • Root     │   ┌────┐ ┌────┐ ┌────┐                       │
//! │   • boot     │   │ 📁 │ │ 📁 │ │ 📄 │                       │
//! │   • usb      │   │ foo│ │ bar│ │ baz│                       │
//! │ BOOKMARKS    │   └────┘ └────┘ └────┘                       │
//! │   (empty)    │                                              │
//! │              │   ...                                        │
//! │              │                                              │
//! ├──────────────┴──────────────────────────────────────────────┤
//! │ 12 items • /home/user                    [status messages]  │  ← status bar
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! All VFS work happens off the UI thread via iced subscriptions; the
//! update function only mutates in-memory state.

mod pathbar;
mod sidebar;
mod grid;
mod menubar;
mod context_menu;
mod about;

use crate::config::Config;
use crate::mounts::{poll_mounts, MountInfo};
use crate::vfs::{
    watch_directory, scan_directory, EntryKind, FileEntry, ScanEvent, WatchEvent,
};
use iced::widget::{button, column, container, row, text};
use iced::{
    stream, Alignment, Color, Element, Font, Length, Subscription, Task, Theme, time,
};
use iced::futures::SinkExt; // for `tx.send(...).await` on futures::mpsc::Sender
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use menubar::MenuKind;

// ---------- Messages --------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Streamed from `scan_directory` (Phase 1, wrapped in a subscription).
    /// Tagged with the directory being scanned so `update()` can drop
    /// events that were already in flight when the user navigated away —
    /// without this guard, stale events from the previous directory would
    /// be appended to the new directory's entry list.
    ScanEvent {
        dir: PathBuf,
        event: ScanEvent,
    },
    /// Streamed from `watch_directory` (Phase 1, wrapped in a subscription).
    /// Tagged with the watched directory for the same reason as `ScanEvent`.
    WatchEvent {
        dir: PathBuf,
        event: WatchEvent,
    },
    /// A single entry has been (re)stat'd off the UI thread — upsert it
    /// into the grid (replace in place if the path is already listed,
    /// otherwise insert at its sorted position). Emitted for inotify
    /// Created/Modified/Renamed events so the grid stays live without
    /// rescanning the whole directory.
    UpsertEntry(FileEntry),
    /// Periodic mount poll tick.
    PollMounts,
    #[allow(dead_code)]
    MountsUpdated(Vec<MountInfo>),

    /// Raw key-press event from iced's global keyboard subscription.
    /// Interpretation (Escape closes the topmost layer, Enter activates
    /// the current selection, J/K navigate, etc.) happens in `update()`
    /// where `&mut App` is available — not in the stateless `fn` that
    /// iced's `on_key_press` requires. Keeping interpretation stateful
    /// ensures Escape closes only the relevant layer rather than
    /// toggling state blindly.
    KeyPressed(iced::keyboard::Key, iced::keyboard::Modifiers),

    // Pathbar
    PathBarToggleEdit,
    PathBarEdit(String),
    PathBarSubmit,
    PathBarSegment(PathBuf),
    GoUp,

    // Sidebar
    MountSelected(PathBuf),
    BookmarkSelected(PathBuf),
    AddBookmarkFor(PathBuf),
    #[allow(dead_code)]
    RemoveBookmark(PathBuf),

    // Grid
    /// Programmatic selection of a row (no activation). Currently only
    /// used internally — the grid emits `EntryClicked` instead, which
    /// handles both single-click-select and double-click-activate. This
    /// variant is kept for future use (e.g. find-and-select from a
    /// search dialog).
    #[allow(dead_code)]
    EntrySelected(usize),
    /// A grid row was clicked. The update() function inspects the
    /// timestamp on this click vs the previous one to decide whether
    /// it's a single-click (select) or double-click (activate). This
    /// is necessary because iced 0.13's button widget exposes only
    /// `on_press`, not native double-click events.
    EntryClicked(usize),
    EntryActivated(usize, bool), // index, shift_modifier
    Reload,

    // Launch feedback
    LaunchDone(LaunchOutcome),

    // Config
    ConfigSaved(Result<(), String>),

    // Menubar + context menu
    /// User clicked a menubar button (File / Edit / View / Go / Bookmarks / Help).
    /// If the same menu is already open, it closes (toggle behavior).
    MenuToggled(MenuKind),
    /// Close any open menu (menubar strip or context menu). Triggered
    /// by clicking elsewhere, pressing Escape, or after a menu action fires.
    CloseMenus,
    /// User right-clicked a grid row at the given index.
    EntryRightClicked(usize),
    /// "Open in Terminal" from a context menu: spawn the user's terminal
    /// emulator (via the terminal association chain) with its working
    /// directory set to `dir` — the right-clicked directory/AppDir, or
    /// the current directory for files.
    OpenInTerminal(PathBuf),
    /// User selected a menu item whose action isn't implemented yet.
    /// The string is the action label, shown in the status bar so the
    /// user knows their click registered.
    NotImplemented(&'static str),
    /// Show the About dialog.
    ShowAbout,
    /// Close the About dialog.
    CloseAbout,
    /// Toggle whether hidden (dotfile) entries are shown in the grid.
    /// Bound to the View → "Show/Hide Hidden Files" menu item and Ctrl+H.
    ToggleHiddenFiles,
    /// No-op. Used by disabled menu entries and separators so they don't
    /// need an `Option<Message>` field.
    Noop,

    // Drag-and-drop (grid → sidebar bookmarks)
    /// Global left mouse button pressed at the given position. Emitted by
    /// the `iced::event::listen_with` subscription for every press, so
    /// update() can record the press position for drag-threshold detection.
    GlobalMousePressed(iced::Point),
    /// Global left mouse button released. Emitted by the subscription.
    /// update() checks if a drag was in progress and, if so, commits the
    /// drop (or cancels if not over the sidebar).
    GlobalMouseReleased,
    /// Mouse moved over a grid row while the button was held. The `Point`
    /// is relative to the row's top-left. Emitted by `mouse_area::on_move`
    /// on each grid row. If the movement exceeds the drag threshold
    /// (5px), update() promotes the drag candidate to an active drag.
    GridRowMouseMoved(iced::Point),
    /// Cursor entered a sidebar bookmark row at the given index while a
    /// drag is in progress. Emitted by `mouse_area::on_enter` on each
    /// bookmark row. update() records the hover index so the drop lands
    /// at the right position.
    DragEnteredBookmark(usize),
    /// Cursor entered the empty area below the last bookmark (drop here
    /// to append to the end of the list).
    DragEnteredBookmarkAppendZone,
    /// Cursor left the bookmark row at the given index. Carries the index
    /// so update() only clears the hover state if the cursor is still
    /// hovering THAT row — mouse_area's on_exit of the previous widget can
    /// arrive after on_enter of the next one, and an unconditional clear
    /// would erase the newly-entered row's hover highlight.
    DragExitedBookmark(usize),
    /// Cursor left the append zone below the last bookmark. Clears the
    /// hover state only if it still points at the append slot.
    DragExitedBookmarkAppendZone,
}

/// Cloneable wrapper around `launch::LaunchResult` so it can travel
/// through iced's Message channel (which requires Clone).
#[derive(Debug, Clone)]
pub enum LaunchOutcome {
    Spawned(String),
    NoHandler,
    Failed(String),
}

impl From<crate::launch::LaunchResult> for LaunchOutcome {
    fn from(r: crate::launch::LaunchResult) -> Self {
        use crate::launch::LaunchResult;
        match r {
            LaunchResult::Spawned(s) => LaunchOutcome::Spawned(s),
            LaunchResult::NoHandler => LaunchOutcome::NoHandler,
            LaunchResult::Failed(s) => LaunchOutcome::Failed(s),
        }
    }
}

// ---------- State -----------------------------------------------------------

pub struct App {
    /// Currently-displayed directory. Changing this triggers a rescan
    /// (the scan subscription's ID incorporates the dir + a counter).
    current_dir: PathBuf,

    /// Monotonic counter bumped on every navigation, so the subscription
    /// is recreated even when returning to a previously-visited dir.
    scan_id: u64,

    /// Entries currently shown. Replaced wholesale on each scan.
    entries: Vec<FileEntry>,

    /// Index into `entries` of the highlighted row, if any.
    selected: Option<usize>,

    /// Pathbar UI mode: breadcrumb trail vs. raw text input.
    pathbar_editing: bool,
    pathbar_input: String,

    /// Mounts polled from `/proc/mounts`.
    mounts: Vec<MountInfo>,

    /// User config (bookmarks + actions).
    config: Config,

    /// Status line shown in the bottom bar.
    status: String,

    /// Whether a scan is currently in flight (for the reload spinner).
    scanning: bool,

    /// Last grid row clicked + when, for double-click detection.
    /// iced 0.13's button widget exposes only `on_press`, not native
    /// double-click events, so we detect them manually: a second press
    /// on the same row within DOUBLE_CLICK_MS of the first is treated
    /// as an activation.
    last_click: Option<(usize, std::time::Instant)>,

    /// Which top-level menu (File/Edit/View/Go/Bookmarks/Help) is
    /// currently open, if any. Clicking a menu button toggles this;
    /// selecting an item or clicking elsewhere clears it.
    open_menu: Option<MenuKind>,

    /// Which grid row has an open context menu (right-click), if any.
    /// Cleared by any navigation, menu action, or click elsewhere.
    context_menu_target: Option<usize>,

    /// True when the About dialog is open.
    show_about: bool,

    /// Whether hidden (dotfile) entries are currently shown in the grid.
    /// Toggled via View → "Show/Hide Hidden Files" or Ctrl+H. Defaults to
    /// false (hidden files hidden), matching Thunar / PCManFM defaults.
    show_hidden: bool,

    // --- Drag-and-drop state ---
    /// The entry path being dragged from the grid, once the drag threshold
    /// (5px movement while button held) is exceeded. None when not dragging.
    drag_source: Option<PathBuf>,
    /// The grid row index that was pressed, recorded on `on_press` so we
    /// know which entry's path to drag. Cleared on release.
    drag_candidate_idx: Option<usize>,
    /// The global cursor position when the button was pressed, for
    /// threshold computation. Set by the global mouse subscription.
    drag_press_pos: Option<iced::Point>,
    /// Which bookmark index the cursor is currently hovering over during
    /// a drag. None = not over any bookmark (or not dragging). Some(len)
    /// = over the append zone below the last bookmark.
    drag_hover_bookmark: Option<usize>,
}

/// Window for double-click detection. 350ms matches GTK/Qt defaults.
const DOUBLE_CLICK_MS: u64 = 350;

impl App {
    /// Construct with an initial directory. Called once by `run_with`.
    pub fn new(initial_dir: PathBuf) -> Self {
        let config = Config::load();
        let mounts = poll_mounts();
        Self {
            current_dir: initial_dir,
            scan_id: 0,
            entries: Vec::new(),
            selected: None,
            pathbar_editing: false,
            pathbar_input: String::new(),
            mounts,
            config,
            status: String::new(),
            scanning: false,
            last_click: None,
            open_menu: None,
            context_menu_target: None,
            show_about: false,
            show_hidden: false,
            drag_source: None,
            drag_candidate_idx: None,
            drag_press_pos: None,
            drag_hover_bookmark: None,
        }
    }

    /// Navigate to `dir`. Bumps scan_id so the subscription restarts,
    /// clears the entry list immediately (so the UI feels responsive
    /// rather than showing stale entries from the old dir).
    ///
    /// If `dir` is relative, it's resolved against `current_dir` first —
    /// this is defensive (the breadcrumb builder now produces absolute
    /// paths, but a relative path typed into the pathbar edit field would
    /// otherwise be interpreted against the process CWD, which is rarely
    /// what the user means). We deliberately don't `canonicalize` because
    /// that resolves symlinks, and the user may have intentionally
    /// navigated to a symlinked path.
    fn navigate(&mut self, dir: PathBuf) {
        let dir = if dir.is_absolute() {
            dir
        } else {
            let mut resolved = self.current_dir.clone();
            resolved.push(dir);
            resolved
        };
        self.current_dir = dir;
        self.scan_id = self.scan_id.wrapping_add(1);
        self.entries.clear();
        self.selected = None;
        self.scanning = true;
        // Reset double-click tracking so a click in the new dir doesn't
        // accidentally pair with a click in the old dir.
        self.last_click = None;
        // Close any open menus — they reference the old dir's state.
        self.open_menu = None;
        self.context_menu_target = None;
        self.status = format!("scanning {}...", self.current_dir.display());
    }

    /// Borrowed view of entries (for grid and main.rs sentinel resolution).
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// Borrowed view of the current dir.
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    /// Borrowed view of mounts.
    pub fn mounts(&self) -> &[MountInfo] {
        &self.mounts
    }

    /// Borrowed view of bookmarks.
    pub fn bookmarks(&self) -> &[crate::config::bookmarks::BookmarkEntry] {
        self.config.bookmarks.entries()
    }

    /// True if a scan is in flight.
    pub fn scanning(&self) -> bool {
        self.scanning
    }

    /// Current status string.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// True if pathbar is in edit mode.
    pub fn pathbar_editing(&self) -> bool {
        self.pathbar_editing
    }

    /// Current pathbar input text.
    pub fn pathbar_input(&self) -> &str {
        &self.pathbar_input
    }

    /// Currently-selected entry index, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Which top-level menu is open, if any.
    pub fn open_menu(&self) -> Option<MenuKind> {
        self.open_menu
    }

    /// Which grid row has an open context menu, if any.
    pub fn context_menu_target(&self) -> Option<usize> {
        self.context_menu_target
    }

    /// True if the About dialog should be shown.
    /// (Currently read directly as `app.show_about` in view(); kept as
    /// an accessor for symmetry with the other state getters and for
    /// future testability.)
    #[allow(dead_code)]
    pub fn show_about(&self) -> bool {
        self.show_about
    }

    /// True if hidden (dotfile) entries are currently shown in the grid.
    /// When false, the grid filters out entries whose name starts with `.`.
    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// True if a drag-from-grid is in progress (threshold exceeded).
    #[allow(dead_code)]
    pub fn is_dragging(&self) -> bool {
        self.drag_source.is_some()
    }

    /// The path being dragged, if any.
    #[allow(dead_code)]
    pub fn drag_source(&self) -> Option<&Path> {
        self.drag_source.as_deref()
    }

    /// Which bookmark index the cursor is hovering over during a drag.
    /// `Some(len)` means the append zone (drop after last bookmark).
    pub fn drag_hover_bookmark(&self) -> Option<usize> {
        self.drag_hover_bookmark
    }
}

// ---------- Update ----------------------------------------------------------

pub fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::ScanEvent { dir, event } => {
            // Drop events from a scan that was in flight when the user
            // navigated away: the old subscription's stream is dropped,
            // but messages it had already queued still arrive. Appending
            // them here would mix two directories into one listing.
            if dir != app.current_dir {
                return Task::none();
            }
            match event {
                ScanEvent::Entry(entry) => {
                    upsert_entry_sorted(&mut app.entries, entry, &mut app.selected);
                }
                ScanEvent::Done { total, .. } => {
                    app.scanning = false;
                    // Reflect the visible count (filtered by show_hidden)
                    // so the status bar matches what the user sees. When
                    // hidden files are visible, `total` is correct; when
                    // hidden, we count only the non-hidden entries.
                    let visible = if app.show_hidden {
                        total
                    } else {
                        app.entries.iter().filter(|e| !e.hidden).count()
                    };
                    app.status =
                        format!("{} items • {}", visible, app.current_dir.display());
                }
                ScanEvent::EntryError { path, message } => {
                    log::warn!("scan error on {}: {message}", path.display());
                }
                ScanEvent::Fatal { dir, message } => {
                    app.scanning = false;
                    app.entries.clear();
                    app.status =
                        format!("cannot read {}: {message}", dir.display());
                }
            }
            Task::none()
        }

        Message::WatchEvent { dir, event } => {
            // Same staleness guard as ScanEvent: the watcher subscription
            // is recreated on navigation, but events already queued from
            // the old directory must not touch the new directory's list.
            if dir == app.current_dir {
                return handle_watch_event(app, event);
            }
            Task::none()
        }

        Message::UpsertEntry(entry) => {
            upsert_entry_sorted(&mut app.entries, entry, &mut app.selected);
            Task::none()
        }

        Message::PollMounts => {
            app.mounts = poll_mounts();
            Task::none()
        }
        Message::MountsUpdated(m) => {
            app.mounts = m;
            Task::none()
        }

        Message::KeyPressed(key, modifiers) => {
            handle_key(app, key, modifiers)
        }

        Message::PathBarToggleEdit => {
            // Close any open menu — the Go → Edit Path menu item emits
            // this, and the strip would otherwise sit directly above the
            // edit field.
            app.open_menu = None;
            app.context_menu_target = None;
            app.pathbar_editing = !app.pathbar_editing;
            if app.pathbar_editing {
                app.pathbar_input = app.current_dir.display().to_string();
            }
            Task::none()
        }
        Message::PathBarEdit(s) => {
            app.pathbar_input = s;
            Task::none()
        }
        Message::PathBarSubmit => {
            app.pathbar_editing = false;
            // Trim stray whitespace (easy to pick up when pasting a path)
            // and expand a leading `~/` — `PathBuf` does neither, so a
            // literal `~/Downloads` would otherwise be reported as
            // "not a directory".
            let raw = app.pathbar_input.trim().to_string();
            if raw.is_empty() {
                return Task::none();
            }
            let target = PathBuf::from(expand_tilde(&raw));
            if target.is_dir() {
                app.navigate(target);
            } else if target.exists() {
                app.status = format!("not a directory: {}", target.display());
            } else {
                app.status = format!("no such directory: {}", target.display());
            }
            Task::none()
        }
        Message::PathBarSegment(p) => {
            app.navigate(p);
            Task::none()
        }
        Message::GoUp => {
            if let Some(parent) = app.current_dir.parent() {
                app.navigate(parent.to_path_buf());
            }
            Task::none()
        }

        Message::MountSelected(p) => {
            app.navigate(p);
            Task::none()
        }
        Message::BookmarkSelected(p) => {
            app.navigate(p);
            Task::none()
        }
        Message::AddBookmarkFor(p) => {
            // Menu-initiated (Bookmarks → Add Current Directory, Ctrl+D):
            // close the menu like any completed menu action.
            app.open_menu = None;
            app.context_menu_target = None;
            if app.config.add_bookmark(p.clone()) {
                let cfg = app.config.clone();
                return Task::perform(
                    async move { cfg.save().map_err(|e| e.to_string()) },
                    Message::ConfigSaved,
                );
            }
            Task::none()
        }
        Message::RemoveBookmark(p) => {
            app.open_menu = None;
            app.context_menu_target = None;
            if app.config.remove_bookmark(&p) {
                let cfg = app.config.clone();
                return Task::perform(
                    async move { cfg.save().map_err(|e| e.to_string()) },
                    Message::ConfigSaved,
                );
            }
            Task::none()
        }

        Message::EntrySelected(i) => {
            if i < app.entries.len() {
                app.selected = Some(i);
            }
            Task::none()
        }
        Message::EntryClicked(i) => {
            // A click anywhere outside an open menu closes it — iced has
            // no native click-outside detection, so the grid row's press
            // doubles as the "clicked elsewhere" signal.
            app.open_menu = None;
            app.context_menu_target = None;

            // Record this row as a drag candidate. If the user holds the
            // button and moves (detected via GridRowMouseMoved), this
            // entry's path becomes the drag source.
            app.drag_candidate_idx = Some(i);

            // Double-click detection: if this click is on the same row as
            // the previous one AND within DOUBLE_CLICK_MS, treat it as an
            // activation. Otherwise just select the row and record the
            // click for next time.
            let now = std::time::Instant::now();
            let is_double = match app.last_click {
                Some((prev_i, prev_t)) => {
                    prev_i == i
                        && now.duration_since(prev_t)
                            < std::time::Duration::from_millis(DOUBLE_CLICK_MS)
                }
                None => false,
            };

            if is_double {
                // Reset click history so a triple-click doesn't re-activate.
                app.last_click = None;
                // Single-click select first (so the row visually highlights
                // even if activation navigates away immediately), then
                // activate. The shift modifier for "enter AppDir as dir"
                // only applies to keyboard Shift+Enter; mouse double-click
                // always activates normally.
                if i < app.entries.len() {
                    app.selected = Some(i);
                }
                if let Some(entry) = app.entries.get(i).cloned() {
                    return activate_entry(app, entry, false);
                }
            } else {
                app.last_click = Some((i, now));
                if i < app.entries.len() {
                    app.selected = Some(i);
                }
            }
            Task::none()
        }
        Message::EntryActivated(i, shift) => {
            // Closes any open menu — the File → Open and context-menu
            // "Open" entries emit this variant, and a menu action closing
            // its menu is standard behavior.
            app.open_menu = None;
            app.context_menu_target = None;
            if let Some(entry) = app.entries.get(i).cloned() {
                return activate_entry(app, entry, shift);
            }
            Task::none()
        }
        Message::Reload => {
            // Close the View menu this was triggered from (or any other
            // open menu / context menu).
            app.open_menu = None;
            app.context_menu_target = None;
            app.scan_id = app.scan_id.wrapping_add(1);
            app.entries.clear();
            app.selected = None;
            app.scanning = true;
            app.status = format!("reloading {}...", app.current_dir.display());
            Task::none()
        }

        Message::LaunchDone(outcome) => {
            match outcome {
                LaunchOutcome::Spawned(msg) => app.status = msg,
                LaunchOutcome::NoHandler => {
                    app.status = "no handler for this file".into()
                }
                LaunchOutcome::Failed(e) => {
                    app.status = format!("launch error: {e}")
                }
            }
            Task::none()
        }

        Message::ConfigSaved(res) => {
            match res {
                Ok(()) => app.status = "bookmarks saved".into(),
                Err(e) => app.status = format!("save failed: {e}"),
            }
            Task::none()
        }

        Message::MenuToggled(kind) => {
            // Toggle: clicking the same menu button again closes it.
            // Clicking a different one switches to it.
            if app.open_menu == Some(kind) {
                app.open_menu = None;
            } else {
                app.open_menu = Some(kind);
                // Close context menu when opening a menubar strip.
                app.context_menu_target = None;
            }
            Task::none()
        }
        Message::CloseMenus => {
            app.open_menu = None;
            app.context_menu_target = None;
            Task::none()
        }
        Message::EntryRightClicked(i) => {
            if i < app.entries.len() {
                // Select the row too (matches Thunar / PCManFM behavior)
                // so subsequent keyboard actions apply to it.
                app.selected = Some(i);
                app.context_menu_target = Some(i);
                // Close menubar if open.
                app.open_menu = None;
            }
            Task::none()
        }
        Message::OpenInTerminal(dir) => {
            // Menu action completed — close the menu that launched it.
            app.open_menu = None;
            app.context_menu_target = None;
            let config = app.config.clone();
            Task::perform(
                async move {
                    LaunchOutcome::from(
                        crate::launch::launch_terminal(&dir, &config).await,
                    )
                },
                Message::LaunchDone,
            )
        }
        Message::NotImplemented(what) => {
            app.open_menu = None;
            app.context_menu_target = None;
            app.status = format!("{what}: not implemented yet");
            Task::none()
        }
        Message::ShowAbout => {
            app.open_menu = None;
            app.context_menu_target = None;
            app.show_about = true;
            Task::none()
        }
        Message::CloseAbout => {
            app.show_about = false;
            Task::none()
        }
        Message::ToggleHiddenFiles => {
            // Flip the visibility flag. The grid view reads `show_hidden`
            // on every render via `app.show_hidden()`, so iced re-renders
            // automatically — no rescan needed (entries stay in memory).
            // The status bar count is recomputed here so it matches the
            // visible count immediately.
            app.show_hidden = !app.show_hidden;
            app.open_menu = None; // close the menubar strip after click
            let visible = if app.show_hidden {
                app.entries.len()
            } else {
                app.entries.iter().filter(|e| !e.hidden).count()
            };
            if app.scanning {
                app.status = format!("scanning {}...", app.current_dir.display());
            } else {
                app.status =
                    format!("{} items • {}", visible, app.current_dir.display());
            }
            Task::none()
        }
        Message::Noop => Task::none(),

        // --- Drag-and-drop ---
        Message::GlobalMousePressed(pos) => {
            // Record press position for threshold computation. The drag
            // candidate index is set separately by the grid row's on_press.
            app.drag_press_pos = Some(pos);
            Task::none()
        }
        Message::GridRowMouseMoved(_pos) => {
            // Fired by mouse_area::on_move on grid rows. on_move fires
            // whenever the cursor moves while over the row — since the
            // row is small (~24px), any on_move while the button is held
            // means the user is moving the cursor, which we treat as a
            // drag. (A proper threshold would require the global cursor
            // position, which iced 0.13's on_move doesn't provide — it
            // gives widget-relative coordinates only.)
            if app.drag_source.is_none() {
                if let Some(idx) = app.drag_candidate_idx {
                    if let Some(entry) = app.entries.get(idx).cloned() {
                        // Only directories and AppDirs can be dragged to
                        // the bookmarks (files don't make sense as
                        // bookmarks — you can't navigate into them).
                        if matches!(
                            entry.kind,
                            EntryKind::Directory | EntryKind::AppDir { .. }
                        ) {
                            app.drag_source = Some(entry.path.clone());
                            app.status = format!(
                                "dragging {}… drop on a bookmark slot",
                                entry.path.display()
                            );
                        }
                    }
                }
            }
            Task::none()
        }
        Message::DragEnteredBookmark(idx) => {
            if app.drag_source.is_some() {
                app.drag_hover_bookmark = Some(idx);
            }
            Task::none()
        }
        Message::DragEnteredBookmarkAppendZone => {
            if app.drag_source.is_some() {
                app.drag_hover_bookmark = Some(app.bookmarks().len());
            }
            Task::none()
        }
        Message::DragExitedBookmark(idx) => {
            // Guarded clear: on_exit of the row the cursor left may be
            // delivered after on_enter of the row it moved to (both derive
            // from the same mouse-move event). Only clear the hover if it
            // still refers to the row that reported the exit, so we never
            // erase the newly-entered drop target.
            if app.drag_source.is_some() && app.drag_hover_bookmark == Some(idx) {
                app.drag_hover_bookmark = None;
            }
            Task::none()
        }
        Message::DragExitedBookmarkAppendZone => {
            if app.drag_source.is_some()
                && app.drag_hover_bookmark == Some(app.bookmarks().len())
            {
                app.drag_hover_bookmark = None;
            }
            Task::none()
        }
        Message::GlobalMouseReleased => {
            // Button released. If we were dragging, commit the drop.
            if let Some(path) = app.drag_source.take() {
                if let Some(idx) = app.drag_hover_bookmark {
                    // Insert at the hovered index. The Bookmarks::insert_at
                    // handles clamping and dedup (move-if-exists).
                    app.config.insert_bookmark_at(path.clone(), idx);
                    let cfg = app.config.clone();
                    app.status = format!("bookmarked {} at position {}", path.display(), idx);
                    // Clear drag visual state.
                    app.drag_hover_bookmark = None;
                    app.drag_candidate_idx = None;
                    app.drag_press_pos = None;
                    return Task::perform(
                        async move { cfg.save().map_err(|e| e.to_string()) },
                        Message::ConfigSaved,
                    );
                } else {
                    // Dropped outside the bookmark zone — cancel.
                    app.status = "drag cancelled (dropped outside bookmarks)".into();
                    app.drag_hover_bookmark = None;
                    app.drag_candidate_idx = None;
                    app.drag_press_pos = None;
                }
            } else {
                // Not dragging — just a regular click release. Clear
                // candidate state (the grid row's on_press already handled
                // selection via EntryClicked).
                app.drag_candidate_idx = None;
                app.drag_press_pos = None;
            }
            Task::none()
        }
    }
}

/// Interpret a raw key-press against current app state.
///
/// This is the heart of keyboard navigation. It runs in `update()` where
/// we have `&mut App`, so we can branch on `pathbar_editing` (Escape only
/// exits edit mode — it does NOT toggle, fixing the bug where the
/// previous stateless handler turned editing ON when it was off), check
/// the current selection before activating, etc.
///
/// Returns a `Task` because some keys (Enter on a File, Enter on an
/// AppDir) trigger async launches.
fn handle_key(
    app: &mut App,
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Task<Message> {
    use iced::keyboard::key::Named;

    let shift = modifiers.shift();

    // Ctrl+L toggles edit mode (matches Nautilus / most file managers).
    if modifiers.control() && matches!(&key, iced::keyboard::Key::Character(c) if c == "l") {
        toggle_pathbar_edit(app);
        return Task::none();
    }

    // Ctrl+H toggles hidden-files visibility (matches Thunar / PCManFM).
    if modifiers.control() && matches!(&key, iced::keyboard::Key::Character(c) if c == "h") {
        return update(app, Message::ToggleHiddenFiles);
    }

    // Ctrl+R reloads the current directory (advertised in the View menu).
    if modifiers.control() && matches!(&key, iced::keyboard::Key::Character(c) if c == "r") {
        return update(app, Message::Reload);
    }

    // Ctrl+D bookmarks the current directory (advertised in the Bookmarks
    // menu; matches the browser "bookmark this" convention).
    if modifiers.control() && matches!(&key, iced::keyboard::Key::Character(c) if c == "d") {
        let here = app.current_dir.clone();
        return update(app, Message::AddBookmarkFor(here));
    }

    // Escape closes things in priority order: About → context menu →
    // menubar strip → pathbar edit mode. Each guard returns early.
    if matches!(key.as_ref(), iced::keyboard::Key::Named(Named::Escape)) {
        if app.show_about {
            return update(app, Message::CloseAbout);
        }
        if app.context_menu_target.is_some() || app.open_menu.is_some() {
            return update(app, Message::CloseMenus);
        }
        app.pathbar_editing = false;
        return Task::none();
    }

    // Plain "/" enters edit mode (ROX/PCManFM convention). Only when
    // not already editing — pressing / while editing lets the text
    // input receive the character normally.
    if !app.pathbar_editing
        && !modifiers.control()
        && !modifiers.alt()
        && matches!(&key, iced::keyboard::Key::Character(c) if c == "/")
    {
        toggle_pathbar_edit(app);
        return Task::none();
    }

    // Arrow / vim navigation — only when not editing the pathbar (so
    // J/K/H/L/Backspace do not hijack text input).
    if app.pathbar_editing {
        return Task::none();
    }

    if matches!(key.as_ref(), iced::keyboard::Key::Named(Named::ArrowUp)) {
        // Alt+Up goes up one directory (advertised in the Go menu);
        // plain ArrowUp moves the selection.
        if modifiers.alt() {
            return update(app, Message::GoUp);
        }
        move_selection(app, -1);
        return Task::none();
    }
    if matches!(key.as_ref(), iced::keyboard::Key::Named(Named::ArrowDown)) {
        move_selection(app, 1);
        return Task::none();
    }
    if !shift && matches!(&key, iced::keyboard::Key::Character(c) if c == "k") {
        move_selection(app, -1);
        return Task::none();
    }
    if !shift && matches!(&key, iced::keyboard::Key::Character(c) if c == "j") {
        move_selection(app, 1);
        return Task::none();
    }
    if matches!(key.as_ref(), iced::keyboard::Key::Named(Named::Backspace)) {
        return update(app, Message::GoUp);
    }
    if !shift && matches!(&key, iced::keyboard::Key::Character(c) if c == "h") {
        return update(app, Message::GoUp);
    }
    // Enter / L activates the current selection. No-op if nothing is
    // selected.
    if matches!(key.as_ref(), iced::keyboard::Key::Named(Named::Enter)) {
        return activate_selected(app, shift);
    }
    if !shift && matches!(&key, iced::keyboard::Key::Character(c) if c == "l") {
        return activate_selected(app, shift);
    }

    Task::none()
}

/// Activate the currently-selected entry, if any. Factored out so both
/// the Enter and `L` key paths share the same logic.
fn activate_selected(app: &mut App, shift: bool) -> Task<Message> {
    match app.selected {
        Some(i) => update(app, Message::EntryActivated(i, shift)),
        None => Task::none(),
    }
}

/// Toggle pathbar between breadcrumb and text-input modes. When entering
/// edit mode, pre-fill the input with the current directory's path so
/// the user can edit it in place.
fn toggle_pathbar_edit(app: &mut App) {
    app.pathbar_editing = !app.pathbar_editing;
    if app.pathbar_editing {
        app.pathbar_input = app.current_dir.display().to_string();
    }
}

/// Move the grid selection by `delta` rows (negative = up, positive =
/// down) over the *visible* entries. When hidden files are filtered out,
/// dotfile entries are skipped rather than selected — selecting an
/// invisible row would make the highlight disappear and Enter would
/// activate something the user can't see. If nothing is selected, down
/// starts at the first visible row and up at the last.
fn move_selection(app: &mut App, delta: i32) {
    let visible: Vec<usize> = (0..app.entries.len())
        .filter(|&i| app.show_hidden || !app.entries[i].hidden)
        .collect();
    if visible.is_empty() {
        return;
    }
    let cur_pos = app
        .selected
        .and_then(|i| visible.iter().position(|&v| v == i));
    let new_pos = match cur_pos {
        Some(p) => (p as i32 + delta).clamp(0, visible.len() as i32 - 1) as usize,
        None if delta > 0 => 0,
        None => visible.len() - 1,
    };
    app.selected = Some(visible[new_pos]);
}

/// Double-click / Enter on an entry. Branches on kind:
///   * Directory     → navigate into it
///   * AppDir        → spawn launcher (unless Shift is held, then navigate)
///   * File          → launch via actions.toml, the built-in MIME table,
///                     or direct execution if the file is executable
///                     (no xdg by design)
///   * Symlink       → resolve, then re-dispatch
fn activate_entry(app: &mut App, entry: FileEntry, shift: bool) -> Task<Message> {
    match entry.kind {
        EntryKind::Directory => {
            app.navigate(entry.path);
            Task::none()
        }
        EntryKind::AppDir { .. } if shift => {
            // Shift+Double-Click enters the directory normally, per spec.
            app.navigate(entry.path);
            Task::none()
        }
        EntryKind::AppDir { .. } => {
            let path = entry.path.clone();
            let kind = entry.kind.clone();
            let config = app.config.clone();
            Task::perform(
                async move {
                    LaunchOutcome::from(
                        crate::launch::launch(&path, &kind, &config).await,
                    )
                },
                Message::LaunchDone,
            )
        }
        EntryKind::File => {
            let path = entry.path.clone();
            let kind = entry.kind.clone();
            let config = app.config.clone();
            Task::perform(
                async move {
                    LaunchOutcome::from(
                        crate::launch::launch(&path, &kind, &config).await,
                    )
                },
                Message::LaunchDone,
            )
        }
        EntryKind::Symlink { .. } => {
            // Resolve and re-dispatch. We don't follow chains here; if the
            // target is itself a symlink, the next activation will resolve
            // again. UI feedback for dangling links could be nicer.
            match std::fs::canonicalize(&entry.path) {
                Ok(real) => {
                    if real.is_dir() {
                        app.navigate(real);
                        Task::none()
                    } else {
                        let config = app.config.clone();
                        Task::perform(
                            async move {
                                LaunchOutcome::from(
                                    crate::launch::launch(&real, &EntryKind::File, &config)
                                        .await,
                                )
                            },
                            Message::LaunchDone,
                        )
                    }
                }
                Err(e) => {
                    app.status = format!("dangling symlink: {e}");
                    Task::none()
                }
            }
        }
    }
}

/// Apply a `WatchEvent` to the entry list incrementally.
///
/// Each event touches exactly one entry: created/modified/renamed-to
/// entries are re-stat'd on a background task (`build_file_entry`) and
/// upserted via `Message::UpsertEntry`; removed/renamed-from entries are
/// dropped in place. The earlier design triggered a full rescan on every
/// Created event, which had two problems: the rescan appended to the
/// existing list (duplicating every entry), and busy directories (a
/// browser writing downloads, /tmp churn) caused continuous
/// rescan-plus-watcher-restart storms.
///
/// We do a linear scan to find entries because directory entry counts in
/// normal use are modest (hundreds, not millions).
fn handle_watch_event(app: &mut App, event: WatchEvent) -> Task<Message> {
    // The watcher is non-recursive, so all event paths should live
    // directly inside the watched directory. Checking the parent guards
    // against upserting a path from elsewhere (e.g. a rename whose
    // destination is in another directory — that's a "moved away", not a
    // new entry for this listing).
    let dir = app.current_dir.clone();
    let in_dir = |p: &Path| p.parent() == Some(dir.as_path());

    match event {
        WatchEvent::Created(path) => {
            if in_dir(&path) {
                app.status = format!("+ {} created", path.display());
                return refresh_entry(path);
            }
            Task::none()
        }
        WatchEvent::Removed(path) => {
            if in_dir(&path)
                && remove_entry_by_path(&mut app.entries, &path, &mut app.selected)
            {
                app.status = format!("- {} removed", path.display());
            }
            Task::none()
        }
        WatchEvent::Modified(path) => {
            // Re-stat so size/modified columns reflect the new contents.
            // Only for entries we already list — Modified events for
            // unlisted paths shouldn't conjure entries out of nowhere.
            if in_dir(&path) && app.entries.iter().any(|e| e.path == path) {
                app.status = format!("~ {} modified", path.display());
                return refresh_entry(path);
            }
            Task::none()
        }
        WatchEvent::Renamed { from, to } => {
            if in_dir(&from) {
                remove_entry_by_path(&mut app.entries, &from, &mut app.selected);
            }
            if in_dir(&to) {
                app.status = format!("{} → {}", from.display(), to.display());
                return refresh_entry(to);
            }
            Task::none()
        }
        WatchEvent::WatchError(msg) => {
            app.status = format!("watch error: {msg}");
            Task::none()
        }
    }
}

/// Stat `path` on a background task and emit `UpsertEntry` with the
/// fresh metadata. A stat failure (file vanished between the event and
/// the stat) is silently dropped — the next event or an explicit reload
/// reconciles the view.
fn refresh_entry(path: PathBuf) -> Task<Message> {
    Task::perform(
        async move { crate::vfs::build_file_entry(&path).await.ok() },
        |entry| entry.map_or(Message::Noop, Message::UpsertEntry),
    )
}

/// Sort key implementing the default grid ordering: directories and
/// AppDirs first, then everything else; each group ordered by
/// case-insensitive file name. (`FileEntry.name` is the file name, not
/// the full path, so this sorts within one directory listing.)
fn entry_sort_key(e: &FileEntry) -> (bool, String) {
    let is_file = !matches!(e.kind, EntryKind::Directory | EntryKind::AppDir { .. });
    (is_file, e.name.to_lowercase())
}

/// Insert `entry` into `entries`, or replace the entry with the same
/// path in place. Insertion keeps the list sorted by `entry_sort_key`.
/// The selection index is shifted so the highlighted row keeps pointing
/// at the same entry.
///
/// The replace-if-exists path also makes upserts idempotent, which
/// matters because a file created mid-scan can arrive twice: once via
/// inotify (Created) and once via the scan stream.
fn upsert_entry_sorted(
    entries: &mut Vec<FileEntry>,
    entry: FileEntry,
    selected: &mut Option<usize>,
) {
    if let Some(i) = entries.iter().position(|e| e.path == entry.path) {
        entries[i] = entry;
        return;
    }
    let key = entry_sort_key(&entry);
    let pos = entries.partition_point(|e| entry_sort_key(e) < key);
    entries.insert(pos, entry);
    if let Some(sel) = *selected {
        if sel >= pos {
            *selected = Some(sel + 1);
        }
    }
}

/// Remove the entry at `path`. The selection index is adjusted so it
/// keeps pointing at the same row (or is clamped/dropped if that exact
/// row is the one that went away). Returns true if an entry was removed.
fn remove_entry_by_path(
    entries: &mut Vec<FileEntry>,
    path: &Path,
    selected: &mut Option<usize>,
) -> bool {
    let Some(i) = entries.iter().position(|e| e.path == path) else {
        return false;
    };
    entries.remove(i);
    match *selected {
        Some(sel) if sel == i => {
            *selected = if entries.is_empty() {
                None
            } else {
                Some(sel.min(entries.len() - 1))
            };
        }
        Some(sel) if sel > i => *selected = Some(sel - 1),
        _ => {}
    }
    true
}

/// Expand a leading `~` or `~/…` in pathbar input to `$HOME`. The
/// `~user` form is left untouched — resolving other users' homes would
/// require parsing /etc/passwd, which isn't worth it for a pathbar
/// convenience. Returns the input unchanged when `$HOME` is unset.
fn expand_tilde(input: &str) -> String {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return input.to_string(),
    };
    if input == "~" {
        return home.to_string_lossy().into_owned();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        let mut p = home;
        p.push(rest);
        return p.to_string_lossy().into_owned();
    }
    input.to_string()
}

// ---------- View ------------------------------------------------------------

pub fn view(app: &App) -> Element<'_, Message> {
    // About dialog takes over the whole window when open.
    if app.show_about {
        return about::view(app);
    }

    let menubar_el = menubar::view(app);
    let pathbar = pathbar::view(app);
    let sidebar = sidebar::view(app);
    let grid = grid::view(app);

    let body = row![sidebar, grid].spacing(0).height(Length::Fill);

    let status_bar = container(
        row![
            text(app.status()).size(12).color(Color::from_rgb(0.5, 0.5, 0.5)),
            text("")
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    )
    .padding([4, 10])
    .style(|_: &Theme| container::Style {
        background: Some(Color::from_rgb(0.12, 0.12, 0.13).into()),
        border: iced::Border {
            color: Color::from_rgb(0.25, 0.25, 0.27),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    });

    // Main column: menubar / [menu strip if open] / pathbar / body / status_bar.
    //
    // iced 0.13 doesn't ship a native overlay/menu widget, so menus
    // render as a horizontal strip of item buttons directly below the
    // menubar (the Scitano style) rather than as vertical dropdown
    // popups. The menubar's open menu and the grid's right-click context
    // menu are mutually exclusive and share the same strip slot, so the
    // strip always appears in one predictable place and pushes the
    // content below it down by one row while open. Both close on any
    // click outside their items (handled by the toggle/CloseMenus
    // message arms in update()).
    let mut main_col = column![menubar_el];

    if let Some(strip) = menubar::strip(app) {
        main_col = main_col.push(strip);
    } else if let Some(ctx_menu) = context_menu::view(app) {
        main_col = main_col.push(ctx_menu);
    }

    main_col = main_col.push(pathbar);
    main_col = main_col.push(body);
    main_col = main_col.push(status_bar);

    main_col.spacing(0).height(Length::Fill).into()
}

// ---------- Subscription ----------------------------------------------------

pub fn subscription(app: &App) -> Subscription<Message> {
    let dir = app.current_dir.clone();
    let scan_id = app.scan_id;

    // Scan: one-shot per (dir, scan_id). The closure captures the dir at
    // subscription creation time; bumping scan_id drops the old sub and
    // starts a new one with the new dir. Every message is tagged with the
    // dir so update() can drop stale events that were already in flight
    // when the user navigated away.
    let scan_sub = Subscription::run_with_id(
        ("scan", scan_id),
        stream::channel(256, move |mut tx| {
            let dir = dir.clone();
            async move {
                let (vfs_tx, mut vfs_rx) =
                    tokio::sync::mpsc::unbounded_channel::<ScanEvent>();
                let scan_dir = dir.clone();
                tokio::spawn(async move {
                    scan_directory(scan_dir, vfs_tx).await;
                });
                while let Some(event) = vfs_rx.recv().await {
                    // iced's futures::mpsc::Sender uses an async `send`.
                    let msg = Message::ScanEvent { dir: dir.clone(), event };
                    if tx.send(msg).await.is_err() {
                        break; // UI dropped the receiver — pane closed.
                    }
                }
            }
        }),
    );

    // Watch: same lifecycle as scan — restarts when scan_id bumps.
    let watch_dir = app.current_dir.clone();
    let watch_sub = Subscription::run_with_id(
        ("watch", scan_id),
        stream::channel(64, move |mut tx| {
            let dir = watch_dir.clone();
            async move {
                let (vfs_tx, mut vfs_rx) =
                    tokio::sync::mpsc::unbounded_channel::<WatchEvent>();
                let _watcher = match watch_directory(&dir, vfs_tx) {
                    Ok(w) => w,
                    Err(e) => {
                        log::warn!("watcher failed for {}: {e}", dir.display());
                        return;
                    }
                };
                while let Some(event) = vfs_rx.recv().await {
                    let msg = Message::WatchEvent { dir: dir.clone(), event };
                    if tx.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        }),
    );

    // Mount polling: every 5 seconds. Cheap (one /proc/mounts read) and
    // catches USB insertions without a udev dependency.
    let mount_sub =
        time::every(Duration::from_secs(5)).map(|_| Message::PollMounts);

    // Global mouse event subscription for drag-and-drop. We listen for
    // left-button press (to record the press position for threshold
    // computation) and left-button release (to commit or cancel a drag).
    // CursorMoved events are NOT subscribed globally (too high-frequency);
    // instead, per-grid-row `mouse_area::on_move` handles drag detection.
    let mouse_sub = iced::event::listen_with(mouse_event_filter);

    Subscription::batch([scan_sub, watch_sub, mount_sub, mouse_sub])
}

/// Filter function for the global mouse event subscription. Emits
/// `GlobalMousePressed(pos)` on left-button press and `GlobalMouseReleased`
/// on left-button release. All other events are filtered out (return None).
fn mouse_event_filter(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::mouse::Button;
    use iced::Event;
    match event {
        Event::Mouse(iced::mouse::Event::ButtonPressed(Button::Left)) => {
            // We don't get the cursor position from this event variant,
            // but we don't need it — the grid row's on_press provides the
            // candidate index, and on_move provides the movement detection.
            // The press position is approximated as the origin; the
            // threshold check uses on_move's relative position instead.
            Some(Message::GlobalMousePressed(iced::Point::ORIGIN))
        }
        Event::Mouse(iced::mouse::Event::ButtonReleased(Button::Left)) => {
            Some(Message::GlobalMouseReleased)
        }
        _ => None,
    }
}

// ---------- Helpers shared by subviews --------------------------------------

/// Build the small square "icon button" used in the pathbar (up, edit, etc.).
pub(crate) fn icon_button(
    icon: crate::icons::Icon,
    msg: Message,
) -> button::Button<'static, Message> {
    button(icon.widget(14.0))
        .padding(4)
        .style(button::secondary)
        .on_press(msg)
}

/// Default font (no custom font loading yet — keeps the binary smaller).
#[allow(dead_code)]
pub(crate) const DEFAULT_FONT: Font = Font::DEFAULT;

/// Format a `SystemTime` for display in the grid. Returns "?" on failure.
///
/// Thin wrapper around `crate::date::format_date`, which uses Howard
/// Hinnant's `civil_from_days` algorithm for exact leap-year-correct
/// date math without pulling in `chrono`.
pub(crate) fn format_modified(t: Option<std::time::SystemTime>) -> String {
    crate::date::format_date(t)
}

/// Human-readable file size (1.2 KB, 3.4 MB, etc.). Directories report 0.
pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::EntryKind;
    use std::time::SystemTime;

    /// A minimal `App` pointing at `dir`, with config/mounts isolated from
    /// the developer's real `~/.config/runar`.
    fn test_app(dir: &str) -> App {
        let mut app = App::new(PathBuf::from(dir));
        app.config = Config::default();
        app
    }

    fn file_entry(dir: &Path, name: &str, kind: EntryKind) -> FileEntry {
        let hidden = name.starts_with('.');
        FileEntry {
            path: dir.join(name),
            name: name.to_string(),
            kind,
            size: 42,
            modified: Some(SystemTime::UNIX_EPOCH),
            readable: true,
            writable: true,
            executable: false,
            mime_type: None,
            hidden,
        }
    }

    fn scan_event(dir: &Path, entry: FileEntry) -> Message {
        Message::ScanEvent {
            dir: dir.to_path_buf(),
            event: ScanEvent::Entry(entry),
        }
    }

    #[test]
    fn scan_entries_are_sorted_dirs_first_case_insensitive() {
        let dir = Path::new("/tmp/runar-ui-test-sort");
        let mut app = test_app("/tmp/runar-ui-test-sort");

        // Feed entries in scrambled, case-mixed order.
        for (name, kind) in [
            ("zebra.txt", EntryKind::File),
            ("Bravo", EntryKind::Directory),
            ("apple.md", EntryKind::File),
            ("alpha", EntryKind::AppDir { launcher: dir.join("alpha/run") }),
            (".dotfile", EntryKind::File),
        ] {
            let _ = update(&mut app, scan_event(dir, file_entry(dir, name, kind)));
        }

        let names: Vec<&str> = app.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            // Directories first (alpha < Bravo case-insensitively), then
            // files (".dotfile" < "apple.md" < "zebra.txt" in byte order).
            vec!["alpha", "Bravo", ".dotfile", "apple.md", "zebra.txt"],
            "directories/AppDirs must sort before files, names case-insensitive"
        );
    }

    #[test]
    fn upsert_replaces_existing_entry_without_duplicating() {
        let dir = Path::new("/tmp/runar-ui-test-upsert");
        let mut app = test_app("/tmp/runar-ui-test-upsert");
        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "a.txt", EntryKind::File)),
        );
        // Same path again (e.g. created mid-scan and seen by both the
        // scanner and inotify) must replace, not append.
        let mut again = file_entry(dir, "a.txt", EntryKind::File);
        again.size = 999;
        let _ = update(&mut app, Message::UpsertEntry(again));

        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.entries[0].size, 999);
    }

    #[test]
    fn stale_scan_events_from_previous_directory_are_dropped() {
        let mut app = test_app("/tmp/runar-ui-test-stale");
        // Event tagged with a different directory than current_dir:
        // already in flight when the user navigated — must not land.
        let _ = update(
            &mut app,
            scan_event(
                Path::new("/elsewhere"),
                file_entry(Path::new("/elsewhere"), "ghost.txt", EntryKind::File),
            ),
        );
        assert!(
            app.entries.is_empty(),
            "stale scan events must not contaminate the new listing"
        );
    }

    #[test]
    fn selection_shifts_when_entry_inserted_before_it() {
        let dir = Path::new("/tmp/runar-ui-test-selshift");
        let mut app = test_app("/tmp/runar-ui-test-selshift");
        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "a.txt", EntryKind::File)),
        );
        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "z.txt", EntryKind::File)),
        );
        app.selected = Some(1); // z.txt

        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "m.txt", EntryKind::File)),
        );

        assert_eq!(app.entries.len(), 3);
        assert_eq!(app.selected, Some(2), "selection must follow z.txt down");
        assert_eq!(app.entries[app.selected.unwrap()].name, "z.txt");
    }

    #[test]
    fn selection_clamps_when_selected_row_removed() {
        let dir = Path::new("/tmp/runar-ui-test-selrm");
        let mut app = test_app("/tmp/runar-ui-test-selrm");
        for n in ["a.txt", "b.txt", "c.txt"] {
            let _ = update(
                &mut app,
                scan_event(dir, file_entry(dir, n, EntryKind::File)),
            );
        }
        app.selected = Some(1); // b.txt

        let _ = update(
            &mut app,
            Message::WatchEvent {
                dir: dir.to_path_buf(),
                event: WatchEvent::Removed(dir.join("b.txt")),
            },
        );

        assert_eq!(app.entries.len(), 2);
        assert_eq!(app.selected, Some(1), "selection must clamp to the new last row");
        assert_eq!(app.entries[1].name, "c.txt");
    }

    #[test]
    fn watch_removed_ignores_paths_outside_current_dir() {
        let mut app = test_app("/tmp/runar-ui-test-foreign");
        let _ = update(
            &mut app,
            scan_event(
                Path::new("/tmp/runar-ui-test-foreign"),
                file_entry(Path::new("/tmp/runar-ui-test-foreign"), "keep.txt", EntryKind::File),
            ),
        );
        // A rename destination (or removal) outside the watched dir must
        // not touch this listing.
        let _ = update(
            &mut app,
            Message::WatchEvent {
                dir: PathBuf::from("/tmp/runar-ui-test-foreign"),
                event: WatchEvent::Removed(PathBuf::from("/etc/passwd")),
            },
        );
        assert_eq!(app.entries.len(), 1);
    }

    #[test]
    fn move_selection_skips_hidden_entries_when_hidden() {
        let dir = Path::new("/tmp/runar-ui-test-nav");
        let mut app = test_app("/tmp/runar-ui-test-nav");
        for (n, k) in [
            ("a.txt", EntryKind::File),
            (".hidden", EntryKind::File),
            ("c.txt", EntryKind::File),
        ] {
            let _ = update(&mut app, scan_event(dir, file_entry(dir, n, k)));
        }
        // Sorted order puts the dotfile first: [".hidden", "a.txt", "c.txt"].
        app.show_hidden = false;
        app.selected = Some(1); // a.txt

        // Down from a.txt must land on c.txt (index 2), skipping the
        // invisible .hidden row (index 0).
        move_selection(&mut app, 1);
        assert_eq!(app.selected, Some(2));
        assert_eq!(app.entries[2].name, "c.txt");

        // And back up lands on a.txt again.
        move_selection(&mut app, -1);
        assert_eq!(app.selected, Some(1));
    }

    #[test]
    fn move_selection_from_hidden_selection_finds_visible_anchor() {
        let dir = Path::new("/tmp/runar-ui-test-nav2");
        let mut app = test_app("/tmp/runar-ui-test-nav2");
        for (n, k) in [
            ("a.txt", EntryKind::File),
            (".hidden", EntryKind::File),
        ] {
            let _ = update(&mut app, scan_event(dir, file_entry(dir, n, k)));
        }
        // Sorted order: [".hidden", "a.txt"]. A stale selection pointing
        // at the hidden row (e.g. set before hidden files were hidden)
        // re-anchors on a visible row when the user navigates.
        app.show_hidden = false;
        app.selected = Some(0);
        move_selection(&mut app, 1);
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.entries[1].name, "a.txt");
    }

    #[test]
    fn grid_click_closes_open_menus() {
        use menubar::MenuKind;
        let dir = Path::new("/tmp/runar-ui-test-menus");
        let mut app = test_app("/tmp/runar-ui-test-menus");
        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "a.txt", EntryKind::File)),
        );
        app.open_menu = Some(MenuKind::File);
        app.context_menu_target = Some(0);

        let _ = update(&mut app, Message::EntryClicked(0));

        assert_eq!(app.open_menu, None, "clicking the grid must close menu strips");
        assert_eq!(app.context_menu_target, None);
        assert_eq!(app.selected, Some(0), "row click still selects the row");
    }

    #[test]
    fn open_in_terminal_is_a_menu_action_that_closes_menus() {
        let mut app = test_app("/tmp/runar-ui-test-term");
        app.open_menu = Some(menubar::MenuKind::Go);
        app.context_menu_target = Some(0);

        let _ = update(
            &mut app,
            Message::OpenInTerminal(PathBuf::from("/tmp/runar-ui-test-term")),
        );

        assert_eq!(app.open_menu, None, "Open in Terminal must close its menu");
        assert_eq!(app.context_menu_target, None);
    }

    #[test]
    fn context_menu_strip_follows_right_click_target() {
        let dir = Path::new("/tmp/runar-ui-test-ctx");
        let mut app = test_app("/tmp/runar-ui-test-ctx");
        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "a.txt", EntryKind::File)),
        );
        assert!(context_menu::view(&app).is_none(), "no menu before right-click");

        let _ = update(&mut app, Message::EntryRightClicked(0));
        assert!(context_menu::view(&app).is_some(), "right-click opens the strip");

        // A stale target (entries changed since the click) shows nothing
        // rather than panicking.
        app.entries.clear();
        assert!(context_menu::view(&app).is_none());
    }

    #[test]
    fn menubar_strip_and_context_strip_are_mutually_exclusive() {
        use menubar::MenuKind;
        let dir = Path::new("/tmp/runar-ui-test-mutex");
        let mut app = test_app("/tmp/runar-ui-test-mutex");
        let _ = update(
            &mut app,
            scan_event(dir, file_entry(dir, "a.txt", EntryKind::File)),
        );

        // Opening the context menu closes a menubar strip...
        app.open_menu = Some(MenuKind::File);
        let _ = update(&mut app, Message::EntryRightClicked(0));
        assert_eq!(app.open_menu, None);
        assert!(context_menu::view(&app).is_some());
        assert!(menubar::strip(&app).is_none());

        // ...and opening a menubar strip closes the context menu.
        let _ = update(&mut app, Message::MenuToggled(MenuKind::View));
        assert_eq!(app.context_menu_target, None);
        assert!(menubar::strip(&app).is_some());
        assert!(context_menu::view(&app).is_none());
    }

    #[test]
    fn drag_hover_exit_is_guarded_by_row_index() {
        let mut app = test_app("/tmp/runar-ui-test-drag");
        app.drag_source = Some(PathBuf::from("/tmp/runar-ui-test-drag/proj"));

        // Enter bookmark 1, then the (out-of-order) exit of bookmark 0
        // arrives — the hover on 1 must survive.
        let _ = update(&mut app, Message::DragEnteredBookmark(1));
        let _ = update(&mut app, Message::DragExitedBookmark(0));
        assert_eq!(app.drag_hover_bookmark, Some(1));

        let _ = update(&mut app, Message::DragExitedBookmark(1));
        assert_eq!(app.drag_hover_bookmark, None);
    }

    #[test]
    fn expand_tilde_expands_home_prefix() {
        match std::env::var("HOME") {
            Ok(home) => {
                assert_eq!(expand_tilde("~"), home);
                assert_eq!(expand_tilde("~/Downloads"), format!("{home}/Downloads"));
                // Non-tilde and ~user forms pass through untouched.
                assert_eq!(expand_tilde("/etc"), "/etc");
                assert_eq!(expand_tilde("~other/x"), "~other/x");
                assert_eq!(expand_tilde("relative/path"), "relative/path");
            }
            // Env not set in this environment: expansion must be a no-op.
            Err(_) => {
                assert_eq!(expand_tilde("~/Downloads"), "~/Downloads");
            }
        }
    }

    #[test]
    fn pathbar_submit_expands_tilde_and_navigates() {
        let Ok(home) = std::env::var("HOME") else {
            return; // no $HOME in this environment — nothing to assert
        };
        let mut app = test_app("/tmp/runar-ui-test-pathbar");
        let _ = update(&mut app, Message::PathBarEdit("~".into()));
        let _ = update(&mut app, Message::PathBarSubmit);
        assert_eq!(app.current_dir, PathBuf::from(&home));
        assert!(!app.pathbar_editing);
    }

    #[test]
    fn pathbar_submit_trims_whitespace() {
        let mut app = test_app("/tmp/runar-ui-test-trim");
        let _ = update(&mut app, Message::PathBarEdit("/etc  ".into()));
        let _ = update(&mut app, Message::PathBarSubmit);
        assert_eq!(app.current_dir, PathBuf::from("/etc"));
    }

    #[test]
    fn pathbar_submit_empty_input_just_closes_editor() {
        let mut app = test_app("/tmp/runar-ui-test-empty");
        let before = app.current_dir.clone();
        let _ = update(&mut app, Message::PathBarEdit("   ".into()));
        let _ = update(&mut app, Message::PathBarSubmit);
        assert_eq!(app.current_dir, before);
        assert!(!app.pathbar_editing);
    }

    #[test]
    fn pathbar_submit_missing_path_reports_error() {
        let mut app = test_app("/tmp/runar-ui-test-missing");
        let bogus = "/tmp/runar-ui-test-missing-does-not-exist-xyz";
        let _ = update(&mut app, Message::PathBarEdit(bogus.into()));
        let _ = update(&mut app, Message::PathBarSubmit);
        assert!(app.status.contains("no such directory"), "status: {}", app.status);
        // Navigating nowhere: current_dir unchanged.
        assert_eq!(app.current_dir, PathBuf::from("/tmp/runar-ui-test-missing"));
    }

    #[test]
    fn ctrl_r_reloads_current_directory() {
        use iced::keyboard::{Key, Modifiers};
        let mut app = test_app("/tmp/runar-ui-test-ctrlr");
        let before = app.scan_id;
        let _ = update(
            &mut app,
            Message::KeyPressed(Key::Character("r".into()), Modifiers::CTRL),
        );
        assert_eq!(app.scan_id, before.wrapping_add(1), "Ctrl+R must bump scan_id");
        assert_eq!(app.open_menu, None, "Reload must close any open menu");
    }
}
