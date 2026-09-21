//! 归一化中间表示（IR）v0 —— 见 docs/data-formats.md 第 2 节。
//!
//! 演化规则：只加不改；消费方对未知值必须降级处理。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一个被发现的历史会话。`source_file` 是 `load_transcript` 的输入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")] // 与 src/types.ts 的 TS 镜像字段对齐
pub struct SessionMeta {
    pub provider: String,
    pub id: String,
    /// 会话原目录（权威来源：文件内容中的 cwd 字段）
    pub cwd: Option<String>,
    /// provider 存储里的编码项目目录名（仅展示兜底，编码有损不可逆）
    pub project_dir: String,
    /// 重命名标题 > summary > 首条用户消息
    pub title: Option<String>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    /// 会话最近所在的 git 分支（取文件中最后一次出现的 gitBranch）
    pub git_branch: Option<String>,
    pub message_count: usize,
    pub source_file: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    Text { text: String },
    ToolUse { name: String, brief: String },
    ToolResult { brief: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub role: Role,
    pub timestamp: Option<String>,
    pub blocks: Vec<Block>,
    /// 子 agent（sidechain）内部消息；`serde(default)` 保证旧数据兼容
    #[serde(default)]
    pub sidechain: bool,
}
