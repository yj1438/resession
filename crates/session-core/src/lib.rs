//! ReSession 核心库：agent 会话的发现、解析与恢复命令构造。
//!
//! 职责边界（见 docs/architecture.md）：
//! - 本 crate 不依赖 Tauri，可独立编译测试
//! - 所有 agent 特定逻辑必须收口在 `providers` 下，外部只认 [`provider::SessionProvider`]

pub mod ir;
pub mod provider;
pub mod providers;

pub use ir::{Block, Event, Role, SessionMeta};
pub use provider::{ResumeSpec, ScanError, SearchHit, SessionProvider};

/// 返回全部已启用的 provider。新增 agent 时在这里注册一行。
pub fn registry() -> Vec<Box<dyn SessionProvider>> {
    vec![Box::new(providers::claude::ClaudeProvider)]
}

/// 应用级 claude 二进制路径覆盖（设置页写入；None = 恢复自动探测）
pub fn set_claude_binary_override(path: Option<String>) {
    providers::claude::binary::set_override(path);
}
