//! Right-click context menu for grid rows.
//!
//! iced 0.13 doesn't ship a native context-menu widget. We approximate
//! it by tracking which row was right-clicked in `App.context_menu_target`
//! and rendering an overlay panel (similar to the menubar dropdown)
//! positioned at the top of the grid area.
//!
//! ## Entry set
//!
//! Inspired by Thunar / PCManFM / ROX-Filer right-click menus, with
//! type-aware entries:
//!
//!   * **Directories**: Open, Open in Terminal, Rename, Cut, Copy, Paste,
//!     Delete, Properties
//!   * **AppDirs**: Open (launch), Enter Directory (Shift-equivalent),
//!     Rename, Cut, Copy, Delete, Properties
//!   * **Files**: Open (with detected handler), Open With…, Copy Path,
//!     Rename, Cut, Copy, Delete, Properties
//!   * **Symlinks**: Follow, Edit Target, Rename, Cut, Copy, Delete,
//!     Properties
//!
//! Entries that aren't wired yet (Rename, Delete, Cut, Copy, Paste)
//! emit `NotImplemented` so the status bar shows feedback.

use super::Message;
use iced::widget::{button, container, text, Space};
use iced::{Alignment, Color, Element, Length};

use crate::vfs::{EntryKind, FileEntry};

