# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 硬性约定

- **不要自行 `git commit` 或 `git push`。** 完成修改后汇报变更内容，等用户明确说"提交/commit"再 commit、说"推送/push"再 push——用户迭代中的文件（图标、文档等）尤其不能提前定进历史。Rust 侧本机没有工具链，编译验证依赖 CI，因此更要避免未授权推送制造红 CI。
- 提交信息用英文 conventional commits（`feat:` / `fix:` / `docs:` / `chore:` / `ui:`）；代码注释、文档、UI 文案用中文。
- 提交前检查 `package-lock.json` 的 diff：本机 npm 配置了内网 registry（antgroup-inc.cn），重新安装依赖会把 `resolved` 字段污染成内网源，CI 拉不到。还原后只提交功能文件。

## 常用命令

```bash
npm install                    # 前端依赖
npm run dev                    # 浏览器 mock 模式（isTauri=false，走 MOCK 数据）
npm run tauri dev              # 完整应用（需要 Rust 工具链，本机没有）
npm run build                  # 前端构建 = tsc && vite build
npx tsc --noEmit               # 仅类型检查
npm run tauri icon <png>       # 从 1024×1024 PNG 生成全平台图标到 src-tauri/icons/

cargo test                     # 在 crates/session-core 与 src-tauri 两个目录分别执行（无工作区合并）
cargo test <test_name>         # 跑单个测试
```

CI（.github/workflows/build.yml）= session-core 测试 + src-tauri 测试 + `npm ci` + `npm run tauri build -- --no-bundle`；`v*` 标签触发 GitHub Release 并附双平台产物。

## 架构（大图）

三层结构：**React 19 前端（src/）→ Tauri 命令层（src-tauri/）→ 会话核心库（crates/session-core/）**。

### crates/session-core —— 唯一的 agent 耦合点

- `provider.rs` 定义 `SessionProvider` trait（scan / load_transcript / search / resume_command / new_session_command）。**UI 与 Tauri 命令层只认这个 trait**，不许直接接触 `~/.claude` 或写死 CLI 命令。新增 agent = 新增 `providers/<name>/` 适配器。
- `ir.rs` 是统一 IR；Rust DTO **必须 camelCase 序列化**（`serde(rename_all = "camelCase")`），前端 `src/types.ts` 是逐字段镜像。契约由 `tests/dto_contracts.rs` + 各文件内联测试把关。改 DTO 三步：Rust 加字段 → 补契约断言 → 改 types.ts。
- 解析纪律：JSONL **宽容解析**——坏行跳过、缺字段降级、未知类型忽略；单文件失败跳过、单 provider 失败不拖垮整体（全部 provider 失败且零结果才向上抛）。`mtime+len` 两级缓存（元数据/可搜索文本）让 15s 重扫廉价。
- `platform.rs`：macOS Dock 启动 PATH 残缺的对策（`extra_bin_dirs` + 登录 shell 环境解析）。登录 shell 解析有 3s 超时 kill，结果进程级缓存（失败也不重试，lib.rs `run()` 启动时后台预热）。

### src-tauri —— 命令层与 PTY 生命周期

- `pty.rs`：PTY 以「会话 id」为键常驻，自然退出时读线程收割子进程并从列表移除，`close()` 处理显式关闭。**不解析、不记录终端内容**（architecture.md 3.3）。切走再切回靠回放缓冲 + `pty-out` 事件的 offset 去重（快照要么整体在事件流内要么整体补写）；快速退出的输出由 finished 快照兜底。锁序：map→buffer→child，勿引入反向嵌套。
- 执行中判定（busy）：三个信号——输出新鲜度（busyMs 阈值）+ **回显排除**（输出晚于末次写入 `ECHO_GRACE_MS` 才算真实响应）+ **年轻子进程**（存活 <60s 的子进程 = 工具调用；MCP server 等常驻子进程是老进程不算，否则空闲永远误报忙）。Windows 无子进程探测，退回输出阈值。
- `lib.rs`：`scan_sessions` 15s 重扫并顺带清洗孤儿别名/归档 key（`prune_orphan_keys`，只动扫描成功的 provider）；`delete_session` 把 JSONL 移入系统废纸篓（trash crate）并清理残留 key，运行中守卫拒绝删除。
- 配置纪律（design.md）：**原生会话数据只读**。ReSession 自己的显式操作（别名、归档、设置）只写 `~/.resession/settings.json`，key 格式 `provider:<uuid>`。

