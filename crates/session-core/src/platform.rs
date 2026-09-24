//! Cross-provider platform path helpers.
//!
//! macOS GUI apps launched from Finder/Dock get a minimal PATH
//! (/usr/bin:/bin:/usr/sbin:/sbin), so which/where cannot see CLIs installed
//! via Homebrew/npm/version managers. These are the standard install roots
//! used as discovery fallbacks. The definitive fix (login-shell env
//! resolution) is tracked in roadmap M2.5.

use std::path::PathBuf;

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
