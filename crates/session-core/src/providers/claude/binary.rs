//! claude 二进制探测与 resume 命令构造（平台差异收口在此）。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ir::SessionMeta;
use crate::provider::{ResumeSpec, ScanError};

/// 探测 claude 可执行体：PATH 优先，再试常见安装位置。
pub fn find_claude_binary() -> Option<PathBuf> {
    if let Some(p) = from_lookup_tool() {
        return Some(p);
    }
    common_locations().into_iter().find(|p| p.exists())
}

fn from_lookup_tool() -> Option<PathBuf> {
    // Windows: where；unix: which。二者都是"每行一个结果"，取第一行。
    let (tool, query) = if cfg!(windows) {
        ("where", "claude")
    } else {
        ("which", "claude")
    };
    let out = Command::new(tool).arg(query).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let first = std::str::from_utf8(&out.stdout)
        .ok()?
        .lines()
        .next()?
        .trim()
        .to_string();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
}

fn common_locations() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    {
        // Claude Code 原生安装器
        v.push(home.join(".local").join("bin").join(claude_exe()));
        // npm 全局安装
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            v.push(appdata.join("npm").join("claude.cmd"));
        }
    }
    v
}

fn claude_exe() -> &'static str {
    if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    }
}

/// Windows 上 `.cmd`/`.bat` 不能被 CreateProcess 直接执行，需要 `cmd /C` 包装。
fn spawn_wrapping(program: PathBuf) -> (String, Vec<String>) {
    let ext = program
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if cfg!(windows) && (ext == "cmd" || ext == "bat") {
        (
            "cmd".to_string(),
            vec!["/C".into(), program.display().to_string()],
        )
    } else {
        (program.display().to_string(), Vec::new())
    }
}

/// 组装 `claude --resume <id>`（或其 Windows cmd 包装）。
pub fn build_resume_spec(meta: &SessionMeta) -> Result<ResumeSpec, ScanError> {
    let binary = find_claude_binary().ok_or_else(|| {
        ScanError::RootMissing("claude binary not found on PATH or common locations".into())
    })?;
    let (program, mut prefix) = spawn_wrapping(binary);
    prefix.push("--resume".into());
    prefix.push(meta.id.clone());
    let cwd = meta
        .cwd
        .as_deref()
        .map(Path::new)
        .filter(|p| p.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(ResumeSpec {
        program,
        args: prefix,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn cmd_wrapper_uses_cmd_c() {
        let (program, args) = spawn_wrapping(PathBuf::from(r"C:\npm\claude.cmd"));
        assert_eq!(program, "cmd");
        assert_eq!(args, vec!["/C", r"C:\npm\claude.cmd"]);
    }

    #[test]
    fn exe_needs_no_wrapper() {
        let (program, args) = spawn_wrapping(PathBuf::from("/usr/local/bin/claude"));
        assert_eq!(args, Vec::<String>::new());
        assert!(program.ends_with("claude"));
    }
}
