//! Launch dispatcher — Phase 3 of the project manifest.
//!
//! Decision tree (applied on double-click or Enter on an entry):
//!
//!   1. AppDir  → spawn the inner launcher directly (AppRun or same-name
//!                executable). No shell, no `$PATH` lookup — the AppDir's
//!                own executable is trusted.
//!
//!   2. File    → consult, in order:
//!                  a. `actions.toml` (user overrides — highest priority).
//!                     Supports a fallback chain: `commands = [...]`. The
//!                     first command whose binary is on `$PATH` wins.
//!                  b. Built-in default MIME table (`src/mime.rs`). The
//!                     default command strings use shell-level fallback
//!                     chains (e.g. `${EDITOR:-vi}`, the scitano→scite→
//!                     geany→nano loop) so they work on any system.
//!                  c. `open::that` (xdg-open on Linux) — final fallback.
//!
//!   3. Directory / Symlink → handled by the UI layer (navigation). This
//!                module is invoked only for "open" actions.
//!
//! All launches are detached: the child process is spawned and not
//! waited on. Errors are returned synchronously only if `spawn()` itself
//! fails (e.g. missing binary); child-side failures surface to the user
//! in the child's terminal.

use crate::config::Config;
use crate::vfs::EntryKind;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Outcome of a launch attempt, for UI feedback.
#[derive(Debug)]
pub enum LaunchResult {
    /// Successfully spawned. Carries a human-readable description for the
    /// status bar (e.g. "launched vim /tmp/notes.txt").
    Spawned(String),
    /// Nothing matched and the dispatcher declined to spawn (e.g. no
    /// action and `open::that` not applicable). UI shows "no handler".
    NoHandler,
    /// Spawn failed. The user should see this.
    Failed(String),
}

/// Launch `path` according to its kind, consulting the user's action
/// overrides before falling back to system defaults.
pub async fn launch(path: &Path, kind: &EntryKind, config: &Config) -> LaunchResult {
    match kind {
        EntryKind::AppDir { launcher } => spawn_direct(launcher, &[]).await,
        EntryKind::File => launch_file(path, config).await,
        // Symlinks are resolved by the UI before calling launch().
        // Directory and Symlink are no-ops here.
        EntryKind::Symlink { .. } | EntryKind::Directory => LaunchResult::NoHandler,
    }
}

/// File-launch step-down: try user overrides, then built-in MIME table,
/// then `xdg-open`. Each step either returns a `LaunchResult` or falls
/// through to the next.
async fn launch_file(path: &Path, config: &Config) -> LaunchResult {
    // 1. User overrides from actions.toml (highest priority).
    if let Some(result) = try_user_action(path, config).await {
        return result;
    }
    // 2. Built-in default MIME table.
    if let Some(result) = try_builtin_default(path).await {
        return result;
    }
    // 3. xdg-open fallback.
    try_xdg_open(path)
}

/// Step 1: user-defined `actions.toml` overrides. Returns `None` if no
/// action matches OR if the action's command chain has no binary on
/// `$PATH` (in which case we fall through to the built-in table).
async fn try_user_action(path: &Path, config: &Config) -> Option<LaunchResult> {
    let action = config.find_action_for(path)?;
    let commands: Vec<String> = action
        .commands_to_try()
        .into_iter()
        .map(|c| c.replace("{}", &shell_quote(path)))
        .collect();
    try_command_chain(&commands).await
}

/// Step 2: built-in default MIME table.
async fn try_builtin_default(path: &Path) -> Option<LaunchResult> {
    let default = crate::mime::default_action_for_path(path)?;
    let cmd = default.command.replace("{}", &shell_quote(path));
    Some(run_shell(&cmd).await)
}

/// Step 3: `xdg-open` final fallback.
fn try_xdg_open(path: &Path) -> LaunchResult {
    match open::that(path) {
        Ok(()) => LaunchResult::Spawned(format!("opened {}", path.display())),
        Err(e) => LaunchResult::Failed(format!("open: {e}")),
    }
}

/// Walk the command chain. Returns `Some(LaunchResult)` for the first
/// command whose leading binary is on `$PATH` (or whose leading binary
/// cannot be statically resolved — those are tried blindly). Returns
/// `None` if every command's leading binary is missing from `$PATH`.
///
/// "Leading binary" extraction: parse the command's first word (after
/// any env-var assignments) and check `which`-style whether it is on
/// `$PATH`. This is a heuristic — full shell semantics would require
/// invoking `sh -c` — but the cost of a failed spawn justifies the
/// pre-check.
async fn try_command_chain(commands: &[String]) -> Option<LaunchResult> {
    // Iterate manually because `run_shell` is async and closures passed
    // to `find_map` cannot be async. Each iteration either returns a
    // LaunchResult (binary found or unresolvable — try it) or continues
    // to the next command (binary missing from `$PATH`).
    for cmd in commands {
        match extract_leading_binary(cmd) {
            Some(bin) if !binary_on_path(&bin).await => {
                log::debug!("launch chain: {bin} not on PATH, trying next");
                continue;
            }
            _ => return Some(run_shell(cmd).await),
        }
    }
    None
}

/// Extract the leading binary name from a shell command string.
///
/// Handles common cases:
///   * `vim /tmp/foo.txt`           → "vim"
///   * `foot -e vim /tmp/foo.txt`   → "foot"
///   * `${EDITOR:-vi} /tmp/foo.txt` → None (env-var expansion — defer to sh)
///   * `sh -c '...complex...'`      → "sh"
///
/// Returns `None` for commands starting with something that cannot be
/// statically resolved (env vars, command substitution). The caller
/// tries those blindly without a pre-check.
fn extract_leading_binary(cmd: &str) -> Option<String> {
    let mut rest = cmd.trim_start();
    if rest.is_empty() {
        return None;
    }
    loop {
        let first_word = rest.split_whitespace().next()?;
        match parse_leading_token(first_word) {
            LeadingToken::EnvAssignment => {
                rest = &rest[first_word.len()..];
            }
            LeadingToken::EnvVarExpansion => return None,
            LeadingToken::Binary(name) => return Some(name),
        }
    }
}

