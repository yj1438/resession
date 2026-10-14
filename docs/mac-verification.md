# Mac 侧安装与验证清单

> 适用：ReSession v0.5.0+。本清单用于**每次新产物**的实机验收，不能用旧版本的结果代替。
> 2026-09-26 已验证从 Dock 启动后可发现 CLI、已有会话内可使用 CLI 工具；
> 2026-09-26 的独立空目录冒烟测试曾遇到新会话终端空白。首帧修复须在下一份 App 中重新验证。
> 2026-10-08：系统 pbcopy 写入、NSPasteboard 与 pbpaste 读取的中文/多行往返验证通过；
> 已恢复原剪贴板。App 内 Cmd+C/复制按钮仍需在包含原生通道的新包中验收。
> 2026-10-14：用户在官方 v0.8.1 再次反馈复制后粘贴为空。运行二进制版本与发布资产
> SHA-256 一致；按用户点击复制按钮后检查，仅记录格式/长度：系统剪贴板发生变化，
> 类型为纯文本，但正文为 0 字节。尚未确定是写入失败还是后续事件覆盖，不能宣称根因已证实。
> 候选实现改为主线程 `NSPasteboard` 写入/读回，补齐 DOM copy 路径；Rust 编译与新 App 实测待完成。
> 同日后续调查已进一步定位到 CLI 自动复制的缺省 locale：真实 Claude 子进程没有
> LANG/LC_CTYPE/LC_ALL，9 组 pbcopy 对照复现中文/Emoji 0 字节，补 LANG 即恢复。
> 主修复位于 PTY 子进程环境，不是单纯更换 App 的复制按钮实现。详见第 6 节。

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
- [ ] 单独验证 Cmd+C、菜单“编辑→复制”和复制按钮；Mac App 成功反馈应明确显示「系统已核验」，不能走「浏览器」分支。核验失败应显示错误，不得显示成功。
- [ ] 选中普通中文、多行、Emoji、RTF/EPS 开头的原文；分别粘贴到外部编辑器和终端输入区，不能为空、丢失或格式被重新解释。
- [ ] 检查日志只有 `[clipboard]` 操作结果/字节数，不含正文；空选区不覆盖剪贴板，快速连续复制与切换终端不会重复触发或残留监听。

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

## 4. 不依赖 Rust 的局部回归

```bash
npm test
swift scripts/mac-clipboard-smoke.swift
```

前者验证事件路由、原生失败不回退、空选区、快捷键和清理逻辑（mock，无真实剪贴板访问）；
后者在唯一临时 pasteboard 中验证原生 API 的 7 组纯文本往返，不读取或覆盖 general 剪贴板。
这些局部测试不等于 Tauri IPC 或打包后 App 已通过；必须先过 CI，再用实际测试包按本清单验收。

## 5. 2026-10-14 多面调查：资料、复现与反证

### 5.1 已有的真实故障证据

- 用户明确反馈：外部复制后可粘贴进终端；故障集中在从终端复制出去。
- 运行中的 App 是官方 v0.8.1 arm64 二进制，SHA-256 与发布资产一致，不是旧版本误用。
- 此前在用户表示点击“复制选中内容”后，仅检查类型/长度：变更计数增加，存在纯文本类型，但数据为 **0 字节**。当时没有同步确认按钮是否启用、原生请求是否发生，不能据此证明 ReSession 的复制命令已执行。未输出正文。
- 当前 App 的 bundle 签名校验仍失败；这与已知分发问题一致，但尚无证据证明它导致复制故障，不能据此建议关闭系统安全措施。

以上确认“发生了复制失败”，**没有证明失败发生在选区、IPC、原生写入还是后续覆盖**。

### 5.2 资料交叉核对

