# Mac 侧验证与安装 TODO

> 适用：ReSession v0.5.0+（Codex Provider 之后）。
> Windows 侧代码已全部完成并推送，以下是需要在 Mac 上执行的部分。
> 完成一项就在 `[ ]` 里打 `x`，异常按 §3 上报。

## 0. 路径发现（1 分钟，输出决定代码收敛）

在 Mac 终端执行，把**全部输出**贴回会话：

```bash
ls -d ~/Library/"Application Support"/OpenAI/Codex/bin/* 2>/dev/null
ls -d ~/Library/"Application Support"/Codex/bin/* 2>/dev/null
find /Applications/Codex.app ~/Applications/Codex.app -maxdepth 4 -name "codex*" 2>/dev/null
which codex; command -v codex
echo $SHELL
```

用途：固化 `desktop_app_bases()` 的 macOS 候选（当前是带/不带 `OpenAI`
段的两种猜测）。

## 1. 安装最新版

- [ ] GitHub Actions 下载最新 run 的 `ReSession-macos-arm64-app`（或 Release 页资产）
- [ ] 解压 → `ReSession.app` 拖入「应用程序」
- [ ] `xattr -cr /Applications/ReSession.app`（未签名解隔离）
- [ ] 启动

## 2. 功能验证清单

- [ ] 会话列表出现 Mac 本地 `~/.claude/projects/` 的 Claude 会话
- [ ] Codex 会话出现在列表（带 provider 标识；依赖 `~/.codex/sessions/` 有数据）
- [ ] 全文搜索中文词 + 命中滚动定位 + 黄色闪烁
- [ ] **从 Dock/Finder 启动**（不要从终端启动！否则 PATH 完整、测不出问题）：
  - [ ] 恢复一个 Claude 会话成功
  - [ ] 内嵌终端里 `node -v`、`gh --version`、`which codex` 都能找到
        （这是登录 shell 环境解析的验收，本次修复的核心）
  - [ ] 恢复一个 Codex 会话（`codex resume` 原生 TUI 正常）
- [ ] ➕ 新建会话（Claude / Codex 各一个）
- [ ] 归档、删除（进废纸篓）、双击改名
- [ ] 设置页：Claude/Codex 路径占位符合理、检查更新正常

## 3. 异常上报

设置页 ⚙ → 打开日志目录 → `resession.log` **尾部 50 行** + 现象描述 +
「从哪里启动的 app（Dock/终端）」一起回传。
