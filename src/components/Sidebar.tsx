import type { SessionMeta } from "../types";

interface Props {
  sessions: SessionMeta[];
  selectedId: string | null;
  onSelect: (id: string) => void;
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

export default function Sidebar({ sessions, selectedId, onSelect }: Props) {
  return (
    <aside className="sidebar">
      <ul>
        {sessions.map((s) => (
          <li
            key={s.id}
            className={s.id === selectedId ? "session selected" : "session"}
            onClick={() => onSelect(s.id)}
          >
            <div className="row">
              <span className="time">{relativeTime(s.modifiedAt)}</span>
              <span className="project">{s.cwd ?? s.projectDir}</span>
            </div>
            <div className="row">
              <span className="title">{s.title ?? "(无标题)"}</span>
              <span className="count">{s.messageCount}</span>
            </div>
          </li>
        ))}
      </ul>
    </aside>
  );
}
