//! runar — a hybrid Thunar/PCManFM + ROX-Filer file manager.
//!
//! Copyright (C) 2026 Jeremy Anderson <jeremy@dcos.net>
//! Licensed under GPL-2.0-only. See LICENSE file for details.
//!
//! Design inspiration (no code copied):
//!   * Thunar (Xfce)         — dual-pane layout, breadcrumb pathbar
//!   * PCManFM                — lightweight GTK file manager
//!   * ROX-Filer              — AppDir paradigm, instant MIME action hooks
//!   * Puppy Linux / DSL      — ROX-Filer as desktop backbone
//!   * SliTaz                 — minimal-footprint philosophy
//!
//! iced-based UI shell wrapping the Phase 1 VFS engine. The window shows
//! a pathbar, a stripped sidebar (Devices + default Locations + user
//! Bookmarks), and a file grid. All directory I/O happens on background
//! tokio tasks via iced subscriptions; the UI thread never blocks.
//!
//! Key bindings (Phase 2 keyboard-first navigation):
//!   * Arrow Up / Down  — move selection (skips hidden rows when hidden)
//!   * J / K            — same (vim-style)
//!   * H / L            — go up / enter selection
//!   * Enter            — activate selection (open / launch / navigate)
//!   * Backspace        — go up one directory
//!   * Alt+Up           — go up one directory (Go menu binding)
//!   * /  or  Ctrl+L   — toggle pathbar edit mode
//!   * Ctrl+H           — toggle hidden files
//!   * Ctrl+R           — reload current directory
//!   * Ctrl+D           — bookmark current directory
//!   * Escape           — exit pathbar edit mode (only when editing)
//!   * Shift+Enter      — on an AppDir, enter it as a directory
//!
//! Usage: runar [directory]   (defaults to current directory)

// Clippy's `doc_overindented_list_items` lint flags continuation lines
// in our doc comments (lines indented to align with a preceding bullet
// item's text) as misindented list items. They're not list items —
// they're prose continuations — but clippy can't tell the difference.
// Suppress crate-wide rather than littering `#[allow]` on every doc
// block.
#![allow(clippy::doc_overindented_list_items)]

mod config;
mod date;
mod icons;
mod launch;
mod mime;
mod mounts;
mod ui;
mod vfs;

use iced::keyboard::on_key_press;
use iced::{application, Subscription};
use std::path::PathBuf;
use ui::{App, Message};

fn main() -> iced::Result {
    env_logger::init();

    let initial_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    // Resolve to a canonical absolute path so breadcrumbs render cleanly.
    let initial_dir = std::fs::canonicalize(&initial_dir).unwrap_or(initial_dir);

    // The first arg to `application()` is the title — either a `&'static str`
    // or a `Fn(&State) -> String`. We use the closure form so the title
    // reflects the current directory.
    application(
        |app: &App| format!("runar — {}", app.current_dir().display()),
        ui::update,
        ui::view,
    )
    .theme(|_| iced::Theme::Dark)
    .subscription(subscription)
    .run_with(move || (App::new(initial_dir), iced::Task::none()))
}

/// Per-app subscription aggregator: combine the VFS/watch/mount
/// subscriptions from `ui::subscription` with a global key-press
/// subscription for keyboard navigation.
fn subscription(app: &App) -> Subscription<Message> {
    // Keyboard handling: iced's `on_key_press` requires a bare `fn` (not a
    // closure), so app state cannot be captured here. Instead, emit a
    // `KeyPressed(key, modifiers)` Message for every key press and let
    // `ui::update` perform the stateful interpretation. This keeps the
    // keyboard handler in `update()` where `&mut App` is available, so
    // Escape closes only the topmost open layer (About → context menu →
    // menubar → pathbar edit) rather than toggling state blindly.
    let key_sub = on_key_press(key_press_bridge);
    Subscription::batch([ui::subscription(app), key_sub])
}

/// Stateless bridge: wrap the raw (Key, Modifiers) pair into a Message
/// and let `update()` interpret it. Returns `None` for key events we
/// never care about (so they propagate to widgets like text_input).
fn key_press_bridge(
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    Some(Message::KeyPressed(key, modifiers))
}
