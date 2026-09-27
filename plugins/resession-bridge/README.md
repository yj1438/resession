# resession-bridge

经 ReSession 桥（loopback HTTP）与本地 Claude Code / Codex 会话互发消息的 skill。
协议沿袭 [`plugins/tmux-agent`](../tmux-agent)（信封、生命周期、安全不变量一致），
传输层不同：tmux send-keys/capture-pane → ReSession PTY 注入 + 原生 jsonl 回读。

## 前置

- ReSession ≥ v0.6.0 且**正在运行**（桥随 app 启动，监听 `127.0.0.1` 随机端口）
- 服务发现文件：`~/.resession/bridge.json`（app 启动时写入 port + token，0600）

## 安装 skill

```bash
# Claude Code
cp -r plugins/resession-bridge/skills ~/.claude/skills/resession-bridge

# Codex
cp -r plugins/resession-bridge/skills ~/.codex/skills/resession-bridge
```

## 用法

```text
trs 评审一下当前未提交的变更        # 目标由上下文推断
trs to codex 按 <方案> 实现        # 可选：to <agent> 显式指定目标
```

`tcc`/`tcx` 属于 tmux-agent；两插件同时安装时本 skill 不响应它们。
完整流程（路由/信封/轮询/回传格式）见 [skills/SKILL.md](skills/SKILL.md)。

## 与 tmux-agent 的关系

| | tmux-agent | resession-bridge |
|---|---|---|
| 平台 | macOS / Linux（需 tmux） | Windows / macOS / Linux（需 ReSession 运行） |
| 注入 | `tmux send-keys` | `POST /bridge/send`（bracketed paste） |
| 回读 | `tmux capture-pane`（刮屏幕） | `GET /bridge/tail`（目标原生 jsonl，结构化） |
| 目标寻址 | tmux 会话名 | ReSession 会话 id |

两者协议一致、可共存；选哪个取决于你偏好 tmux 还是 ReSession 做传输层。
