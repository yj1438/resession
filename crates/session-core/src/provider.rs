//! `SessionProvider` trait —— 所有 agent 耦合逻辑的唯一入口。
//!
//! 纪律（docs/architecture.md 第 2 节）：UI 与 Tauri 命令层只认这个 trait，
//! 不许直接接触 `~/.claude` 或写死 claude 命令。

use std::path::PathBuf;

use crate::ir::{Event, SessionMeta};

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("session root not found: {0}")]
    RootMissing(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait SessionProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// 发现全部会话，按 modified_at 倒序。单文件失败应跳过而非整体失败。
    fn scan(&self) -> Result<Vec<SessionMeta>, ScanError>;

    /// 读取完整转录并归一化为 IR。单行解析失败应跳过。
    fn load_transcript(&self, meta: &SessionMeta) -> Result<Vec<Event>, ScanError>;

    /// 构造"在会话原目录恢复该会话"的命令。
    fn resume_command(&self, meta: &SessionMeta) -> Result<ResumeSpec, ScanError>;
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
