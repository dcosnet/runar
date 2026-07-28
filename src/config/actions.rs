//! `actions.toml` — file extension / MIME-type → shell command overrides.
//!
//! ## Schema
//!
//! Single-command form (backward compatible with v0.1):
//! ```toml
//! [[actions]]
//! pattern = "txt"
//! command = "foot -e vim {}"
//! ```
//!
//! Fallback-chain form (v0.2+) — try each command in order, succeed on
//! the first one whose binary is installed:
//! ```toml
//! [[actions]]
//! pattern = "txt"
//! commands = ["scitano {}", "scite {}", "geany {}", "nano {}"]
//! ```
//!
//! When the launcher resolves an action, it walks `commands` (or the
//! single-element list `[command]`) and runs the first command whose
//! leading binary is found on `$PATH`. If none are installed, the
//! launcher falls through to the built-in MIME table or `xdg-open`.
//!
//! ## Pattern forms
//!
//!   * `image/*`         → MIME prefix glob (matched against `mime.essence_str()`)
//!   * `image/png`       → exact MIME
//!   * `.txt` / `txt`    → file extension (case-insensitive)
//!   * `Makefile`        → exact filename match

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::bookmarks::atomic_write;

/// A single user-defined file-association override.
///
/// Backward-compat note: serializes with `command` if there's only one
/// entry, otherwise with `commands`. Both forms are accepted on load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Action {
    pub pattern: String,
    /// Single-command form. Mutually exclusive with `commands` — if both
    /// are set, `commands` wins. If neither is set, the action is a no-op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Fallback-chain form. Each command is tried in order; the first one
    /// whose leading binary is on `$PATH` runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

