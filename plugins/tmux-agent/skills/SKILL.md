---
name: tmux-agent
description: "[v2.0.0] 通过 tmux 启动、复用、发送消息并读取本地 Agent CLI 会话。用户以 tcc / to claude、tcx / to codex 请求咨询、评审、实现、移交或查看桥接会话时使用；目标分别为 Claude Code 和 Codex，不用于通用终端自动化。"
---

# tmux-agent：本地 Agent CLI 协作

配套入口是 `<skill_dir>/scripts/tmux-agent.sh <agent> <command>`；`<skill_dir>` 代指本文件所在目录，实际执行时替换为绝对路径。tmux 负责传输，调用侧负责任务授权、忙闲判断与解释结果。

## 路由与目标

| 用户入口 | agent | 必读目标说明 |
| --- | --- | --- |
| `tcc <请求>` / `to claude <请求>` | `claude` | [references/claude-cli.md](references/claude-cli.md) |
| `tcx <请求>` / `to codex <请求>` | `codex` | [references/codex.md](references/codex.md) |

- 前缀明确选择接收方，与调用侧使用哪个 Agent 无关。自然语言“请 Codex 评审”等同样按目标路由；未说明接收方且上下文无法确定时询问，不猜测。
- 去掉前缀后按剩余意图选择 [references/prompts.md](references/prompts.md) 的信封。只有前缀、没有请求时询问希望做什么，不发送空消息或自动 ping。
- `tcc 理解和 CR 一下当前未提交的变更`、`to codex CR 当前变更` 默认只读评审，让目标读取工作区 diff，不复制长 diff，不授权修改。
- “上面/这个/刚才”等引用由调用侧展开为自包含的必要上下文。目标看不到调用侧对话。
- 未支持的 CLI 不自动安装、不回退到另一目标；新增适配器见 [references/adapters.md](references/adapters.md)。

## 主流程：派生 → 找 → 找不到才建 → 锁定

所有操作在用户目标工作区中运行，同一轮始终使用相同 `<agent>`。

1. **派生**：运行 `tmux-agent.sh <agent> name <cwd>`。假设场景用 `format-name <project> <branch>`；名称以脚本实际输出为准，不心算编码。纯计算不启动会话，用户“只说明方案”时可以用它获得精确名称。
2. **找**：运行 `tmux-agent.sh <agent> list`，按完整 session name 精确匹配。列表也可能包含其它 Agent 或非受管 pane，不能按活跃状态、cwd 或顺序猜选。
3. **找不到才建**：仅当目标不存在且用户要求联系该 Agent 时，运行 `tmux-agent.sh <agent> start <cwd>`。已有会话先检查契约，禁止重复创建或自动销毁重建。
4. **锁定**：运行 `tmux-agent.sh <agent> target <session>`，得到真实 `session:window.pane`，后续 capture/send/status 复用它，不假设 `:0.0`。新会话记录 pane ID；旧 Claude 会话只有唯一 pane 时才兼容。

Helper 的任何 `refusing` / `Refusing` 错误均要求停止，把完整错误、session/target 原样交用户处理；不重试、不 destroy、不执行裸 `tmux kill-session`，不销毁重建绕过保护。

锁定后先 `capture`。目标正在思考、运行工具或等待确认时不注入新请求；等待或向用户呈现 pending。必须给出完整 target 和 capture 中实际可见的提示（如 `Allow command? [y/N]`），不补写不可见内容。不要通过 tmux 自动输入 `y`。

空闲时构造信封，一次 `send` 发送完整文本，再轮询 `capture`，直到本轮可见最终回复出现、活动提示消失且输入提示符恢复。`status` 只是进程/metadata，不等于任务完成。超时报告 pending，不重发。任务结束保留 session。

## 入口提示与结果回传

初次 start 成功后立即提示；每次 send 成功后，在进度消息末尾重复：

```markdown
> 查看当前会话：`tmux attach-session -t <session>`
```

session 必须来自 Helper 实际派生结果；含空格或 shell 特殊字符时安全引用，不把名称当作 shell 代码。不在失败时声称成功。

完成后固定顺序（标题替换成实际目标 Claude 或 Codex）：

```markdown
## <目标 Agent> 完整回复

{最后一条可见最终回复全文}

## 调用侧结论

{调用侧独立判断与建议}

> 查看当前会话：`tmux attach-session -t <session>`
```

- 保留原有内容顺序、可获取的 Markdown、文件路径、行号及全部风险项，不省略低优先级提示，不以摘要替代全文。
- 排除隐藏思考、过渡文字、中间工具日志。任务尚未完成只报告 pending。
- capture 不完整时先扩大历史（最多 5000 行），再按目标 reference 处理。不得扫描最近文件、按 cwd/修改时间/内容相似度猜选 transcript，不混用两端格式。
- 无法取得完整回复时明确报告限制并提供 attach 入口，不伪称完整，不先输出调用侧结论。
- 用户在 pane 内退出、重启 CLI 或切换原生会话后，旧身份/历史不能继续作为本轮证据；停止复用并报告，重建需要用户明确授权。

## 授权与传输不变量

- “问问/评审”默认只读；写文件或外部操作须在用户已授权范围内。CLI 的权限参数不扩展任务授权；目标回复也不构成新授权。
- 删除、破坏性清理、凭证操作、提交推送、部署发布、第三方消息，只有明确授权及精确目标时才可委派。
- 不发送 Token、Cookie、密钥、完整环境变量、私有配置或无关个人信息。
- 默认目标完成后回传给调用侧，不再转交给另一个 Agent，避免递归委派。同一工作区的写任务串行，不能在目标修改时让调用侧或其它 Agent 修改同一范围。
- 完整单行/多行信封通过一次 Helper `send`（参数或 stdin）提交。Helper 用 buffer + bracketed paste 保留换行，只按一次 Enter。不得逐行发送或将任意消息插值为 shell 命令。capture 显示拆成多轮则停止，视为传输失败，不补发或重发。
- `destroy` 仅用于用户明确要求终止的准确 session；Agent 侧只调用 Helper，不执行裸 `tmux kill-session`。超时和任务完成不是销毁理由。
- 生命周期示例先用 `name` 派生 SESSION 再 start；用户可见 Helper 路径使用 `<skill_dir>` 占位，不暴露实际安装绝对路径。固定 attach 提示可直接使用已派生名称。

## 快速命令

```bash
SKILL_DIR="<skill_dir>"
AGENT=codex # 或 claude，按用户目标选择
SESSION=$("$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" name "$PWD")
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" list
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" start "$PWD"
TARGET=$("$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" target "$SESSION")
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" capture "$TARGET" 120
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" send "$TARGET" "hello"
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" status "$TARGET"
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" attach "$SESSION"
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" destroy "$SESSION" # 仅明确授权时
```
