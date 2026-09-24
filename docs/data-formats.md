# 会话数据格式笔记

> 本文记录**实测观测**（Claude Code：2026-09-19 / Windows；Codex：2026-09-24 / macOS），非官方文档。
> 格式是私有的、会演化的——解析必须宽容（见 design.md 风险表）。

## 1. Claude Code 会话存储

### 1.1 目录布局

```
~/.claude/projects/
├── D--code-my-app/                               ← 项目目录（路径编码）
│   ├── 06c76be5-72c8-4040-ae49-4864098993ed.jsonl  ← 一个会话 = 一个文件（UUID 命名）
│   └── ...
├── D--code-web-frontend/
└── C--Users-me-dotfiles/
```

- **项目目录编码规则（观测）**：绝对路径中的 `:` `\` `/` 均替换为 `-`。
  例：`D:\code\my-app` → `D--code-my-app`
- ⚠️ 编码是有损的（不可逆）：真实 cwd 要从文件内容每行的 `cwd` 字段取，
  目录名只作展示兜底
- 每个 `.jsonl` 是**追加写**的行流（JSON Lines），一行一个 JSON 事件

### 1.2 已观测的行类型

| type | 含义 | 备注 |
|---|---|---|
| `user` | 用户消息 | `message.content` **可能是 string 也可能是 block 数组** |
| `assistant` | 助手消息 | content 为 block 数组（text / tool_use / tool_result） |
| `summary` | 会话摘要 | 压缩/compact 后生成，含 `summary` 字段 |
| `queue-operation` | 消息排队事件 | enqueue/dequeue，**回放时应忽略** |
| `attachment` | 上下文附件 | 如 agent 列表注入，回放可折叠 |

### 1.3 关键字段（user/assistant 行）

```json
{
  "type": "user",
  "message": { "role": "user", "content": "hello" },
  "uuid": "09a2...",
  "parentUuid": null,              ← 消息树（fork 会分叉）
  "timestamp": "2026-09-08T16:45:16.522Z",
  "cwd": "D:\\code\\my-app",  ← 真实项目路径（权威来源）
  "sessionId": "06c76...",
  "gitBranch": "HEAD",
  "version": "2.1.237",
  "isSidechain": false             ← true 为子 agent 内部会话，列表应过滤
}
```

### 1.4 其他观测

- **fork**：fork 出的会话是独立文件，含 `forkedFrom.sessionId` 指向父会话
- **`/rename`**：重命名的会话标题存于文件内（cc-sessions 显示 `★` 前缀），具体字段待实现时确认
- content block 类型：`{"type":"text","text":...}` / `{"type":"tool_use","name":...,"input":...}` / `{"type":"tool_result","content":...}`

## 2. IR（归一化中间表示）v0

Rust 定义：`crates/session-core/src/ir.rs`；TS 镜像：`src/types.ts`。

```ts
interface SessionMeta {
  provider: "claude" | "codex";
  id: string;                // UUID
  cwd: string | null;        // 权威项目路径
  projectDir: string;        // cwd 缺失时的项目展示兜底
  title: string | null;      // rename 标题 > summary > 首条用户消息
  createdAt: string | null;  // ISO8601
  modifiedAt: string | null;
  gitBranch: string | null;
  messageCount: number;
  sourceFile: string;        // 绝对路径，load_transcript 的输入
}

interface Event  { role: "user"|"assistant"|"system"; timestamp: string|null; blocks: Block[]; sidechain: boolean }
type   Block     = { kind:"text", text:string }
                 | { kind:"toolUse", name:string, brief:string }
                 | { kind:"toolResult", brief:string }
```

演化规则：**只加不改**；前端对未知 block 降级为原始 JSON。

## 3. Codex 本地会话（2026-09-24 实测）

- 活跃会话目录：`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`；未设置 `CODEX_HOME` 时使用 `~/.codex/sessions/`。
- 会话 ID 取首部 `session_meta.payload.id`，不从文件名反推（归档文件名可能含多个 UUID）。
- 同一 ID 可能有多个 `rollout` 分段；后续文件带 `history_base`，只记录续写部分。扫描按 ID 合并，回放与搜索按分段时间串接，搜索下标加上前段事件数。
- `SessionMeta.sourceFile` 指向最后一个活跃分段；读取转录时 Provider 会按 ID 找到并合并所有活跃分段。
- 大于 1MB 的 rollout 在列表扫描时只读取开头元信息；该分段的 `messageCount` 暂记 0（表示未统计），所以整条会话的计数可能不完整。搜索和回放按需读取完整内容。
- `session_meta.payload` 提供 `timestamp`、`cwd`、`git.branch`；后续 `turn_context.payload.cwd` 可能改变当前工作目录，取最后一次。
- `$CODEX_HOME/session_index.jsonl`（默认 `~/.codex/session_index.jsonl`）的 `id` / `thread_name` 提供显示标题；未入索引时回退到首条用户文本。
- `response_item.payload.type == "message"`：`role` 为 user/assistant；`content` 数组里 `input_text` / `output_text` 映射为 IR Text。
- `response_item` 的 `function_call` / `custom_tool_call` 和对应 output 映射为工具块；`reasoning`、加密内容、图片及未知项不进入 IR。
- `event_msg` 含与部分 `response_item` 重复的工具状态事件，不重复映射到转录。
- 本阶段扫描活跃 `sessions/`。`archived_sessions/` 由 Codex 自身管理，暂不混入可恢复列表。
- 原生恢复命令：`codex resume <SESSION-ID>`；新会话命令：`codex`。本机 `codex resume --all` 的选择器可列出 Codex Desktop 会话，但逐 ID 的 TUI 恢复仍需发布产物实测。
- ReSession 对 Codex 原生 JSONL 只读；其标题索引独立维护，故暂不提供 ReSession 的“移入废纸篓”，可在 Codex 中删除。

格式是私有实现细节，解析器按行跳过坏行及未知类型；测试 fixture 使用合成且脱敏的数据。
