//! Left sidebar — three sections:
//!
//!   1. DEVICES     — mounts polled from /proc/mounts (real storage only,
//!                    pseudo-FSes filtered out by src/mounts.rs).
//!
//!   2. LOCATIONS   — hardcoded default shortcuts: /mnt, /var/run/media,
//!                    /opt, /usr/src, $HOME, $HOME/Downloads. Always
//!                    present; not user-removable. Defined in
//!                    src/config/defaults.rs.
//!
//!   3. BOOKMARKS   — purely user-added paths. Seeded empty on first run.
//!                    No XDG defaults are ever injected here.
//!
//! The sidebar is intentionally narrow — this is a "clean canvas" tuned
//! for power users, not a clone of Nautilus's preset-heavy sidebar.

use super::Message;
use crate::config::bookmarks::BookmarkEntry;
use crate::config::DefaultLocation;
use crate::icons::Icon;
use crate::mounts::MountInfo;
use iced::widget::{button, column, container, mouse_area, scrollable, text, Space};
use iced::{Alignment, Color, Element, Length};
use std::path::PathBuf;

pub fn view<'a>(app: &'a super::App) -> Element<'a, Message> {
    let devices_label = section_label("DEVICES");
    let mut devices_col = column![].spacing(2);

    for mount in app.mounts() {
        devices_col = devices_col.push(mount_row(mount));
    }

    if app.mounts().is_empty() {
        devices_col = devices_col.push(
            text("(no mounts found)")
                .size(11)
                .color(Color::from_rgb(0.45, 0.45, 0.5)),
        );
    }

    let locations_label = section_label("LOCATIONS");
    let mut locations_col = column![].spacing(2);
    for loc in crate::config::default_locations() {
        locations_col = locations_col.push(location_row(&loc));
    }

    let bookmarks_label = section_label("BOOKMARKS");
    let mut bookmarks_col = column![].spacing(2);

    // Render each bookmark row with its index, so on_enter can report
    // which slot the cursor is hovering over during a drag.
    for (idx, entry) in app.bookmarks().iter().enumerate() {
        let is_drop_target = app.drag_hover_bookmark() == Some(idx);
        bookmarks_col = bookmarks_col.push(bookmark_row(idx, entry, is_drop_target));
    }

    // Append zone: the empty area below the last bookmark. During a drag,
    // entering this zone sets hover = Some(len) so the drop appends.
    let is_append_target = app.drag_hover_bookmark() == Some(app.bookmarks().len());
    if app.bookmarks().is_empty() {
        // Show hint text when empty — also serves as the drop zone.
        let hint = text("(drag a folder here,")
            .size(11)
            .color(Color::from_rgb(0.45, 0.45, 0.5));
        let hint2 = text("or edit bookmarks.toml)")
            .size(11)
            .color(Color::from_rgb(0.45, 0.45, 0.5));
        let hints = column![hint, hint2].spacing(0).padding([3, 6]);
        let zone = if is_append_target {
            container(hints)
                .width(Length::Fill)
                .style(|_: &iced::Theme| container::Style {
                    background: Some(Color::from_rgb(0.18, 0.32, 0.55).into()),
                    border: iced::Border {
                        color: Color::from_rgb(0.3, 0.5, 0.8),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            container(hints).width(Length::Fill)
        };
        bookmarks_col = bookmarks_col.push(mouse_area(zone)
            .on_enter(Message::DragEnteredBookmarkAppendZone)
            .on_exit(Message::DragExitedBookmarks));
    } else {
        // Non-empty: show a small drop zone below the last entry.
        let zone_height = if is_append_target { 24.0 } else { 12.0 };
        let zone = container(Space::new(Length::Fill, zone_height))
            .width(Length::Fill)
            .style(move |_: &iced::Theme| {
                if is_append_target {
                    container::Style {
                        background: Some(Color::from_rgb(0.18, 0.32, 0.55).into()),
                        border: iced::Border {
                            color: Color::from_rgb(0.3, 0.5, 0.8),
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    container::Style::default()
                }
            });
        bookmarks_col = bookmarks_col.push(mouse_area(zone)
            .on_enter(Message::DragEnteredBookmarkAppendZone)
            .on_exit(Message::DragExitedBookmarks));
    }

    let content = column![
        devices_label,
        devices_col,
        Space::new(0, 12),
        locations_label,
        locations_col,
        Space::new(0, 12),
        bookmarks_label,
        bookmarks_col,
    ]
    .spacing(4)
    .padding([8, 6]);

    // Scrollable in case the user has many mounts / bookmarks.
    let scrolled = scrollable(content).width(Length::Fill).height(Length::Fill);

    container(scrolled)
        .width(180)
        .height(Length::Fill)
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgb(0.11, 0.11, 0.12).into()),
            border: iced::Border {
                color: Color::from_rgb(0.25, 0.25, 0.27),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn section_label(s: &'static str) -> Element<'static, Message> {
    text(s)
        .size(10)
        .font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        })
        .color(Color::from_rgb(0.55, 0.55, 0.6))
        .into()
}

fn mount_row(mount: &MountInfo) -> Element<'_, Message> {
    let icon = if mount.is_network {
        Icon::Network
    } else {
        Icon::Device
    };
    button(
        iced::widget::row![
            icon.widget(14.0),
            text(mount.label.clone()).size(12),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([3, 6])
    .width(Length::Fill)
    .style(button::secondary)
    .on_press(Message::MountSelected(mount.target.clone()))
    .into()
}

fn location_row(loc: &DefaultLocation) -> Element<'static, Message> {
    // Locations whose path didn't resolve (e.g. $HOME unset) are shown
    // grayed-out and non-interactive, so users can see they exist but
    // can't click into nothing.
    let label_color = if loc.path.is_some() {
        Color::from_rgb(0.92, 0.92, 0.93)
    } else {
        Color::from_rgb(0.4, 0.4, 0.45)
    };

    // Static label + Copy icon, so we can build a 'static row_content.
    // Path gets cloned into the message closure.
    let label: &'static str = loc.label;
    let icon = loc.icon;

    let row_content = iced::widget::row![
        icon.widget(14.0),
        text(label).size(12).color(label_color),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    match loc.path.clone() {
        Some(p) => button(row_content)
            .padding([3, 6])
            .width(Length::Fill)
            .style(button::secondary)
            .on_press(Message::MountSelected(p))
            .into(),
        None => container(row_content)
            .padding([3, 6])
            .width(Length::Fill)
            .into(),
    }
}

fn bookmark_row(
    idx: usize,
    entry: &BookmarkEntry,
    is_drop_target: bool,
) -> Element<'_, Message> {
    let label: String = entry
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

    let row_content = iced::widget::row![
        Icon::Bookmark.widget(14.0),
        text(label).size(12),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let styled = container(row_content)
        .padding([3, 6])
        .width(Length::Fill)
        .style(move |_: &iced::Theme| {
            if is_drop_target {
                // Highlight the row when the cursor is hovering over it
                // during a drag — visual feedback for where the drop lands.
                container::Style {
                    background: Some(Color::from_rgb(0.18, 0.32, 0.55).into()),
                    border: iced::Border {
                        color: Color::from_rgb(0.3, 0.5, 0.8),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }
            } else {
                container::Style {
                    background: Some(Color::from_rgb(0.14, 0.14, 0.15).into()),
                    ..Default::default()
                }
            }
        });

    // mouse_area gives us on_enter (for drag hover tracking) AND on_press
    // (for click-to-navigate). The on_enter fires on every cursor entry,
    // but update() only records the hover index when a drag is in progress.
    mouse_area(styled)
        .on_press(Message::BookmarkSelected(entry.path.clone()))
        .on_enter(Message::DragEnteredBookmark(idx))
        .on_exit(Message::DragExitedBookmarks)
        .into()
}

/// Helper for the "add current dir to bookmarks" action — exposed so the
/// main view can show a star button in the pathbar area if desired.
#[allow(dead_code)]
pub fn add_bookmark_button(current: PathBuf) -> Element<'static, Message> {
    button(Icon::Bookmark.widget(14.0))
        .padding(4)
        .style(button::secondary)
        .on_press(Message::AddBookmarkFor(current))
        .into()
}
