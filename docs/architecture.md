# ReSession 架构文档

## 1. 总览

```
┌─────────────────────────────────────────────────────┐
│ Tauri 2 应用                                        │
│                                                     │
│  WebView (React + TS)            Rust 进程          │
│ ┌──────────────────┐   invoke    ┌───────────────┐  │
│ │ Sidebar 列表     │ ──────────► │ session-core  │  │
│ │ TranscriptPane   │             │ (纯逻辑crate) │  │
│ │ TerminalPane     │  PTY 流     │               │  │
│ │  └─ xterm.js ◄──────────────┐  │  providers/   │  │
│ └──────────────────┘  事件桥    │   └─ claude/  │  │
│                                │  pty.rs       │  │
│                                │  └─ portable-pty  │
│                                └───────────────┘  │
└─────────────────────────────────────────────────────┘
```

三层职责严格分离：

| 层 | 位置 | 依赖 | 说明 |
|---|---|---|---|
| 核心逻辑 | `crates/session-core` | 仅 serde/serde_json + std | 扫描、解析、IR、resume 命令构造。**不依赖 Tauri**，可独立测试 |
| 壳/桥 | `src-tauri`（M1） | tauri 2, portable-pty, session-core | invoke 命令（scan/transcript/resume）、PTY 生命周期、事件推送 |
| 界面 | `src/` | react, @xterm/xterm | 三区布局、转录渲染、xterm 终端 |

## 2. SessionProvider 接口

所有 agent 耦合逻辑的唯一入口：

```rust
pub trait SessionProvider: Send + Sync {
    fn name(&self) -> &'static str;                       // "claude" / "codex" / ...
    fn scan(&self) -> Result<Vec<SessionMeta>>;           // 发现全部会话
    fn load_transcript(&self, meta: &SessionMeta)
        -> Result<Vec<Event>>;                            // 原始 JSONL → IR
    fn resume_command(&self, meta: &SessionMeta)
        -> Result<ResumeSpec>;                            // 二进制查找 + 命令模板
}

pub struct ResumeSpec {
    pub program: String,        // 经 .cmd 处理后的可执行体
    pub args: Vec<String>,      // ["--resume", "<id>"]
    pub cwd: PathBuf,           // 会话原目录
}
```

纪律：**UI 层和 Tauri 命令层只认这个 trait**。新增 Codex = 新增一个 adapter 目录 + registry 注册一行。

## 3. 数据流

### 3.1 列表（扫描）
```
~/.claude/projects/            （ClaudeProvider::scan）
  └─ <编码目录名>/*.jsonl   →   流式读取元数据（首条 user 消息、时间戳、消息数）
                            →   Vec<SessionMeta> → invoke 返回 → Sidebar
```
- 元数据扫描只逐行反序列化必要字段，遇到消息体可整行跳过（性能关键）
- 排序：`modified_at` 倒序

### 3.2 回放（转录）
```
选中 SessionMeta → ClaudeProvider::load_transcript
  → 逐行 JSON → 宽容解析 → Vec<Event> (IR) → 前端渲染
```
- 前端对 IR 渲染；未知 block 类型降级为原始 JSON 折叠块

### 3.3 恢复（终端）
```
用户点"终端"标签 → invoke resume
  → Provider.resume_command() → pty.rs 打开 portable-pty
  → spawn(program, args, cwd)
  → 双向桥：pty 输出 → window.emit("pty://<id>/out") → xterm.write
            xterm.onData → invoke write → pty 输入
  → resize 事件 → pty.resize(cols, rows)
```
- 每个 PTY 一个 uuid 会话 id，前端标签页销毁时 invoke close 释放
- **不解析、不记录、不干预 PTY 内容**——这就是"原生"的含义

## 4. IR（中间表示）v0

定义在 `session-core/src/ir.rs`，前端有对应的 TS 镜像（`src/types.ts`）：

- `SessionMeta`：provider / id / cwd / project / title / created_at / modified_at / message_count / source_file
- `Event`：role + timestamp + Vec&lt;Block&gt;
- `Block`：Text(String) | ToolUse { name, brief } | ToolResult { brief }

演化规则：只加不改；前端对未知枚举值降级处理。

## 5. 平台矩阵

| 能力 | Windows | macOS | Linux |
|---|---|---|---|
| Tauri WebView | WebView2（系统自带） | WKWebView | WebKitGTK |
| portable-pty | ConPTY | openpty | openpty |
| claude 二进制探测 | `where claude` + npm/本地安装路径 | `which claude` + `~/.local/bin` | 同 macOS |
| 默认 shell（PTY login 环境兜底） | PowerShell | zsh | bash |
| 构建门槛 | **需 MSVC Build Tools**（M1 门槛，见下） | Xcode CLT | 常规 |

### MSVC 门槛（当前阻塞项）

Tauri 2 在 Windows 官方只支持 `x86_64-pc-windows-msvc` 目标（wry/webview2-com 链路），
现有本地工具链是 `windows-gnu`。M1 开工前需要安装
**Visual Studio Build Tools（C++ 桌面开发工作负载）**——标准微软安装器，可卸载，约 3GB。
这是本机全局环境的唯一增项；`session-core` 的开发与测试不依赖它（gnu 工具链即可）。

## 6. 错误处理策略

- 解析层：单行失败不导致整个文件失败（`lines().filter_map`），累积 warning 计数
- 扫描层：单项目目录失败告警并继续（参照 cc-sessions 的 `--strict` 语义）
- PTY 层：claude 启动失败时把错误原样写进 xterm 终端（用户看到的是终端错误，不是弹窗）
