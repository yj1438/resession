import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import TranscriptPane from "./components/TranscriptPane";
import TerminalPane from "./components/TerminalPane";
import NewSessionPanel from "./components/NewSessionPanel";
import SettingsPanel from "./components/SettingsPanel";
import { invoke, isTauri } from "./api";
import { ask } from "@tauri-apps/plugin-dialog";
import { pathBaseName, pathsEqual } from "./paths";
import type { AppSettings, PtyStatus, SearchHit, SessionMeta } from "./types";

// 浏览器直开（npm run dev）时的占位数据；Tauri 内一律走真实扫描
const MOCK_SESSIONS: SessionMeta[] = [
  {
    provider: "claude",
    id: "06c76be5-72c8-4040-ae49-4864098993ed",
    cwd: "D:\\code\\my-app",
    projectDir: "D--code-my-app",
    title: "claude code 的会话总是关闭后忘记找回来了（mock）",
    createdAt: "2026-09-19T15:40:00Z",
    modifiedAt: "2026-09-19T16:10:00Z",
    messageCount: 42,
    sourceFile: "C:\\Users\\you\\.claude\\projects\\mock.jsonl",
  },
];

interface PtyEvent {
  id: string;
  data: number[];
}

const DEFAULT_SIDEBAR_WIDTH = 320;
const MIN_SIDEBAR_WIDTH = 220;
const SIDEBAR_WIDTH_KEY = "resession.sidebarWidth";

function currentSidebarMaxWidth(): number {
  if (typeof window === "undefined") return DEFAULT_SIDEBAR_WIDTH;
  return Math.max(MIN_SIDEBAR_WIDTH, window.innerWidth * 0.55);
}

function initialSidebarWidth(): number {
  if (typeof window === "undefined") return DEFAULT_SIDEBAR_WIDTH;
  const saved = Number(window.localStorage.getItem(SIDEBAR_WIDTH_KEY));
  const preferred = Number.isFinite(saved) && saved >= MIN_SIDEBAR_WIDTH
    ? saved
    : DEFAULT_SIDEBAR_WIDTH;
  return Math.min(preferred, currentSidebarMaxWidth());
}

// 归档/别名共用的会话键（与 Rust 侧 `provider:<uuid>` 格式一致）
function sessionKey(s: SessionMeta): string {
  return `${s.provider}:${s.id}`;
}

