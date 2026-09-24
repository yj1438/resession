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
| 核心逻辑 | `crates/session-core` | 仅 serde/serde_json + std | Claude、Codex 扫描、解析、IR、resume 命令构造。**不依赖 Tauri**，可独立测试 |
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

纪律：**UI 层和 Tauri 命令层只认这个 trait**。Claude 与 Codex 分别实现适配器；Codex 的同 ID 多 rollout 分段由适配器合并。

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

### 3.2b 全文搜索（M2 v1）

```
顶栏搜索框（防抖 300ms）→ invoke search_sessions(query)
  → provider.search()：遍历会话文件 → 可搜索文本缓存（mtime+size 键，同扫描缓存模式）
  → 大小写不敏感匹配（ASCII 折叠 + CJK 子串）→ 片段（命中前后 60 字符）
  → 按会话最近活跃排序、截断 50、套用别名 → 结果替换左侧列表
```

- **范围（设计确认）**：只搜 user/assistant 的 Text 块；工具输入输出不搜（噪音/体积）
- **点击命中**：打开该会话转录，纯文本块内命中词 `<mark>` 高亮；
  markdown 块内不高亮（v1 限制）；滚动定位为 v2（需 IR 事件锚点，`event_index` 已预留）
- **无索引**：内容缓存即索引；会话量上千再考虑 tantivy/SQLite

### 3.3 恢复（终端）
```
用户点"终端"标签 → invoke resume
  → Provider.resume_command() → pty.rs 打开 portable-pty
  → spawn(program, args, cwd)   ← 以 provider:id 为键，幂等；已存在则直接附着
  → 双向桥：pty 输出(原始字节) → window.emit("pty-out") → xterm.write(Uint8Array)
            xterm.onData → invoke pty_write → pty 输入
  → resize 事件 → pty.resize(cols, rows)
```
- **PTY 常驻后端**，以会话 id 为键，生命周期与 UI 无关：切换会话/标签只是
  断开观看，切回时经 `pty_snapshot` 回放缓冲（512KB 滚动）重新附着，
  天然支持并行多会话。UI 上以侧栏绿点 + 状态栏计数标识，页头按钮手动关闭
- **不解析、不记录、不干预 PTY 内容**——这就是"原生"的含义

#### ConPTY 实战笔记（M1.2 踩坑）

| 问题 | 根因 | 解法 |
|---|---|---|
| TUI 黑白无色 | claude/Node 在 ConPTY 下的终端能力探测不可靠 | spawn 时注入 `FORCE_COLOR=3`（chalk 层强制真彩）+ `TERM=xterm-256color` + `COLORTERM=truecolor` |
| 恢复的会话不保存转录 | 宿主进程的 `CLAUDE*` 环境变量被 PTY 继承，claude 视为 child session 关闭写入 | spawn 前 `env_clear` + 白名单重灌，剥离全部 `CLAUDE*` 及 `CI`/`NO_COLOR` |
| 多字节字符跨块乱码 | 逐块 lossy UTF-8 解码 | 输出改字节级传输（`Vec<u8>` → `Uint8Array`），xterm 自带 UTF-8 状态机 |

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