### src/ 前端要点

- `paths.ts` `normalizePath`：路径**比较/分组**用（Windows+macOS 大小写折叠，Linux 不折）；展示一律用原始路径。
- `Toasts.tsx` `reportError(title, detail)`：全局 invoke 失败统一右下角 toast（去重防 15s 扫描刷屏）；上下文内能就地显示的错误（TerminalPane/SettingsPanel/TranscriptPane）不进 toast。
- `Sidebar.tsx`：项目分组（key = provider + normalizePath(cwd||projectDir)）；搜索/点击定位走 `locateRequest{ id, sequence }` 显式请求 + `pendingLocateId` 布局效应滚动，15s 重扫不会重置折叠或滚动位置。
- 前端节奏：15s 重扫 sessions、3s 轮询 pty_list、2s tick 驱动忙闲重算——改任何"实时状态"逻辑先想清楚落在哪个节拍上。

## 发版流程（vX.Y.Z）

版本语义按 semver：feat → 次版本，fix → 修订号。全程两处需要用户确认：commit 入库前、每次 push 前。

1. **质量检查**：本地 `npx tsc --noEmit && npm run build`；有 Rust 环境时两个目录各跑 `cargo test`。本机没有 Rust 时，先把 main 推上去等 build workflow 绿——**CI 绿是打 tag 的硬前置**，tag 会直接触发 Release 产物。
2. **版本号四处对齐**：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`crates/session-core/Cargo.toml`，并同步两个 `Cargo.lock`（src-tauri 与 crates/session-core 各一个）。这是历史漂移重灾区，tag 前逐一核对。
3. **CHANGELOG 起草 + README 与文档检查**：在 `CHANGELOG.md` 的 `Unreleased` 段写好本版要点（随发版 commit 入库）；EN/ZH 两份 README **对称**更新（功能清单、「当前状态」、安装说明、`resession.png` 是否反映当前 UI）；`docs/roadmap.md` 顶部的「当前开发版本 / 最近发布版本」行（第二个漂移重灾区）；顺手清理工作区里的临时文件（`_patch_*.py`、`app-icon copy.png` 之类），别让它们混进发版 commit。
4. **commit + push（两步推送）**：发版 commit 入库 → push main → 等 CI 绿 → 再单独 `git push origin vX.Y.Z`。**不要 main 和 tag 一起推**：tag 先于 CI 结果会放出未经验证的 Release。
5. **跟踪 Actions**：`gh` 可用则 `gh run watch`；否则用 GitHub API 轮询（匿名有限流，脚本变量名避开 `status` 这类 zsh 保留字）。
6. **Release 验证**：把 `CHANGELOG.md` 对应段落贴进 Release notes 头部（`generate_release_notes` 的 compare 链接会自动附在尾部）；产物齐全（windows-x64.exe / macos-arm64 / .app.zip）、Release notes 正确；最好下载实际启动冒烟一遍。产物有问题时删除远端 tag 修复后重推（`git push origin :refs/tags/vX.Y.Z`），或放弃该版本号顺延。

## 测试惯例

- 契约测试优先：DTO key 必须 camelCase（`settings.rs` / `pty.rs` / `dto_contracts.rs` 各有断言）。
- fixture 用 `std::env::temp_dir()` + 唯一目录名 + 用后清理（见 `claude/parse.rs` tests）；不因测试写真实 `~/.claude` 或 `~/.resession`。
- 边界测试覆盖：坏 JSONL、非 UTF-8 行、缺字段、目录失效、二进制缺失、`İ` 类大小写扩张导致的切片越界。

## 文档

`docs/` 是本仓库的设计文档体系（design / architecture / data-formats / roadmap / benchmarks，中文）。动手前先读对应章节；改完代码顺手更新 roadmap 勾选状态。data-formats.md 是实测观测格式笔记，新增 provider 时先补格式调研再写解析。
