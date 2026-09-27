# Agent Bridge 设计稿（v1 草案）

> 状态：**待评审**。本文档是 `plugins/tmux-agent`（Mac 实测版）协议层向 ReSession 传输层的移植设计。
> 结论先行：**协议原样移植，只换传输层**。代码实现开工前需用户确认本文档。

## 1. 目标与非目标

### 目标

让任意 agent 会话（Claude Code / Codex）通过一个 skill 触发词（`trs`），把**由 agent 自己整理的内容**投递给另一个会话，并能回读对方的回复。人在环上：agent 发送的前提是用户在会话里触发了它。

### 非目标（明确不做）

- **L3 自主对话**：两个 agent 自主循环（用户已确认人主导）
- **无头 ask 模式**：`plugins/tmux-agent` 已验证单路径（注入 + 回读）够用，不引入第二机制
- **服务端排队**：目标忙时返回错误由发起方决定重试（v2 再议）
- **新持久化**：通信记录 = 双方原生 jsonl，不建任何新存储
- **枢纽会话特殊化**：任何会话皆可为目标，运行中的常驻会话天然是枢纽

## 2. 架构

```
┌─ Agent 会话 A（任意 provider）────────────────────┐
│  用户: "trs 把这个方案发给 codex 会话评审"          │
│  agent 调用 skill → curl http://127.0.0.1:<port>  │
└──────────────┬────────────────────────────────────┘
               │ loopback HTTP + token
┌──────────────▼────────────────────────────────────┐
│  ReSession 进程内 Bridge（Rust, ~200 行）           │
│  GET  /bridge/sessions   ← 扫描缓存 + PTY 忙闲     │
│  POST /bridge/send       ← pty_write（bracketed    │
│                            paste + 单次回车）       │
│  GET  /bridge/tail       ← 目标会话 jsonl 增量      │
└──────────────┬────────────────────────────────────┘
               │ PTY 注入 / jsonl 读取
┌──────────────▼────────────────────────────────────┐
│  目标会话（Claude Code / Codex，原生 TUI 原样运行）  │
└───────────────────────────────────────────────────┘
```

服务发现与鉴权：ReSession 启动时在 `127.0.0.1` 绑定随机端口，把
`{ "port": N, "token": "<随机>" }` 写入 `~/.resession/bridge.json`
（0600）。skill 读该文件构造请求；文件不存在 = ReSession 未运行，skill 直接报告。

## 3. API 契约

### 3.1 `GET /bridge/sessions` — 目标列表

复用扫描缓存 + PTY 状态，返回：

```json
{ "sessions": [ {
    "id": "<provider>:<uuid>",   // PTY 键同构
    "provider": "claude|codex",
    "title": "...",              // 别名 > 原生 title > summary
    "cwd": "...",
    "running": true,             // PTY 存活
    "busy": false                // 输出静默判定（现忙闲三态信号）
} ] }
```

### 3.2 `POST /bridge/send` — 投递

请求 `{ "target_id": "...", "content": "..." }`。

处理顺序：

1. 目标存在且 PTY 存活，否则 `404`
2. **忙闲守卫**：目标 busy → `409 { "busy": true, "last_output_ms": ... }`，
   由发起方 agent 决定等待重试（skill 会写明重试姿势）；不排队、不吞消息
3. bracketed paste 包裹：`ESC[200~` + content + `ESC[201~` + `\r`，
   一次 `pty_write` 完成（保多行、单次提交，与 tmux-agent core.sh 同款）
4. 返回 `{ "delivered": true, "target": {元数据} }`——回显目标元数据，
   供发起方向用户陈述"发给了谁"

### 3.3 `GET /bridge/tail` — 回读

请求 `?target_id=...&after_index=N&limit=M`（默认 after_index=-1 即最后
`limit` 条）。返回目标会话 jsonl 的结构化增量：

```json
{ "messages": [ { "index": 12, "role": "user|assistant",
                  "text": "...", "timestamp": "..." } ],
  "latest_index": 15 }
```

- 数据源是**目标会话的原生 jsonl**（Claude projects / Codex rollout），
  复用 session-core 现有解析与缓存——不刮终端屏幕，铁律不破
- 相比 tmux 版 capture-pane 的升级点：**Codex 也有转录回退了**
  （tmux 版因无 `--session-id` 只能读可见 pane，此处补齐）
- 发起方轮询姿势：记录 send 前的 `latest_index` → 间隔轮询直到出现新的
  assistant 消息或超时（超时只报 pending，不重发）

## 4. 生命周期与不变量（自 tmux-agent 移植，协议原样）

