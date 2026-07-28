//! Default locations — hardcoded shortcuts shown in the sidebar's
//! "LOCATIONS" section. These are always present (cannot be removed by
//! the user) and complement the user-editable Bookmarks section.
//!
//! Per the project's customization request, the following locations are
//! pinned by default:
//!   * /mnt              — traditional mount point
//!   * /var/run/media    — udisks2 auto-mount point (systemd systems)
//!   * /opt              — optional software packages
//!   * /usr/src          — kernel sources / build trees
//!   * $HOME             — user's home directory
//!   * $HOME/Downloads   — downloads folder (deliberate exception to the
//!                         "no hardcoded XDG dirs" rule — see README)
//!
//! $HOME is resolved at runtime via the HOME env var, falling back to
//! /home/userdir if HOME isn't set (matching the user-spec wording).
//!
//! ## Testing note
//!
//! `home_dir()` reads the `HOME` env var. Tests don't mutate `HOME`
//! (that's a flakiness risk under parallel `cargo test` and an `unsafe`
//! operation on Rust 1.82+); instead they call `defaults_with(home)`
//! directly with an explicit home path.

use std::path::PathBuf;

/// A single default location entry.
#[derive(Debug, Clone)]
pub struct DefaultLocation {
    /// Display label (sidebar text).
    pub label: &'static str,
    /// Resolved filesystem path. May be `None` if $HOME couldn't be
    /// resolved for the home-relative entries.
    pub path: Option<PathBuf>,
    /// Icon to display next to the label.
    pub icon: crate::icons::Icon,
}

/// Resolve $HOME at runtime. Returns None if HOME isn't set and the
/// fallback path doesn't exist either.
///
/// Production callers use this. Tests should call `defaults_with(home)`
/// directly with an explicit home path.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("HOME") {
        return Some(PathBuf::from(h));
    }
    // Fallback per user spec wording.
    let fallback = PathBuf::from("/home/userdir");
    if fallback.is_dir() {
        Some(fallback)
    } else {
        None
    }
}

/// Return the hardcoded list of default locations, using $HOME resolved
/// from the environment. Called fresh on each sidebar render so $HOME
/// resolution stays current (rare to change at runtime, but cheap to
/// recompute).
///
/// Entries whose path doesn't resolve (e.g. /var/run/media on a system
/// that doesn't use udisks2) are still shown — clicking them navigates
/// there and surfaces a "cannot read" error in the status bar, which is
/// more discoverable than silently hiding them.
pub fn defaults() -> Vec<DefaultLocation> {
    defaults_with(home_dir())
}

/// Same as `defaults()` but takes an explicit home directory. Used by
/// tests so they don't have to mutate the `HOME` env var.
pub fn defaults_with(home: Option<PathBuf>) -> Vec<DefaultLocation> {
    let mut out = Vec::with_capacity(6);

    out.push(DefaultLocation {
        label: "mnt",
        path: Some(PathBuf::from("/mnt")),
        icon: crate::icons::Icon::Device,
    });

    out.push(DefaultLocation {
        label: "removable",
        path: Some(PathBuf::from("/var/run/media")),
        icon: crate::icons::Icon::Device,
    });

    out.push(DefaultLocation {
        label: "opt",
        path: Some(PathBuf::from("/opt")),
        icon: crate::icons::Icon::Folder,
    });

    out.push(DefaultLocation {
        label: "src",
        path: Some(PathBuf::from("/usr/src")),
        icon: crate::icons::Icon::Folder,
    });

    if let Some(h) = &home {
        out.push(DefaultLocation {
            label: "Home",
            path: Some(h.clone()),
            icon: crate::icons::Icon::Folder,
        });
        out.push(DefaultLocation {
            label: "Downloads",
            path: Some(h.join("Downloads")),
            icon: crate::icons::Icon::Folder,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_mnt_opt_src() {
        let d = defaults_with(Some(PathBuf::from("/home/testuser")));
        let paths: Vec<String> = d
            .iter()
            .filter_map(|x| x.path.as_ref().map(|p| p.to_string_lossy().into_owned()))
            .collect();
        assert!(paths.iter().any(|p| p == "/mnt"));
        assert!(paths.iter().any(|p| p == "/opt"));
        assert!(paths.iter().any(|p| p == "/usr/src"));
        assert!(paths.iter().any(|p| p == "/var/run/media"));
    }

    #[test]
    fn defaults_include_home_and_downloads_when_home_provided() {
        let d = defaults_with(Some(PathBuf::from("/home/testuser")));
        let has_home = d.iter().any(|x| x.label == "Home" && x.path.as_deref() == Some(std::path::Path::new("/home/testuser")));
        let has_downloads = d.iter().any(|x| x.label == "Downloads" && x.path.as_deref() == Some(std::path::Path::new("/home/testuser/Downloads")));
        assert!(has_home, "Home entry missing or wrong path: {:?}", d.iter().filter(|x| x.label == "Home").collect::<Vec<_>>());
        assert!(has_downloads, "Downloads entry missing or wrong path");
    }

    #[test]
    fn defaults_omit_home_entries_when_home_is_none() {
        // Simulates $HOME unset and /home/userdir not existing.
        let d = defaults_with(None);
        assert!(!d.iter().any(|x| x.label == "Home"));
        assert!(!d.iter().any(|x| x.label == "Downloads"));
        // But the non-home entries are still present.
        assert!(d.iter().any(|x| x.label == "mnt"));
        assert!(d.iter().any(|x| x.label == "opt"));
        assert!(d.iter().any(|x| x.label == "src"));
        assert!(d.iter().any(|x| x.label == "removable"));
    }

    #[test]
    fn defaults_total_count() {
        // 4 fixed + 2 home-relative = 6 when home is provided
        let d = defaults_with(Some(PathBuf::from("/home/testuser")));
        assert_eq!(d.len(), 6);
        // 4 fixed only when home is None
        let d = defaults_with(None);
        assert_eq!(d.len(), 4);
    }
}