| 问题 | 一手资料 | 可以支持的结论与边界 |
|---|---|---|
| 选区是否等于自动复制 | [xterm 5.5.0 SelectionService](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/services/SelectionService.ts)、[Clipboard](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Clipboard.ts) | Linux 有独立的鼠标选区处理；Mac 选区通知不等于写系统剪贴板。正常复制另走 copy 事件 |
| CLI 的 OSC 52 是否就是鼠标复制 | [xterm 维护者说明](https://github.com/xtermjs/xterm.js/discussions/5350) | OSC 52 与用户复制是不同路径；本项目只加载 FitAddon，没有接入 ClipboardAddon，不能拿其他终端的行为作保证 |
| copy 事件与异步 API 是否相同 | [W3C Clipboard 草案](https://w3c.github.io/clipboard-apis/#copy-action)、[WebKit 官方说明](https://webkit.org/blog/10855/async-clipboard-api/) | copy 事件的数据提交与异步 Clipboard API 不同；后者还有用户交互等条件。这只能提示检查时序，不能证明当前空内容来自竞态 |
| 写入返回成功是否足够 | [Apple setString](https://developer.apple.com/documentation/appkit/nspasteboard/setstring(_:fortype:))、[string(forType:)](https://developer.apple.com/documentation/appkit/nspasteboard/string(fortype:)) | 写入结果、类型与读回内容应分开检查；即时读回也不能证明内容之后从未被覆盖 |
| pbcopy 是否强制纯文本 | 本机 `man pbcopy` | 它可能识别 RTF/EPS 文件头；退出成功不保证所需的纯文本类型存在 |
| 是否所有 AppKit 操作都必须主线程 | [Apple 线程说明](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/ThreadSafetySummary/ThreadSafetySummary.html)、[相关上游报告](https://github.com/tauri-apps/plugins-workspace/issues/3205) | Apple 不支持这种全框架概括；上游报告是其他版本的崩溃问题，且包含未验证的 AI 建议。当前主线程串行化是保守设计选择，不是已证实的根因修复 |

还核对了 [WebKit #177715](https://bugs.webkit.org/show_bug.cgi?id=177715) 和 [Tauri #9159](https://github.com/tauri-apps/tauri/issues/9159)：前者是已关闭的旧 iOS 问题，后者涉及旧 alpha/beta 与快捷键。它们是排查线索，不能直接套作本次 macOS 故障的答案。尤其“换一个 clipboard 插件就一定解决”没有证据。

### 5.3 真实 WKWebView 对照探针

运行：`swift scripts/mac-webkit-clipboard-probe.swift`。

- 环境：本机 macOS 27.0.1、真实 WKWebView、项目已有 xterm 5.5.0，`navigator.platform=MacIntel`。
- 复用 v0.8.1 的实际 `clipboard.ts`，以及当前候选模块；JS 到原生桥使用 Swift 实现，**不是 Tauri IPC**。
- 四类固定样本：ASCII、中文多行/Emoji、RTF 开头原文、EPS 开头原文。
- 分别设置 xterm 选区、点击模拟按钮、派发合成 keydown、通过本测试窗口的 responder 发起原生菜单 copy；菜单 copy 的 `isTrusted=true`。
- keydown 是合成事件，设置选区是程序化操作，不能冒充用户物理快捷键或真实鼠标拖选；模拟按钮也不是完整 React 组件的实机点击。
- 探针只暂写固定样本，原 general 剪贴板数据仅在内存保存；完成后已恢复，未输出正文。检测到非测试内容或外部修改会停止，不能自动覆盖用户新复制内容。

| 对照组 | 通过 | 未通过 | 解释 |
|---|---:|---:|---|
| xterm 默认路径 | 8 | 0 | 四种样本的程序化选区、原生菜单 copy |
| v0.8.1 旧路径 | 14 | 2 | RTF 开头的按钮、合成快捷键提示成功，但读不到纯文本；普通中文在这里成功 |
| 当前候选路径 | 16 | 0 | 主线程原生写入/读回，以及候选 copy 捕获路径在隔离壳中通过 |
| 强制早写、copy 事件不填 DataTransfer | 4 | 0 | 人为让原生写入先完成，未复现后续清空；不支持据此认定通用覆盖竞态 |
| 强制早写、copy 事件填同一文本 | 4 | 0 | 补充时序控制；同样没有复现普通文字为空 |

48 组中 46 组通过、2 组暴露旧实现的 RTF 类型缺口。**RTF 没有纯文本类型，与用户普通文字存在纯文本类型但 0 字节，不是同一观测。** EPS 样本在这台机器没有复现失败，不能扩大成所有版本的格式保证。

程序化选区的 12 个案例均未修改 general 剪贴板；这与源码方向一致，但仍不排除 CLI 自行调用系统工具，或打包 App 中其他事件、运行状态的影响。

### 5.4 阶段结论：仅隔离壳尚不足以定位（已由第 6 节补充）

隔离壳中新旧路径都能复制普通文字，因此**尚未定位普通文字复制为空的根因**。不能把“候选 16 组通过”写成“用户问题已解决”，也不能把代码签名、第三方剪贴板工具或权限列为已证实原因。

候选中增加只含元数据的链路诊断：操作编号、button/key/copy 来源、前端 UTF-8 字节数、Rust 收到的字节数、写入/即时读回结果。选区已失效时显式提示失败，不沿用旧的“已复制”。

下一份 **CI 测试包，不打发布 tag**，在真实终端中逐个验证：

1. 只拖选，不复制：剪贴板内容是否改变；
2. 点击按钮：有无请求、选区/IPC 长度是否一致、原生核验是否成功；
3. 物理 Cmd+C 与菜单复制：分别走哪个入口，有无重复操作；
4. 当场与稍后粘贴到外部编辑器：核验成功后是否又变空；
5. 外部复制再粘贴回终端，以及 Ctrl+C 中断：确认未回归。

没有原生请求 → 检查前端入口/选区；JS 与 Rust 长度不一致 → 检查 IPC；原生核验失败 → 检查系统写入；即时核验成功但外部随后为空 → 再查后续覆盖/跨应用行为。若仍不能定位，应保留失败证据并继续诊断，而不是直接升版宣称修复。

## 6. 后续闭环：Claude 自选区 + GUI 缺省 locale

### 6.1 官方说明修正了调查范围

[Claude 全屏模式文档](https://code.claude.com/docs/en/fullscreen#keep-native-text-selection) 明确区分 CLI 自身选区与终端选区：普通拖选由 Claude 处理并可在松开时自动复制，本地 Mac 调用 `pbcopy`；使用终端修饰键则绕过鼠标上报，建立终端自己的选区。这说明“xterm 没有自动复制”不能推出“CLI 也不会自动复制”。

### 6.2 真实 v0.8.1 的两条路径对照

| 操作 | App 状态 | 仅类型/长度观测 |
|---|---|---|
| Cmd+A 建立本地全选 | 复制按钮启用 | 剪贴板计数未变，仍为原来 7 字节 |
| 点击启用的复制按钮 | 显示“已复制” | 纯文本 2,973 字节，不为空 |
| 用户仅普通拖选，不按键/按钮 | 复制按钮禁用，即没有 xterm 本地选区 | 剪贴板计数增加，纯文本变为 0 字节 |
| 无本地选区时对固定测试剪贴板按 Cmd+C | 复制按钮禁用 | 本次 31 字节测试内容保留，未复现覆盖 |

所有观测未输出用户正文、未向 Agent 提交任务。自动化拖拽接口报窗口定位错误，因此不把失败的拖拽动作计为实测；普通拖选的样本由用户按要求手动完成。它已经在显式复制之前触发了空剪贴板，调查重点应转到 CLI 自身复制，而非仅怀疑 App 按钮。

只检查了目标 App 与其 Claude 子进程的三个 locale 字段（未输出完整环境）：`LANG`、`LC_CTYPE`、`LC_ALL` 均未设置。`build_command` 原先只补登录 shell 的 PATH，没有补 locale。

### 6.3 编码控制实验

运行 `swift scripts/mac-pbcopy-locale-probe.swift`，只使用固定样本，保留/恢复原 general 剪贴板：

| pbcopy 环境 | ASCII | 中文（12 字节） | Emoji（4 字节） |
|---|---|---|---|
| 三个 locale 字段均未设置 | 7 字节，正确 | **0 字节，退出码仍 0** | **0 字节，退出码仍 0** |
| `LC_ALL=C` | 7 字节，正确 | **0 字节，退出码仍 0** | **0 字节，退出码仍 0** |
| 仅设置 `LANG=en_US.UTF-8` | 7 字节，正确 | 12 字节，正确 | 4 字节，正确 |

这与本机 `man pbcopy` 关于编码受 locale 控制、缺省退回 C 编码的说明，以及真实子进程的缺省环境相符。环境缺省 → CLI 自动复制 → 中文空内容有一条可重复验证的因果链；但新的 Rust 实现和实际 PTY 重建仍须测试包验收。

### 6.4 收口后的修复与验收

- **主修复**：仅在 macOS 的三个 locale 字段都缺失/为空时，为新建或恢复 PTY 补 `LANG=en_US.UTF-8`；保留显式配置，不写全局环境，不切换 Claude 的渲染/鼠标设置。
- **适用边界**：显式设置 `LC_ALL=C` 不是缺省环境，不会被擅自覆盖；此类配置仍可能有编码限制。
- **已运行会话**：旧 PTY 的环境不会就地改变，不自动终止任何运行会话。测试须明确关闭旧 PTY，再按原 ID 恢复；不是只切换标签页。
- **独立防护**：主线程原生写入/读回用于 App 自己的复制入口和已确认的 RTF 类型缺口；它不是解决 CLI 自动复制的主因。菜单竞争在本机没有复现，不作为已证实根因。
- **Option 短选择**：本地源码还显示 `altClickMovesCursor` 可能将短选择变成方向键输入。Mac 上关闭该选项，避免与 Option 强制本地选择共用一个手势。
- **测试包门槛**：Rust 测试/双平台构建 → 重建实际 Mac PTY 确认 UTF-8 生效 → 普通拖选中文/Emoji自动复制 → Option 局部拖选及按钮/Cmd+C/菜单 → 外部粘贴与反向粘贴；全部通过后才讨论正式版本。
