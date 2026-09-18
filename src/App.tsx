import { useMemo, useState } from "react";
import Sidebar from "./components/Sidebar";
import TranscriptPane from "./components/TranscriptPane";
import TerminalPane from "./components/TerminalPane";
import type { SessionMeta } from "./types";

type Tab = "transcript" | "terminal";

// M0 用 mock 数据铺 UI；M1 换成 invoke("scan_sessions") 的真实返回
const MOCK_SESSIONS: SessionMeta[] = [
  {
    provider: "claude",
    id: "06c76be5-72c8-4040-ae49-4864098993ed",
    cwd: "F:\\git-workspace\\ai",
    projectDir: "F--git-workspace-ai",
    title: "claude code 的会话总是关闭后忘记找回来了（mock）",
    createdAt: "2026-09-19T15:40:00Z",
    modifiedAt: "2026-09-19T16:10:00Z",
    messageCount: 42,
    sourceFile: "C:\\Users\\yinjie\\.claude\\projects\\mock.jsonl",
  },
  {
    provider: "claude",
    id: "11111111-2222-3333-4444-555555555555",
    cwd: "F:\\git-workspace\\h5-mario-game",
    projectDir: "F--git-workspace-h5-mario-game",
    title: "h5-mario-game（mock）",
    createdAt: "2026-07-01T09:00:00Z",
    modifiedAt: "2026-09-05T18:00:00Z",
    messageCount: 128,
    sourceFile: "C:\\Users\\yinjie\\.claude\\projects\\mock2.jsonl",
  },
];

export default function App() {
  const [sessions] = useState<SessionMeta[]>(MOCK_SESSIONS);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("transcript");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) =>
      [s.title, s.cwd, s.projectDir].some((f) => f?.toLowerCase().includes(q)),
    );
  }, [sessions, query]);

  const selected = sessions.find((s) => s.id === selectedId) ?? null;

  return (
    <div className="app">
      <header className="topbar">
        <span className="logo">ReSession</span>
        <input
          className="search"
          placeholder="搜索项目 / 摘要…（Ctrl+K）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </header>

      <div className="main">
        <Sidebar sessions={filtered} selectedId={selectedId} onSelect={(id) => {
          setSelectedId(id);
          setTab("transcript");
        }} />

        <section className="content">
          {selected ? (
            <>
              <nav className="tabs">
                <button
                  className={tab === "transcript" ? "tab active" : "tab"}
                  onClick={() => setTab("transcript")}
                >
                  转录
                </button>
                <button
                  className={tab === "terminal" ? "tab active" : "tab"}
                  onClick={() => setTab("terminal")}
                >
                  终端
                </button>
              </nav>
              {tab === "transcript" ? (
                <TranscriptPane session={selected} />
              ) : (
                <TerminalPane session={selected} />
              )}
            </>
          ) : (
            <div className="empty">
              <p>从左侧选择一个会话</p>
              <p className="hint">转录回放找回记忆 → 终端恢复原生会话</p>
            </div>
          )}
        </section>
      </div>

      <footer className="statusbar">
        {sessions.length} 个会话 · provider: claude · PTY: 未连接（M1）
      </footer>
    </div>
  );
}