/// Build the context menu overlay. Returns `None` if no row has been
/// right-clicked (i.e. `app.context_menu_target()` is `None`).
pub fn view<'a>(app: &'a super::App) -> Option<Element<'a, Message>> {
    let idx = app.context_menu_target()?;
    let entry = app.entries().get(idx)?;

    let items = items_for(entry, idx);
    let mut col = iced::widget::column![].spacing(0).padding([4, 0]);

    for item in items {
        if item.is_separator {
            col = col.push(separator());
        } else if item.disabled {
            col = col.push(disabled_row(item.label));
        } else {
            col = col.push(entry_row(item.label, item.shortcut, item.message));
        }
    }

    Some(
        container(col)
            .width(240)
            .style(|_: &iced::Theme| container::Style {
                background: Some(Color::from_rgb(0.14, 0.14, 0.16).into()),
                border: iced::Border {
                    color: Color::from_rgb(0.30, 0.30, 0.34),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into(),
    )
}

/// Context menu item — same shape as `menubar::MenuItem` but defined
/// here to avoid a cross-module dependency.
#[derive(Debug, Clone)]
struct CtxMenuItem {
    label: String,
    message: Message,
    shortcut: Option<&'static str>,
    is_separator: bool,
    disabled: bool,
}

impl CtxMenuItem {
    fn entry(label: impl Into<String>, message: Message) -> Self {
        Self {
            label: label.into(),
            message,
            shortcut: None,
            is_separator: false,
            disabled: false,
        }
    }

    fn entry_with_shortcut(
        label: impl Into<String>,
        shortcut: &'static str,
        message: Message,
    ) -> Self {
        Self {
            label: label.into(),
            message,
            shortcut: Some(shortcut),
            is_separator: false,
            disabled: false,
        }
    }

    fn disabled_entry(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            message: Message::Noop,
            shortcut: None,
            is_separator: false,
            disabled: true,
        }
    }

    fn separator() -> Self {
        Self {
            label: String::new(),
            message: Message::Noop,
            shortcut: None,
            is_separator: true,
            disabled: false,
        }
    }
}

fn items_for(entry: &FileEntry, idx: usize) -> Vec<CtxMenuItem> {
    match &entry.kind {
        EntryKind::Directory => directory_items(idx),
        EntryKind::AppDir { .. } => appdir_items(idx),
        EntryKind::File => file_items(entry, idx),
        EntryKind::Symlink { .. } => symlink_items(idx),
    }
}

fn directory_items(idx: usize) -> Vec<CtxMenuItem> {
    vec![
        CtxMenuItem::entry_with_shortcut(
            "Open",
            "Enter",
            Message::EntryActivated(idx, false),
        ),
        CtxMenuItem::entry("Open in Terminal", Message::NotImplemented("Open in Terminal")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        CtxMenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        CtxMenuItem::disabled_entry_with_shortcut("Paste", "Ctrl+V"),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        CtxMenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

fn appdir_items(idx: usize) -> Vec<CtxMenuItem> {
    vec![
        CtxMenuItem::entry_with_shortcut(
            "Launch",
            "Enter",
            Message::EntryActivated(idx, false),
        ),
        CtxMenuItem::entry_with_shortcut(
            "Enter Directory",
            "Shift+Enter",
            Message::EntryActivated(idx, true),
        ),
        CtxMenuItem::separator(),
        CtxMenuItem::entry("Open in Terminal", Message::NotImplemented("Open in Terminal")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        CtxMenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        CtxMenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

fn file_items(entry: &FileEntry, idx: usize) -> Vec<CtxMenuItem> {
    let mime_label = entry
        .mime_type
        .as_deref()
        .unwrap_or("unknown type");

    vec![
        CtxMenuItem::entry_with_shortcut(
            format!("Open ({})", mime_label),
            "Enter",
            Message::EntryActivated(idx, false),
        ),
        CtxMenuItem::entry("Open With…", Message::NotImplemented("Open With")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry("Copy Path", Message::NotImplemented("Copy Path")),
        CtxMenuItem::entry("Open in Terminal", Message::NotImplemented("Open in Terminal")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        CtxMenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        CtxMenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

fn symlink_items(idx: usize) -> Vec<CtxMenuItem> {
    vec![
        CtxMenuItem::entry_with_shortcut(
            "Follow Link",
            "Enter",
            Message::EntryActivated(idx, false),
        ),
        CtxMenuItem::entry("Edit Target…", Message::NotImplemented("Edit Target")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry("Copy Path", Message::NotImplemented("Copy Path")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Cut", "Ctrl+X", Message::NotImplemented("Cut")),
        CtxMenuItem::entry_with_shortcut("Copy", "Ctrl+C", Message::NotImplemented("Copy")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut("Rename", "F2", Message::NotImplemented("Rename")),
        CtxMenuItem::entry_with_shortcut("Delete", "Delete", Message::NotImplemented("Delete")),
        CtxMenuItem::separator(),
        CtxMenuItem::entry_with_shortcut(
            "Properties",
            "Alt+Enter",
            Message::NotImplemented("Properties"),
        ),
    ]
}

// --- Render helpers (mirror the menubar module's helpers) ------------------

fn entry_row<'a>(
    label: String,
    shortcut: Option<&'static str>,
    message: Message,
) -> Element<'a, Message> {
    let mut r = iced::widget::row![
        text(label).size(13).color(Color::from_rgb(0.92, 0.92, 0.93)),
        Space::new(Length::Fill, 0),
    ]
    .align_y(Alignment::Center);

    if let Some(s) = shortcut {
        if !s.is_empty() {
            r = r.push(
                text(s)
                    .size(11)
                    .color(Color::from_rgb(0.55, 0.55, 0.6)),
            );
        }
    }

    button(r)
        .padding([4, 10])
        .width(Length::Fill)
        .style(button::secondary)
        .on_press(message)
        .into()
}

fn disabled_row<'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(13)
            .color(Color::from_rgb(0.45, 0.45, 0.5)),
    )
    .padding([4, 10])
    .width(Length::Fill)
    .into()
}

fn separator<'a>() -> Element<'a, Message> {
    container(Space::new(Length::Fill, 1))
        .padding([2, 6])
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgb(0.28, 0.28, 0.32).into()),
            ..Default::default()
        })
        .into()
}

// Small extension trait to keep call sites clean.
impl CtxMenuItem {
    fn disabled_entry_with_shortcut(label: &str, _shortcut: &'static str) -> Self {
        Self::disabled_entry(label)
    }
}