/// Classify the first whitespace-delimited token of a command.
enum LeadingToken {
    /// `VAR=value` — skip and continue scanning.
    EnvAssignment,
    /// `$VAR` or `${VAR:-default}` — defer to the shell.
    EnvVarExpansion,
    /// A concrete binary name.
    Binary(String),
}

/// Parse a single leading token. Step-down: each guard returns early.
fn parse_leading_token(word: &str) -> LeadingToken {
    if word.starts_with('$') {
        return LeadingToken::EnvVarExpansion;
    }
    if let Some(eq_pos) = word.find('=') {
        let name = &word[..eq_pos];
        if is_valid_env_var_name(name) {
            return LeadingToken::EnvAssignment;
        }
    }
    LeadingToken::Binary(word.to_string())
}

/// True if `name` is a valid POSIX environment variable name: non-empty,
/// alphanumeric + underscore, not starting with a digit.
fn is_valid_env_var_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True if `binary` is found on `$PATH`. Implements `which` logic
/// manually to avoid pulling in the `which` crate. If `$PATH` is unset,
/// falls back to `/usr/bin:/bin` per POSIX.
async fn binary_on_path(binary: &str) -> bool {
    // Absolute or relative path: check the file directly.
    if binary.contains('/') {
        return std::path::Path::new(binary).is_file();
    }
    let path = std::env::var_os("PATH")
        .unwrap_or_else(|| std::ffi::OsString::from("/usr/bin:/bin"));
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(binary)))
}

/// True if `path` is a regular file with at least one executable bit set.
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
}

/// Spawn an executable directly (no shell). Used for AppDir launchers.
async fn spawn_direct(exe: &Path, args: &[&str]) -> LaunchResult {
    match Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => LaunchResult::Spawned(format!("launched {}", exe.display())),
        Err(e) => LaunchResult::Failed(format!("spawn {}: {e}", exe.display())),
    }
}

/// Run a shell command string via `sh -c`. Used for `actions.toml`
/// overrides and built-in default actions. The user has full shell
/// syntax available, including `${EDITOR:-vi}` env-var expansion.
async fn run_shell(cmd: &str) -> LaunchResult {
    match Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(_) => LaunchResult::Spawned(format!("ran: {cmd}")),
        Err(e) => LaunchResult::Failed(format!("sh -c: {e}")),
    }
}

/// Quote a path for safe inclusion in a shell command. Single-quote,
/// escaping any embedded single-quotes with the `'\''` idiom.
fn shell_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    s.chars().for_each(|c| match c {
        '\'' => out.push_str("'\\''"),
        _ => out.push(c),
    });
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_simple_path() {
        assert_eq!(shell_quote(Path::new("/tmp/notes.txt")), "'/tmp/notes.txt'");
    }

    #[test]
    fn quote_path_with_space() {
        assert_eq!(
            shell_quote(Path::new("/tmp/my file.txt")),
            "'/tmp/my file.txt'"
        );
    }

    #[test]
    fn quote_path_with_single_quote() {
        assert_eq!(shell_quote(Path::new("/tmp/it's")), "'/tmp/it'\\''s'");
    }

    #[test]
    fn extract_leading_binary_simple() {
        assert_eq!(extract_leading_binary("vim /tmp/foo"), Some("vim".into()));
        assert_eq!(
            extract_leading_binary("foot -e vim /tmp/foo"),
            Some("foot".into())
        );
    }

    #[test]
    fn extract_leading_binary_with_env_assignment() {
        assert_eq!(
            extract_leading_binary("EDITOR=vim /tmp/foo"),
            Some("/tmp/foo".into())
        );
    }

    #[test]
    fn extract_leading_binary_env_var_expansion_returns_none() {
        assert_eq!(extract_leading_binary("${EDITOR:-vi} /tmp/foo"), None);
        assert_eq!(extract_leading_binary("$EDITOR /tmp/foo"), None);
    }

    #[test]
    fn extract_leading_binary_empty() {
        assert_eq!(extract_leading_binary(""), None);
        assert_eq!(extract_leading_binary("   "), None);
    }

    #[test]
    fn extract_leading_binary_sh_dash_c() {
        assert_eq!(extract_leading_binary("sh -c 'echo hi'"), Some("sh".into()));
    }

    #[test]
    fn is_valid_env_var_name_accepts_standard_names() {
        assert!(is_valid_env_var_name("EDITOR"));
        assert!(is_valid_env_var_name("FOO_BAR"));
        assert!(is_valid_env_var_name("A1B2"));
    }

    #[test]
    fn is_valid_env_var_name_rejects_invalid() {
        assert!(!is_valid_env_var_name(""));
        assert!(!is_valid_env_var_name("1FOO"));
        assert!(!is_valid_env_var_name("FOO-BAR"));
        assert!(!is_valid_env_var_name("FOO.BAR"));
    }

    #[tokio::test]
    async fn binary_on_path_finds_sh() {
        assert!(binary_on_path("sh").await);
    }

    #[tokio::test]
    async fn binary_on_path_rejects_nonexistent() {
        assert!(!binary_on_path("definitely-not-a-real-binary-xyz123").await);
    }

    #[tokio::test]
    async fn binary_on_path_handles_absolute_path() {
        assert!(binary_on_path("/bin/sh").await);
        assert!(!binary_on_path("/nonexistent/path/to/binary").await);
    }
}
