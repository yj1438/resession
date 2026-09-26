# ReSession

<p align="center">
  <img src="resession.png" alt="ReSession" width="800" />
</p>

<p align="center">
  A lightweight, <strong>native</strong> local agent session manager — Claude Code and Codex in one place.<br/>
  <a href="README.zh-CN.md">中文文档</a> · <a href="docs/">Docs (中文)</a>
</p>

---

ReSession brings local Claude Code and Codex sessions into one browser and search view. Resuming runs the corresponding native CLI in a real terminal.

## Philosophy

1. **Native sessions only** — ReSession reads Claude's `~/.claude/projects/` and Codex's `~/.codex/sessions/`, then resumes with the corresponding native CLI. No protocol proxying, no re-rendered TUI, no private session format.
2. **Thin shell** — proven wheels only: Tauri 2 (≈10 MB portable exe), xterm.js, portable-pty.
3. **Agent-agnostic core** — Claude and Codex each have a `SessionProvider` adapter for their own format and commands.

## Features

- **Find** — every session across every project; fuzzy title search + full-text search over conversation content (debounced, cache-backed, highlighted)
- **Replay** — read-only transcript rendering with markdown, code highlighting, collapsible tool calls and sidechain runs
- **Resume** — one click opens an embedded terminal running native `claude --resume` or `codex resume`
- **Parallel** — PTYs live in the backend keyed by session; switch away and back without killing anything; busy/idle dots show what's working
- **Start** — choose Claude or Codex and start a session in any directory
- **Rename** — non-destructive aliases stored in ReSession's own config; native `/rename` titles are recognized too
- **Portable** — single ~10 MB exe, statically linked CRT; only runtime dependencies are OS-shipped (WebView2, system UCRT)

## Status

Claude workflows have been used on Windows and macOS. Codex support is in development; native resume and packaged builds still need end-to-end validation. ReSession does not currently trash Codex's native session files.

## Install

**Portable exe (recommended):** grab `resession.exe` from [Releases](../../releases), put it anywhere, double-click. Browsing and searching native sessions are read-only; explicitly deleting a Claude session moves its file to the system trash. ReSession's settings and logs live in `~/.resession/`.

**macOS app:** download the macOS arm64 `.app.zip` from [Releases](../../releases) and extract `ReSession.app`. The current CI bundle is not Developer ID signed and notarized; its bundle signature also fails validation. A quarantined download may therefore show **“ReSession.app is damaged and can’t be opened. You should move it to the Trash.”** This does not by itself prove that the ZIP download is corrupt, but the warning must not be ignored for an untrusted copy.

If you have verified that this exact app came from this project's release or CI artifact and accept the risk, the temporary workaround is to remove the quarantine attribute **only from that app** (replace the example path with its actual location):

```bash
xattr -dr com.apple.quarantine "/path/to/ReSession.app"
```

This does not repair the signature or notarize the app. Proper Developer ID signing and notarization are still required for normal macOS distribution; do not run the command on an app of uncertain origin or on the entire Downloads folder.

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
cargo test            # Rust core and Tauri tests, including DTO contracts
```

⚠️ A **debug** build expects the vite dev server (`tauri dev`); double-clicking it shows `ERR_CONNECTION_REFUSED`. Standalone use = release build. More pitfalls in [docs/build.md](docs/build.md).

## Documentation

Documentation is currently written in Chinese:

| Doc | Content |
|---|---|
| [docs/design.md](docs/design.md) | Goals, non-goals, principles, UI layout |
| [docs/architecture.md](docs/architecture.md) | Modules, `SessionProvider` trait, data flow, platform matrix, ConPTY field notes |
| [docs/data-formats.md](docs/data-formats.md) | Observed Claude and Codex JSONL formats and the shared IR |
| [docs/build.md](docs/build.md) | Build modes, prerequisites, pitfall log, artifact checklist |
| [docs/roadmap.md](docs/roadmap.md) | Milestones and progress |

## License

[MIT](LICENSE)
