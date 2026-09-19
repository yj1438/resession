# 构建与打包指南

> 本文记录 ReSession 的构建体系，含 2026-09-19 M1 阶段踩坑实录。
> 遇到"窗口白屏 / localhost 拒绝连接"先来查这里。

## 1. 环境前置

| 组件 | 用途 | 备注 |
|---|---|---|
| Node.js ≥ 20 + npm | 前端构建、tauri CLI | — |
| Rust `x86_64-pc-windows-msvc` 工具链 | Rust 侧编译 | 本机隔离装在 `F:\git-workspace\ai\tools\rust`，仓库根 `rust-toolchain.toml` 自动选用 |
| VS Build Tools（VC.Tools.x86.x64 + Windows11SDK.22621） | 提供 `link.exe` 与 Windows SDK 库 | **仅编译期需要**，运行产物不依赖它；rustc 自动探测，无需 vcvars |
| WebView2 Runtime | 运行期渲染 | Windows 11 系统自带 |

编译环境变量（本机，工具链隔离在 tools/，未动全局 PATH）：

```bash
export RUSTUP_HOME='F:\git-workspace\ai\tools\rust\rustup'
export CARGO_HOME='F:\git-workspace\ai\tools\rust\cargo'
export PATH="/f/git-workspace/ai/tools/rust/cargo/bin:$PATH"
```

## 2. 三种构建模式

```bash
npm run tauri dev              # ① 开发模式：自动起 vite:5173 + 热重载（日常开发用这个）
npm run dev                    # ② 纯前端：浏览器直开，mock 数据，无后端
npm run tauri build -- --no-bundle   # ③ 独立 exe：前端压缩内嵌，单文件自包含
npm run tauri build            # ④ 正式安装包：bundle/nsis/ReSession_0.1.0_x64-setup.exe
```

产物路径：`src-tauri/target/release/resession.exe`（③④共用）。
首次 `tauri build` 打 bundle 时 CLI 会自动下载 NSIS 到 `%LOCALAPPDATA%\tauri`。

## 3. ⚠️ 踩坑实录（重要）

### 坑 1：裸 `cargo build --release` 产出的是坏 exe

**症状**：窗口报 `ERR_CONNECTION_REFUSED / localhost 拒绝连接`。
**原因**：tauri CLI 在构建时注入 `TAURI_ENV_*` 等环境并负责把 `frontendDist`
（压缩后 ~164KB）嵌入二进制；裸 cargo 跳过这套流程，产出的 exe **不含前端资源**，
运行时仍尝试连接 `devUrl`（localhost:5173）。
**正确做法**：正式产物一律走 `npm run tauri build`（含 `--no-bundle` 变体）。

### 坑 2：debug 版 exe 不能直接双击

debug 构建运行时加载 `devUrl`（需 vite:5173 在跑），未起 vite 即报同样错误。
单独运行请用模式 ③。**两种坑症状完全一致**，但病因都不是代码 bug。

### 坑 3：验收疏漏教训

当时只验证了"进程存活"，没有验证"窗口内实际渲染"，导致坏产物连过两轮测试。
**构建产物的验收必须包含窗口内容**，见下节检查清单。

### 坑 4：重编时 `拒绝访问 (os error 5)`

目标 exe 还在运行占着文件。`Get-Process resession | Stop-Process -Force` 后重编。

## 4. 产物正确性检查清单

1. **体积对比**：正常 ~8.9MB；缺资源嵌入的坏产物 ~8.8MB（差值 ≈ 压缩后前端体积）
2. **运行时采样**（强检查）：
   ```powershell
   # 启动 exe 后循环采样，正常自包含 exe 不应有任何 5173 连接尝试
   netstat -ano | Select-String ":5173"
   ```
3. ~~grep 二进制找 `scan_sessions`~~：**只对 debug 版有效**；release 的资源是
   压缩嵌入的，grep 不到 ≠ 缺失（本条就是用弱检查误判过才补的强检查）

## 5. 其他已知构建事项

- **图标**：源文件 `app-icon.png`（512×512 占位图），改动后 `npx tauri icon app-icon.png`
  重新生成 `src-tauri/icons/` 全套
- **前端资源更新后**：必须重新走 `tauri build`（嵌入发生在编译期），只改 dist 不重编无效
- **交叉编译/macOS**：见 architecture.md 平台矩阵；macOS 正式分发需签名+公证（自用可跳）

## 6. 日常使用（自用部署）

- **稳定副本**：`F:\git-workspace\ai\tools\ReSession.exe`（桌面快捷方式 `ReSession` 指向它）。
  `target/` 会被 `cargo clean` 清掉，所以日常用这份拷贝
- **更新方式**：改代码后 `npm run tauri build -- --no-bundle`，再手动把新 exe 覆盖到 tools 副本
- **数据安全**：只读 `~/.claude/projects/`；ReSession 自身只写 `~/.resession/settings.json`（别名、
  设置），不动任何原生会话数据

### 6.1 绿色单文件属性（crt-static）

仓库根 `.cargo/config.toml` 为 MSVC 目标启用 `target-feature=+crt-static`：
VC++ 运行库（vcruntime140.dll）静态链入 exe，**不再依赖 VC++ 可再分发组件**。
产物运行期仅依赖 Windows 10/11 出厂自带组件：

- WebView2 Runtime（Win11 自带）
- 系统 UCRT（api-ms-win-crt-*，OS 组件）

验证方式：字节搜索 exe 中 `vcruntime140` 应为 0 命中（ASCII GetString 后
IndexOf，grep 对二进制不可靠）。

**安装包（nsis）已从路线图移除**：自用场景单 exe 即全部；nsis 的价值只在
分发给他人（开始菜单/卸载器/WebView2 引导），需要时随时可加回。
