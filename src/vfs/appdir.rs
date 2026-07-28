//! AppDir detection (ROX-Filer / Puppy Linux paradigm).
//!
//! A directory is treated as an "AppDir" — a double-click-to-launch
//! application bundle — if it contains either:
//!   1. An `AppRun` file that is executable, or
//!   2. A file matching the directory's own name that is executable.
//!
//! Shift+Double-Click is handled at the UI layer to force normal directory
//! entry regardless of AppDir status; this module only answers the
//! detection question.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Result of an AppDir probe: the executable that should be launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirInfo {
    /// The directory that was probed.
    pub dir: PathBuf,
    /// The executable entry point inside it (AppRun, or a same-named file).
    pub launcher: PathBuf,
}

/// Probe `dir_path` to see whether it qualifies as an AppDir.
///
/// Returns `None` if the path is not a directory, or contains no viable
/// launcher. This does I/O (metadata lookups) so it is async; callers
/// scanning many directories should run these concurrently (e.g. via
/// `tokio::task::JoinSet` or a `rayon` pool for the sync metadata calls).
pub async fn detect(dir_path: &Path) -> Option<AppDirInfo> {
    let meta = tokio::fs::metadata(dir_path).await.ok()?;
    if !meta.is_dir() {
        return None;
    }

    // 1. Canonical ROX convention: AppRun
    let apprun = dir_path.join("AppRun");
    if is_executable_file(&apprun).await {
        return Some(AppDirInfo {
            dir: dir_path.to_path_buf(),
            launcher: apprun,
        });
    }

    // 2. Puppy/DSL convention: a file named identically to the directory
    if let Some(dir_name) = dir_path.file_name() {
        let candidate = dir_path.join(dir_name);
        if is_executable_file(&candidate).await {
            return Some(AppDirInfo {
                dir: dir_path.to_path_buf(),
                launcher: candidate,
            });
        }
    }

    None
}

/// True if `path` exists, is a regular file (or symlink to one), and has
/// at least one executable bit set for user, group, or other.
async fn is_executable_file(path: &Path) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs;

    #[tokio::test]
    async fn detects_apprun() {
        let tmp = tempdir();
        let apprun = tmp.join("AppRun");
        fs::write(&apprun, b"#!/bin/sh\necho hi\n").await.unwrap();
        let mut perms = fs::metadata(&apprun).await.unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&apprun, perms).await.unwrap();

        let result = detect(&tmp).await;
        assert_eq!(result.unwrap().launcher, apprun);

        let _ = fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn detects_same_name_executable() {
        let tmp = tempdir();
        let name = tmp.file_name().unwrap().to_owned();
        let bin = tmp.join(&name);
        fs::write(&bin, b"#!/bin/sh\necho hi\n").await.unwrap();
        let mut perms = fs::metadata(&bin).await.unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin, perms).await.unwrap();

        let result = detect(&tmp).await;
        assert_eq!(result.unwrap().launcher, bin);

        let _ = fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn plain_directory_is_not_an_appdir() {
        let tmp = tempdir();
        assert!(detect(&tmp).await.is_none());
        let _ = fs::remove_dir_all(&tmp).await;
    }

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("runar-fm-test-{}", uuid_like()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn uuid_like() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    }
}
