//! `bookmarks.toml` — purely user-added paths.
//!
//! Schema:
//! ```toml
//! [[bookmarks]]
//! path = "/mnt/data"
//! label = "Data"
//! ```
//!
//! `label` is optional; if absent, the path's basename (or "/" for root)
//! is used for display. No XDG defaults are ever injected.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Bookmarks {
    entries: Vec<BookmarkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookmarkEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileShape {
    #[serde(default)]
    bookmarks: Vec<BookmarkEntry>,
}

impl Bookmarks {
    #[allow(dead_code)]
    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|e| e.path.clone()).collect()
    }

    pub fn entries(&self) -> &[BookmarkEntry] {
        &self.entries
    }

    /// Add a path if it isn't already bookmarked. Returns true on insertion.
    pub fn add(&mut self, path: PathBuf) -> bool {
        if self.entries.iter().any(|e| e.path == path) {
            return false;
        }
        self.entries.push(BookmarkEntry { path, label: None });
        true
    }

    /// Insert a path at a specific index. Used by drag-and-drop when the
    /// user drops a folder between two existing bookmarks. If the path is
    /// already bookmarked, it's moved to the new index (remove + reinsert).
    /// The index is clamped to `[0, len]`.
    ///
    /// Returns true if the bookmark was inserted (or moved).
    pub fn insert_at(&mut self, path: PathBuf, index: usize) -> bool {
        // Remove existing entry if present (this is a move, not a dup).
        self.entries.retain(|e| e.path != path);
        let clamped = index.min(self.entries.len());
        self.entries.insert(clamped, BookmarkEntry { path, label: None });
        true
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.path != path);
        self.entries.len() != before
    }

    pub fn load(dir: &Path) -> Self {
        let path = dir.join("bookmarks.toml");
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<FileShape>(&s) {
                Ok(shape) => Self {
                    entries: shape.bookmarks,
                },
                Err(e) => {
                    log::warn!("malformed {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let shape = FileShape {
            bookmarks: self.entries.clone(),
        };
        let s = toml::to_string_pretty(&shape).unwrap_or_default();
        atomic_write(dir.join("bookmarks.toml"), s.as_bytes())
    }
}

/// Write to a temp file in the same dir, fsync it, then rename — avoids
/// leaving a half-written config file if we crash mid-write, and (with
/// the fsync) gives a much stronger guarantee that the write survives a
/// power loss immediately after the function returns.
///
/// The temp file is fsync'd before the rename so that the rename itself
/// only exposes a fully-durably-stored file. The parent directory is
/// fsync'd after the rename so the rename itself is durable — without
/// this step, a power loss after `rename()` returns could leave the old
/// file (or no file) visible on reboot, even though the data was written.
///
/// This is the standard "atomic durable write" pattern on POSIX filesystems.
pub(crate) fn atomic_write(target: PathBuf, data: &[u8]) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let dir = target.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("cfg")
    ));

    // Write data to temp file.
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?; // fsync the temp file before rename
    } // file handle dropped here

    // Atomic rename.
    std::fs::rename(&tmp, &target)?;

    // fsync the parent directory so the rename is durable. On Linux
    // this requires opening the dir as a file (O_RDONLY), which std
    // supports via File::open. Errors here are non-fatal — the file
    // content is already durable, we just can't guarantee the rename
    // survives a power loss. Log and continue.
    if let Ok(dir_file) = std::fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bookmarks() {
        let tmp = std::env::temp_dir().join(format!(
            "runar-fm-bm-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let mut bm = Bookmarks::default();
        bm.add(PathBuf::from("/mnt/data"));
        bm.add(PathBuf::from("/home/user/projects"));
        bm.save(&tmp).unwrap();

        let loaded = Bookmarks::load(&tmp);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].path, PathBuf::from("/mnt/data"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn insert_at_beginning() {
        let mut bm = Bookmarks::default();
        bm.add(PathBuf::from("/a"));
        bm.add(PathBuf::from("/b"));
        bm.add(PathBuf::from("/c"));
        bm.insert_at(PathBuf::from("/new"), 0);
        assert_eq!(bm.entries.len(), 4);
        assert_eq!(bm.entries[0].path, PathBuf::from("/new"));
        assert_eq!(bm.entries[1].path, PathBuf::from("/a"));
    }

    #[test]
    fn insert_at_middle() {
        let mut bm = Bookmarks::default();
        bm.add(PathBuf::from("/a"));
        bm.add(PathBuf::from("/b"));
        bm.add(PathBuf::from("/c"));
        bm.insert_at(PathBuf::from("/new"), 2);
        assert_eq!(bm.entries.len(), 4);
        assert_eq!(bm.entries[2].path, PathBuf::from("/new"));
        assert_eq!(bm.entries[3].path, PathBuf::from("/c"));
    }

    #[test]
    fn insert_at_end() {
        let mut bm = Bookmarks::default();
        bm.add(PathBuf::from("/a"));
        bm.add(PathBuf::from("/b"));
        bm.insert_at(PathBuf::from("/new"), 5); // clamp to len=2
        assert_eq!(bm.entries.len(), 3);
        assert_eq!(bm.entries[2].path, PathBuf::from("/new"));
    }

    #[test]
    fn insert_at_moves_existing() {
        // If the path is already bookmarked, insert_at moves it to the
        // new index rather than creating a duplicate.
        let mut bm = Bookmarks::default();
        bm.add(PathBuf::from("/a"));
        bm.add(PathBuf::from("/b"));
        bm.add(PathBuf::from("/c"));
        // Move /a to index 2 (between /b and /c... actually after remove,
        // /b and /c shift left, so index 2 = after /c)
        bm.insert_at(PathBuf::from("/a"), 2);
        assert_eq!(bm.entries.len(), 3); // no duplicate
        assert_eq!(bm.entries[0].path, PathBuf::from("/b"));
        assert_eq!(bm.entries[1].path, PathBuf::from("/c"));
        assert_eq!(bm.entries[2].path, PathBuf::from("/a"));
    }
}
