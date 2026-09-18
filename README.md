# ReSession

轻量的 Claude Code 会话管理桌面客户端。左侧跨项目会话列表，右侧会话转录回放 + 内嵌真终端，恢复会话时直接运行原生 `claude --resume`，**不做任何包装层**。

> 设计哲学：Claude Code 本身已经足够好，ReSession 只做"找回会话"这一件事的 UX 层。

## 核心原则

1. **原生会话** — 只读 `~/.claude/projects/*.jsonl`，只通过 `claude --resume <id>` 恢复。不代理协议、不重渲染 TUI、不建私有会话格式
2. **轻** — Tauri 2 壳（安装包 ~10MB 级），左右两侧能力都来自现成组件（xterm.js / portable-pty / MIT 的 cc-sessions 扫描逻辑）
3. **Provider 抽象** — 所有 agent 耦合逻辑关进 `SessionProvider` 接口，v1 只实现 Claude，Codex 等后续以适配器接入，UI 零改动

## 当前状态

**M0 已完成**（详见 [docs/roadmap.md](docs/roadmap.md)）：

- `crates/session-core/` — 纯 Rust 核心库：会话扫描、JSONL → IR 解析、resume 命令构造，`cargo test` 全绿
- `src/` — 前端骨架：三区布局 + xterm.js 终端组件（PTY 桥接待 M1）
- `src-tauri/` — **待 M1**，被 MSVC Build Tools 门槛挡住（见架构文档）

## 文档

| 文档 | 内容 |
|---|---|
| [docs/design.md](docs/design.md) | 目标、非目标、设计原则、UI 布局 |
| [docs/architecture.md](docs/architecture.md) | 模块划分、SessionProvider 接口、数据流、平台矩阵 |
| [docs/data-formats.md](docs/data-formats.md) | Claude JSONL 格式笔记、IR 定义、Codex 格式占位 |
| [docs/roadmap.md](docs/roadmap.md) | 里程碑与当前进度 |

## 开发

```bash
npm install          # 前端依赖
npm run dev          # 前端开发服务器（M0 阶段用 mock 数据）

cd crates/session-core
cargo test           # 核心库测试（无需 MSVC，本地工具链即可）
```

本地 Rust 工具链（隔离安装，未动全局环境）见 `F:\git-workspace\ai\tools\`，
编译时需要设置的环境变量封装在 `tools\build-cc-sessions.cmd` 可参考。

## 目录结构

```
resession/
├── crates/session-core/   # 纯 Rust 核心：Provider trait、IR、Claude 适配器
├── src/                   # React + TS 前端（Tauri WebView）
├── docs/                  # 设计文档
└── src-tauri/             # (M1) Tauri 壳 + portable-pty 桥
```

## License

MIT
