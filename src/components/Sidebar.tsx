import { useState } from "react";
import type { SessionMeta } from "../types";

interface Props {
  sessions: SessionMeta[];
  selectedId: string | null;
  activeIds: string[];
  busyIds: Set<string>;
  onSelect: (id: string) => void;
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
  onSelect,
  onRename,
}: Props) {
  const [editing, setEditing] = useState<{ id: string; value: string } | null>(
    null,
  );

  return (
    <aside className="sidebar">
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
    </aside>
  );
}
