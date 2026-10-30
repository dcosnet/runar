//! MIME type detection and built-in default action table.
//!
//! Extension → MIME resolution is fully self-contained, in two layers:
//!
//!   1. `/etc/mime.types` — if the sysadmin maintains one, runar parses
//!      it (a simple whitespace-delimited `mime/type ext1 ext2 …` flat
//!      file) and honors it. Parsed at most once per process and cached.
//!   2. mime_guess's compiled-in table (derived from Apache's
//!      `mime.types`) — the always-present fallback that keeps runar
//!      working on a minimal box with no /etc/mime.types at all.
//!
//! There is deliberately no xdg anything: no `xdg-open`, no
//! `shared-mime-info` globs, no `.desktop` files, no MIME database
//! daemon. We layer a table-driven action mapping on top so common file
//! types open in sensible applications out-of-the-box, without requiring
//! the user to write an `actions.toml`.
//!
//! ## Resolution order
//!
//! When the user activates a file, the launch dispatcher consults these
//! sources in order:
//!   1. `~/.config/runar/actions.toml` — user overrides (highest priority)
//!   2. Built-in default table in this module
//!   3. Direct execution — if the file itself carries an executable bit,
//!      run it (the ROX-Filer convention). Otherwise: no handler.
//!
//! ## App fallback chains
//!
//! Every table entry that needs an external application uses a
//! shell-level fallback chain rather than a single binary, so they work
//! on any system with at least one reasonable app installed. Each chain:
//!
//!   1. the category's environment variable (`$BROWSER`, `$IMAGE_VIEWER`,
//!      ...) if set — always wins, matches user's shell convention
//!   2. the owner's preferred binaries first (including custom apps like
//!      `marten` and `ferret`), then lightweight fallbacks, tried via
//!      `command -v`
//!
//! A missing binary costs nothing: the chain checks each name with
//! `command -v` and simply walks past apps that aren't installed, so a
//! personal default on one machine stays harmless on another.

use mime_guess::MimeGuess;
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

/// Shell snippet that walks a fallback chain: the category's env var
/// wins when set, then each binary in the list is tried via `command -v`
/// and the first match is exec'd. `$X` is deliberately unquoted on the
/// `exec` line so a multi-word value like `EDITOR="vi -R"` still works;
/// `{}` arrives already shell-quoted. `$arg` is the argument suffix —
/// `" {}"` for file-opening apps, `""` for apps launched bare (terminal
/// emulators get their working directory via `current_dir`, not argv).
macro_rules! app_chain {
    ($env:literal, $kind:literal, $binaries:expr, $arg:literal) => {
        concat!(
            "if [ -n \"$", $env, "\" ]; then exec $", $env, $arg, "; fi; ",
            "for e in ", $binaries, "; do ",
            "  if command -v \"$e\" >/dev/null 2>&1; then exec \"$e\"", $arg, "; fi; ",
            "done; ",
            "echo 'no ", $kind, " found (tried: $", $env, ", ", $binaries, ")' >&2; ",
            "false"
        )
    };
}

/// Text editor: $EDITOR → scitano (custom) → scite → geany → kate →
/// mousepad.
const EDITOR_CHAIN: &str =
    app_chain!("EDITOR", "editor", "scitano scite geany kate mousepad", " {}");

/// Web browser: $BROWSER → firefox → librewolf → zen → mullvad-browser →
/// swell → ladybird → gnomeweb/epiphany → netsurf. GNOME Web ships as
/// `epiphany` upstream; `gnomeweb` covers a personal wrapper by that
/// name. Both names are checked — an absent one just falls through.
/// No chromium; no obsolete legacy lightweights (tazweb, dillo, midori).
const BROWSER_CHAIN: &str = app_chain!(
    "BROWSER",
    "browser",
    "firefox librewolf zen mullvad-browser swell ladybird gnomeweb epiphany netsurf",
    " {}"
);

/// Image viewer: $IMAGE_VIEWER → marten (custom) → gpicview → gphoto2.
const IMAGE_CHAIN: &str = app_chain!(
    "IMAGE_VIEWER",
    "image viewer",
    "marten gpicview gphoto2",
    " {}"
);

/// Video player: $VIDEO_PLAYER → ferret (custom) → smplayer → mpv → vlc.
const VIDEO_CHAIN: &str =
    app_chain!("VIDEO_PLAYER", "video player", "ferret smplayer mpv vlc", " {}");

