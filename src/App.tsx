import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import TranscriptPane from "./components/TranscriptPane";
import TerminalPane from "./components/TerminalPane";
import NewSessionPanel from "./components/NewSessionPanel";
import { invoke, isTauri } from "./api";
import type { PtyStatus, SearchHit, SessionMeta } from "./types";

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

// Windows 路径比较：忽略大小写与结尾分隔符
function normPath(p: string): string {
  return p.replace(/[\\/]+$/, "").toLowerCase();
}

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>(MOCK_SESSIONS);
  const [usingMock] = useState(!isTauri);
  const [activePtys, setActivePtys] = useState<PtyStatus[]>([]);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);
  // 正在观看的合成 PTY（"新会话"，没有对应会话条目）
  const [ptyViewId, setPtyViewId] = useState<string | null>(null);

  // pty-out 高频到达：写入 ref，靠 2s tick 驱动重渲染（避免每块输出一次 setState）
  const activityRef = useRef<Record<string, number>>({});
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setTick((x) => x + 1), 2000);
    return () => clearInterval(t);
  }, []);

  const refreshSessions = useCallback(() => {
    if (!isTauri) return;
    invoke<SessionMeta[]>("scan_sessions")
      .then(setSessions)
      .catch((e) => {
        console.error("scan_sessions failed:", e);
      });
  }, []);

  // 列表不是静态的：新会话（含外部终端开的）要陆续进来。
  // mtime 缓存让重扫很便宜，15s 一个周期足够"新鲜"。
  useEffect(() => {
    if (!isTauri) return;
    refreshSessions();
    const t = setInterval(refreshSessions, 15000);
    return () => clearInterval(t);
  }, [refreshSessions]);

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

  // 全文搜索：防抖 300ms，query 非空即进入结果模式（清空恢复列表）
  useEffect(() => {
    if (!isTauri) return;
    const q = query.trim();
    if (!q) {
      setHits(null);
      return;
    }
    const t = setTimeout(() => {
      invoke<SearchHit[]>("search_sessions", { query: q })
        .then(setHits)
        .catch(() => setHits(null));
    }, 300);
    return () => clearTimeout(t);
  }, [query]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) =>
      [s.title, s.cwd, s.projectDir].some((f) => f?.toLowerCase().includes(q)),
    );
  }, [sessions, query]);

  const selected = sessions.find((s) => s.id === selectedId) ?? null;
  // 视图优先级：选中会话的终端 > 正在观看的合成终端 > 转录态 > 空
  const runningSelected = selected
    ? activePtys.some((p) => p.id === selected.id)
    : false;
  const ptyAlive = ptyViewId
    ? activePtys.some((p) => p.id === ptyViewId)
    : false;

  const busyIds = useMemo(
    () =>
      new Set(
        activePtys
          .filter((p) => {
            // ?? 0 兜底：字段缺失时不让 NaN 污染比较（宁可显示忙也别永远空闲）
            const last = Math.max(p.lastOutputMs ?? 0, activityRef.current[p.id] ?? 0);
            return Date.now() - last < BUSY_MS;
          })
          .map((p) => p.id),
      ),
    // tick 参与：2s 脉冲让忙闲状态随时间推进
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activePtys, tick],
  );
  const activeIds = useMemo(() => activePtys.map((p) => p.id), [activePtys]);

  const resumeSession = (s: SessionMeta) => {
    if (!isTauri) return;
    // P0 守卫：该目录已有存活的"新会话"PTY（就是同一个对话在跑），
    // 直接附着观看，绝不能再拉起第二个 resume 进程踩同一个转录
    const liveNew = activePtys.find(
      (p) =>
        p.id.startsWith("new:") &&
        !!s.cwd &&
        !!p.cwd &&
        normPath(p.cwd) === normPath(s.cwd),
    );
    if (liveNew) {
      setSelectedId(null);
      setPtyViewId(liveNew.id);
      return;
    }
    void invoke<string>("resume_session", { meta: s })
      .then(() => refreshPtys())
      .catch(() => {});
  };

  const startNewSession = (cwd: string) => {
    if (!isTauri) return;
    void invoke<string>("new_session", { cwd })
      .then((id) => {
        setPtyViewId(id);
        setPanelOpen(false);
        refreshPtys();
        // claude 写 jsonl 有几秒延迟，先补一枪，余下的交给 15s 周期
        setTimeout(refreshSessions, 4000);
      })
      .catch((e) => console.error("new_session failed:", e));
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
          placeholder="搜索会话 / 全文内容…（Ctrl+K）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className="new-btn" onClick={() => setPanelOpen(true)}>
          ➕ 新会话
        </button>
      </header>

      <div className="main">
        <Sidebar
          sessions={filtered}
          selectedId={selectedId}
          activeIds={activeIds}
          busyIds={busyIds}
          runningPtys={activePtys}
          hits={hits}
          highlight={query.trim()}
          onSelect={(id) => {
            setSelectedId(id);
            setPtyViewId(null);
          }}
          onViewPty={(id) => {
            setSelectedId(null);
            setPtyViewId(id);
          }}
          onOpenHit={(h) => setSelectedId(h.session.id)}
          onRename={handleRename}
        />

        <section className="content">
          {runningSelected && selected ? (
            <TerminalPane
              key={selected.id}
              session={selected}
              onPtyStateChange={refreshPtys}
            />
          ) : ptyAlive ? (
            <TerminalPane
              key={ptyViewId}
              attachPtyId={ptyViewId ?? undefined}
              onPtyStateChange={refreshPtys}
            />
          ) : selected ? (
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
              <TranscriptPane
                key={selected.id}
                session={selected}
                highlight={hits ? query.trim() : undefined}
              />
            </>
          ) : (
            <div className="empty">
              <p>从左侧选择一个会话，或新建一个</p>
              <p className="hint">转录找回记忆 → 恢复会话进入原生终端</p>
            </div>
          )}
        </section>
      </div>

      {panelOpen && (
        <NewSessionPanel onPick={startNewSession} onClose={() => setPanelOpen(false)} />
      )}

      <footer className="statusbar" data-tick={tick}>
        {sessions.length} 个会话 · {usingMock ? "mock 数据" : "provider: 已扫描"} ·{" "}
        {activePtys.filter((p) => busyIds.has(p.id)).length} 忙 /{" "}
        {activePtys.length} 跑
        {!isTauri && " · 浏览器模式"}
      </footer>
    </div>
  );
}
