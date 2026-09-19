# Roadmap

原则：每一步都可独立验证；唯一的硬技术风险（Windows PTY + 原生 TUI）最早消除。

## M0 — 骨架与核心库（✅ 2026-09-19 完成）

- [x] 仓库初始化、文档四件套（design / architecture / data-formats / roadmap）
- [x] `crates/session-core`：SessionProvider trait + IR v0
- [x] Claude 适配器：扫描 `~/.claude/projects/`、JSONL 宽容解析、resume 命令构造
- [x] 单元测试（fixture 样本）+ 真实目录集成测试，本地 gnu 工具链全绿
- [x] 前端骨架：三区布局 + xterm.js 终端组件（未接 PTY，显示占位信息）

## M1 — Tauri 壳与原生恢复（下一步）

**前置门槛：安装 MSVC Build Tools（约 3GB，可卸载）**，随后 Rust 增加
`x86_64-pc-windows-msvc` target。

- [ ] `src-tauri` crate：Tauri 2 最小窗口加载现有前端
- [ ] invoke 命令：`scan_sessions` / `load_transcript`（桥接 session-core）
- [ ] `pty.rs`：portable-pty 打开/读写/resize/关闭，事件桥到 xterm.js
- [ ] 选中会话 → 终端运行 `claude --resume <id>`，**真机验证原生 TUI 完整可用**
- [ ] Sidebar 换真实数据，替换 mock

验收标准：在 ReSession 里恢复一个旧会话并继续对话，体验与直接开终端无异。

## M1.5 — 日用打磨（✅ 2026-09-19 完成）

- [x] 原生 /rename 标题解析（custom-title 行）；sidechain 折叠（方案 b）
- [x] 双轨命名：`~/.resession/settings.json` 别名，优先级 别名 > 原生 > summary > 首条消息
- [x] 单视图状态机：转录（历史态）↔ 终端（活跃态），"▶ 恢复会话"显式动作
- [x] 转录 markdown 渲染（react-markdown + gfm + highlight）；工具调用 details 折叠
- [x] 忙闲三态感知（输出时间戳近似，4s 阈值；静默长任务会误判为空闲，已知限制）
- [x] 扫描 mtime+size 增量缓存

## M2 — 回放与搜索体验

- [x] 转录富文本渲染：markdown、代码高亮、工具调用折叠（M1.5 #4）
- [x] 全文检索：正文搜索 + 结果视图 + 转录高亮（v1，滚动定位 v2）
- [ ] 虚拟滚动（大会话文件）
- [x] 设置页 v1：claude 路径覆盖、忙闲阈值、别名管理（主题后续）
- [ ] 系统终端兜底：在 Windows Terminal 里打开同一 resume 命令
- [ ] 正式安装包：nsis + 图标 + 静态 CRT
- [x] DTO 序列化契约测试：session-core 集成测试 + src-tauri 各 DTO 单测
      （key 列表断言 + 递归蛇形命名检查；注意 serde_json::Value 的 key 是字母序，断言排序后比较）

### M1.x 已提前消化（2026-09-19）

- [x] PTY 常驻与并行多会话（M1.1，提前自 M2"会话保活"）
- [x] 终端全彩（FORCE_COLOR=3，见 architecture.md 3.3 ConPTY 笔记）
- [x] 字节级输出传输（中文跨块乱码修复）

## M3 — 扩展

- [ ] 会话备注/置顶（存 ReSession 自己的配置，不污染 Claude 数据）
- [ ] fork 浏览（forkedFrom 树）
- [ ] Codex Provider 适配器（`~/.codex/sessions/` 实测后接入）
- [ ] 跨机器同步（参考 cc-sessions remotes 设计）
