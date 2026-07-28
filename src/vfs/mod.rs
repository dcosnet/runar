//! Virtual filesystem layer: non-blocking directory scanning.
//!
//! Design goal (per project spec, "Zero-Lock Non-Blocking I/O"): the GUI
//! thread must never block on directory reads, stat() calls, or AppDir
//! probing. `scan_directory` runs entirely on the tokio runtime and
//! streams results back over a tokio channel as they become available,
//! so the UI can render entries incrementally instead of waiting for the
//! whole directory to be read (important for large dirs / slow NFS mounts).
//!
//! The iced UI layer bridges these tokio channels into iced subscriptions
//! via a small adapter (see `src/ui/mod.rs::subscription`).

pub mod appdir;
pub mod watcher;

use mime_guess::MimeGuess;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::sync::mpsc::UnboundedSender;

pub use watcher::{watch_directory, WatchEvent};

/// What kind of thing a scanned entry is, for launch-dispatch purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    /// A directory that qualifies as a ROX-style AppDir; carries the
    /// launcher path so the dispatcher doesn't need to re-probe it.
    AppDir { launcher: PathBuf },
    Symlink { target: Option<PathBuf> },
}

/// A single scanned filesystem entry, ready for display in the grid/list.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Permission bits — populated during scan, used by future file-ops
    /// and permissions-dialog UI. Currently unread but kept for that
    /// upcoming feature rather than re-adding later.
    #[allow(dead_code)]
    pub readable: bool,
    #[allow(dead_code)]
    pub writable: bool,
    #[allow(dead_code)]
    pub executable: bool,
    pub mime_type: Option<String>,
    pub hidden: bool,
}

/// Events streamed out of `scan_directory` as the read progresses.
#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// One entry has been fully stat'd (and AppDir-probed, if applicable)
    /// and is ready to display.
    Entry(FileEntry),
    /// The scan of `dir` has finished; `total` entries were emitted.
    Done {
        #[allow(dead_code)]
        dir: PathBuf,
        total: usize,
    },
    /// A per-entry error (e.g. permission denied on stat) — non-fatal,
    /// the scan continues with the next entry.
    EntryError { path: PathBuf, message: String },
    /// The directory itself could not be opened at all — fatal for this
    /// scan.
    Fatal { dir: PathBuf, message: String },
}

/// Scan `dir` asynchronously, sending a `ScanEvent` for every entry as
/// soon as it's ready, plus a final `Done`. Errors on individual entries
/// are reported via `EntryError` and do not abort the scan; only a
/// failure to open the directory itself is `Fatal`.
///
/// This does not recurse — each pane scans exactly one directory level,
/// consistent with the non-recursive watcher above.
pub async fn scan_directory(dir: PathBuf, tx: UnboundedSender<ScanEvent>) {
    let mut read_dir = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) => {
            let _ = tx.send(ScanEvent::Fatal {
                dir,
                message: e.to_string(),
            });
            return;
        }
    };

    let mut total = 0usize;

    loop {
        let next = read_dir.next_entry().await;
        let entry = match next {
            Ok(Some(entry)) => entry,
            Ok(None) => break, // exhausted
            Err(e) => {
                let _ = tx.send(ScanEvent::EntryError {
                    path: dir.clone(),
                    message: e.to_string(),
                });
                break;
            }
        };

        let path = entry.path();
        match build_file_entry(&path).await {
            Ok(file_entry) => {
                total += 1;
                if tx.send(ScanEvent::Entry(file_entry)).is_err() {
                    // Receiver dropped — pane was closed mid-scan. Stop
                    // doing work nobody is listening to.
                    return;
                }
            }
            Err(e) => {
                let _ = tx.send(ScanEvent::EntryError {
                    path: path.clone(),
                    message: e.to_string(),
                });
            }
        }
    }

    let _ = tx.send(ScanEvent::Done { dir, total });
}

async fn build_file_entry(path: &Path) -> std::io::Result<FileEntry> {
    // Use symlink_metadata first so we can tell a symlink from its target
    // without silently following it.
    let link_meta = tokio::fs::symlink_metadata(path).await?;
    let is_symlink = link_meta.file_type().is_symlink();

    let meta = if is_symlink {
        // Follow the link for size/permissions display, but don't fail
        // the whole entry if it's a dangling link.
        tokio::fs::metadata(path).await.ok()
    } else {
        Some(link_meta.clone())
    };

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let hidden = name.starts_with('.');

    let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

    let kind = if is_symlink {
        let target = tokio::fs::read_link(path).await.ok();
        EntryKind::Symlink { target }
    } else if is_dir {
        match appdir::detect(path).await {
            Some(info) => EntryKind::AppDir {
                launcher: info.launcher,
            },
            None => EntryKind::Directory,
        }
    } else {
        EntryKind::File
    };

    let (readable, writable, executable) = match &meta {
        Some(m) => {
            let mode = m.permissions().mode();
            (mode & 0o400 != 0, mode & 0o200 != 0, mode & 0o100 != 0)
        }
        None => (false, false, false),
    };

    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let modified = meta.as_ref().and_then(|m| m.modified().ok());

    let mime_type = if matches!(kind, EntryKind::File) {
        MimeGuess::from_path(path)
            .first()
            .map(|m| m.essence_str().to_string())
    } else {
        None
    };

    Ok(FileEntry {
        path: path.to_path_buf(),
        name,
        kind,
        size,
        modified,
        readable,
        writable,
        executable,
        mime_type,
        hidden,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn scans_a_directory_and_reports_done() {
        let mut tmp = std::env::temp_dir();
        tmp.push(format!(
            "runar-fm-scan-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        tokio::fs::write(tmp.join("a.txt"), b"hi").await.unwrap();
        tokio::fs::write(tmp.join("b.txt"), b"there").await.unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let dir = tmp.clone();
        tokio::spawn(scan_directory(dir, tx));

        let mut seen = 0;
        let mut done = false;
        while let Some(event) = rx.recv().await {
            match event {
                ScanEvent::Entry(_) => seen += 1,
                ScanEvent::Done { total, .. } => {
                    assert_eq!(total, 2);
                    done = true;
                    break;
                }
                ScanEvent::Fatal { message, .. } => panic!("scan failed: {message}"),
                ScanEvent::EntryError { message, .. } => panic!("entry error: {message}"),
            }
        }
        assert_eq!(seen, 2);
        assert!(done);

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
