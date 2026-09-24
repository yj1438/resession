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
    let lines: Vec<&str> = std::str::from_utf8(&output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    // Windows 的 `where` 会把 npm shim 的无扩展名 sh 脚本排在最前（不可执行），
    // 需要按 .exe > .cmd/.bat > 首行的优先级挑选；与 claude 侧同一规则。
    if !cfg!(windows) {
        return lines.first().map(PathBuf::from);
    }
    let lower = |l: &str| l.to_ascii_lowercase();
    lines
        .iter()
        .find(|l| lower(l).ends_with(".exe"))
        .or_else(|| {
            lines
                .iter()
                .find(|l| lower(l).ends_with(".cmd") || lower(l).ends_with(".bat"))
        })
        .or_else(|| lines.first())
        .map(PathBuf::from)
}

pub fn find_binary() -> Option<PathBuf> {
    if let Some(path) = override_path().lock().unwrap().clone() {
        return Some(path);
    }
    if let Some(path) = lookup_binary() {
        return Some(path);
    }
    // Codex 桌面版自带 CLI（不在 PATH 上）；会话 rollout 也由它写入，
    // 所以"有会话但找不到二进制"的机器多半装的是桌面版。
    if let Some(path) = desktop_app_binary() {
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
    // macOS GUI 启动时 PATH 是残缺的（见 platform::extra_bin_dirs）：
    // Homebrew/npm/版本管理器安装的 codex 藏在这些标准落点里。
    for dir in crate::platform::extra_bin_dirs() {
        candidates.push(dir.join(exe));
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// Codex 桌面版的内置 CLI 位于 `<base>/bin/<build-hash>/codex(.exe)`，
/// build-hash 目录随 app 更新轮换——枚举后取修改时间最新的一个。
/// Windows: `%LOCALAPPDATA%\OpenAI\Codex\bin`（本机实测）；
/// macOS 桌面版路径未实测，按同构布局尝试，找不到就走设置页手动指定。
fn desktop_app_binary() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "codex.exe" } else { "codex" };
    for base in desktop_app_bases() {
        let mut versions: Vec<(std::time::SystemTime, PathBuf)> = match std::fs::read_dir(&base)
        {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .map(|p| {
                    let modified = p
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    (modified, p)
                })
                .collect(),
            Err(_) => continue,
        };
        versions.sort_by_key(|(modified, _)| *modified);
        // 新到旧逐个尝试：最新版本目录可能只带工具（如 rg.exe）不带 CLI，
        // 不能只看最新的一个。
        while let Some((_, dir)) = versions.pop() {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Desktop CLI candidate roots. Windows layout verified on-device
/// (%LOCALAPPDATA%/OpenAI/Codex/bin/<hash>/codex.exe); macOS layout is
/// unverified, so both with/without OpenAI-segment candidates are probed
/// until confirmed on a real Mac.
fn desktop_app_bases() -> Vec<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .map(|l| vec![PathBuf::from(l).join("OpenAI").join("Codex").join("bin")])
            .unwrap_or_default();
    }
    let mut bases = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let support = home.join("Library").join("Application Support");
        bases.push(support.join("OpenAI").join("Codex").join("bin"));
        bases.push(support.join("Codex").join("bin"));
    }
    bases
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
        ScanError::RootMissing(
            "codex binary not found (PATH / common locations / Codex desktop app); 可在设置页手动指定"
                .into(),
        )
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
