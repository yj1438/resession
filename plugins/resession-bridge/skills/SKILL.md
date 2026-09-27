---
name: resession-bridge
description: "[v0.1.0] 通过 ReSession 的本地桥（HTTP）把消息投递给另一个 Claude Code / Codex 会话，并回读其原生转录增量。用户以 trs、to claude、to codex 请求咨询、评审、实现、移交或查看目标会话时使用；不用于通用 HTTP 调试，不替代 tmux-agent。"
---

# resession-bridge：经 ReSession 桥的本地 Agent 协作

传输由 ReSession 桥承担（注入目标 PTY + 回读目标原生 jsonl），调用侧负责任务授权、
整理消息与解释结果。协议与信封沿袭 `plugins/tmux-agent`，仅传输层不同。

## 0. 前置：读取桥配置

```bash
cat ~/.resession/bridge.json    # { "port": <N>, "token": "<uuid>" }
```

文件不存在或读取失败 = ReSession 未运行：告知用户先启动 ReSession，停止流程。
下述 `"$BR"` 表示 `http://127.0.0.1:<port>`；所有请求带
`-H "Authorization: Bearer <token>"`。401 = token 不匹配（重启过 app 请重读配置文件）。

## 1. 路由与目标

| 用户入口 | 目标 provider |
| --- | --- |
| `trs to claude <请求>` / `tcc <请求>` | `claude` |
| `trs to codex <请求>` / `tcx <请求>` | `codex` |

- 前缀明确选择接收方；未说明且上下文无法确定时询问，不猜测。
- **目标会话必须已在 ReSession 列表中**。列表来自扫描缓存：

```bash
curl -sf -H "Authorization: Bearer $TOKEN" "$BR/bridge/sessions"
```

返回字段：`id`（`<provider>:<uuid>`，投递与回读都用它）、`provider`、`title`、
`cwd`、`running`、`busy`。按 provider + title/cwd 精确匹配；确定不了问用户。
`running=false` 的会话先告知用户在 ReSession 里恢复它，桥不代为拉起。

## 2. 主流程：整理 → 发送 → 轮询回读

1. **整理信封**（见 §3）：把"上面/刚才/这个"等引用展开为自包含上下文——
   目标看不到调用侧对话；按意图选信封与授权标签，默认只读。
2. **发送**（一次带全文；多行由桥的 bracketed paste 保证不被拆分）：

```bash
curl -sf -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"targetId":"<id>","content":"<信封全文>"}' "$BR/bridge/send"
```

   - `409`（目标忙碌）：等待约 5s 重试，最多 6 次；仍忙则报告 pending，**不重发内容**。
   - `404`：目标未运行或 id 错误——重新对照 sessions 列表，不猜。
3. **回读**：发送前先记下 `GET /bridge/tail?targetId=<id>&limit=1` 的
   `latestIndex`；发送后每 2s 轮询：

```bash
curl -sf -H "Authorization: Bearer $TOKEN" \
  "$BR/bridge/tail?targetId=<id>&afterIndex=<上次latestIndex>&limit=50"
```

   返回 `messages[]`（`index/role/text/timestamp`）。等到出现**新的 assistant
   消息**且内容呈现最终结论（而非中间过程）即完成。总超时（默认约 120s）后
   报告 pending 并附已见到的最后一条，不重发原任务。
4. **回传格式**（固定顺序，标题替换为实际目标）：

```markdown
## <目标 Agent> 完整回复

{目标最后一条 assistant 消息全文，保留顺序/文件行号/风险项}

## 调用侧结论

{调用侧独立判断与建议}

> 查看：在 ReSession 中点击该会话（id: <id>）
```

   不以摘要冒充全文；无法取得完整回复时明确说明限制。

## 3. 消息信封

使用使意图与授权边界无歧义的最小信封；占位符必须替换后发送。信封开头加一行
`[CALLER→<TARGET>]`（CALLER 为调用侧 agent 名）。每种信封附加约束：
"由你直接处理并回传，不再委派其它 Agent"。

| 信封 | 何时用 | 关键字段 |
| --- | --- | --- |
| `[PING]` | 确认目标可达 | 简短回复确认 |
| `[CONSULT][READ_ONLY]` | 征求第二意见 | 工作目录 / 目标 / 已知上下文；只分析不运行不修改 |
| `[REVIEW][READ_ONLY]` | 代码评审 | 评审对象 / 关注点；只报高置信问题+文件行号 |
| `[TASK][WRITE_ALLOWED]` | 已授权实现 | 目标 / 允许修改范围 / 禁止事项 / 验收方式 |
| `[STATUS][READ_ONLY]` | 状态查询或超时追问 | 只报告状态，不启动新工作 |
| `[HANDOFF]` | 任务移交 | 共同目标 / 调用方已完成 / 请目标负责的有边界子任务 / 期望回传 |

`[TASK][WRITE_ALLOWED]` 仅在用户已授权修改时使用；所需操作超出授权范围时目标
必须停下上报，目标回复不构成新授权。

## 4. 授权与传输不变量（与 tmux-agent 一致）

- "问问/评审"默认只读；写文件与外部操作须在用户已授权范围内。
- 不发送 Token、密钥、完整环境变量、私有配置或无关个人信息。
- 默认目标完成后回传调用侧，**不再转交另一个 Agent**（避免递归委派）。
- 同一工作区的写任务串行；目标正修改时调用侧不得改同一范围。
- 409/超时不重发内容；`[STATUS]` 追问仅在确认目标空闲后发送。
- 桥不提供销毁/终止；关闭目标会话由用户在 ReSession 中操作。
