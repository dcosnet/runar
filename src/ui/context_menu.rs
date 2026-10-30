//! Right-click context menu for grid rows.
//!
//! iced 0.13 doesn't ship a native context-menu widget. Following the
//! same pattern as the menubar (and Scitano, which treats its editor
//! context menu as the same overlay mechanism), the row's menu renders
//! as a HORIZONTAL STRIP directly below the menubar: a row of flat item
//! buttons, separators as fixed-width gaps, hidden horizontal scrollbar
//! for overflow. See `menubar::strip_from_items` for the renderer —
//! this module only builds the item list, type-aware per entry kind:
//!
//!   * **Directories**: Open, Open in Terminal, Rename, Cut, Copy, Paste,
//!     Delete, Properties
//!   * **AppDirs**: Launch (or Enter Directory with Shift), Open in
//!     Terminal, Rename, Cut, Copy, Delete, Properties
//!   * **Files**: Open (with detected handler), Open With…, Copy Path,
//!     Open in Terminal, Rename, Cut, Copy, Delete, Properties
//!   * **Symlinks**: Follow, Edit Target, Rename, Cut, Copy, Delete,
//!     Properties
//!
//! Entries that aren't wired yet (Rename, Delete, Cut, Copy, Paste)
//! emit `NotImplemented` so the status bar shows feedback.

use super::menubar::{strip_from_items as strip_from, MenuItem};
use super::Message;
use iced::Element;

use crate::vfs::{EntryKind, FileEntry};
use std::path::PathBuf;

/// Build the context menu strip. Returns `None` if no row has been
/// right-clicked (i.e. `app.context_menu_target()` is `None`) or the
/// target row no longer exists (entries changed since the right-click).
pub fn view(app: &super::App) -> Option<Element<'static, Message>> {
    let idx = app.context_menu_target()?;
    let entry = app.entries().get(idx)?;

    Some(strip_from(items_for(app, entry, idx)))
}

fn items_for(app: &super::App, entry: &FileEntry, idx: usize) -> Vec<MenuItem> {
    match &entry.kind {
        EntryKind::Directory => directory_items(entry.path.clone(), idx),
        EntryKind::AppDir { .. } => appdir_items(entry.path.clone(), idx),
        EntryKind::File => file_items(entry, idx, app.current_dir().to_path_buf()),
        EntryKind::Symlink { .. } => symlink_items(idx),
    }
}

fn directory_items(path: PathBuf, idx: usize) -> Vec<MenuItem> {
    vec![
        MenuItem::entry_with_shortcut("Open", "Enter", Message::EntryActivated(idx, false)),
        MenuItem::entry("Open in Terminal", Message::OpenInTerminal(path)),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        MenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        MenuItem::disabled_entry("Paste"),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        MenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

fn appdir_items(path: PathBuf, idx: usize) -> Vec<MenuItem> {
    vec![
        MenuItem::entry_with_shortcut("Launch", "Enter", Message::EntryActivated(idx, false)),
        MenuItem::entry_with_shortcut(
            "Enter Directory",
            "Shift+Enter",
            Message::EntryActivated(idx, true),
        ),
        MenuItem::separator(),
        MenuItem::entry("Open in Terminal", Message::OpenInTerminal(path)),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        MenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        MenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

fn file_items(entry: &FileEntry, idx: usize, cwd: PathBuf) -> Vec<MenuItem> {
    let mime_label = entry
        .mime_type
        .as_deref()
        .unwrap_or("unknown type");

    vec![
        MenuItem::entry_with_shortcut(
            format!("Open ({})", mime_label),
            "Enter",
            Message::EntryActivated(idx, false),
        ),
        MenuItem::entry("Open With…", Message::NotImplemented("Open With")),
        MenuItem::separator(),
        MenuItem::entry("Copy Path", Message::NotImplemented("Copy Path")),
        // Files open the terminal in the directory containing them.
        MenuItem::entry("Open in Terminal", Message::OpenInTerminal(cwd)),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        MenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        MenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

fn symlink_items(idx: usize) -> Vec<MenuItem> {
    vec![
        MenuItem::entry_with_shortcut(
            "Follow Link",
            "Enter",
            Message::EntryActivated(idx, false),
        ),
        MenuItem::entry("Edit Target…", Message::NotImplemented("Edit Target")),
        MenuItem::separator(),
        MenuItem::entry("Copy Path", Message::NotImplemented("Copy Path")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        MenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        MenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        MenuItem::separator(),
        MenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}
