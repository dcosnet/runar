//! Real-time directory monitoring via `notify` (inotify on Linux).
//!
//! `notify`'s watcher calls back on its own internal thread, so we bridge
//! those callbacks into a `tokio::sync::mpsc::UnboundedSender`. The GUI
//! layer (Phase 2) will hold the receiving end on the async runtime and
//! forward events to the directory model via `glib::MainContext::spawn`.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::UnboundedSender;

/// A filesystem change relevant to the currently displayed directory.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    Created(PathBuf),
    Removed(PathBuf),
    Modified(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    /// Emitted when the watch itself hits an error (e.g. watched dir was
    /// deleted out from under us, or an unmounted network share dropped).
    WatchError(String),
}

/// Owns the live `notify` watcher. Dropping this stops the watch, so the
/// caller must keep it alive (e.g. store it alongside the active tab/pane
/// state) for as long as the directory should be monitored.
pub struct DirWatcher {
    _inner: RecommendedWatcher,
    #[allow(dead_code)]
    pub path: PathBuf,
}

/// Begin watching `path` (non-recursively — each open pane watches only
/// its own directory, so navigating into a subdirectory spins up a new
/// watcher for that path and drops the old one).
pub fn watch_directory(
    path: &Path,
    tx: UnboundedSender<WatchEvent>,
) -> notify::Result<DirWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        match res {
            Ok(event) => {
                for mapped in map_event(event) {
                    // Receiver may have been dropped (pane closed); ignore.
                    let _ = tx.send(mapped);
                }
            }
            Err(e) => {
                let _ = tx.send(WatchEvent::WatchError(e.to_string()));
            }
        }
    })?;

    watcher.watch(path, RecursiveMode::NonRecursive)?;

    Ok(DirWatcher {
        _inner: watcher,
        path: path.to_path_buf(),
    })
}

fn map_event(event: Event) -> Vec<WatchEvent> {
    match event.kind {
        EventKind::Create(_) => event
            .paths
            .into_iter()
            .map(WatchEvent::Created)
            .collect(),
        EventKind::Remove(_) => event
            .paths
            .into_iter()
            .map(WatchEvent::Removed)
            .collect(),
        EventKind::Modify(notify::event::ModifyKind::Name(notify::event::RenameMode::Both)) => {
            // Both halves of a rename arrive together with two paths:
            // [from, to]. If the platform only gives us one half at a
            // time, we fall back to treating it as remove+create below.
            if event.paths.len() == 2 {
                vec![WatchEvent::Renamed {
                    from: event.paths[0].clone(),
                    to: event.paths[1].clone(),
                }]
            } else {
                event.paths.into_iter().map(WatchEvent::Modified).collect()
            }
        }
        EventKind::Modify(_) => event
            .paths
            .into_iter()
            .map(WatchEvent::Modified)
            .collect(),
        _ => Vec::new(),
    }
}
