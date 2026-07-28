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
    ScanEvent(ScanEvent),
    /// Streamed from `watch_directory` (Phase 1, wrapped in a subscription).
    WatchEvent(WatchEvent),
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
    /// Close any open menu (menubar dropdown or context menu). Triggered
    /// by clicking elsewhere, pressing Escape, or after a menu action fires.
    CloseMenus,
    /// User right-clicked a grid row at the given index.
    EntryRightClicked(usize),
    /// User selected a menu item whose action isn't implemented yet.
    /// The string is the action label, shown in the status bar so the
    /// user knows their click registered.
    NotImplemented(&'static str),
    /// Show the About dialog.
    ShowAbout,
    /// Close the About dialog.
    CloseAbout,
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
    /// Cursor left the bookmark area entirely.
    DragExitedBookmarks,
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
            drag_source: None,
            drag_candidate_idx: None,
            drag_press_pos: None,
            drag_hover_bookmark: None,
        }
    }

    /// Navigate to `dir`. Bumps scan_id so the subscription restarts,
    /// clears the entry list immediately (so the UI feels responsive
    /// rather than showing stale entries from the old dir).
    fn navigate(&mut self, dir: PathBuf) {
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
        Message::ScanEvent(event) => {
            match event {
                ScanEvent::Entry(entry) => {
                    app.entries.push(entry);
                }
                ScanEvent::Done { total, .. } => {
                    app.scanning = false;
                    app.status =
                        format!("{} items • {}", total, app.current_dir.display());
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

        Message::WatchEvent(event) => {
            handle_watch_event(app, event);
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
            let target = PathBuf::from(&app.pathbar_input);
            app.pathbar_editing = false;
            if target.is_dir() {
                app.navigate(target);
            } else {
                app.status = format!("not a directory: {}", target.display());
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
            if let Some(entry) = app.entries.get(i).cloned() {
                return activate_entry(app, entry, shift);
            }
            Task::none()
        }
        Message::Reload => {
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
                // Close context menu when opening a menubar dropdown.
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
        Message::DragExitedBookmarks => {
            if app.drag_source.is_some() {
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

    // Escape closes things in priority order: About → context menu →
    // menubar dropdown → pathbar edit mode. Each guard returns early.
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
/// down). Clamps to [0, len-1]. If nothing is selected, starts at row 0
/// (for down) or the last row (for up).
fn move_selection(app: &mut App, delta: i32) {
    if app.entries.is_empty() {
        return;
    }
    let total = app.entries.len() as i32;
    let cur = app.selected.map(|i| i as i32).unwrap_or_else(|| {
        // No current selection: J/ArrowDown starts at 0, K/ArrowUp starts at last.
        if delta > 0 { 0 } else { total - 1 }
    });
    let next = (cur + delta).clamp(0, total - 1);
    app.selected = Some(next as usize);
}

/// Double-click / Enter on an entry. Branches on kind:
///   * Directory     → navigate into it
///   * AppDir        → spawn launcher (unless Shift is held, then navigate)
///   * File          → launch via actions.toml or xdg-open fallback
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

/// Apply a `WatchEvent` to the entry list. We do a linear scan because
/// directory entry counts in normal use are modest (hundreds, not
/// millions); for the 10k+ pathological case a HashMap by path would
/// be faster — left as a future optimization.
fn handle_watch_event(app: &mut App, event: WatchEvent) {
    match event {
        WatchEvent::Created(path) => {
            // For simplicity, trigger a reload — re-listing the whole dir
            // is correct in all cases (file replaced, attributes changed,
            // etc.). A future optimization would stat the new entry and
            // append it without a full rescan.
            app.scan_id = app.scan_id.wrapping_add(1);
            app.scanning = true;
            app.status = format!("rescanning: {} created", path.display());
        }
        WatchEvent::Removed(path) => {
            app.entries.retain(|e| e.path != path);
            if let Some(sel) = app.selected {
                if sel >= app.entries.len() {
                    app.selected = if app.entries.is_empty() {
                        None
                    } else {
                        Some(app.entries.len() - 1)
                    };
                }
            }
            app.status = format!("- {} removed", path.display());
        }
        WatchEvent::Modified(path) => {
            // Touch the entry's display by reloading its metadata.
            // For now, just update the status bar.
            if app.entries.iter().any(|e| e.path == path) {
                app.status = format!("~ {} modified", path.display());
            }
        }
        WatchEvent::Renamed { from, to } => {
            if let Some(entry) = app.entries.iter_mut().find(|e| e.path == from) {
                entry.path = to.clone();
                entry.name = to
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                entry.hidden = entry.name.starts_with('.');
            }
            app.status = format!("{} → {}", from.display(), to.display());
        }
        WatchEvent::WatchError(msg) => {
            app.status = format!("watch error: {msg}");
        }
    }
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

    // Main column: menubar / pathbar / [dropdown if open] / body / status_bar.
    //
    // iced 0.13 doesn't ship a native overlay/menu widget, so we render
    // dropdowns and context menus as inline panels rather than true
    // overlays. The menubar dropdown pushes the body down by its height
    // while open; the context menu appears inside a row alongside the
    // sidebar+grid body. Both close on any click outside their items
    // (handled by the toggle/CloseMenus message arms in update()).
    let mut main_col = column![menubar_el, pathbar].spacing(0);

    if let Some(dropdown) = menubar::dropdown(app) {
        main_col = main_col.push(dropdown);
    }

    // If a context menu is open, render it as a panel to the right of
    // the sidebar+grid body. Clicking any item in it emits the item's
    // message AND we manually close via the item's handler (or the
    // user can press Escape).
    if let Some(ctx_menu) = context_menu::view(app) {
        let with_menu = row![body, ctx_menu].spacing(0).height(Length::Fill);
        main_col = main_col.push(with_menu);
    } else {
        main_col = main_col.push(body);
    }

    main_col = main_col.push(status_bar);

    main_col.spacing(0).height(Length::Fill).into()
}

// ---------- Subscription ----------------------------------------------------

pub fn subscription(app: &App) -> Subscription<Message> {
    let dir = app.current_dir.clone();
    let scan_id = app.scan_id;

    // Scan: one-shot per (dir, scan_id). The closure captures the dir at
    // subscription creation time; bumping scan_id drops the old sub and
    // starts a new one with the new dir.
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
                    if tx.send(Message::ScanEvent(event)).await.is_err() {
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
                    if tx.send(Message::WatchEvent(event)).await.is_err() {
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
