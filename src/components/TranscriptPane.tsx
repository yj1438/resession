import { useEffect, useMemo, useState } from "react";
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

function EventView({ event, dim }: { event: Event; dim?: boolean }) {
  return (
    <div className={dim ? "evt sidechain-evt" : "evt"}>
      <span className={`evt-role role-${event.role}`}>
        {event.role === "user" ? "你" : event.role === "assistant" ? "Claude" : "系统"}
      </span>
      <div className="evt-blocks">
        {event.blocks.map((b, j) => (
          <BlockView key={j} block={b} />
        ))}
      </div>
    </div>
  );
}

// 连续的 sidechain 事件折叠为一个可展开块（M1.5 方案 b）
function SidechainRun({ events }: { events: Event[] }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="sidechain">
      <button className="sidechain-toggle" onClick={() => setOpen(!open)}>
        🤖 子 agent 执行了 {events.length} 条消息 {open ? "▲" : "▼"}
      </button>
      {open && events.map((e, i) => <EventView key={i} event={e} dim />)}
    </div>
  );
}

type Item = { kind: "event"; event: Event } | { kind: "sidechain"; events: Event[] };

// 把事件流按"主对话 / 连续 sidechain 段"分组
function groupEvents(events: Event[]): Item[] {
  const items: Item[] = [];
  let pending: Event[] = [];
  const flush = () => {
    if (pending.length > 0) {
      items.push({ kind: "sidechain", events: pending });
      pending = [];
    }
  };
  for (const e of events) {
    if (e.sidechain) {
      pending.push(e);
    } else {
      flush();
      items.push({ kind: "event", event: e });
    }
  }
  flush();
  return items;
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

  const items = useMemo(() => (events ? groupEvents(events) : []), [events]);

  // 浏览器模式：保持元信息占位
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
  if (events.length === 0) return <div className="transcript hint">空会话</div>;

  return (
    <div className="transcript">
      {items.map((item, i) =>
        item.kind === "event" ? (
          <EventView key={i} event={item.event} />
        ) : (
          <SidechainRun key={i} events={item.events} />
        ),
      )}
    </div>
  );
}