/// Audio player: $AUDIO_PLAYER → mpv → mplayer → audacious → deadbeef.
const AUDIO_CHAIN: &str =
    app_chain!("AUDIO_PLAYER", "audio player", "mpv mplayer audacious deadbeef", " {}");

/// PDF viewer: $PDF_VIEWER → evince → atril.
const PDF_CHAIN: &str =
    app_chain!("PDF_VIEWER", "pdf viewer", "evince atril", " {}");

/// Terminal emulator: $TERMINAL → rs-mrxvt (custom) → mrxvt → urxvt →
/// ghostty → alacritty → tabby → guake. `rxvt-unicode`'s binary is
/// `urxvt` on every distro that ships it. Launched bare — the child's
/// working directory is set by the launcher, so the shell inside starts
/// in the right folder without per-terminal flag guessing.
pub(crate) const TERMINAL_CHAIN: &str = app_chain!(
    "TERMINAL",
    "terminal emulator",
    "rs-mrxvt mrxvt urxvt ghostty alacritty tabby guake",
    ""
);

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
///
/// Design rule: every command in this table is fully self-contained —
/// env-var override + `command -v` chain over real binaries. Nothing
/// here defers to xdg-open, a MIME database, or .desktop files, and a
/// unit test guards that property.
const MIME_RULES: &[MimeRule] = &[
    // HTML → browser. Must precede the text/ prefix rule.
    MimeRule {
        matcher: Matcher::Exact("text/html"),
        action: DefaultAction {
            label: "open in browser",
            command: BROWSER_CHAIN,
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
            command: IMAGE_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Prefix("video/"),
        action: DefaultAction {
            label: "open video",
            command: VIDEO_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Prefix("audio/"),
        action: DefaultAction {
            label: "open audio",
            command: AUDIO_CHAIN,
        },
    },
    MimeRule {
        matcher: Matcher::Exact("application/pdf"),
        action: DefaultAction {
            label: "open PDF",
            command: PDF_CHAIN,
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
/// the caller's final fallback is direct execution (if the file is
/// executable) or an honest "no handler".
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
    default_action_for(resolve_mime_type(path).as_deref())
}

// ---------- Extension → MIME resolution -------------------------------------

use std::collections::HashMap;
use std::sync::LazyLock;

/// Inverted view of a mime.types file: extension (lowercased) → MIME type.
/// When one extension is listed under several types, the first listing wins.
fn parse_mime_types(content: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        // Skip blank lines and `#` comments (Apache/Debian mime.types format).
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(mime) = tokens.next() else {
            continue;
        };
        for ext in tokens {
            out.entry(ext.to_ascii_lowercase())
                .or_insert_with(|| mime.to_string());
        }
    }
    out
}

/// `/etc/mime.types`, read and parsed at most once per process. `None`
/// when the file doesn't exist (a minimal install) or can't be read.
fn etc_mime_types() -> Option<&'static HashMap<String, String>> {
    static TABLE: LazyLock<Option<HashMap<String, String>>> = LazyLock::new(|| {
        match std::fs::read_to_string("/etc/mime.types") {
            Ok(s) => Some(parse_mime_types(&s)),
            Err(_) => None,
        }
    });
    TABLE.as_ref()
}

/// Resolve the MIME type for `path` by file extension: `/etc/mime.types`
/// first (honoring the sysadmin's table), then mime_guess's compiled-in
/// table. `None` when neither source knows the extension — the caller's
/// final fallback is direct execution or "no handler", never xdg.
pub fn resolve_mime_type(path: &Path) -> Option<String> {
    resolve_mime_type_with(etc_mime_types(), path)
}

/// Same as `resolve_mime_type` with an explicit /etc table, for tests.
fn resolve_mime_type_with(
    table: Option<&HashMap<String, String>>,
    path: &Path,
) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if let Some(mime) = table.and_then(|t| t.get(&ext)) {
        return Some(mime.clone());
    }
    MimeGuess::from_ext(&ext)
        .first()
        .map(|m| m.essence_str().to_string())
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
        assert!(action.command.contains("kate"));
        assert!(action.command.contains("mousepad"));
    }

    #[test]
    fn text_subtypes_get_editor() {
        assert!(default_action_for(Some("text/markdown")).is_some());
        assert!(default_action_for(Some("text/csv")).is_some());
        assert!(default_action_for(Some("text/x-rust")).is_some());
    }

    #[test]
    fn images_get_viewer_chain() {
        let action = default_action_for(Some("image/png")).unwrap();
        assert!(action.command.contains("$IMAGE_VIEWER"));
        assert!(action.command.contains("marten"));
        assert!(default_action_for(Some("image/jpeg")).is_some());
        assert!(default_action_for(Some("image/svg+xml")).is_some());
        assert!(default_action_for(Some("image/gif")).is_some());
    }

    #[test]
    fn video_gets_player_chain() {
        let action = default_action_for(Some("video/mp4")).unwrap();
        assert!(action.command.contains("$VIDEO_PLAYER"));
        assert!(action.command.contains("ferret"));
    }

    #[test]
    fn audio_gets_player_chain() {
        let action = default_action_for(Some("audio/mpeg")).unwrap();
        assert!(action.command.contains("$AUDIO_PLAYER"));
        assert!(action.command.contains("mpv"));
    }

    #[test]
    fn pdf_gets_viewer_chain() {
        let action = default_action_for(Some("application/pdf")).unwrap();
        assert_eq!(action.label, "open PDF");
        assert!(action.command.contains("$PDF_VIEWER"));
        assert!(action.command.contains("evince"));
        assert!(action.command.contains("atril"));
    }

    #[test]
    fn html_gets_browser_chain() {
        let action = default_action_for(Some("text/html")).unwrap();
        assert!(action.command.contains("$BROWSER"));
        // Firefox is the default browser; chromium and the removed
        // legacy lightweights must not appear at all.
        assert!(action.command.contains("firefox"));
        assert!(!action.command.contains("chromium"));
        for removed in ["tazweb", "dillo", "netsurf-gtk", "midori"] {
            assert!(
                !action.command.contains(removed),
                "obsolete browser '{removed}' should not be in the chain"
            );
        }
    }

    /// Every app chain follows the same shape: the category's env var is
    /// checked first, then `command -v` walks the binary list — owner's
    /// preferred apps first, lightweight fallbacks after.
    #[test]
    fn app_chains_check_env_var_first_then_binaries_in_order() {
        let cases = [
            (
                "text/plain",
                "$EDITOR",
                &["scitano", "scite", "geany", "kate", "mousepad"][..],
            ),
            (
                "text/html",
                "$BROWSER",
                &[
                    "firefox",
                    "librewolf",
                    "zen",
                    "mullvad-browser",
                    "swell",
                    "ladybird",
                    "gnomeweb",
                    "epiphany",
                    "netsurf",
                ][..],
            ),
            (
                "image/png",
                "$IMAGE_VIEWER",
                &["marten", "gpicview", "gphoto2"][..],
            ),
            (
                "video/mp4",
                "$VIDEO_PLAYER",
                &["ferret", "smplayer", "mpv", "vlc"][..],
            ),
            (
                "audio/mpeg",
                "$AUDIO_PLAYER",
                &["mpv", "mplayer", "audacious", "deadbeef"][..],
            ),
            (
                "application/pdf",
                "$PDF_VIEWER",
                &["evince", "atril"][..],
            ),
        ];
        for (mime, env_var, binaries) in cases {
            let cmd = default_action_for(Some(mime))
                .unwrap_or_else(|| panic!("no action for {mime}"))
                .command;
            let env_pos = cmd.find(&format!("if [ -n \"{0}\" ]; then exec {0}", env_var))
                .unwrap_or_else(|| panic!("{mime} chain must check {env_var} first: {cmd}"));
            let mut prev = env_pos;
            for bin in binaries {
                let pos = cmd.find(bin)
                    .unwrap_or_else(|| panic!("{mime} chain missing {bin}: {cmd}"));
                assert!(pos > prev, "{mime} chain: {bin} must follow its predecessor");
                prev = pos;
            }
        }
    }

    #[test]
    fn terminal_chain_env_var_then_binaries_in_order() {
        let cmd = TERMINAL_CHAIN;
        assert!(
            cmd.contains("if [ -n \"$TERMINAL\" ]; then exec $TERMINAL; fi"),
            "terminal chain must check $TERMINAL first, and launch it bare: {cmd}"
        );
        // No `{}` — terminals launch with a working directory, not argv.
        assert!(!cmd.contains("{}"), "terminal chain must not pass a path argument: {cmd}");
        let mut prev = 0;
        for bin in [
            "rs-mrxvt", "mrxvt", "urxvt", "ghostty", "alacritty", "tabby", "guake",
        ] {
            let pos = cmd
                .find(bin)
                .unwrap_or_else(|| panic!("terminal chain missing {bin}: {cmd}"));
            assert!(pos > prev, "terminal chain: {bin} must follow its predecessor");
            prev = pos;
        }
    }

    /// Design-principle guard: runar is self-contained, so no built-in
    /// command may defer to xdg — no `xdg-open`, no `xdg-mime`, nothing.
    /// If this test fails, a system MIME database has crept back in.
    #[test]
    fn builtin_table_never_defers_to_xdg() {
        for rule in MIME_RULES {
            assert!(
                !rule.action.command.to_lowercase().contains("xdg"),
                "rule '{}' defers to xdg: {}",
                rule.action.label,
                rule.action.command
            );
        }
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
        let i_kate = cmd.find("kate").unwrap();
        let i_mousepad = cmd.find("mousepad").unwrap();
        assert!(i_scitano < i_scite, "scitano must precede scite");
        assert!(i_scite < i_geany, "scite must precede geany");
        assert!(i_geany < i_kate, "geany must precede kate");
        assert!(i_kate < i_mousepad, "kate must precede mousepad");
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
        assert!(action.command.contains("$IMAGE_VIEWER"));

        let action = default_action_for_path(Path::new("/tmp/README.md")).unwrap();
        assert!(action.command.contains("scitano"));
    }

    #[test]
    fn parse_mime_types_builds_extension_map() {
        let sample = "\
# A comment line
text/html                     html htm
application/x-bzip2 bz2 bz2-tmp
# Another comment
image/png  PNG
";
        let table = parse_mime_types(sample);
        assert_eq!(table.get("html").map(String::as_str), Some("text/html"));
        assert_eq!(table.get("htm").map(String::as_str), Some("text/html"));
        assert_eq!(table.get("bz2").map(String::as_str), Some("application/x-bzip2"));
        assert_eq!(table.get("bz2-tmp").map(String::as_str), Some("application/x-bzip2"));
        // Extension lookups are case-insensitive.
        assert_eq!(table.get("png").map(String::as_str), Some("image/png"));
        assert_eq!(table.get("jpeg"), None);
    }

    #[test]
    fn parse_mime_types_first_listing_wins_for_duplicate_ext() {
        let sample = "\
text/old        xyz
text/new        xyz
";
        let table = parse_mime_types(sample);
        assert_eq!(table.get("xyz").map(String::as_str), Some("text/old"));
    }

    #[test]
    fn resolve_mime_type_with_empty_table_falls_back_to_embedded() {
        // No /etc table at all (a minimal install): the compiled-in
        // table must carry the day.
        assert_eq!(
            resolve_mime_type_with(None, Path::new("/tmp/notes.txt")),
            Some("text/plain".to_string())
        );
        assert_eq!(resolve_mime_type_with(None, Path::new("/tmp/photo.PNG")), Some("image/png".to_string()));
        // Unknown extension on both layers → None ("no handler" territory).
        assert_eq!(resolve_mime_type_with(None, Path::new("/tmp/blob.xyzzy")), None);
        // No extension at all → None.
        assert_eq!(resolve_mime_type_with(None, Path::new("/tmp/Makefile")), None);
    }

    #[test]
    fn sysadmin_etc_table_overrides_embedded() {
        let mut table = HashMap::new();
        table.insert("txt".to_string(), "text/x-runar-custom".to_string());
        assert_eq!(
            resolve_mime_type_with(Some(&table), Path::new("/tmp/notes.txt")),
            Some("text/x-runar-custom".to_string())
        );
        // Extensions the sysadmin table doesn't cover still fall back.
        assert_eq!(
            resolve_mime_type_with(Some(&table), Path::new("/tmp/notes.md")),
            Some("text/markdown".to_string())
        );
    }

    #[test]
    fn resolve_mime_type_end_to_end() {
        // Environment-independent smoke test: whatever this box has in
        // /etc/mime.types, well-known extensions must resolve.
        assert_eq!(
            resolve_mime_type(Path::new("/tmp/notes.txt")),
            Some("text/plain".to_string())
        );
    }
}
