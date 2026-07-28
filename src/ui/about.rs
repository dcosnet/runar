//! About dialog — shown when the user picks Help → About runar.
//!
//! Renders as a full-window panel (not a true modal — iced 0.13 doesn't
//! have those) with the project name, version, author, license, and a
//! short description. Closes on Escape, on clicking the Close button,
//! or on clicking anywhere outside the dialog (the background is a
//! button that emits CloseAbout).

use super::Message;
use crate::icons::Icon;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Color, Element, Length};

pub fn view(_app: &super::App) -> Element<'static, Message> {
    // The whole window is a button — clicking anywhere closes the dialog.
    // Inside it, a centered panel shows the actual content.
    let content = column![
        Icon::Folder.widget(48.0),
        text("runar").size(28).color(Color::from_rgb(0.95, 0.95, 0.95)),
        text("v0.1.0").size(12).color(Color::from_rgb(0.6, 0.6, 0.65)),
        Space::new(0, 16),
        text("A hybrid Thunar/PCManFM + ROX-Filer file manager.")
            .size(13)
            .color(Color::from_rgb(0.85, 0.85, 0.88)),
        text("Built in Rust with iced. Pure-Rust, statically linkable.")
            .size(13)
            .color(Color::from_rgb(0.85, 0.85, 0.88)),
        Space::new(0, 16),
        text("Author").size(11).color(Color::from_rgb(0.55, 0.55, 0.6)),
        text("Jeremy Anderson <info@dcos.net>")
            .size(13)
            .color(Color::from_rgb(0.92, 0.92, 0.93)),
        text("https://git.dcos.net/dcosnet/runar")
            .size(12)
            .color(Color::from_rgb(0.5, 0.7, 0.95)),
        Space::new(0, 16),
        text("License").size(11).color(Color::from_rgb(0.55, 0.55, 0.6)),
        text("GPL-2.0-only (same as Thunar)")
            .size(13)
            .color(Color::from_rgb(0.92, 0.92, 0.93)),
        Space::new(0, 20),
        text("Design inspiration").size(11).color(Color::from_rgb(0.55, 0.55, 0.6)),
        text("Thunar (Xfce) — dual-pane layout, breadcrumb pathbar")
            .size(12)
            .color(Color::from_rgb(0.75, 0.75, 0.78)),
        text("PCManFM (LXDE) — lightweight GTK file manager")
            .size(12)
            .color(Color::from_rgb(0.75, 0.75, 0.78)),
        text("ROX-Filer — AppDir paradigm, MIME action hooks")
            .size(12)
            .color(Color::from_rgb(0.75, 0.75, 0.78)),
        text("Puppy Linux / DSL — ROX-Filer as desktop backbone")
            .size(12)
            .color(Color::from_rgb(0.75, 0.75, 0.78)),
        text("SliTaz — minimal-footprint philosophy")
            .size(12)
            .color(Color::from_rgb(0.75, 0.75, 0.78)),
        Space::new(0, 20),
        button(text("Close").size(13))
            .padding([6, 16])
            .style(button::secondary)
            .on_press(Message::CloseAbout),
    ]
    .spacing(2)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    // Wrap content in a scrollable in case the window is too short.
    let scroll = scrollable(content).height(Length::Fill);

    // Centered panel on a dimmed background. The background is itself a
    // button so clicking outside the panel closes the dialog.
    let panel = container(scroll)
        .width(420)
        .max_height(560)
        .padding([24, 32])
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgb(0.13, 0.13, 0.15).into()),
            border: iced::Border {
                color: Color::from_rgb(0.32, 0.32, 0.36),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

    let centered = container(panel)
        .center(iced::Length::Fill)
        .style(|_: &iced::Theme| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
            ..Default::default()
        });

    // Wrap the whole thing in a button so clicks on the dimmed background
    // close the dialog. The panel content has its own interactive widgets
    // (the Close button) which take precedence.
    button(centered)
        .padding(0)
        .style(|_theme: &iced::Theme, _status: button::Status| button::Style {
            background: Some(iced::Color::TRANSPARENT.into()),
            border: iced::Border {
                color: iced::Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .on_press(Message::CloseAbout)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// Suppress unused-import warning for `row` (kept for future layout needs).
#[allow(unused_imports)]
use row as _row;
