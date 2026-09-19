# 会话数据格式笔记

> 本文记录**实测观测**（2026-09-19，Claude Code 2.1.237，Windows），非官方文档。
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
  provider: "claude";        // 未来: "codex" | ...
  id: string;                // UUID
  cwd: string | null;        // 权威项目路径
  projectDir: string;        // ~/.claude/projects 下的编码目录名
  title: string | null;      // rename 标题 > summary > 首条用户消息
  createdAt: string | null;  // ISO8601
  modifiedAt: string | null;
  messageCount: number;
  sourceFile: string;        // 绝对路径，load_transcript 的输入
}

interface Event  { role: "user"|"assistant"|"system"; timestamp: string|null; blocks: Block[] }
type   Block     = { kind:"text", text:string }
                 | { kind:"toolUse", name:string, brief:string }
                 | { kind:"toolResult", brief:string }
```

演化规则：**只加不改**；前端对未知 block 降级为原始 JSON。

## 3. Codex CLI（占位，接入 M3 时实测）

- 会话目录：`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`（待实测确认）
- 恢复命令：`codex resume <SESSION-ID>` / `codex resume --last`
- 接入 = 新增 `crates/session-core/src/providers/codex/`，实现同一 trait，UI 不动
