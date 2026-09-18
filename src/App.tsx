import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import TranscriptPane from "./components/TranscriptPane";
import TerminalPane from "./components/TerminalPane";
import { invoke, isTauri } from "./api";
import type { PtyStatus, SessionMeta } from "./types";

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

// 输出静默超过该时长视为空闲（近似值：静默执行长任务会误判，见 roadmap）
const BUSY_MS = 4000;

interface PtyEvent {
  id: string;
  data: number[];
}

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>(MOCK_SESSIONS);
  const [usingMock, setUsingMock] = useState(!isTauri);
  const [activePtys, setActivePtys] = useState<PtyStatus[]>([]);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // pty-out 高频到达：写入 ref，靠 2s tick 驱动重渲染（避免每块输出一次 setState）
  const activityRef = useRef<Record<string, number>>({});
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setTick((x) => x + 1), 2000);
    return () => clearInterval(t);
  }, []);

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
    invoke<PtyStatus[]>("pty_list")
      .then(setActivePtys)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshPtys();
    if (!isTauri) return;
    let unExit: UnlistenFn | undefined;
    let unOut: UnlistenFn | undefined;
    void listen("pty-exit", () => refreshPtys()).then((u) => (unExit = u));
    void listen<PtyEvent>("pty-out", (e) => {
      activityRef.current[e.payload.id] = Date.now();
    }).then((u) => (unOut = u));
    return () => {
      unExit?.();
      unOut?.();
    };
  }, [refreshPtys]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) =>
      [s.title, s.cwd, s.projectDir].some((f) => f?.toLowerCase().includes(q)),
    );
  }, [sessions, query]);

  const selected = sessions.find((s) => s.id === selectedId) ?? null;
  // 单视图状态机：会话有存活 PTY → 终端态；否则 → 转录态（历史态）
  const running = selected ? activePtys.some((p) => p.id === selected.id) : false;

  const busyIds = useMemo(
    () =>
      new Set(
        activePtys
          .filter(
            (p) =>
              Date.now() - Math.max(p.lastOutputMs, activityRef.current[p.id] ?? 0) <
              BUSY_MS,
          )
          .map((p) => p.id),
      ),
    // tick 参与：2s 脉冲让忙闲状态随时间推进
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activePtys, tick],
  );
  const activeIds = useMemo(() => activePtys.map((p) => p.id), [activePtys]);

  const resumeSession = (s: SessionMeta) => {
    if (!isTauri) return;
    void invoke<string>("resume_session", { meta: s })
      .then(() => refreshPtys())
      .catch(() => {});
  };

  const handleRename = (s: SessionMeta, name: string) => {
    if (!isTauri) return;
    void invoke<void>("rename_session", {
      provider: s.provider,
      id: s.id,
      name,
    })
      .then(() =>
        setSessions((prev) =>
          prev.map((x) =>
            x.id === s.id ? { ...x, title: name.trim() || x.title } : x,
          ),
        ),
      )
      .catch(() => {});
  };

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
          activeIds={activeIds}
          busyIds={busyIds}
          onSelect={(id) => setSelectedId(id)}
          onRename={handleRename}
        />

        <section className="content">
          {selected ? (
            running ? (
              <TerminalPane
                key={selected.id}
                session={selected}
                onPtyStateChange={refreshPtys}
              />
            ) : (
              <>
                <div className="session-bar">
                  <span className="session-bar-title">
                    {selected.title ?? "(无标题)"}
                  </span>
                  <button
                    className="resume-btn"
                    onClick={() => resumeSession(selected)}
                  >
                    ▶ 恢复会话
                  </button>
                </div>
                <TranscriptPane key={selected.id} session={selected} />
              </>
            )
          ) : (
            <div className="empty">
              <p>从左侧选择一个会话</p>
              <p className="hint">转录找回记忆 → 恢复会话进入原生终端</p>
            </div>
          )}
        </section>
      </div>

      <footer className="statusbar" data-tick={tick}>
        {sessions.length} 个会话 · {usingMock ? "mock 数据" : "provider: 已扫描"} ·{" "}
        {activePtys.filter((p) => busyIds.has(p.id)).length} 忙 /{" "}
        {activePtys.length} 跑
        {!isTauri && " · 浏览器模式"}
      </footer>
    </div>
  );
}
