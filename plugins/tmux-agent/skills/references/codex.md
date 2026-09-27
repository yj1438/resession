# Codex 适配

- 路由：`tcx` / `to codex` → `codex`。
- 会话名：`ta-codex-<项目名>-<分支名>`，与 Claude 隔离。项目、分支、字符编码使用公共 Helper，不手算。
- 启动：`codex --no-alt-screen --sandbox workspace-write --ask-for-approval never`。关闭 alternate screen 以保留可见历史；仅对子进程移除 NO_COLOR。
- `workspace-write + never` 允许工作区写入且不等待工具审批。只读任务仍受消息信封约束；工具权限不是写任务授权。权限失败要上报，不自动切换 `danger-full-access` 或 `--dangerously-bypass-approvals-and-sandbox`，不借另一 Agent 绕过。
- 契约：`@tmux_codex_managed=1`、`@tmux_codex_project_root`、`@tmux_codex_permission_mode=workspace-write/never`、`@tmux_codex_pane_id`。前台命令必须为 `codex`。
- 首次登录、目录信任或其它交互提示交用户 attach 处理，不自动确认。

## 完整回复取证与限制

当前适配器没有预先分配或可靠绑定 Codex 原生 session ID，`native_session_id` 输出为空是预期行为，不是 Claude 的 UUID 校验失败。tmux session name / pane ID 不等于 Codex 原生 session ID。

只从已经锁定的 pane 读取本轮可见最终回复。扩大 capture 后仍不完整，必须报告“无法取得 Codex 完整回复”，提供精确 attach 入口，不能用摘要冒充完整结果或先给调用侧结论。终端文本可能不保留原始 Markdown 排版，应如实保留可见内容，不补造格式或缺失文本。

不扫描用户历史、不选最新 rollout、不读取 Claude transcript、不使用 `codex resume --last` 猜选任务。未来如接入精确原生 ID 或结构化结果读取，需在本适配器扩展并验证，不改变公共传输策略。手动重启/切换会话后不把旧输出作为新任务证据。

## 参数依据

实现时已核对本机 CLI help。参数及恢复能力参见 [Codex CLI 官方文档](https://learn.chatgpt.com/docs/developer-commands?surface=cli)。支持这些启动参数的本地 Codex CLI 是依赖；不支持时报告错误，不自动猜测其它参数。

本地 Codex CLI 0.157.1 已完成真实 start → 多行 send → capture 验证，前台进程名为 `codex`；仅代表该安装形态的收发证据，不保证其它版本、shim 或终端环境。完整动态 eval 另行验证。
