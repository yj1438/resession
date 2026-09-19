import { useState } from "react";
import Highlight from "./Highlight";
import type { PtyStatus, SearchHit, SessionMeta } from "../types";

interface Props {
  sessions: SessionMeta[];
  selectedId: string | null;
  activeIds: string[];
  busyIds: Set<string>;
  runningPtys: PtyStatus[];
  hits: SearchHit[] | null;
  highlight: string;
  onSelect: (id: string) => void;
  onViewPty: (ptyId: string) => void;
  onOpenHit: (hit: SearchHit) => void;
  onRename: (session: SessionMeta, name: string) => void;
}

function relativeTime(iso: string | null): string {
  if (!iso) return "—";
  const diff = Date.now() - new Date(iso).getTime();
  const min = Math.floor(diff / 60_000);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  return `${Math.floor(hr / 24)}d`;
}

export default function Sidebar({
  sessions,
  selectedId,
  activeIds,
  busyIds,
  runningPtys,
  hits,
  highlight,
  onSelect,
  onViewPty,
  onOpenHit,
  onRename,
}: Props) {
  const [editing, setEditing] = useState<{ id: string; value: string } | null>(
    null,
  );

  return (
    <aside className="sidebar">
      {runningPtys.length > 0 && (
        <div className="chips">
          <span className="chips-label">运行中</span>
          {runningPtys.map((p) => {
            const s = sessions.find((x) => x.id === p.id);
            const busy = busyIds.has(p.id);
            const label = s?.title ?? (p.id.startsWith("new:") ? "新会话" : p.id.slice(0, 8));
            return (
              <button
                key={p.id}
                className="chip"
                title={`${label}${busy ? "（执行中）" : "（空闲）"}`}
                onClick={() => (s ? onSelect(s.id) : onViewPty(p.id))}
              >
                <span className={busy ? "dot busy" : "dot idle"}>
                  {busy ? "●" : "○"}
                </span>
                <span className="chip-label">{label}</span>
              </button>
            );
          })}
        </div>
      )}
      {hits ? (
        <ul className="hits">
          {hits.map((h) => (
            <li
              key={`${h.session.id}:${h.eventIndex}`}
              className="session hit"
              onClick={() => onOpenHit(h)}
            >
              <div className="row">
                <span className={`hit-role role-${h.role}`}>
                  {h.role === "user" ? "你" : h.role === "assistant" ? "Claude" : "系统"}
                </span>
                <span className="project">{h.session.title ?? "(无标题)"}</span>
                {h.sidechain && <span title="子 agent 消息">🤖</span>}
              </div>
              <div className="hit-snippet">
                <Highlight text={h.snippet} query={highlight} />
              </div>
            </li>
          ))}
          {hits.length === 0 && <li className="hint empty-hits">无命中</li>}
        </ul>
      ) : (
        <ul>
        {sessions.map((s) => {
          const running = activeIds.includes(s.id);
          const busy = busyIds.has(s.id);
          const isEditing = editing?.id === s.id;
          return (
            <li
              key={s.id}
              className={s.id === selectedId ? "session selected" : "session"}
              onClick={() => onSelect(s.id)}
            >
              <div className="row">
                <span className="time">{relativeTime(s.modifiedAt)}</span>
                <span className="project">{s.cwd ?? s.projectDir}</span>
                {running && (
                  <span
                    className={busy ? "dot busy" : "dot idle"}
                    title={busy ? "任务执行中" : "空闲等待输入"}
                  >
                    {busy ? "●" : "○"}
                  </span>
                )}
              </div>
              <div className="row">
                {isEditing ? (
                  <input
                    className="rename-input"
                    autoFocus
                    value={editing.value}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) =>
                      setEditing({ id: s.id, value: e.target.value })
                    }
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        onRename(s, editing.value);
                        setEditing(null);
                      } else if (e.key === "Escape") {
                        setEditing(null);
                      }
                    }}
                    onBlur={() => setEditing(null)}
                  />
                ) : (
                  <>
                    <span
                      className="title"
                      title="双击重命名（存在 ReSession 别名中，不影响原生会话）"
                      onDoubleClick={(e) => {
                        e.stopPropagation();
                        setEditing({ id: s.id, value: s.title ?? "" });
                      }}
                    >
                      {s.title ?? "(无标题)"}
                    </span>
                    <button
                      className="rename-btn"
                      title="重命名"
                      onClick={(e) => {
                        e.stopPropagation();
                        setEditing({ id: s.id, value: s.title ?? "" });
                      }}
                    >
                      ✎
                    </button>
                  </>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      )}
    </aside>
  );
}
