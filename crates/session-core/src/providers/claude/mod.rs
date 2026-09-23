//! Claude Code 适配器：扫描 `~/.claude/projects/`，解析 JSONL，构造 resume 命令。
//!
//! 格式笔记见 docs/data-formats.md（实测观测，非官方文档）。

pub(crate) mod binary;
mod parse;

use std::path::{Path, PathBuf};

use crate::ir::{Event, SessionMeta};
use crate::provider::{ResumeSpec, ScanError, SearchHit, SessionProvider};

pub struct ClaudeProvider {
    /// 覆盖会话根目录（测试/基准用；None = ~/.claude/projects）
    root: Option<PathBuf>,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        ClaudeProvider { root: None }
    }
}

impl ClaudeProvider {
    pub fn with_root(root: PathBuf) -> Self {
        ClaudeProvider { root: Some(root) }
    }

    /// 会话根目录：显式 root 优先，否则 `~/.claude/projects`。
    /// 找不到时返回错误（未安装 / 未产生过会话）。
    fn sessions_root(&self) -> Result<PathBuf, ScanError> {
        if let Some(root) = &self.root {
            if !root.is_dir() {
                return Err(ScanError::RootMissing(root.display().to_string()));
            }
            return Ok(root.clone());
        }
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map_err(|_| ScanError::RootMissing("neither USERPROFILE nor HOME is set".into()))?;
        let root = Path::new(&home).join(".claude").join("projects");
        if !root.is_dir() {
            return Err(ScanError::RootMissing(root.display().to_string()));
        }
        Ok(root)
    }

    /// 清空元数据与可搜索文本缓存（基准/测试用：测量冷路径时需要）。
    #[doc(hidden)]
    pub fn clear_caches() {
        parse::clear_caches();
    }
}

/// 遍历根目录下所有项目的 `*.jsonl` 文件（路径回调）。
/// 单个项目目录打不开就跳过——扫描与搜索都不允许因坏目录整体失败。
fn for_each_session_file(root: &Path, mut f: impl FnMut(PathBuf)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&project_dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            f(path);
        }
    }
}

/// 对一个根目录执行扫描（`scan()` 的可测内核：root 由调用方注入）。
/// 单文件解析失败跳过，永不整体失败；不存在/不可读的根返回空列表。
fn scan_root(root: &Path) -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    for_each_session_file(root, |path| {
        let project_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        match parse::scan_session_file(&path, &project_name) {
            Ok(meta) => sessions.push(meta),
            Err(e) => eprintln!("[claude-provider] skip {}: {e}", path.display()),
        }
    });
    // modified_at 倒序；None 排最后
    sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    sessions
}

/// 对一个根目录执行全文搜索（`search()` 的可测内核）。
fn search_root(root: &Path, query: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for_each_session_file(root, |path| {
        let project_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if let Ok(meta) = parse::scan_session_file(&path, &project_name) {
            hits.extend(parse::search_file(&meta, query));
        }
    });
    hits
}

impl SessionProvider for ClaudeProvider {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn scan(&self) -> Result<Vec<SessionMeta>, ScanError> {
        Ok(scan_root(&self.sessions_root()?))
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
        Ok(search_root(&self.sessions_root()?, query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 搭一个临时 projects 根：好文件、全坏文件、非 jsonl、坏目录各一。
    /// 锁定扫描的宽容语义：坏文件降级为空元数据会话而不是整体失败。
    #[test]
    fn scan_root_tolerates_bad_files_and_dirs() {
        let root = std::env::temp_dir().join("resession-test-root-scan");
        let _ = std::fs::remove_dir_all(&root);
        let proj_a = root.join("proj-a");
        let proj_b = root.join("proj-b");
        std::fs::create_dir_all(&proj_a).unwrap();
        std::fs::create_dir_all(&proj_b).unwrap();

        std::fs::write(
            proj_a.join("aaaa0000-0000-0000-0000-000000000000.jsonl"),
            "{\"type\":\"user\",\"message\":{\"content\":\"hi\"},\"cwd\":\"/a\"}\n",
        )
        .unwrap();
        // 全坏行：降级为空元数据会话（不改语义，测试锁定现状）
        std::fs::write(proj_b.join("bbbb0000-0000-0000-0000-000000000000.jsonl"), "garbage\n").unwrap();
        // 非 jsonl：忽略
        std::fs::write(proj_a.join("notes.txt"), "skip me").unwrap();

        // 断言不依赖 mtime 排序（连续写入的 mtime 可能相同）
        let sessions = scan_root(&root);
        assert_eq!(sessions.len(), 2);
        let good = sessions.iter().find(|s| s.id == "aaaa0000-0000-0000-0000-000000000000").expect("好文件应被扫出");
        assert_eq!(good.project_dir, "proj-a");
        assert_eq!(good.cwd.as_deref(), Some("/a"));
        let bad = sessions.iter().find(|s| s.id == "bbbb0000-0000-0000-0000-000000000000").expect("坏文件应降级而非跳过");
        assert_eq!(bad.title, None);
        assert_eq!(bad.message_count, 0);

        // 不存在的根：空结果而非报错（RootMissing 只针对 ~/.claude 缺失场景）
        assert!(scan_root(&root.join("no-such-dir")).is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn search_root_finds_hits_across_projects() {
        let root = std::env::temp_dir().join("resession-test-root-search");
        let _ = std::fs::remove_dir_all(&root);
        for (proj, id, text) in [
            ("alpha", "aaa10000-0000-0000-0000-000000000000", "the quantum flux"),
            ("beta", "bbb20000-0000-0000-0000-000000000000", "QUANTUM leap"),
        ] {
            let dir = root.join(proj);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{id}.jsonl")),
                format!("{{\"type\":\"user\",\"message\":{{\"content\":\"{text}\"}}}}\n"),
            )
            .unwrap();
        }
        // 大小写不敏感跨项目命中
        let hits = search_root(&root, "quantum");
        assert_eq!(hits.len(), 2);
        let mut projects: Vec<&str> = hits.iter().map(|h| h.session.project_dir.as_str()).collect();
        projects.sort();
        assert_eq!(projects, vec!["alpha", "beta"]);

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 真机集成测试：本机装有 Claude Code 时应能扫出会话且 resume 命令可构造。
    #[test]
    fn scan_real_projects_dir_when_present() {
        let Ok(sessions) = ClaudeProvider::default().scan() else {
            eprintln!("~/.claude/projects 不存在，跳过集成断言");
            return;
        };
        assert!(!sessions.is_empty(), "本机应有历史会话");
        // 最近会话排在最前
        let first = &sessions[0];
        assert_eq!(first.provider, "claude");
        if let Ok(spec) = ClaudeProvider::default().resume_command(first) {
            let cmdline = spec.command_line();
            assert!(cmdline.contains("--resume"), "命令应含 --resume: {cmdline}");
            assert!(cmdline.contains(&first.id), "命令应含会话 id: {cmdline}");
        }
    }
}