impl Action {
    /// Construct a single-command action. Convenience for tests.
    #[cfg(test)]
    pub fn single(pattern: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            command: Some(command.into()),
            commands: Vec::new(),
        }
    }

    /// Construct a fallback-chain action.
    #[cfg(test)]
    pub fn chain(pattern: impl Into<String>, commands: Vec<String>) -> Self {
        Self {
            pattern: pattern.into(),
            command: None,
            commands,
        }
    }

    /// All commands to try, in order. If `commands` is non-empty it wins;
    /// otherwise we fall back to the single `command` (if any).
    pub fn commands_to_try(&self) -> Vec<&str> {
        if !self.commands.is_empty() {
            self.commands.iter().map(|s| s.as_str()).collect()
        } else if let Some(c) = &self.command {
            vec![c.as_str()]
        } else {
            Vec::new()
        }
    }

    pub fn load_all(dir: &Path) -> Vec<Self> {
        let path = dir.join("actions.toml");
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<FileShape>(&s) {
                Ok(shape) => shape.actions,
                Err(e) => {
                    log::warn!("malformed {}: {e}", path.display());
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        }
    }

    pub fn save_all(dir: &Path, actions: &[Self]) -> std::io::Result<()> {
        let shape = FileShape {
            actions: actions.to_vec(),
        };
        let s = toml::to_string_pretty(&shape).unwrap_or_default();
        atomic_write(dir.join("actions.toml"), s.as_bytes())
    }

    /// True if this action's pattern matches `path` (with optional MIME).
    pub fn matches(&self, path: &Path, mime: Option<&mime_guess::Mime>) -> bool {
        let p = &self.pattern;

        // MIME patterns contain a slash.
        if p.contains('/') {
            if let Some(m) = mime {
                let essence = m.essence_str();
                if p == essence {
                    return true;
                }
                if let Some(prefix) = p.strip_suffix("/*") {
                    return essence == prefix
                        || essence.starts_with(&format!("{prefix}/"));
                }
            }
            return false;
        }

        // Extension patterns.
        let ext_form = p.strip_prefix('.').unwrap_or(p);
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case(ext_form) {
                return true;
            }
        }

        // Filename match (catches extensionless files like "Makefile").
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name == p {
                return true;
            }
        }

        false
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileShape {
    #[serde(default)]
    actions: Vec<Action>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_match() {
        let a = Action::single("txt", "vim {}");
        assert!(a.matches(Path::new("/tmp/notes.txt"), None));
        assert!(!a.matches(Path::new("/tmp/notes.md"), None));
    }

    #[test]
    fn dot_prefix_extension() {
        let a = Action::single(".pdf", "zathura {}");
        assert!(a.matches(Path::new("/tmp/book.pdf"), None));
    }

    #[test]
    fn mime_glob() {
        let a = Action::single("image/*", "feh {}");
        let png_mime: mime_guess::Mime = "image/png".parse().unwrap();
        let jpg_mime: mime_guess::Mime = "image/jpeg".parse().unwrap();
        let txt_mime: mime_guess::Mime = "text/plain".parse().unwrap();
        assert!(a.matches(Path::new("x.png"), Some(&png_mime)));
        assert!(a.matches(Path::new("x.jpg"), Some(&jpg_mime)));
        assert!(!a.matches(Path::new("x.txt"), Some(&txt_mime)));
    }

    #[test]
    fn exact_filename() {
        let a = Action::single("Makefile", "make -C {}");
        assert!(a.matches(Path::new("/proj/Makefile"), None));
        assert!(!a.matches(Path::new("/proj/main.c"), None));
    }

    #[test]
    fn commands_to_try_with_single_command() {
        let a = Action::single("txt", "vim {}");
        assert_eq!(a.commands_to_try(), vec!["vim {}"]);
    }

    #[test]
    fn commands_to_try_with_chain() {
        let a = Action::chain(
            "txt",
            vec![
                "scitano {}".into(),
                "scite {}".into(),
                "geany {}".into(),
                "nano {}".into(),
            ],
        );
        assert_eq!(
            a.commands_to_try(),
            vec!["scitano {}", "scite {}", "geany {}", "nano {}"]
        );
    }

    #[test]
    fn commands_to_try_chain_wins_over_command() {
        // If both `command` and `commands` are set, `commands` wins.
        // (This shouldn't happen with normal TOML parsing — both fields
        // are optional — but we defend against it anyway.)
        let a = Action {
            pattern: "txt".into(),
            command: Some("vim {}".into()),
            commands: vec!["scite {}".into(), "geany {}".into()],
        };
        assert_eq!(a.commands_to_try(), vec!["scite {}", "geany {}"]);
    }

    #[test]
    fn commands_to_try_empty() {
        let a = Action {
            pattern: "txt".into(),
            command: None,
            commands: Vec::new(),
        };
        assert!(a.commands_to_try().is_empty());
    }

    #[test]
    fn roundtrip_single_command() {
        let tmp = std::env::temp_dir().join(format!(
            "runar-fm-act-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = vec![
            Action::single("txt", "vim {}"),
            Action::single("image/*", "feh {}"),
        ];
        Action::save_all(&tmp, &actions).unwrap();
        let loaded = Action::load_all(&tmp);
        assert_eq!(loaded, actions);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn roundtrip_chain() {
        let tmp = std::env::temp_dir().join(format!(
            "runar-fm-act-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = vec![Action::chain(
            "txt",
            vec![
                "scitano {}".into(),
                "scite {}".into(),
                "geany {}".into(),
                "nano {}".into(),
            ],
        )];
        Action::save_all(&tmp, &actions).unwrap();
        let loaded = Action::load_all(&tmp);
        assert_eq!(loaded, actions);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn toml_single_command_form_loads() {
        // Old-format TOML (just `command = ...`) should still load.
        let toml = r#"
[[actions]]
pattern = "txt"
command = "vim {}"
"#;
        let shape: FileShape = toml::from_str(toml).unwrap();
        assert_eq!(shape.actions.len(), 1);
        assert_eq!(shape.actions[0].command.as_deref(), Some("vim {}"));
        assert!(shape.actions[0].commands.is_empty());
        assert_eq!(shape.actions[0].commands_to_try(), vec!["vim {}"]);
    }

    #[test]
    fn toml_chain_form_loads() {
        let toml = r#"
[[actions]]
pattern = "txt"
commands = ["scitano {}", "scite {}", "geany {}", "nano {}"]
"#;
        let shape: FileShape = toml::from_str(toml).unwrap();
        assert_eq!(shape.actions.len(), 1);
        assert!(shape.actions[0].command.is_none());
        assert_eq!(
            shape.actions[0].commands_to_try(),
            vec!["scitano {}", "scite {}", "geany {}", "nano {}"]
        );
    }
}
