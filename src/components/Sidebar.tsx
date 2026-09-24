import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import Highlight from "./Highlight";
import { normalizePath, pathBaseName } from "../paths";
import { providerCanTrash, providerLabel, ptySessionId } from "../providers";
import type { SearchHit, SessionMeta } from "../types";

interface Props {
  sessions: SessionMeta[];
  selectedId: string | null;
  activeIds: string[];
  busyIds: Set<string>;
  hits: SearchHit[] | null;
  highlight: string;
  width: number;
  locateRequest: { id: string; sequence: number } | null;
  archivedIds: Set<string>;
  showArchived: boolean;
  archivedCount: number;
  onSelect: (id: string) => void;
  onOpenHit: (hit: SearchHit) => void;
  onRename: (session: SessionMeta, name: string) => void;
  onArchive: (sessions: SessionMeta[], archived: boolean) => void;
  onDelete: (sessions: SessionMeta[]) => void;
  onToggleShowArchived: () => void;
}

interface ProjectGroup {
  key: string;
  path: string;
  name: string;
  modifiedAt: string;
  sessions: SessionMeta[];
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

function projectIdentity(session: SessionMeta): { key: string; path: string } {
  const path = session.cwd || session.projectDir;
  return { key: normalizePath(path), path };
}

function sessionKey(session: SessionMeta): string {
  return ptySessionId(session.provider, session.id);
}

function groupSessions(sessions: SessionMeta[]): ProjectGroup[] {
  const byProject = new Map<string, ProjectGroup>();
  for (const session of sessions) {
    const { key, path } = projectIdentity(session);
    const modifiedAt = session.modifiedAt ?? "";
    const existing = byProject.get(key);
    if (existing) {
      existing.sessions.push(session);
      if (modifiedAt > existing.modifiedAt) existing.modifiedAt = modifiedAt;
    } else {
      byProject.set(key, {
        key,
        path,
        name: pathBaseName(path),
        modifiedAt,
        sessions: [session],
      });
    }
  }

  return [...byProject.values()]
    .map((group) => ({
      ...group,
      sessions: [...group.sessions].sort((a, b) =>
        (b.modifiedAt ?? "").localeCompare(a.modifiedAt ?? ""),
      ),
    }))
    .sort((a, b) => b.modifiedAt.localeCompare(a.modifiedAt));
}

export default function Sidebar({
  sessions,
  selectedId,
  activeIds,
  busyIds,
  hits,
  highlight,
  width,
  locateRequest,
  archivedIds,
  showArchived,
  archivedCount,
  onSelect,
  onOpenHit,
  onRename,
  onArchive,
  onDelete,
  onToggleShowArchived,
}: Props) {
  const [editing, setEditing] = useState<{ id: string; value: string } | null>(
    null,
  );
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [pendingLocateId, setPendingLocateId] = useState<string | null>(null);
  const sessionNodes = useRef(new Map<string, HTMLLIElement>());
  const handledLocateSequence = useRef(0);
  const groups = useMemo(() => groupSessions(sessions), [sessions]);
  const groupsRef = useRef(groups);
  groupsRef.current = groups;

  // 只响应显式定位请求；sessions 的 15s 重扫不会重新展开或拉动滚动位置。
  useEffect(() => {
    if (
      !locateRequest ||
      hits ||
      handledLocateSequence.current === locateRequest.sequence
    ) {
      return;
    }
    const group = groupsRef.current.find((item) =>
      item.sessions.some((session) => sessionKey(session) === locateRequest.id),
    );
    if (!group) return;
    handledLocateSequence.current = locateRequest.sequence;
    setPendingLocateId(locateRequest.id);
    setCollapsed((previous) => {
      if (!previous.has(group.key)) return previous;
      const next = new Set(previous);
      next.delete(group.key);
      return next;
    });
  }, [hits, locateRequest]);

  // 折叠状态提交、目标节点真正挂载后再滚动，避免依赖 rAF 调度时序。
  useLayoutEffect(() => {
    if (!pendingLocateId) return;
    const node = sessionNodes.current.get(pendingLocateId);
    if (!node) return;
    node.scrollIntoView({ block: "nearest", behavior: "smooth" });
    setPendingLocateId(null);
  }, [collapsed, groups, pendingLocateId]);

  return (
    <aside id="session-sidebar" className="sidebar" style={{ width }}>
      {hits ? (
        <ul className="hits">
          {hits.map((hit) => (
            <li
              key={`${sessionKey(hit.session)}:${hit.eventIndex}`}
              className={
                sessionKey(hit.session) === selectedId ? "session hit selected" : "session hit"
              }
              onClick={() => onOpenHit(hit)}
            >
              <div className="row">
                <span className={`hit-role role-${hit.role}`}>
                  {hit.role === "user" ? "你" : hit.role === "assistant" ? providerLabel(hit.session.provider) : "系统"}
                </span>
                <span className="provider-badge">{providerLabel(hit.session.provider)}</span>
                <span className="project">{hit.session.title ?? "(无标题)"}</span>
                {hit.sidechain && <span title="子 agent 消息">🤖</span>}
              </div>
              <div className="hit-snippet">
                <Highlight text={hit.snippet} query={highlight} />
              </div>
            </li>
          ))}
          {hits.length === 0 && <li className="hint empty-hits">无命中</li>}
        </ul>
      ) : (
        <>
        <div className="project-tree">
          {groups.map((group) => {
            const isCollapsed = collapsed.has(group.key);
            const containsSelected = group.sessions.some((s) => sessionKey(s) === selectedId);
            return (
              <section
                className={containsSelected ? "project-group has-selected" : "project-group"}
                key={group.key}
              >
                <div
                  className={containsSelected ? "project-header has-selected" : "project-header"}
                >
                  <button
                    className="project-header-main"
                    title={group.path}
                    aria-expanded={!isCollapsed}
                    onClick={() =>
                      setCollapsed((previous) => {
                        const next = new Set(previous);
                        if (next.has(group.key)) next.delete(group.key);
                        else next.add(group.key);
                        return next;
                      })
                    }
                  >
                    <span className="project-chevron">{isCollapsed ? "▸" : "▾"}</span>
                    <span className="project-icon" aria-hidden="true" />
                    <span className="project-name">{group.name}</span>
                  </button>
                  <span className="row-actions">
                    <button
                      className="row-btn"
                      title="归档该项目全部会话"
                      onClick={() => onArchive(group.sessions, true)}
                    >
                      🗄
                    </button>
                    {group.sessions.every((session) => providerCanTrash(session.provider)) && (
                      <button
                        className="row-btn"
                        title="删除该项目全部会话（移入废纸篓）"
                        onClick={() => onDelete(group.sessions)}
                      >
                        🗑
                      </button>
                    )}
                  </span>
                  <span className="project-count">{group.sessions.length}</span>
                </div>

                {!isCollapsed && (
                  <ul>
                    {group.sessions.map((session) => {
                      const key = sessionKey(session);
                      const running = activeIds.includes(key);
                      const busy = busyIds.has(key);
                      const isEditing = editing?.id === key;
                      const isArchived = archivedIds.has(key);
                      return (
                        <li
                          key={key}
                          ref={(node) => {
                            if (node) sessionNodes.current.set(key, node);
                            else sessionNodes.current.delete(key);
                          }}
                          className={[
                            key === selectedId ? "session selected" : "session",
                            isArchived ? "archived" : "",
                          ]
                            .filter(Boolean)
                            .join(" ")}
                          onClick={() => onSelect(key)}
                        >
                          <div className="row session-title-row">
                            {isEditing ? (
                              <input
                                className="rename-input"
                                autoFocus
                                value={editing.value}
                                onClick={(event) => event.stopPropagation()}
                                onChange={(event) =>
                                  setEditing({ id: key, value: event.target.value })
                                }
                                onKeyDown={(event) => {
                                  if (event.key === "Enter") {
                                    onRename(session, editing.value);
                                    setEditing(null);
                                  } else if (event.key === "Escape") {
                                    setEditing(null);
                                  }
                                }}
                                onBlur={() => setEditing(null)}
                              />
                            ) : (
                              <>
                                <span
                                  className="title"
                                  title={`${session.title ?? "(无标题)"}\n双击重命名（不影响原生会话）`}
                                  onDoubleClick={(event) => {
                                    event.stopPropagation();
                                    setEditing({ id: key, value: session.title ?? "" });
                                  }}
                                >
                                  {session.title ?? "(无标题)"}
                                </span>
                                <span className="provider-badge">{providerLabel(session.provider)}</span>
                                <span className="row-actions">
                                  {isArchived ? (
                                    <button
                                      className="row-btn"
                                      title="恢复到列表"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        onArchive([session], false);
                                      }}
                                    >
                                      ♻
                                    </button>
                                  ) : (
                                    <button
                                      className="row-btn"
                                      title="归档（列表隐藏，可恢复）"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        onArchive([session], true);
                                      }}
                                    >
                                      🗄
                                    </button>
                                  )}
                                  {providerCanTrash(session.provider) && (
                                    <button
                                      className="row-btn"
                                      title="删除（移入废纸篓）"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        onDelete([session]);
                                      }}
                                    >
                                      🗑
                                    </button>
                                  )}
                                  <button
                                    className="rename-btn"
                                    title="重命名"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      setEditing({ id: key, value: session.title ?? "" });
                                    }}
                                  >
                                    ✎
                                  </button>
                                </span>
                              </>
                            )}
                          </div>
                          <div className="session-meta">
                            <span className="time">{relativeTime(session.modifiedAt)}</span>
                            {isArchived && <span className="archived-tag">已归档</span>}
                            {running && (
                              <span
                                className={busy ? "dot busy" : "dot idle"}
                                title={busy ? "任务执行中" : "空闲等待输入"}
                              >
                                {busy ? "● 执行中" : "○ 空闲"}
                              </span>
                            )}
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            );
          })}
          {groups.length === 0 && <div className="hint sidebar-empty">无会话</div>}
        </div>
        {archivedCount > 0 && (
          <button className="archived-toggle" onClick={onToggleShowArchived}>
            {showArchived ? "隐藏已归档" : `显示已归档（${archivedCount}）`}
          </button>
        )}
        </>
      )}
    </aside>
  );
}
