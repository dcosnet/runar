//! Config engine: TOML-backed user configuration.
//!
//! Two files live under `~/.config/runar/`:
//!
//!   * `bookmarks.toml`  — purely user-added paths (no XDG defaults injected,
//!                         per project spec: "Excluded: Default hardcoded XDG
//!                         directories (~/Downloads, ~/Documents, etc.)").
//!
//!   * `actions.toml`    — file-extension / MIME-type → shell command overrides.
//!                         Checked before `xdg-open`/`open::that` fallback.
//!
//! Both files are created empty on first run. The engine is fail-soft: if
//! a file is missing or malformed, the corresponding config is treated as
//! empty and the app still launches.
//!
//! ## Testing note
//!
//! The directory-resolution functions (`config_dir`, `config_dir_with`)
//! take an explicit override parameter so tests don't have to mutate
//! `XDG_CONFIG_HOME` / `HOME` env vars (which is a flakiness risk under
//! parallel `cargo test` and an `unsafe` operation on Rust 1.82+).

pub mod bookmarks;
pub mod actions;
pub mod defaults;

pub use actions::Action;
pub use bookmarks::Bookmarks;
pub use defaults::{defaults as default_locations, DefaultLocation};

use std::path::{Path, PathBuf};

/// Combined in-memory config, loaded once at startup and re-saved on edit.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub bookmarks: Bookmarks,
    pub actions: Vec<Action>,
}

/// Where config files live. Resolved via `XDG_CONFIG_HOME` → `~/.config`,
/// then suffixed with `runar/`. Created on first save if missing.
///
/// Production callers use this. Tests should call `config_dir_with` to
/// inject an explicit base path (avoids env-var mutation races under
/// parallel test execution).
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".config");
                p
            })
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    config_dir_with(&base)
}

/// Same as `config_dir` but takes an explicit base directory (the parent
/// of the `runar/` suffix). Used by tests so they don't have to touch
/// `XDG_CONFIG_HOME`.
pub fn config_dir_with(base: &Path) -> PathBuf {
    let mut p = base.to_path_buf();
    p.push("runar");
    p
}

impl Config {
    /// Load both files from the default config dir. Missing files →
    /// empty section. Malformed → empty section plus a logged warning
    /// (we never panic on bad config).
    pub fn load() -> Self {
        Self::load_from(&config_dir())
    }

    /// Load both files from an explicit directory. Used by tests.
    pub fn load_from(dir: &Path) -> Self {
        let bookmarks = Bookmarks::load(dir);
        let actions = Action::load_all(dir);
        Self { bookmarks, actions }
    }

    /// Persist both files atomically (write-to-temp + fsync + rename).
    /// Creates the config dir on demand.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_dir())
    }

    /// Persist both files to an explicit directory. Used by tests.
    pub fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        self.bookmarks.save(dir)?;
        Action::save_all(dir, &self.actions)?;
        Ok(())
    }

    /// Find the first action whose pattern matches the given path (by
    /// extension or MIME type). The launch dispatcher consults this before
    /// falling back to `xdg-open`.
    pub fn find_action_for(&self, path: &Path) -> Option<&Action> {
        let mime = mime_guess::from_path(path).first();
        self.actions.iter().find(|a| a.matches(path, mime.as_ref()))
    }

    /// Add a bookmark (deduped). Caller is responsible for `save()`.
    pub fn add_bookmark(&mut self, path: PathBuf) -> bool {
        self.bookmarks.add(path)
    }

    /// Insert a bookmark at a specific index (for drag-and-drop reordering).
    /// If the path is already bookmarked, it's moved to the new index.
    /// Caller is responsible for `save()`.
    pub fn insert_bookmark_at(&mut self, path: PathBuf, index: usize) -> bool {
        self.bookmarks.insert_at(path, index)
    }

    /// Remove a bookmark by exact path. Caller is responsible for `save()`.
    pub fn remove_bookmark(&mut self, path: &Path) -> bool {
        self.bookmarks.remove(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_loads_empty() {
        // Use an explicit non-existent dir; no env-var mutation required.
        let tmp = std::env::temp_dir().join(format!(
            "runar-fm-cfg-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Note: dir doesn't exist on disk — Config::load_from should
        // treat that as "no config" rather than panicking.
        let cfg = Config::load_from(&tmp);
        assert!(cfg.bookmarks.paths().is_empty());
        assert!(cfg.actions.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn add_then_remove_bookmark_roundtrip() {
        let mut cfg = Config::default();
        let p = PathBuf::from("/tmp/example");
        assert!(cfg.add_bookmark(p.clone()));
        assert!(!cfg.add_bookmark(p.clone())); // dedup
        assert!(cfg.bookmarks.paths().contains(&p));
        assert!(cfg.remove_bookmark(&p));
        assert!(!cfg.bookmarks.paths().contains(&p));
    }

    #[test]
    fn config_dir_with_appends_runar() {
        let p = config_dir_with(Path::new("/tmp/test-config-base"));
        assert_eq!(p, PathBuf::from("/tmp/test-config-base/runar"));
    }
}
