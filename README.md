# ReSession

<p align="center">
  <img src="resession.png" alt="ReSession" width="800" />
</p>

<p align="center">
  A lightweight, <strong>native</strong> Claude Code session manager — every session, every project, one thin shell.<br/>
  <a href="README.zh-CN.md">中文文档</a> · <a href="docs/">Docs (中文)</a>
</p>

---

Claude Code's `/resume` only shows sessions for the current project. ReSession fixes that: browse, search, replay and resume **all** of your sessions across **all** projects — and start new ones — without ever wrapping or reimplementing Claude Code.

## Philosophy

1. **Native sessions only** — ReSession reads `~/.claude/projects/*.jsonl` and resumes with a real `claude --resume <id>` inside a real terminal. No protocol proxying, no re-rendered TUI, no private session format.
2. **Thin shell** — proven wheels only: Tauri 2 (≈10 MB portable exe), xterm.js, portable-pty.
3. **Agent-agnostic core** — everything Claude-specific lives behind a `SessionProvider` trait; Codex etc. are future adapters, not rewrites.

## Features

- **Find** — every session across every project; fuzzy title search + full-text search over conversation content (debounced, cache-backed, highlighted)
- **Replay** — read-only transcript rendering with markdown, code highlighting, collapsible tool calls and sidechain runs
- **Resume** — one click opens an embedded terminal running the native `claude --resume`; full-color TUI, completely untouched
- **Parallel** — PTYs live in the backend keyed by session; switch away and back without killing anything; busy/idle dots show what's working
- **Start** — new sessions in any directory (known projects, folder picker, or manual path)
- **Rename** — non-destructive aliases stored in ReSession's own config; native `/rename` titles are recognized too
- **Portable** — single ~10 MB exe, statically linked CRT; only runtime dependencies are OS-shipped (WebView2, system UCRT)

## Status

Daily-driver quality on **Windows 10/11**. The architecture is written cross-platform (macOS/Linux branches in place, standard PTY paths) but **not yet verified on non-Windows machines**.

## Install

**Portable exe (recommended):** grab `resession.exe` from [Releases](../../releases), put it anywhere, double-click. Uninstall = delete the file (session data is never touched; ReSession only writes its own `~/.resession/settings.json`).

**Build from source:**

```bash
npm install
npm run tauri build -- --no-bundle   # → src-tauri/target/release/resession.exe
```

Requires Node ≥ 20, Rust (MSVC toolchain on Windows) and the VS Build Tools C++ workload. Details in [docs/build.md](docs/build.md).

## Development

```bash
npm run tauri dev     # full dev mode (vite + hot reload)
npm run dev           # frontend only, in a browser, with mock data
cargo test            # session-core + src-tauri, 17 tests incl. DTO contracts
```

⚠️ A **debug** build expects the vite dev server (`tauri dev`); double-clicking it shows `ERR_CONNECTION_REFUSED`. Standalone use = release build. More pitfalls in [docs/build.md](docs/build.md).

## Documentation

Documentation is currently written in Chinese:

| Doc | Content |
|---|---|
| [docs/design.md](docs/design.md) | Goals, non-goals, principles, UI layout |
| [docs/architecture.md](docs/architecture.md) | Modules, `SessionProvider` trait, data flow, platform matrix, ConPTY field notes |
| [docs/data-formats.md](docs/data-formats.md) | Observed Claude JSONL format, IR definition, Codex placeholder |
| [docs/build.md](docs/build.md) | Build modes, prerequisites, pitfall log, artifact checklist |
| [docs/roadmap.md](docs/roadmap.md) | Milestones and progress |

## License

[MIT](LICENSE)
