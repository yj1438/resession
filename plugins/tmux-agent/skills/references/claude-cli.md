# Claude Code 适配

- 路由：`tcc` / `to claude` → `claude`。
- 会话名：`tc-<项目名>-<分支名>`，保持 v1 兼容；项目取 Git 根目录名，无分支或 detached HEAD 用 main。`%`、`.`、`:` 依次编码 `%25`、`%2E`、`%3A`，必须由 Helper 计算。
- 启动：Helper 生成 UUID，使用 `claude --permission-mode bypassPermissions --session-id <uuid>`，仅对新子进程移除 NO_COLOR。
- 契约：保留 `@tmux_claude_managed`、`@tmux_claude_project_root`、`@tmux_claude_permission_mode` 和 `@tmux_claude_session_id`。新会话额外记录 `@tmux_claude_pane_id`。
- `bypassPermissions` 避免工具确认卡住，不扩大用户授权。首次启动仍可能出现确认或登录界面，应呈现实际提示让用户 attach，不自动确认。
- 前台命令必须为 `claude`。已有普通权限或 UUID 缺失/非法的会话拒绝 start/send；仍可 capture/status/attach，不自动销毁或升级。

## 完整回复取证

capture 不完整时先扩大 capture；仍不足时，使用 `@tmux_claude_session_id` 精确定位同名 `<uuid>.jsonl`，只提取本轮最后一条 assistant 的可见 `text`，不读取 thinking、tool use 或其它消息。不得按 cwd 最新文件、修改时间或内容相似度猜测。

UUID 只对 Helper 启动的原生会话有效；用户手动退出/重启 CLI 或切换原生会话后必须停止使用旧 metadata。需由用户明确授权后销毁并用 Helper 重建，不能自动操作。UUID 缺失、非法或精确文件不存在时只使用 capture，或明确报告无法取得全文。
