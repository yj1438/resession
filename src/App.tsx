import { useCallback, useEffect, useMemo, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import TranscriptPane from "./components/TranscriptPane";
import TerminalPane from "./components/TerminalPane";
import { invoke, isTauri } from "./api";
import type { SessionMeta } from "./types";

// 浏览器直开（npm run dev）时的占位数据；Tauri 内一律走真实扫描
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
];

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>(MOCK_SESSIONS);
  const [usingMock, setUsingMock] = useState(!isTauri);
  const [activePtys, setActivePtys] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<"transcript" | "terminal">("transcript");

  useEffect(() => {
    if (!isTauri) return;
    invoke<SessionMeta[]>("scan_sessions")
      .then(setSessions)
      .catch((e) => {
        console.error("scan_sessions failed:", e);
        setUsingMock(true);
      });
  }, []);

  const refreshPtys = useCallback(() => {
    if (!isTauri) return;
    invoke<string[]>("pty_list")
      .then(setActivePtys)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshPtys();
    if (!isTauri) return;
    // 后台 PTY 退出（用户在里面 /exit 或崩溃）时刷新运行中标识
    let un: UnlistenFn | undefined;
    void listen("pty-exit", () => refreshPtys()).then((u) => (un = u));
    return () => un?.();
  }, [refreshPtys]);

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
        <Sidebar
          sessions={filtered}
          selectedId={selectedId}
          activePtys={activePtys}
          onSelect={(id) => {
            setSelectedId(id);
            setTab("transcript");
          }}
        />

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
                <TranscriptPane key={selected.id} session={selected} />
              ) : (
                <TerminalPane
                  key={selected.id}
                  session={selected}
                  onPtyStateChange={refreshPtys}
                />
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
        {sessions.length} 个会话 · {usingMock ? "mock 数据" : "provider: 已扫描"}
        {isTauri ? ` · ${activePtys.length} 个终端运行中` : " · 浏览器模式"}
      </footer>
    </div>
  );
}
