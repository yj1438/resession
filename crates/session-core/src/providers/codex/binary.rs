//! Codex CLI executable discovery and native command construction.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::ir::SessionMeta;
use crate::provider::{ResumeSpec, ScanError};

fn override_path() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

pub fn set_override(path: Option<String>) {
    *override_path().lock().unwrap() = path.map(PathBuf::from);
}

fn lookup_binary() -> Option<PathBuf> {
    let (tool, name) = if cfg!(windows) {
        ("where", "codex")
    } else {
        ("which", "codex")
    };
    let output = Command::new(tool).arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
}

pub fn find_binary() -> Option<PathBuf> {
    if let Some(path) = override_path().lock().unwrap().clone() {
        return Some(path);
    }
    if let Some(path) = lookup_binary() {
        return Some(path);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let exe = if cfg!(windows) { "codex.exe" } else { "codex" };
    let mut candidates = vec![home.join(".local").join("bin").join(exe)];
    if let Some(appdata) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(appdata).join("npm").join("codex.cmd"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn spawn_wrapping(binary: PathBuf) -> (String, Vec<String>) {
    let extension = binary
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if cfg!(windows) && (extension == "cmd" || extension == "bat") {
        (
            "cmd".into(),
            vec!["/C".into(), binary.display().to_string()],
        )
    } else {
        (binary.display().to_string(), Vec::new())
    }
}

fn executable() -> Result<(String, Vec<String>), ScanError> {
    let binary = find_binary().ok_or_else(|| {
        ScanError::RootMissing("codex binary not found on PATH or common locations".into())
    })?;
    Ok(spawn_wrapping(binary))
}

pub fn build_resume_spec(meta: &SessionMeta) -> Result<ResumeSpec, ScanError> {
    let (program, mut args) = executable()?;
    args.extend(["resume".into(), meta.id.clone()]);
    let cwd = meta
        .cwd
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(ResumeSpec { program, args, cwd })
}

pub fn build_new_session_spec(cwd: PathBuf) -> Result<ResumeSpec, ScanError> {
    let (program, args) = executable()?;
    Ok(ResumeSpec { program, args, cwd })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_builds_resume_command() {
        set_override(Some("/fake/codex".into()));
        let meta = SessionMeta {
            provider: "codex".into(),
            id: "01234567-89ab-cdef-0123-456789abcdef".into(),
            cwd: None,
            project_dir: "project".into(),
            title: None,
            created_at: None,
            modified_at: None,
            git_branch: None,
            message_count: 0,
            source_file: PathBuf::from("session.jsonl"),
        };
        let spec = build_resume_spec(&meta).unwrap();
        set_override(None);
        assert_eq!(spec.program, "/fake/codex");
        assert_eq!(spec.args, vec!["resume".to_string(), meta.id.clone()]);
        assert_eq!(spec.cwd, PathBuf::from("."));
    }
}
