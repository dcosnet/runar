//! Top pathbar — breadcrumb view, toggleable to a raw editable text input
//! via the edit button, `/` key, or `Ctrl+L` (handled in main.rs key bindings).
//!
//! In breadcrumb mode, each path segment is a clickable button that
//! navigates directly to that ancestor. The full path is always visible.

use super::{icon_button, Message};
use crate::icons::Icon;
use iced::widget::{button, row, text, text_input};
use iced::{Alignment, Color, Element, Length};
use std::path::PathBuf;

pub fn view<'a>(app: &'a super::App) -> Element<'a, Message> {
    let up = icon_button(Icon::Up, Message::GoUp);
    let reload = icon_button(Icon::Reload, Message::Reload);

    let middle: Element<Message> = if app.pathbar_editing() {
        text_input("path…", app.pathbar_input())
            .on_submit(Message::PathBarSubmit)
            .on_input(Message::PathBarEdit)
            .size(14)
            .width(Length::Fill)
            .into()
    } else {
        breadcrumbs(app)
    };

    let edit_btn = icon_button(Icon::Edit, Message::PathBarToggleEdit);

    row![up, reload, middle, edit_btn]
        .spacing(4)
        .padding([6, 8])
        .align_y(Alignment::Center)
        .height(Length::Shrink)
        .into()
}

/// Render the current path as a series of clickable segments separated
/// by `/`. Clicking any segment navigates to that ancestor.
fn breadcrumbs<'a>(app: &'a super::App) -> Element<'a, Message> {
    let dir = app.current_dir();
    let mut segments: Vec<(String, PathBuf)> = Vec::new();
    let mut acc = PathBuf::new();

    // Always start with "/" as the root segment.
    segments.push(("/".to_string(), PathBuf::from("/")));

    for component in dir.components() {
        use std::path::Component;
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                acc.push(name);
                let label = name.to_string_lossy().into_owned();
                segments.push((label, acc.clone()));
            }
            Component::ParentDir => {
                acc.push("..");
                segments.push(("..".into(), acc.clone()));
            }
            _ => {}
        }
    }

    let total = segments.len();
    let mut row_state = row![].spacing(2).align_y(Alignment::Center);
    for (i, (label, path)) in segments.into_iter().enumerate() {
        if i > 0 {
            row_state = row_state.push(
                text("/")
                    .size(13)
                    .color(Color::from_rgb(0.55, 0.55, 0.6)),
            );
        }
        let is_last = i == total - 1;
        let color = if is_last {
            Color::from_rgb(0.95, 0.95, 0.95)
        } else {
            Color::from_rgb(0.6, 0.7, 0.85)
        };
        let btn = button(
            text(label)
                .size(13)
                .color(color)
                .width(Length::Shrink),
        )
        .padding([2, 6])
        .style(if is_last {
            button::primary
        } else {
            button::secondary
        })
        .on_press(Message::PathBarSegment(path));
        row_state = row_state.push(btn);
    }

    row_state.width(Length::Fill).into()
}
