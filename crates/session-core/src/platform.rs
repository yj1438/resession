//! Cross-provider platform path & environment helpers.
//!
//! macOS GUI apps launched from Finder/Dock get a minimal PATH
//! (/usr/bin:/bin:/usr/sbin:/sbin), so which/where cannot see CLIs installed
//! via Homebrew/npm/version managers, and restored agent sessions inherit
//! that crippled environment. Two countermeasures:
//! 1. `extra_bin_dirs`: standard install roots probed one by one (cheap);
//! 2. `login_shell_path`: grab the full environment from the user's login
//!    shell once and cache it (the definitive fix, same idea as VS Code).
//! Both are macOS-only; other platforms keep the previous behavior.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// Binary dirs invisible to GUI processes on macOS (most common first).
/// Empty on other platforms.
pub fn extra_bin_dirs() -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let mut dirs = vec![
        // Apple Silicon Homebrew (default prefix on M-series)
        PathBuf::from("/opt/homebrew/bin"),
        // Intel Homebrew + default npm global on macOS
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        // custom npm prefix
        dirs.push(home.join(".npm-global").join("bin"));
        // version managers / standalone installers
        dirs.push(home.join(".volta").join("bin"));
        dirs.push(home.join(".local").join("share").join("mise").join("shims"));
        dirs.push(home.join(".asdf").join("shims"));
        dirs.push(home.join(".local").join("share").join("pnpm"));
    }
    dirs
}

/// PATH resolved by the user's login shell (macOS; cached for the process
/// lifetime). Returns None on other platforms, or when anything fails
/// (no SHELL / timed out / unparseable output) so callers keep the old
/// app-environment behavior.
pub fn login_shell_path() -> Option<String> {
    login_shell_env().and_then(|env| env.get("PATH").cloned())
}

/// `login_shell_path` split into directories (unix `:` separator).
pub fn login_shell_path_dirs() -> Vec<PathBuf> {
    login_shell_path()
        .map(|p| p.split(':').map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn login_shell_env() -> Option<&'static HashMap<String, String>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    static CACHE: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(compute_login_shell_env).as_ref()
}

fn compute_login_shell_env() -> Option<HashMap<String, String>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    // -i makes zsh read .zshrc (where macOS users usually add PATH);
    // -l makes it read .zprofile. env -0 is NUL-delimited so rc-file stdout
    // noise without '=' is dropped during parsing.
    let mut child = Command::new(&shell)
        .args(["-ilc", "command env -0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    // std has no wait-with-timeout: reader thread + channel, kill after 3s
    // (defends against rc files that hang the login shell)
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::BufReader::new(stdout).read_to_string(&mut buf);
        let _ = child.wait();
        let _ = tx.send(buf);
    });
    let output = rx.recv_timeout(Duration::from_secs(3)).ok()?;
    let env = parse_env_null(&output);
    // If rc noise polluted stdout beyond parsing, prefer no env over a
    // crippled one.
    if env.is_empty() {
        None
    } else {
        Some(env)
    }
}

/// Parse `env -0` output: NUL-separated KEY=VALUE entries. Lines without
/// '=' (rc-file stdout noise) are dropped; '=' inside a value belongs to it.
fn parse_env_null(out: &str) -> HashMap<String, String> {
    out.split('\0')
        .filter_map(|entry| entry.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_null_keeps_values_with_equals_and_drops_noise() {
        let env = parse_env_null("PATH=/usr/bin:/bin\0JUNK LINE\0LANG=zh_CN.UTF-8\0");
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
        assert_eq!(env.get("LANG").map(String::as_str), Some("zh_CN.UTF-8"));
        assert!(!env.contains_key("JUNK LINE"));
    }

    #[test]
    fn extra_bin_dirs_is_empty_off_macos() {
        let dirs = extra_bin_dirs();
        if !cfg!(target_os = "macos") {
            assert!(dirs.is_empty());
        }
    }
}
