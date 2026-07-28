//! Main content area — file list with icon + name, single-click to select,
//! double-click (or Enter) to activate.
//!
//! ## Click handling
//!
//! iced 0.13's `button` widget exposes only `on_press`, not native
//! double-click events. Grid rows emit `EntryClicked(usize)` on every
//! press; `ui::update` performs double-click detection by comparing the
//! current click timestamp against the previous one. A second press on
//! the same row within ~350ms (matching GTK/Qt defaults) activates.
//! Single-click selects.
//!
//! ## Layout
//!
//! A single-column vertical list of rows, not a multi-column icon grid.
//! Multi-column layout with spatial H/J/K/L navigation is tracked as a
//! roadmap item.
//!
//! ## Performance: full layout, no virtualization
//!
//! iced 0.13's stock `scrollable` lays out all children and clips the
//! result — it does not skip layout for off-screen rows. This module
//! builds one `mouse_area` widget per entry on every render, regardless
//! of how many entries are visible. Directories in the low-thousands
//! perform fine; the 10k+ case (e.g. `/usr/bin` on a developer
//! workstation) incurs noticeable layout cost on every scroll.
//!
//! Remediation options:
//!   1. Wait for iced to ship a virtualized list widget.
//!   2. Build a custom widget on top of `iced::advanced` that lays out
//!      only the visible window of rows.
//!   3. Page the entry list — keep ~500 entries in memory, load more on
//!      scroll-to-bottom.
//!
//! Option 3 is the cheapest stopgap and is tracked in the README roadmap.

use super::{format_modified, format_size, Message};
use crate::icons::Icon;
use crate::vfs::{EntryKind, FileEntry};
use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(app: &'a super::App) -> Element<'a, Message> {
    let mut list = column![].spacing(0);

    // Optional header row.
    let header = row![
        text("").width(Length::Fixed(24.0)), // icon column
        text("Name").width(Length::Fill).size(11),
        text("Size").width(Length::Fixed(72.0)).size(11),
        text("Modified")
            .width(Length::Fixed(96.0))
            .size(11),
        text("Type").width(Length::Fixed(96.0)).size(11),
    ]
    .spacing(8)
    .padding([4, 8])
    .align_y(Alignment::Center);
    list = list.push(header);

    list = list.push(divider());

    let entries = app.entries();
    if entries.is_empty() {
        let empty = if app.scanning() {
            "reading directory…"
        } else {
            "(empty)"
        };
        list = list.push(
            container(text(empty).size(12).color(Color::from_rgb(0.5, 0.5, 0.55)))
                .padding([20, 12])
                .width(Length::Fill),
        );
    } else {
        for (i, entry) in entries.iter().enumerate() {
            let is_selected = app.selected() == Some(i);
            list = list.push(entry_row(i, entry, is_selected));
        }
    }

    let scroll = scrollable(list).width(Length::Fill).height(Length::Fill);

    container(scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgb(0.09, 0.09, 0.10).into()),
            ..Default::default()
        })
        .into()
}

fn entry_row<'a>(i: usize, entry: &'a FileEntry, selected: bool) -> Element<'a, Message> {
    let icon = entry_icon(&entry.kind);
    let name_color = name_color_for(entry);
    let size = size_label_for(entry);
    let modified = format_modified(entry.modified);
    let kind_label = kind_label_for(entry);

    let row_content = row![
        container(icon.widget(14.0))
            .width(Length::Fixed(24.0))
            .align_x(iced::alignment::Horizontal::Center),
        text(entry.name.clone())
            .size(13)
            .color(name_color)
            .width(Length::Fill),
        text(size)
            .size(11)
            .color(Color::from_rgb(0.6, 0.6, 0.65))
            .width(Length::Fixed(72.0)),
        text(modified)
            .size(11)
            .color(Color::from_rgb(0.6, 0.6, 0.65))
            .width(Length::Fixed(96.0)),
        text(kind_label)
            .size(11)
            .color(Color::from_rgb(0.6, 0.6, 0.65))
            .width(Length::Fixed(96.0)),
    ]
    .spacing(8)
    .padding([3, 8])
    .align_y(Alignment::Center);

    // `mouse_area` captures both left-click (selection / drag start) and
    // right-click (context menu). Visual styling is applied via the
    // wrapping container.
    let styled = container(row_content)
        .width(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(row_background_color(selected).into()),
            ..Default::default()
        });

    mouse_area(styled)
        .on_press(Message::EntryClicked(i))
        .on_right_press(Message::EntryRightClicked(i))
        .on_move(Message::GridRowMouseMoved)
        .into()
}

/// Muted gray for hidden (dotfile) entries, near-white for normal ones.
fn name_color_for(entry: &FileEntry) -> Color {
    if entry.hidden {
        Color::from_rgb(0.55, 0.55, 0.6)
    } else {
        Color::from_rgb(0.92, 0.92, 0.93)
    }
}

/// Em-dash for directories and AppDirs (no meaningful file size);
/// human-readable size for files.
fn size_label_for(entry: &FileEntry) -> String {
    match entry.kind {
        EntryKind::Directory | EntryKind::AppDir { .. } => "—".to_string(),
        _ => format_size(entry.size),
    }
}

/// Type column text: "Folder", "AppDir", the file's MIME type, or a
/// symlink-target annotation.
fn kind_label_for(entry: &FileEntry) -> String {
    match &entry.kind {
        EntryKind::Directory => "Folder".to_string(),
        EntryKind::AppDir { .. } => "AppDir".to_string(),
        EntryKind::File => entry.mime_type.clone().unwrap_or_else(|| "file".into()),
        EntryKind::Symlink { target } => match target {
            Some(t) => format!("→ {}", t.display()),
            None => "symlink".into(),
        },
    }
}

/// Selected-row highlight color vs. default row background.
fn row_background_color(selected: bool) -> Color {
    if selected {
        Color::from_rgb(0.18, 0.32, 0.55)
    } else {
        Color::from_rgb(0.11, 0.11, 0.12)
    }
}

fn divider() -> Element<'static, Message> {
    container(text(""))
        .height(Length::Fixed(1.0))
        .width(Length::Fill)
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgb(0.22, 0.22, 0.25).into()),
            ..Default::default()
        })
        .into()
}

fn entry_icon(kind: &EntryKind) -> Icon {
    match kind {
        EntryKind::Directory => Icon::Folder,
        EntryKind::AppDir { .. } => Icon::AppDir,
        EntryKind::File => Icon::File,
        EntryKind::Symlink { .. } => Icon::Symlink,
    }
}
