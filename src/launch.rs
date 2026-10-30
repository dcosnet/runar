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
//!                     default commands use shell-level fallback chains
//!                     (env var first, then lightweight binaries), so
//!                     they work on any system.
//!                  c. Direct execution — if the file itself carries an
//!                     executable bit, run it (ROX-Filer convention).
//!
//!   3. Directory / Symlink → handled by the UI layer (navigation). This
//!                module is invoked only for "open" actions.
//!
//! There is no xdg-open fallback by design: runar is self-contained and
//! defers to no system MIME database. Files with no matching action, no
//! table entry, and no executable bit report "no handler" honestly —
//! the fix is a line in `actions.toml`, not a desktop database.
//!
//! All launches are detached: the child process is spawned and not
//! waited on. Errors are returned synchronously only if `spawn()` itself
//! fails (e.g. missing binary); child-side failures surface to the user
//! in the child's terminal.

use crate::config::Config;
use crate::vfs::EntryKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Outcome of a launch attempt, for UI feedback.
#[derive(Debug)]
pub enum LaunchResult {
    /// Successfully spawned. Carries a human-readable description for the
    /// status bar (e.g. "launched vim /tmp/notes.txt").
    Spawned(String),
    /// Nothing matched and the dispatcher declined to spawn (no action,
    /// no built-in table entry, and no executable bit). UI shows
    /// "no handler".
    NoHandler,
    /// Spawn failed. The user should see this.
    Failed(String),
}

/// Launch `path` according to its kind, consulting the user's action
/// overrides before falling back to system defaults.
pub async fn launch(path: &Path, kind: &EntryKind, config: &Config) -> LaunchResult {
    match kind {
        EntryKind::AppDir { launcher } => {
            // ROX convention: AppRun executes with the AppDir itself as
            // the working directory — many AppRun scripts resolve their
            // bundled resources relative to it, and would break with
            // runar's inherited CWD.
            let appdir = launcher.parent().map(|p| p.to_path_buf());
            spawn_direct(launcher, &[], appdir.as_deref()).await
        }
        EntryKind::File => launch_file(path, config).await,
        // Symlinks are resolved by the UI before calling launch().
        // Directory and Symlink are no-ops here.
        EntryKind::Symlink { .. } | EntryKind::Directory => LaunchResult::NoHandler,
    }
}

