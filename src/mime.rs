//! MIME type detection and built-in default action table.
//!
//! `mime_guess` resolves file extensions to MIME types. We layer a
//! table-driven action mapping on top so common file types open in
//! sensible applications out-of-the-box, without requiring the user to
//! write an `actions.toml`.
//!
//! ## Resolution order
//!
//! When the user activates a file, the launch dispatcher consults these
//! sources in order:
//!   1. `~/.config/runar/actions.toml` — user overrides (highest priority)
//!   2. Built-in default table in this module
//!   3. `open::that` (xdg-open on Linux) — final fallback
//!
//! ## Editor fallback chain
//!
//! Text-file defaults use a shell-level fallback chain rather than a
//! single binary, so they work on any system with at least one
//! reasonable editor installed. The chain, in order:
//!
//!   1. `$EDITOR` (if set — always wins, matches user's shell convention)
//!   2. `scitano`
//!   3. `scite`
//!   4. `geany`
//!   5. `nano` (near-universal on Linux, terminal-based)
//!
//! `$EDITOR` is checked first via `if [ -n "$EDITOR" ]`; the remaining
//! binaries are tried via `command -v` and the first match is exec'd.
//! `nano` is the final fallback because it is near-universal and
//! terminal-based.

use std::path::Path;

/// A built-in default action for a category of files.
#[derive(Debug, Clone)]
pub struct DefaultAction {
    /// Human-readable label for the action (e.g. "open in editor").
    /// Reserved for future use in a right-click context menu.
    #[allow(dead_code)]
    pub label: &'static str,
    /// Shell command template. `{}` is substituted with the shell-quoted
    /// file path.
    pub command: &'static str,
}

/// Shell snippet that walks the editor fallback chain. Used by the
/// text/*, application/json, application/x-shellscript, etc. defaults.
///
/// Order: $EDITOR → scitano → scite → geany → nano. `$EDITOR` wins
/// unconditionally when set (matches shell convention). Otherwise each
/// binary is tried via `command -v` and the first match is exec'd.
const EDITOR_CHAIN: &str = "\
if [ -n \"$EDITOR\" ]; then exec $EDITOR {}; fi; \
for e in scitano scite geany nano; do \
  if command -v \"$e\" >/dev/null 2>&1; then exec \"$e\" {}; fi; \
done; \
echo 'no editor found (tried: $EDITOR, scitano, scite, geany, nano)' >&2; \
false";

