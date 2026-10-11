# Mac 侧安装与验证清单

> 适用：ReSession v0.5.0+。本清单用于**每次新产物**的实机验收，不能用旧版本的结果代替。
> 2026-09-26 已验证从 Dock 启动后可发现 CLI、已有会话内可使用 CLI 工具；
> 2026-09-26 的独立空目录冒烟测试曾遇到新会话终端空白。首帧修复须在下一份 App 中重新验证。

## 0. Codex App 内置 CLI 路径

Mac 实机观察到两种布局：

- 旧版：`/Applications/ChatGPT.app/Contents/Resources/codex`
- 新版：`/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex`（可执行 shim）

探测逻辑还覆盖 `~/Applications` 与 `Codex.app` 同构路径；仅安装 App、没有独立
`codex` CLI 时，需要用新产物单独验证自动探测。检查本机实际路径可运行：

```bash
for app in /Applications/ChatGPT.app "$HOME/Applications/ChatGPT.app" /Applications/Codex.app "$HOME/Applications/Codex.app"; do
  for relative in Contents/Resources/codex-cli/bin/codex Contents/Resources/codex; do
    candidate="$app/$relative"
    [ -x "$candidate" ] && printf '%s\n' "$candidate"
  done
done
```

## 1. 安装与安全

- [ ] 从本项目 [Releases](https://github.com/yj1438/resession/releases) 或对应 commit 的 Actions 下载 macOS arm64 `.app.zip`，确认产物版本与 commit。
- [ ] 解压到独立位置；不要直接覆盖正在使用的 App。
- [ ] 正常启动并记录 macOS 提示。当前 CI 包未完成 Developer ID 签名与公证，下载态可能被 Gatekeeper 报为“已损坏”。
- [ ] **仅确认来源可信并接受风险时**，可临时对这一份 App 移除下载隔离标记，再启动：

  ```bash
  xattr -dr com.apple.quarantine "/path/to/ReSession.app"
  ```

  此操作不修复签名或公证；不要对整个下载目录运行，也不要修改全局安全设置。详见 [README](../README.zh-CN.md#安装)。

## 2. 新产物功能验证

- [ ] 在 Claude/Codex TUI 中按住 Option（⌥）拖选，出现选区；Cmd+C 和“复制选中内容”按钮都可复制到外部文本编辑器，中文与多行内容完整；粘贴仍可用，Ctrl+C 保留终端中断行为。

- [ ] 从 Finder/Dock 启动（不要从终端启动，以免终端 PATH 掩盖 GUI 环境问题）。
- [ ] 列表出现本机 `~/.claude/projects/` 的 Claude 会话，以及 `~/.codex/sessions/` 的 Codex 会话；项目与 Agent 标签正确。
- [ ] 中文全文搜索、命中定位和高亮正常；Claude/Codex 筛选各自只显示对应结果。
- [ ] 恢复一个已有 Claude 会话、一个已有 Codex 会话：内嵌原生 TUI 可见且能交互；会话内已安装的 `node`、`gh` 等工具可用。
- [ ] 在独立空目录分别新建 Claude、Codex 会话：首次目录信任提示或启动界面**必须可见**，不能只看到运行中进程和空白终端；快速退出时保留真实错误输出直到关闭终端视图，运行中条目则消失。
- [ ] 在仅有 Codex App 内置 CLI、没有独立 CLI 的隔离环境中验证自动探测；不要为测试去移动或删除日常使用的 CLI。
- [ ] 验证归档/恢复、移入废纸篓、双击改名及失败回滚；勿用重要会话做删除测试。
- [ ] 设置页的 Claude/Codex 路径提示、检查更新正常。

## 3. 异常记录

记录 App 版本与 build 时间、启动方式（Dock/Finder/终端）、Agent 类型、是否独立安装
CLI、界面截图，以及设置页日志目录中 `resession.log` 的相关行。日志或截图可能含有本机
路径、会话内容和凭据；对外提交前先检查并脱敏。