/// File-launch step-down: try user overrides, then built-in MIME table,
/// then direct execution if the file is executable. Each step either
/// returns a `LaunchResult` or falls through to the next.
async fn launch_file(path: &Path, config: &Config) -> LaunchResult {
    // 1. User overrides from actions.toml (highest priority).
    if let Some(result) = try_user_action(path, config).await {
        return result;
    }
    // 2. Built-in default MIME table.
    if let Some(result) = try_builtin_default(path).await {
        return result;
    }
    // 3. Direct execution fallback (no xdg by design).
    try_run_directly(path).await
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

/// Step 3: xdg-free final fallback. If the file itself carries an
/// executable bit, run it — the ROX-Filer convention: a double-clicked
/// script or binary executes, exactly like ROX/Filer treats executables.
/// The kernel's `#!` handling covers scripts. Anything else has no
/// handler: without an xdg database there is nothing left to ask, so the
/// honest answer is "no handler" and the fix is a line in `actions.toml`.
async fn try_run_directly(path: &Path) -> LaunchResult {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.is_file() && (meta.permissions().mode() & 0o111 != 0) => {
            spawn_direct(path, &[], None).await
        }
        Ok(_) => LaunchResult::NoHandler,
        Err(e) => LaunchResult::Failed(format!("stat {}: {e}", path.display())),
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
/// `cwd` sets the child's working directory (the AppDir itself for
/// AppRun-style launches).
async fn spawn_direct(exe: &Path, args: &[&str], cwd: Option<&Path>) -> LaunchResult {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(_) => LaunchResult::Spawned(format!("launched {}", exe.display())),
        Err(e) => LaunchResult::Failed(format!("spawn {}: {e}", exe.display())),
    }
}

/// Run a shell command string via `sh -c`. Used for `actions.toml`
/// overrides and built-in default actions. The user has full shell
/// syntax available, including `${EDITOR:-vi}` env-var expansion.
async fn run_shell(cmd: &str) -> LaunchResult {
    run_shell_in(cmd, None).await
}

/// `run_shell`, with an optional working directory for the child. Used
/// by the terminal launcher so the shell inside the new terminal window
/// starts in the directory the user asked for.
async fn run_shell_in(cmd: &str, cwd: Option<&Path>) -> LaunchResult {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    match command.spawn() {
        Ok(_) => LaunchResult::Spawned(format!("ran: {cmd}")),
        Err(e) => LaunchResult::Failed(format!("sh -c: {e}")),
    }
}

/// Launch a terminal emulator rooted at `dir`. Resolves the emulator via
/// the same chain machinery as the file associations: `$TERMINAL` env
/// var first, then the built-in terminal chain (`src/mime.rs`:
/// rs-mrxvt → mrxvt → urxvt → ghostty → alacritty → tabby → guake).
/// The chain runs with the child's working directory set to `dir`, so
/// the terminal's shell starts there without per-terminal
/// `--working-directory` flag guessing.
pub async fn launch_terminal(dir: &Path, _config: &Config) -> LaunchResult {
    run_shell_in(crate::mime::TERMINAL_CHAIN, Some(dir)).await
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

    /// The xdg-free final fallback: an executable file runs directly,
    /// exactly as ROX-Filer would.
    #[tokio::test]
    async fn executable_file_runs_directly() {
        let tmp = std::env::temp_dir().join(format!(
            "runar-launch-exec-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let script = tmp.join("runme.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        let result = try_run_directly(&script).await;
        assert!(
            matches!(result, LaunchResult::Spawned(_)),
            "executable should spawn directly, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A plain data file with no handler anywhere reports NoHandler —
    /// there is no xdg-open to defer to, by design.
    #[tokio::test]
    async fn plain_data_file_without_handler_reports_no_handler() {
        let tmp = std::env::temp_dir().join(format!(
            "runar-launch-none-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let data = tmp.join("data.xyzzy");
        std::fs::write(&data, b"not an executable").unwrap();

        let result = try_run_directly(&data).await;
        assert!(matches!(result, LaunchResult::NoHandler), "got: {result:?}");

        // A directory is not executable-as-file either (is_file gate).
        let result = try_run_directly(&tmp).await;
        assert!(matches!(result, LaunchResult::NoHandler), "got: {result:?}");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The terminal launcher runs its chain with the child's working
    /// directory set to the requested folder. A nonexistent directory
    /// must fail the spawn. The success path is exercised through
    /// `run_shell_in` with a stub command rather than the real
    /// TERMINAL_CHAIN — on a machine where a chained terminal exists,
    /// spawning it for real would leave a live GUI process holding the
    /// test runner's inherited stdio pipes open, and `cargo test` would
    /// hang after printing its results.
    #[tokio::test]
    async fn terminal_launch_respects_cwd() {
        let tmp = std::env::temp_dir().join(format!(
            "runar-launch-term-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Success path: cwd wiring spawns cleanly with a stub command
        // that exits immediately.
        let result = run_shell_in("exit 0", Some(&tmp)).await;
        assert!(matches!(result, LaunchResult::Spawned(_)), "got: {result:?}");

        // A nonexistent directory must fail the spawn before any
        // process starts — true for both run_shell_in and
        // launch_terminal.
        let bogus = tmp.join("does-not-exist");
        let result = run_shell_in("exit 0", Some(&bogus)).await;
        assert!(matches!(result, LaunchResult::Failed(_)), "got: {result:?}");
        let config = Config::default();
        let result = launch_terminal(&bogus, &config).await;
        assert!(matches!(result, LaunchResult::Failed(_)), "got: {result:?}");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
