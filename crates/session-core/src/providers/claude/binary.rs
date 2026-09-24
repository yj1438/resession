//! claude 二进制探测与 resume 命令构造（平台差异收口在此）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::ir::SessionMeta;
use crate::provider::{ResumeSpec, ScanError};

/// 用户显式指定的路径（设置页写入），优先于自动探测
fn override_path() -> &'static Mutex<Option<PathBuf>> {
    static O: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(None))
}

pub fn set_override(path: Option<String>) {
    *override_path().lock().unwrap() = path.map(PathBuf::from);
}

/// 探测 claude 可执行体：用户设置优先，其次 PATH，再试常见安装位置。
pub fn find_claude_binary() -> Option<PathBuf> {
    // 显式指定就照用（哪怕文件暂缺——spawn 的报错比静默回退更直观）
    if let Some(p) = override_path().lock().unwrap().clone() {
        return Some(p);
    }
    if let Some(p) = from_lookup_tool() {
        return Some(p);
    }
    common_locations().into_iter().find(|p| p.exists())
}

fn from_lookup_tool() -> Option<PathBuf> {
    // Windows: where；unix: which。二者都是"每行一个结果"。
    let (tool, query) = if cfg!(windows) {
        ("where", "claude")
    } else {
        ("which", "claude")
    };
    let out = Command::new(tool).arg(query).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let lines: Vec<&str> = std::str::from_utf8(&out.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    pick_result_line(&lines).map(PathBuf::from)
}

/// Windows 的 `where` 按字母序返回全部命中，而 npm shim 三件套（`claude` /
/// `claude.cmd` / `claude.ps1`）里排第一的无扩展名文件是 sh 脚本，CreateProcess
/// 无法执行。优先 `.exe`，其次 `.cmd`/`.bat`（由 spawn_wrapping 做 cmd /C 包装），
/// 最后才回退首行；unix 的 `which` 只有一行，直接取。
fn pick_result_line<'a>(lines: &'a [&'a str]) -> Option<&'a str> {
    if !cfg!(windows) {
        return lines.first().copied();
    }
    let lower = |l: &str| l.to_ascii_lowercase();
    let exe = lines.iter().find(|l| lower(l).ends_with(".exe")).copied();
    let cmd = lines
        .iter()
        .find(|l| lower(l).ends_with(".cmd") || lower(l).ends_with(".bat"))
        .copied();
    exe.or(cmd).or_else(|| lines.first().copied())
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

/// 组装全新会话：同目录跑裸 `claude`，无任何参数。
pub fn build_new_session_spec(cwd: PathBuf) -> Result<ResumeSpec, ScanError> {
    let binary = find_claude_binary().ok_or_else(|| {
        ScanError::RootMissing("claude binary not found on PATH or common locations".into())
    })?;
    let (program, prefix) = spawn_wrapping(binary);
    let cwd = if cwd.is_dir() { cwd } else { PathBuf::from(".") };
    Ok(ResumeSpec {
        program,
        args: prefix,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// npm shim 三件套场景：无扩展名 sh 脚本排最前，必须跳过选 .cmd；
    /// 有 .exe 时优先 .exe（原生安装器与 npm 并存）。
    #[test]
    fn lookup_result_prefers_runnable_shim() {
        let npm_shims = vec![
            r"C:\npm\claude",
            r"C:\npm\claude.cmd",
            r"C:\npm\claude.ps1",
        ];
        let picked = pick_result_line(&npm_shims).unwrap();
        assert!(
            picked.ends_with(".cmd") || picked.ends_with(".exe"),
            "不应选中无扩展名的 sh shim: {picked}"
        );

        let with_exe = vec![r"C:\local\claude.exe", r"C:\npm\claude"];
        assert_eq!(pick_result_line(&with_exe), Some(r"C:\local\claude.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn cmd_wrapper_uses_cmd_c() {
        let (program, args) = spawn_wrapping(PathBuf::from(r"C:\npm\claude.cmd"));
        assert_eq!(program, "cmd");
        assert_eq!(args, vec!["/C", r"C:\npm\claude.cmd"]);
    }

    /// 二进制不存在时的行为：显式 override 照用（spawn 报错比静默回退直观）；
    /// 无 cwd 的会话回退到当前目录。
    #[test]
    fn explicit_override_used_verbatim_and_cwd_falls_back() {
        set_override(Some("/no/such/claude-anywhere".into()));
        let meta = SessionMeta {
            provider: "claude".into(),
            id: "abc123".into(),
            cwd: None,
            project_dir: "p".into(),
            title: None,
            created_at: None,
            modified_at: None,
            git_branch: None,
            message_count: 0,
            source_file: PathBuf::from("x.jsonl"),
        };
        let spec = build_resume_spec(&meta).expect("override 应短路 PATH 探测");
        // 先清理再断言：即使断言失败也不污染其它测试
        set_override(None);
        assert!(spec.program.contains("claude-anywhere"));
        assert!(spec.args.contains(&"--resume".to_string()));
        assert!(spec.args.contains(&"abc123".to_string()));
        assert_eq!(spec.cwd, PathBuf::from("."));
    }

    #[test]
    fn exe_needs_no_wrapper() {
        let (program, args) = spawn_wrapping(PathBuf::from("/usr/local/bin/claude"));
        assert_eq!(args, Vec::<String>::new());
        assert!(program.ends_with("claude"));
    }
}