export default function App() {
  const [sessions, setSessions] = useState<SessionMeta[]>(MOCK_SESSIONS);
  const [usingMock] = useState(!isTauri);
  const [activePtys, setActivePtys] = useState<PtyStatus[]>([]);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  // 归档集合与“显示已归档”开关（归档只藏不删，删除走 delete_session）
  const [archivedIds, setArchivedIds] = useState<Set<string>>(() => new Set());
  const [showArchived, setShowArchived] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState(initialSidebarWidth);
  const [sidebarMaxWidth, setSidebarMaxWidth] = useState(currentSidebarMaxWidth);
  const [locateRequest, setLocateRequest] = useState<{
    id: string;
    sequence: number;
  } | null>(null);
  const sidebarWidthRef = useRef(sidebarWidth);
  const locateSequenceRef = useRef(0);
  const resizeRef = useRef<{ startX: number; startWidth: number } | null>(null);
  // 正在观看的合成 PTY（"新会话"，没有对应会话条目）
  const [ptyViewId, setPtyViewId] = useState<string | null>(null);
  // 转录滚动定位：来自搜索命中（sequence 递增，支持重复点击同一命中重新定位）
  const [locateHit, setLocateHit] = useState<{
    eventIndex: number;
    sequence: number;
  } | null>(null);
  const locateSeqRef = useRef(0);

  const stopSidebarResize = useCallback(() => {
    if (!resizeRef.current) return;
    resizeRef.current = null;
    document.body.classList.remove("resizing-sidebar");
    window.localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidthRef.current));
  }, []);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      const resize = resizeRef.current;
      if (!resize) return;
      const maxWidth = currentSidebarMaxWidth();
      const next = Math.min(
        maxWidth,
        Math.max(MIN_SIDEBAR_WIDTH, resize.startWidth + event.clientX - resize.startX),
      );
      sidebarWidthRef.current = next;
      setSidebarWidth(next);
    };
    const clampOnWindowResize = () => {
      const maxWidth = currentSidebarMaxWidth();
      setSidebarMaxWidth(maxWidth);
      if (sidebarWidthRef.current <= maxWidth) return;
      sidebarWidthRef.current = maxWidth;
      setSidebarWidth(maxWidth);
      window.localStorage.setItem(SIDEBAR_WIDTH_KEY, String(maxWidth));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stopSidebarResize);
    window.addEventListener("pointercancel", stopSidebarResize);
    window.addEventListener("resize", clampOnWindowResize);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stopSidebarResize);
      window.removeEventListener("pointercancel", stopSidebarResize);
      window.removeEventListener("resize", clampOnWindowResize);
      document.body.classList.remove("resizing-sidebar");
    };
  }, [stopSidebarResize]);

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
    invoke<AppSettings>("get_settings")
      .then((s) => {
        setSettings(s);
        setArchivedIds(new Set(s.archived));
      })
      .catch((e) => console.error("get_settings failed:", e));
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
    let cancelled = false;
    const t = setTimeout(() => {
      invoke<SearchHit[]>("search_sessions", { query: q })
        .then((result) => {
          if (!cancelled) setHits(result);
        })
        .catch(() => {
          if (!cancelled) setHits(null);
        });
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query]);

  const filtered = useMemo(() => {
    const kept = showArchived
      ? sessions
      : sessions.filter((s) => !archivedIds.has(sessionKey(s)));
    const q = query.trim().toLowerCase();
    if (!q) return kept;
    return kept.filter((s) =>
      [s.title, s.cwd, s.projectDir].some((f) => f?.toLowerCase().includes(q)),
    );
  }, [sessions, query, archivedIds, showArchived]);

  // 搜索结果同样屏蔽已归档会话
  const visibleHits = useMemo(
    () =>
      hits?.filter((h) => showArchived || !archivedIds.has(sessionKey(h.session))) ??
      null,
    [hits, archivedIds, showArchived],
  );

  // 侧栏底部“已归档”开关的计数（按全量会话，不随搜索过滤）
  const archivedCount = useMemo(
    () => sessions.reduce((n, s) => n + (archivedIds.has(sessionKey(s)) ? 1 : 0), 0),
    [sessions, archivedIds],
  );

  const selected = sessions.find((s) => s.id === selectedId) ?? null;
  // 视图优先级：选中会话的终端 > 正在观看的合成终端 > 转录态 > 空
  const runningSelected = selected
    ? activePtys.some((p) => p.id === selected.id)
    : false;
  const ptyAlive = ptyViewId
    ? activePtys.some((p) => p.id === ptyViewId)
    : false;

  // 忙闲阈值可由设置页调整；走 ref 让 memo 免于依赖链，2s tick 内生效
  const busyMsRef = useRef(4000);
  busyMsRef.current = settings?.busyMs ?? 4000;

  const busyIds = useMemo(
    () =>
      new Set(
        activePtys
          .filter((p) => {
            // ?? 0 兜底：字段缺失时不让 NaN 污染比较（宁可显示忙也别永远空闲）
            const last = Math.max(p.lastOutputMs ?? 0, activityRef.current[p.id] ?? 0);
            return Date.now() - last < busyMsRef.current;
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
        pathsEqual(p.cwd, s.cwd),
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

  // 归档只动 ReSession 配置层；items 支持批量（项目级归档一次调用）
  const handleArchive = (items: SessionMeta[], archived: boolean) => {
    if (!isTauri || items.length === 0) return;
    const keys = items.map(sessionKey);
    invoke<void>("set_archived", { keys, archived })
      .then(() =>
        setArchivedIds((prev) => {
          const next = new Set(prev);
          for (const key of keys) {
            if (archived) next.add(key);
            else next.delete(key);
          }
          return next;
        }),
      )
      .catch((e) => console.error("set_archived failed:", e));
  };

  // 删除 = 移入系统废纸篓（后端守卫：运行中的会话拒绝）；逐个调用后统一刷新
  const handleDelete = async (items: SessionMeta[]) => {
    if (!isTauri || items.length === 0) return;
    // 提前拦截：由"新会话"终端创建中的会话（合成 PTY 键不是会话 id，
    // 后端 contains_key 守卫查不到，这里按 cwd 匹配；后端另有兜底）
    const creating = items.find((s) => {
      const cwd = s.cwd;
      if (!cwd) return false;
      return activePtys.some(
        (p) => p.id.startsWith("new:") && !!p.cwd && pathsEqual(p.cwd, cwd),
      );
    });
    if (creating) {
      window.alert("该会话正由\"新会话\"终端创建中，请先关闭那个终端再删除。");
      return;
    }
    const noun = items.length > 1 ? `该项目下的 ${items.length} 个会话` : "该会话";
    const ok = await ask(`把${noun}的记录移入系统废纸篓（可从废纸篓恢复）。继续吗？`, {
      title: "删除会话",
      kind: "warning",
    });
    if (!ok) return;
    const deletedIds: string[] = [];
    const errors: string[] = [];
    for (const s of items) {
      try {
        await invoke<void>("delete_session", { meta: s });
        deletedIds.push(s.id);
      } catch (e) {
        errors.push(String(e));
      }
    }
    if (errors.length > 0) {
      window.alert(`部分会话删除失败：\n${errors.join("\n")}`);
    }
    if (selectedId && deletedIds.includes(selectedId)) {
      setSelectedId(null);
    }
    refreshSessions();
  };

  const viewRunningPty = (pty: PtyStatus) => {
    const session = sessions.find((item) => item.id === pty.id);
    // 从搜索结果返回项目树，才能真正展开并定位左侧会话。
    setQuery("");
    setHits(null);
    setLocateHit(null);
    if (session) {
      setSelectedId(session.id);
      setPtyViewId(null);
      locateSequenceRef.current += 1;
      setLocateRequest({ id: session.id, sequence: locateSequenceRef.current });
    } else {
      // new:<uuid> 在 Claude 写出真实 JSONL 前还没有可定位的会话节点。
      setSelectedId(null);
      setPtyViewId(pty.id);
    }
  };

  const resetSidebarWidth = () => {
    const next = Math.min(DEFAULT_SIDEBAR_WIDTH, sidebarMaxWidth);
    sidebarWidthRef.current = next;
    setSidebarWidth(next);
    window.localStorage.setItem(SIDEBAR_WIDTH_KEY, String(next));
  };

  const resizeSidebarFromKeyboard = (next: number) => {
    const clamped = Math.min(
      sidebarMaxWidth,
      Math.max(MIN_SIDEBAR_WIDTH, next),
    );
    sidebarWidthRef.current = clamped;
    setSidebarWidth(clamped);
    window.localStorage.setItem(SIDEBAR_WIDTH_KEY, String(clamped));
  };

  return (
    <div className="app">
      <header className="topbar">
        <span className="logo">
          <span className="logo-mark" aria-hidden="true">R</span>
          <span>ReSession</span>
        </span>
        <input
          className="search"
          placeholder="搜索会话 / 全文内容…（Ctrl+K）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className="new-btn" onClick={() => setPanelOpen(true)}>
          ➕ 新会话
        </button>
        <button
          className="icon-btn"
          title="设置"
          aria-label="设置"
          onClick={() => setSettingsOpen(true)}
        >
          ⚙
        </button>
      </header>

      {activePtys.length > 0 && (
        <nav className="running-bar" aria-label="运行中的会话">
          <span className="running-label">
            <span className="running-pulse" aria-hidden="true" />
            运行中
            <span className="running-count">{activePtys.length}</span>
          </span>
          <div className="running-list">
            {activePtys.map((pty) => {
              const session = sessions.find((item) => item.id === pty.id);
              const busy = busyIds.has(pty.id);
              const label = session?.title ?? (pty.id.startsWith("new:") ? "新会话" : pty.id.slice(0, 8));
              const project = session
                ? pathBaseName(session.cwd || session.projectDir)
                : pathBaseName(pty.cwd);
              const active = session
                ? selectedId === session.id
                : ptyViewId === pty.id;
              return (
                <button
                  key={pty.id}
                  className={active ? "chip active" : "chip"}
                  title={`${project} · ${label}${busy ? "（执行中）" : "（空闲）"}`}
                  onClick={() => viewRunningPty(pty)}
                >
                  <span className={busy ? "dot busy" : "dot idle"}>
                    {busy ? "●" : "○"}
                  </span>
                  <span className="chip-project">{project}</span>
                  <span className="chip-label">{label}</span>
                </button>
              );
            })}
          </div>
        </nav>
      )}

      <div className="main">
        <Sidebar
          sessions={filtered}
          selectedId={selectedId}
          activeIds={activeIds}
          busyIds={busyIds}
          hits={visibleHits}
          highlight={query.trim()}
          width={sidebarWidth}
          locateRequest={locateRequest}
          archivedIds={archivedIds}
          showArchived={showArchived}
          archivedCount={archivedCount}
          onSelect={(id) => {
            setSelectedId(id);
            setPtyViewId(null);
            setLocateHit(null);
          }}
          onOpenHit={(h) => {
            setSelectedId(h.session.id);
            setPtyViewId(null);
            locateSeqRef.current += 1;
            setLocateHit({ eventIndex: h.eventIndex, sequence: locateSeqRef.current });
          }}
          onRename={handleRename}
          onArchive={handleArchive}
          onDelete={handleDelete}
          onToggleShowArchived={() => setShowArchived((v) => !v)}
        />

        <div
          className="sidebar-resizer"
          role="separator"
          aria-label="调整会话列表宽度"
          aria-orientation="vertical"
          aria-valuemin={MIN_SIDEBAR_WIDTH}
          aria-valuemax={Math.round(sidebarMaxWidth)}
          aria-valuenow={Math.round(sidebarWidth)}
          aria-controls="session-sidebar"
          tabIndex={0}
          title="拖动调整宽度，双击恢复默认"
          onPointerDown={(event) => {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            resizeRef.current = {
              startX: event.clientX,
              startWidth: sidebarWidthRef.current,
            };
            document.body.classList.add("resizing-sidebar");
          }}
          onLostPointerCapture={stopSidebarResize}
          onKeyDown={(event) => {
            let next: number | null = null;
            if (event.key === "ArrowLeft") next = sidebarWidth - 16;
            else if (event.key === "ArrowRight") next = sidebarWidth + 16;
            else if (event.key === "Home") next = MIN_SIDEBAR_WIDTH;
            else if (event.key === "End") next = sidebarMaxWidth;
            if (next === null) return;
            event.preventDefault();
            resizeSidebarFromKeyboard(next);
          }}
          onDoubleClick={resetSidebarWidth}
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
                locateEventIndex={locateHit?.eventIndex}
                locateSequence={locateHit?.sequence ?? 0}
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

      {settingsOpen && settings && (
        <SettingsPanel
          settings={settings}
          onClose={() => setSettingsOpen(false)}
          onSaved={setSettings}
        />
      )}

      <footer className="statusbar" data-tick={tick}>
        {sessions.length} 个会话 · {usingMock ? "mock 数据" : "provider: 已扫描"} ·{" "}
        {activePtys.filter((p) => busyIds.has(p.id)).length} 忙 /{" "}
        {activePtys.length} 跑
        {!isTauri && " · 浏览器模式"}
        <span className="build-stamp" title="前端构建时间（判断 exe 内嵌资源是否新鲜）">
          {" "}· build {__BUILD_DATE__}
        </span>
      </footer>
    </div>
  );
}