/// How a MIME essence string matches a rule.
#[derive(Debug, Clone, Copy)]
enum Matcher {
    /// Exact equality, e.g. `text/html`.
    Exact(&'static str),
    /// Prefix match, e.g. `text/` matches `text/plain`, `text/html`, etc.
    Prefix(&'static str),
}

/// A single row in the built-in MIME action table.
struct MimeRule {
    matcher: Matcher,
    action: DefaultAction,
}

/// The built-in MIME → action table. Evaluated in declaration order; the
/// first matching rule wins. Exact matches must precede prefix matches
/// that would otherwise shadow them (e.g. `text/html` before `text/`).
///
/// Table-driven dispatch replaces a cascade of if/else branches. Adding
/// a new MIME category is a one-line change: append a row here.
const MIME_RULES: &[MimeRule] = &[
    // HTML → browser. Must precede the text/ prefix rule.
    MimeRule {
        matcher: Matcher::Exact("text/html"),
        action: DefaultAction {
            label: "open in browser",
            command: "${BROWSER:-xdg-open} {}",
        },
    },
    // Text files → editor chain. Catches text/plain, text/markdown,
    // text/csv, text/x-rust, etc.
    MimeRule {
        matcher: Matcher::Prefix("text/"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Prefix("image/"),
        action: DefaultAction {
            label: "open image",
            command: "${IMAGE_VIEWER:-xdg-open} {}",
        },
    },
    MimeRule {
        matcher: Matcher::Prefix("video/"),
        action: DefaultAction {
            label: "open video",
            command: "${VIDEO_PLAYER:-xdg-open} {}",
        },
    },
    MimeRule {
        matcher: Matcher::Prefix("audio/"),
        action: DefaultAction {
            label: "open audio",
            command: "${AUDIO_PLAYER:-xdg-open} {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/pdf"),
        action: DefaultAction {
            label: "open PDF",
            command: "${PDF_VIEWER:-xdg-open} {}",
        },
    },
    // Archives — list contents rather than extract (safer default).
    MimeRule {
        matcher: Matcher::Exact("application/zip"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-tar"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/gzip"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-bzip2"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-xz"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-7z-compressed"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-rar"),
        action: DefaultAction {
            label: "list archive contents",
            command: "ls -l {}",
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-shellscript"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/json"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/yaml"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/x-yaml"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/toml"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/xml"),
        action: DefaultAction {
            label: "open in editor",
            command: EDITOR_CHAIN,
        },
    },
];

/// Classify a file by its MIME type and return the built-in default
/// action, if any. Returns `None` for MIME types with no built-in rule;
/// the caller falls back to `xdg-open`.
///
/// `mime_type` is the result of `mime_guess::from_path(path).first()`
/// (already computed by the VFS scanner and stored in
/// `FileEntry.mime_type`). The charset suffix (`; charset=utf-8`) is
/// stripped before matching.
pub fn default_action_for(mime_type: Option<&str>) -> Option<DefaultAction> {
    let mime = mime_type?;
    let essence = mime.split(';').next().unwrap_or(mime);
    MIME_RULES
        .iter()
        .find(|rule| match rule.matcher {
            Matcher::Exact(s) => essence == s,
            Matcher::Prefix(s) => essence.starts_with(s),
        })
        .map(|rule| rule.action.clone())
}

/// Convenience: classify a path directly (resolves MIME internally).
pub fn default_action_for_path(path: &Path) -> Option<DefaultAction> {
    let essence: Option<String> = mime_guess::from_path(path)
        .first()
        .map(|m| m.essence_str().to_string());
    default_action_for(essence.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_use_editor_chain() {
        let action = default_action_for(Some("text/plain")).unwrap();
        assert_eq!(action.label, "open in editor");
        assert!(action.command.contains("$EDITOR"));
        assert!(action.command.contains("scitano"));
        assert!(action.command.contains("scite"));
        assert!(action.command.contains("geany"));
        assert!(action.command.contains("nano"));
    }

    #[test]
    fn text_subtypes_get_editor() {
        assert!(default_action_for(Some("text/markdown")).is_some());
        assert!(default_action_for(Some("text/csv")).is_some());
        assert!(default_action_for(Some("text/x-rust")).is_some());
    }

    #[test]
    fn images_get_viewer() {
        let action = default_action_for(Some("image/png")).unwrap();
        assert!(action.command.contains("${IMAGE_VIEWER:-xdg-open}"));
        assert!(default_action_for(Some("image/jpeg")).is_some());
        assert!(default_action_for(Some("image/svg+xml")).is_some());
        assert!(default_action_for(Some("image/gif")).is_some());
    }

    #[test]
    fn video_gets_player() {
        let action = default_action_for(Some("video/mp4")).unwrap();
        assert!(action.command.contains("${VIDEO_PLAYER:-xdg-open}"));
    }

    #[test]
    fn audio_gets_player() {
        let action = default_action_for(Some("audio/mpeg")).unwrap();
        assert!(action.command.contains("${AUDIO_PLAYER:-xdg-open}"));
    }

    #[test]
    fn pdf_gets_viewer() {
        let action = default_action_for(Some("application/pdf")).unwrap();
        assert_eq!(action.label, "open PDF");
        assert!(action.command.contains("${PDF_VIEWER:-xdg-open}"));
    }

    #[test]
    fn html_gets_browser() {
        let action = default_action_for(Some("text/html")).unwrap();
        assert!(action.command.contains("${BROWSER:-xdg-open}"));
    }

    #[test]
    fn html_takes_precedence_over_text_prefix() {
        // text/html must match the browser rule, not the editor rule.
        // This verifies that exact matches precede prefix matches in the
        // MIME_RULES table.
        let action = default_action_for(Some("text/html")).unwrap();
        assert_eq!(action.label, "open in browser");
        assert!(!action.command.contains("scitano"));
    }

    #[test]
    fn archives_get_ls() {
        for mime in [
            "application/zip",
            "application/x-tar",
            "application/gzip",
            "application/x-bzip2",
            "application/x-xz",
            "application/x-7z-compressed",
            "application/x-rar",
        ] {
            let action = default_action_for(Some(mime)).unwrap_or_else(|| {
                panic!("no action for {mime}")
            });
            assert!(action.command.contains("ls -l"), "archive {mime} should use ls -l");
        }
    }

    #[test]
    fn json_yaml_toml_get_editor() {
        let json = default_action_for(Some("application/json")).unwrap();
        assert!(json.command.contains("scitano"));
        assert!(default_action_for(Some("application/yaml")).is_some());
        assert!(default_action_for(Some("application/x-yaml")).is_some());
        assert!(default_action_for(Some("application/toml")).is_some());
        assert!(default_action_for(Some("application/xml")).is_some());
    }

    #[test]
    fn shellscript_gets_editor() {
        let action = default_action_for(Some("application/x-shellscript")).unwrap();
        assert!(action.command.contains("scitano"));
    }

    #[test]
    fn editor_chain_order_is_correct() {
        let action = default_action_for(Some("text/plain")).unwrap();
        let cmd = action.command;
        let i_scitano = cmd.find("scitano").unwrap();
        let i_scite = cmd.find("scite").unwrap();
        let i_geany = cmd.find("geany").unwrap();
        let i_nano = cmd.find("nano").unwrap();
        assert!(i_scitano < i_scite, "scitano must precede scite");
        assert!(i_scite < i_geany, "scite must precede geany");
        assert!(i_geany < i_nano, "geany must precede nano");
    }

    #[test]
    fn editor_chain_respects_editor_env_var() {
        let action = default_action_for(Some("text/plain")).unwrap();
        assert!(
            action.command.starts_with("if [ -n \"$EDITOR\" ]"),
            "editor chain must check $EDITOR first, got: {}",
            &action.command[..50]
        );
    }

    #[test]
    fn unknown_mime_returns_none() {
        assert!(default_action_for(Some("application/octet-stream")).is_none());
        assert!(default_action_for(Some("application/x-some-weird-thing")).is_none());
    }

    #[test]
    fn none_mime_returns_none() {
        assert!(default_action_for(None).is_none());
    }

    #[test]
    fn charset_suffix_stripped() {
        let action = default_action_for(Some("text/plain; charset=utf-8")).unwrap();
        assert_eq!(action.label, "open in editor");
    }

    #[test]
    fn default_action_for_path_uses_extension() {
        let action = default_action_for_path(Path::new("/tmp/notes.txt")).unwrap();
        assert!(action.command.contains("scitano"));

        let action = default_action_for_path(Path::new("/tmp/photo.png")).unwrap();
        assert!(action.command.contains("${IMAGE_VIEWER:-xdg-open}"));

        let action = default_action_for_path(Path::new("/tmp/README.md")).unwrap();
        assert!(action.command.contains("scitano"));
    }
}
