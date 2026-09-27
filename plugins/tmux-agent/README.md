# tmux-agent

通过 tmux 与本地 Claude Code、Codex CLI 协作。调用方可以是任一 Agent；用户明确选择接收方，同一项目分支复用对应目标会话。

```text
tcc 理解和 CR 一下当前未提交的变更
to claude 你觉得上面的方案怎么样
tcx CR 一下当前变更
to codex 按上面的方案实现，修改范围限于 src/cache
```

调用侧会展开“上面”等上下文，按咨询、只读评审、授权实现、状态或移交生成消息。默认接收方完成后回传，不递归转交；共享工作区写任务串行。

## 命令与会话

```bash
SKILL_DIR="<skill_dir>" # tmux-agent 的 skills 目录
AGENT=codex            # 或 claude
SESSION=$("$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" name "$PWD")
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" list
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" start "$PWD"
TARGET=$("$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" target "$SESSION")
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" capture "$TARGET" 120
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" send "$TARGET" "请只读评审当前未提交变更"
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" status "$TARGET"
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" attach "$SESSION"
```

多行消息可通过 stdin 交给一次 `send`，Helper 使用 bracketed paste 并只提交一次。名称、项目边界、归属和传输共用公共层，启动及 CLI 差异由适配器实现。`list` 显示 tmux 全部 pane，managed/权限/原生 ID 字段按所选适配器展示，必须按完整名称定位。

| 目标 | 入口 | 稳定名称 | 启动与读取规则 |
| --- | --- | --- | --- |
| Claude Code | `tcc` / `to claude` | `tc-<项目>-<分支>` | [Claude 适配](skills/references/claude-cli.md) |
| Codex | `tcx` / `to codex` | `ta-codex-<项目>-<分支>` | [Codex 适配](skills/references/codex.md) |

项目名取 Git 根目录名，非 Git 目录取规范化 cwd；无分支或 detached HEAD 用 main。`%`、`.`、`:` 依次编码为 `%25`、`%2E`、`%3A`。真实名称必须通过 Helper 得到，假设场景用 `format-name <project> <branch>`，不手算。不同 checkout 的同名项目/分支发生名称碰撞时拒绝接管，不自动换用其它会话。

## 结果与可见入口

初次成功创建及每次成功发送后，调用侧提供可复制的 `tmux attach-session -t <session>` 入口。任务完成先展示“Claude 完整回复”或“Codex 完整回复”，再展示“调用侧结论”，最后重复 attach 入口。

完整回复保留可见内容顺序、文件/行号及全部风险项，排除隐藏思考和中间工具日志。capture 不完整时扩大历史并依目标规则取证。**Codex 当前只读取锁定 pane 的可见内容，没有原生 session ID/transcript 回退能力**；无法取得全文会报告限制，不把摘要冒充完整结果。CLI 手动重启或切换会话后停止信任原身份及历史。

## 授权与恢复

- 咨询/评审默认只读；写入及外部操作遵守用户授权。CLI 的工具权限设置不扩大任务范围。
- 发送前核验目标进程、managed 标记、项目根、权限契约及 pane 归属。旧 Claude 会话多 pane 无法确定时拒绝猜选。
- 复用或锁定会话时拒绝 dead pane；已知 target 仍可 capture/status 查看退出信息。attach/destroy 先解析并检查非空 session ID，目标消失时失败，不回退到其它会话。
- 忙碌或确认界面不注入消息，不自动输入 y；显示精确 target 和可见提示交用户处理。超时不重发。
- Helper 返回拒绝接管错误时停止，报告完整错误，不通过销毁重建绕过。
- 新建会话会短暂检查 pane 存活；CLI 立即退出或初始化失败时返回含 agent/session 的 `failed to start` 错误，不报告 started、不自动重试或销毁。存活检查不代表登录完成或输入就绪，仍需先 capture。
- 不传递密钥、Token、Cookie、完整环境变量等无关敏感信息。

Agent 仅在用户明确要求终止准确 session 时调用：

```bash
"$SKILL_DIR/scripts/tmux-agent.sh" "$AGENT" destroy "$SESSION"
```

裸 `tmux kill-session -t "$SESSION"` 仅供用户手工管理，Agent 不执行。任务完成和超时不自动销毁。

## 从 tmux-claude 迁移

2.0.0 将 Plugin 正式改名为 `tmux-agent`。通过原安装渠道移除旧 `tmux-claude` Plugin，再安装新 `tmux-agent`；Marketplace 的名称变化不保证自动替换。兼容 npm 安装同样选择新名称，并按原渠道移除旧 Skill，避免重复触发。迁移不需要销毁已有 tmux 会话。

- 保留 `tcc` 入口、Claude `tc-` 名称及 `@tmux_claude_*` metadata。
- 新 Plugin 内 `scripts/tmux-claude.sh` 是兼容包装，等价于 `scripts/tmux-agent.sh claude`。外部脚本仍需把旧安装目录路径改到新 Plugin，仓库不保留第二份 Plugin 源。
- 普通权限模式、UUID 缺失/非法等旧会话仍会拒绝 start/send，不静默升级；可 capture/status/attach，由用户决定是否重建。
- 旧版评测不代表 2.0.0 已通过动态门禁；需以新版本的验证结果为准。

## 依赖与扩展

依赖 Bash、tmux 3.2+ 和所选目标 CLI 及其正常登录状态。Claude 需支持其目标 reference 的启动参数，以及 uuidgen、Linux `/proc/sys/kernel/random/uuid`、Python 3 三者之一生成 UUID；Codex 不依赖 UUID 生成器。`name` / `format-name` 不启动 tmux，纯计算不要求安装 tmux。

目录提供 Claude Code 和 Codex 两端的 Plugin manifest；当前仓库尚无 Marketplace 索引，发布到 Marketplace 前需另行配置。兼容安装可将 `plugins/tmux-agent/skills` 安装到两端对应的 `tmux-agent` Skill 目录。新增其它 CLI 参考 [适配器契约](skills/references/adapters.md)，公共生命周期只维护一份。

确定性回归：`python3 plugins/tmux-agent/tests/test_bridge.py`，使用独立 tmux socket、临时 HOME 和本地假 CLI（仅模拟进程身份，真实验证 tmux 传输），不调用模型或用户会话；测试依赖 Python 3 和 tmux。行为用例位于 `evals/evals.json`，动态门禁另以原生 eval 结果为准。