| 不变量 | 桥上的实现 |
|---|---|
| 派生 → 找 → 锁定 | list 精确匹配 id；不按 cwd/时间猜选 |
| 空闲才注入 | busy → 409；确认界面无法识别时靠忙闲兜底 |
| 一次完整投递 | 单次 send 请求带全文；发现被拆轮 → 停止，视为传输失败 |
| 超时不重发 | 只报 pending；重复执行有副作用的操作被禁止 |
| 默认只读 | 授权写在信封里（见 §5），目标回复不构成新授权 |
| 不递归转交 | 接收方处理完回传给调用侧，不再委派 |
| 共享工作区写串行 | 信封约束（v1 不做服务端强制） |
| 不传密钥 | skill 声明；桥不做内容审查 |
| 销毁仅限显式授权 | 桥本身不提供 kill；关终端走现有 ⏹ |

## 5. 消息信封（自 `plugins/tmux-agent` 原样保留）

六种信封：`PING / CONSULT / REVIEW / TASK / HANDOFF / STATUS`，授权标签
`READ_ONLY / WRITE_ALLOWED`，格式与语义见
`plugins/tmux-agent/skills/references/prompts.md`。唯一修改：
`CALLER`/`TARGET` 后缀补充会话来源标注（ReSession 会话 id），便于双方在
转录里对账。

## 6. Skill 设计（`resession-bridge`，触发词 `trs`）

- 双端安装：`~/.claude/skills/` 与 `~/.codex/skills/`（同一份 SKILL.md，
  命令示例与 tmux-agent 版同构，仅把 Helper 调用换成 bridge HTTP 调用）
- SKILL.md 骨架（移植自 tmux-agent/skills/SKILL.md）：
  1. 读 `~/.resession/bridge.json` 取 port/token（不存在 → 报告未运行）
  2. `GET /bridge/sessions` → 用户指定的接收方精确匹配（provider+标题/cwd），
     确定不了问用户，不猜
  3. **整理消息**：按用户意图选信封，填充自包含上下文（"上面/刚才"必须展开，
     目标看不到调用侧对话）
  4. `POST /bridge/send`；409 → 等待重试（上限后报 pending）
  5. 需要回传时轮询 `/bridge/tail`，按固定格式汇报：**目标完整回复 → 调用侧
     结论 → 查看入口**（"在 ReSession 中点击该会话查看"）
- 触发词与 tmux 版保持习惯：`trs <请求>`、`to claude ...` / `to codex ...`

## 7. 与 tmux-agent 的能力对照

| 能力 | tmux-agent（Mac） | ReSession 桥 | 备注 |
|---|---|---|---|
| 稳定后台会话 | tmux session | PTY 常驻 | 同构 |
| 寻址 | `tc-<项目>-<分支>` 名字 | 会话 id + 标题/cwd | ReSession 天然多项目 |
| 注入 | send-keys | pty_write + bracketed paste | 同款技术 |
| 回读 | capture-pane（刮屏幕） | **jsonl 结构化 tail** | Codex 也补上了转录回退 |
| 忙闲判定 | capture 启发式 | 输出静默 + jsonl 双信号 | 更可靠 |
| 查看入口 | `tmux attach-session` | GUI 点击会话 | GUI 优势 |
| 新建目标会话 | `start`（v1.5 再加 `POST /bridge/sessions`） | 现有 new-session 能力的暴露 | 后端已有，桥端点 trivial |
| 平台 | macOS/Linux | **跨平台**（含 Windows） | 核心差异化 |

## 8. 里程碑

- **M-B1**：Bridge HTTP 服务三端点 + `bridge.json` 鉴权（Rust，仅 src-tauri 层）
- **M-B2**：skill 双端安装包（SKILL.md + 安装说明）
- **M-B3**：双端实测（Windows：Claude↔Codex 桌面版；Mac 同），验收 =
  跨 provider 投递 + 回读 + 409 重试路径全部走通
- v1.5 候选：`start` 端点、服务端排队、GUI 投递提示 toast

## 9. 风险与开放问题

1. **bracketed paste 的 TUI 兼容性**：Claude Code / Codex 对 paste 序列的
   支持需实测（tmux 版已验证 Claude 侧；Codex `--no-alt-screen` 场景待验）
2. **jsonl 写入延迟**：send 后目标回复要等它 flush 到 jsonl，轮询节奏
   （建议 2s 起、指数退避，总超时用户可配）
3. **busy 误判窗口**：静默 ≠ 没在干活（已知近似），409 可能误拒 →
   force 标志留作逃生门（v1 先不做，报了再说）
4. **安全边界**：loopback + 本地同用户信任模型（同 Docker socket 级别），
   文档明示"同机恶意进程可读取 bridge.json"的假设
