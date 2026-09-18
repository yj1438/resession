import { useEffect, useState } from "react";
import { invoke, isTauri } from "../api";
import type { Block, Event, SessionMeta } from "../types";

function BlockView({ block }: { block: Block }) {
  switch (block.kind) {
    case "text":
      return <div className="evt-text">{block.text}</div>;
    case "toolUse":
      return (
        <div className="evt-tool">
          ⚙ {block.name}
          {block.brief && <span className="tool-brief"> {block.brief}</span>}
        </div>
      );
    case "toolResult":
      return <div className="evt-toolresult">↳ {block.brief}</div>;
    default:
      return null;
  }
}

export default function TranscriptPane({ session }: { session: SessionMeta }) {
  const [events, setEvents] = useState<Event[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    setEvents(null);
    setError(null);
    invoke<Event[]>("load_transcript", { meta: session })
      .then(setEvents)
      .catch((e) => setError(String(e)));
  }, [session]);

  // 浏览器模式：保持 M0 的元信息占位
  if (!isTauri) {
    return (
      <div className="transcript">
        <h3>{session.title ?? "(无标题)"}</h3>
        <dl>
          <dt>会话 ID</dt>
          <dd className="mono">{session.id}</dd>
          <dt>转录渲染</dt>
          <dd className="hint">浏览器模式无后端；在 Tauri 窗口内显示真实转录</dd>
        </dl>
      </div>
    );
  }

  if (error) return <div className="transcript hint">转录加载失败：{error}</div>;
  if (events === null) return <div className="transcript hint">加载中…</div>;
  if (events.length === 0)
    return <div className="transcript hint">空会话</div>;

  return (
    <div className="transcript">
      {events.map((e, i) => (
        <div key={i} className={`evt evt-${e.role}`}>
          <span className={`evt-role role-${e.role}`}>
            {e.role === "user" ? "你" : e.role === "assistant" ? "Claude" : "系统"}
          </span>
          <div className="evt-blocks">
            {e.blocks.map((b, j) => (
              <BlockView key={j} block={b} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
