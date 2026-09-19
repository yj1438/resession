//! Claude Code 适配器：扫描 `~/.claude/projects/`，解析 JSONL，构造 resume 命令。
//!
//! 格式笔记见 docs/data-formats.md（实测观测，非官方文档）。

pub(crate) mod binary;
mod parse;

use std::path::{Path, PathBuf};

use crate::ir::{Event, SessionMeta};
use crate::provider::{ResumeSpec, ScanError, SearchHit, SessionProvider};

pub struct ClaudeProvider;

impl ClaudeProvider {
    /// `~/.claude/projects`。找不到时返回错误（未安装 / 未产生过会话）。
    fn sessions_root() -> Result<PathBuf, ScanError> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map_err(|_| ScanError::RootMissing("neither USERPROFILE nor HOME is set".into()))?;
        let root = Path::new(&home).join(".claude").join("projects");
        if !root.is_dir() {
            return Err(ScanError::RootMissing(root.display().to_string()));
        }
        Ok(root)
    }
}

impl SessionProvider for ClaudeProvider {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn scan(&self) -> Result<Vec<SessionMeta>, ScanError> {
        let root = Self::sessions_root()?;
        let mut sessions = Vec::new();
        // 单项目目录失败跳过：扫描永远不能因为一个坏目录整体失败
        for entry in std::fs::read_dir(&root)?.flatten() {
            let project_dir = entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            let project_name = project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let Ok(files) = std::fs::read_dir(&project_dir) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                match parse::scan_session_file(&path, &project_name) {
                    Ok(meta) => sessions.push(meta),
                    Err(e) => eprintln!("[claude-provider] skip {}: {e}", path.display()),
                }
            }
        }
        // modified_at 倒序；None 排最后
        sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        Ok(sessions)
    }

    fn load_transcript(&self, meta: &SessionMeta) -> Result<Vec<Event>, ScanError> {
        Ok(parse::load_events(&meta.source_file)?)
    }

    fn resume_command(&self, meta: &SessionMeta) -> Result<ResumeSpec, ScanError> {
        binary::build_resume_spec(meta)
    }

    fn new_session_command(&self, cwd: PathBuf) -> Result<ResumeSpec, ScanError> {
        binary::build_new_session_spec(cwd)
    }

    fn search(&self, query: &str) -> Result<Vec<SearchHit>, ScanError> {
        let root = Self::sessions_root()?;
        let mut hits = Vec::new();
        for entry in std::fs::read_dir(&root)?.flatten() {
            let project_dir = entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            let project_name = project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let Ok(files) = std::fs::read_dir(&project_dir) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(meta) = parse::scan_session_file(&path, &project_name) else {
                    continue;
                };
                hits.extend(parse::search_file(&meta, query));
            }
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机集成测试：本机装有 Claude Code 时应能扫出会话且 resume 命令可构造。
    #[test]
    fn scan_real_projects_dir_when_present() {
        let Ok(sessions) = ClaudeProvider.scan() else {
            eprintln!("~/.claude/projects 不存在，跳过集成断言");
            return;
        };
        assert!(!sessions.is_empty(), "本机应有历史会话");
        // 最近会话排在最前
        let first = &sessions[0];
        assert_eq!(first.provider, "claude");
        if let Ok(spec) = ClaudeProvider.resume_command(first) {
            let cmdline = spec.command_line();
            assert!(cmdline.contains("--resume"), "命令应含 --resume: {cmdline}");
            assert!(cmdline.contains(&first.id), "命令应含会话 id: {cmdline}");
        }
    }
}
