//! Top menubar — File / Edit / View / Go / Bookmarks / Help.
//!
//! iced 0.13 doesn't ship a native menubar widget, so we build one from
//! `button` primitives. Each menu button toggles a dropdown panel that
//! overlays the layout below it (rendered by `ui::view` when
//! `app.open_menu` matches).
//!
//! ## Menu structure
//!
//! Inspired by the three parent influencers:
//!
//!   * **Thunar** (Xfce): File / Edit / View / Go / Bookmarks / Help
//!   * **PCManFM** (LXDE): File / Edit / View / Go / Bookmarks / Help
//!   * **ROX-Filer**: doesn't use a traditional menubar (toolbar + popup
//!     menus), but its menu *entries* (Show, Actions, Type, ...) map
//!     cleanly onto View / File / View in our schema.
//!
//! Entries that aren't wired yet (Rename, Delete, Copy, Paste) emit a
//! `NotImplemented` message so the status bar shows feedback rather
//! than silently swallowing the click. Background file ops are tracked
//! in the README roadmap.

use super::Message;
use iced::widget::{button, container, row, text, Space};
use iced::{Alignment, Color, Element, Length};
use std::path::PathBuf;

/// Which top-level menu is currently open (if any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    File,
    Edit,
    View,
    Go,
    Bookmarks,
    Help,
}

impl MenuKind {
    pub fn label(self) -> &'static str {
        match self {
            MenuKind::File => "File",
            MenuKind::Edit => "Edit",
            MenuKind::View => "View",
            MenuKind::Go => "Go",
            MenuKind::Bookmarks => "Bookmarks",
            MenuKind::Help => "Help",
        }
    }

    pub const ALL: [MenuKind; 6] = [
        MenuKind::File,
        MenuKind::Edit,
        MenuKind::View,
        MenuKind::Go,
        MenuKind::Bookmarks,
        MenuKind::Help,
    ];
}

/// A single entry in a dropdown menu.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub message: Message,
    /// Optional shortcut hint shown on the right (e.g. "Enter", "Ctrl+L").
    pub shortcut: Option<&'static str>,
    /// True for separators (label is ignored, message is never emitted).
    pub is_separator: bool,
    /// True if this entry is grayed out (e.g. "Paste" when clipboard empty).
    pub disabled: bool,
}

impl MenuItem {
    /// Regular clickable entry.
    pub fn entry(label: impl Into<String>, message: Message) -> Self {
        Self {
            label: label.into(),
            message,
            shortcut: None,
            is_separator: false,
            disabled: false,
        }
    }

    pub fn entry_with_shortcut(
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

    /// Disabled entry (shown grayed out).
    pub fn disabled_entry(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            message: Message::Noop,
            shortcut: None,
            is_separator: false,
            disabled: true,
        }
    }

    /// Disabled entry with a shortcut hint (shortcut is currently
    /// dropped — disabled entries don't render their shortcut — but the
    /// method exists so call sites are symmetric with `entry_with_shortcut`).
    pub fn disabled_entry_with_shortcut(
        label: impl Into<String>,
        _shortcut: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            message: Message::Noop,
            shortcut: None,
            is_separator: false,
            disabled: true,
        }
    }

    /// Horizontal separator line.
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            message: Message::Noop,
            shortcut: None,
            is_separator: true,
            disabled: false,
        }
    }
}

/// Build the menubar (a horizontal row of menu buttons). Returns the
/// bar element; the actual dropdown panel is rendered separately by
/// `ui::view` based on `app.open_menu`.
pub fn view<'a>(app: &'a super::App) -> Element<'a, Message> {
    let mut bar = row![].spacing(0).align_y(Alignment::Center);

    for kind in MenuKind::ALL {
        let is_open = app.open_menu() == Some(kind);
        let btn = button(text(kind.label()).size(13))
            .padding([4, 10])
            .style(move |theme: &iced::Theme, status: button::Status| {
                let base = button::secondary(theme, status);
                let bg = if is_open {
                    Some(Color::from_rgb(0.18, 0.32, 0.55).into())
                } else if matches!(status, button::Status::Hovered) {
                    Some(Color::from_rgb(0.18, 0.18, 0.20).into())
                } else {
                    None
                };
                button::Style {
                    background: bg.or(base.background),
                    ..base
                }
            })
            .on_press(Message::MenuToggled(kind));
        bar = bar.push(btn);
    }

    // Push remaining space to the right so the menubar fills the width.
    bar = bar.push(Space::new(Length::Fill, 0));

    container(bar)
        .padding([2, 4])
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgb(0.10, 0.10, 0.11).into()),
            border: iced::Border {
                color: Color::from_rgb(0.22, 0.22, 0.25),
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Build the dropdown panel for the currently-open menu. Returns None
/// if no menu is open.
pub fn dropdown<'a>(app: &'a super::App) -> Option<Element<'a, Message>> {
    let kind = app.open_menu()?;
    let items = menu_items_for(kind, app);

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
            .width(220)
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

