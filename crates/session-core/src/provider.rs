//! `SessionProvider` trait —— 所有 agent 耦合逻辑的唯一入口。
//!
//! 纪律（docs/architecture.md 第 2 节）：UI 与 Tauri 命令层只认这个 trait，
//! 不许直接接触 `~/.claude` 或写死 claude 命令。

use std::path::PathBuf;

use serde::Serialize;

use crate::ir::{Event, Role, SessionMeta};

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("session root not found: {0}")]
    RootMissing(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait SessionProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether moving a native session file to the system trash is safe for this provider.
    /// Codex also maintains a separate session index, so its rollouts are read-only here.
    fn can_trash_native(&self) -> bool {
        false
    }

    /// Whether absence from this provider's active scan means a session was deleted.
    /// Codex can move native sessions into archived_sessions, so absence is ambiguous.
    fn can_prune_missing_metadata(&self) -> bool {
        false
    }

    /// 发现全部会话，按 modified_at 倒序。单文件失败应跳过而非整体失败。
    fn scan(&self) -> Result<Vec<SessionMeta>, ScanError>;

    /// 读取完整转录并归一化为 IR。单行解析失败应跳过。
    fn load_transcript(&self, meta: &SessionMeta) -> Result<Vec<Event>, ScanError>;

    /// 构造"在会话原目录恢复该会话"的命令。
    fn resume_command(&self, meta: &SessionMeta) -> Result<ResumeSpec, ScanError>;

    /// 构造"在指定目录启动全新会话"的原生 Agent 命令。
    fn new_session_command(&self, cwd: PathBuf) -> Result<ResumeSpec, ScanError>;

    /// 全文搜索对话正文（user/assistant 文本块，大小写不敏感；工具块不搜）。
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, ScanError>;
}

/// 一条全文搜索命中
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")] // 勿漏：DTO 跨端，同 SessionMeta/PtyStatus 的教训
pub struct SearchHit {
    pub session: SessionMeta,
    /// 命中消息在转录事件流中的下标（v2 滚动定位的预留锚点）
    pub event_index: usize,
    pub role: Role,
    pub sidechain: bool,
    /// 命中词前后各 ~60 字符的片段
    pub snippet: String,
}

/// 交给 PTY 层直接 spawn 的命令。PTY 层不解析、不修改。
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl ResumeSpec {
    pub fn command_line(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }
}