/// Build the list of items for a given menu, considering app state
/// (e.g. disable "Paste" when clipboard is empty, hide "Remove Bookmark"
/// when not in a bookmarked dir).
fn menu_items_for(kind: MenuKind, app: &super::App) -> Vec<MenuItem> {
    match kind {
        MenuKind::File => {
            let mut items = vec![
                MenuItem::entry_with_shortcut(
                    "Open",
                    "Enter",
                    Message::EntryActivated(app.selected().unwrap_or(usize::MAX), false),
                ),
                MenuItem::entry("Open With…", Message::NotImplemented("Open With")),
                MenuItem::separator(),
                MenuItem::entry_with_shortcut(
                    "New Folder",
                    "Ctrl+Shift+N",
                    Message::NotImplemented("New Folder"),
                ),
                MenuItem::entry_with_shortcut(
                    "New File",
                    "Ctrl+Alt+N",
                    Message::NotImplemented("New File"),
                ),
                MenuItem::separator(),
                MenuItem::entry_with_shortcut(
                    "Rename",
                    "F2",
                    Message::NotImplemented("Rename"),
                ),
                MenuItem::entry_with_shortcut(
                    "Delete",
                    "Delete",
                    Message::NotImplemented("Delete"),
                ),
                MenuItem::separator(),
                MenuItem::entry_with_shortcut(
                    "Properties",
                    "Alt+Enter",
                    Message::NotImplemented("Properties"),
                ),
            ];
            // Disable Open if nothing is selected.
            if app.selected().is_none() {
                items[0] = MenuItem::disabled_entry("Open");
            }
            items
        }
        MenuKind::Edit => vec![
            MenuItem::entry_with_shortcut(
                "Cut",
                "Ctrl+X",
                Message::NotImplemented("Cut"),
            ),
            MenuItem::entry_with_shortcut(
                "Copy",
                "Ctrl+C",
                Message::NotImplemented("Copy"),
            ),
            MenuItem::disabled_entry_with_shortcut("Paste", "Ctrl+V"),
            MenuItem::separator(),
            MenuItem::entry_with_shortcut(
                "Select All",
                "Ctrl+A",
                Message::NotImplemented("Select All"),
            ),
            MenuItem::entry_with_shortcut(
                "Invert Selection",
                "Ctrl+I",
                Message::NotImplemented("Invert Selection"),
            ),
            MenuItem::separator(),
            MenuItem::entry_with_shortcut(
                "Preferences…",
                "",
                Message::NotImplemented("Preferences"),
            ),
        ],
        MenuKind::View => {
            // Dynamic label: shows "Hide Hidden Files" when they're
            // currently visible (so clicking hides them), and "Show
            // Hidden Files" when they're hidden (so clicking shows them).
            // This makes the toggle direction obvious from the label
            // alone, matching Thunar / PCManFM behavior.
            let hidden_label = if app.show_hidden() {
                "Hide Hidden Files"
            } else {
                "Show Hidden Files"
            };
            vec![
                MenuItem::entry_with_shortcut(
                    "Reload",
                    "Ctrl+R",
                    Message::Reload,
                ),
                MenuItem::separator(),
                MenuItem::entry_with_shortcut(
                    hidden_label,
                    "Ctrl+H",
                    Message::ToggleHiddenFiles,
                ),
                MenuItem::entry("Sort by Name", Message::NotImplemented("Sort by Name")),
                MenuItem::entry("Sort by Size", Message::NotImplemented("Sort by Size")),
                MenuItem::entry("Sort by Modified", Message::NotImplemented("Sort by Modified")),
                MenuItem::separator(),
                MenuItem::entry("Icon View", Message::NotImplemented("Icon View")),
                MenuItem::entry("Detailed List", Message::NotImplemented("Detailed List")),
            ]
        }
        MenuKind::Go => {
            let mut items = vec![
                MenuItem::entry_with_shortcut(
                    "Up",
                    "Alt+Up",
                    Message::GoUp,
                ),
                MenuItem::entry_with_shortcut(
                    "Back",
                    "Alt+Left",
                    Message::NotImplemented("Back"),
                ),
                MenuItem::entry_with_shortcut(
                    "Forward",
                    "Alt+Right",
                    Message::NotImplemented("Forward"),
                ),
                MenuItem::separator(),
                MenuItem::entry_with_shortcut(
                    "Edit Path…",
                    "Ctrl+L",
                    Message::PathBarToggleEdit,
                ),
                MenuItem::separator(),
            ];
            // Append all default locations (from config::defaults).
            for loc in crate::config::default_locations() {
                if let Some(p) = loc.path {
                    items.push(MenuItem::entry(
                        format!("  {}", loc.label),
                        Message::MountSelected(p),
                    ));
                }
            }
            items
        }
        MenuKind::Bookmarks => {
            let mut items = vec![
                MenuItem::entry_with_shortcut(
                    "Add Current Directory",
                    "Ctrl+D",
                    Message::AddBookmarkFor(PathBuf::from(app.current_dir())),
                ),
                MenuItem::entry_with_shortcut(
                    "Edit Bookmarks…",
                    "",
                    Message::NotImplemented("Edit Bookmarks"),
                ),
                MenuItem::separator(),
            ];
            if app.bookmarks().is_empty() {
                items.push(MenuItem::disabled_entry("(no bookmarks)"));
            } else {
                for entry in app.bookmarks() {
                    let label = entry
                        .label
                        .clone()
                        .or_else(|| {
                            entry
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| entry.path.display().to_string());
                    items.push(MenuItem::entry(
                        format!("  {}", label),
                        Message::BookmarkSelected(entry.path.clone()),
                    ));
                }
            }
            items
        }
        MenuKind::Help => vec![
            MenuItem::entry("About runar", Message::ShowAbout),
            MenuItem::entry("Keyboard Shortcuts", Message::NotImplemented("Keyboard Shortcuts")),
            MenuItem::separator(),
            MenuItem::entry("Online Documentation", Message::NotImplemented("Online Docs")),
        ],
    }
}

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
