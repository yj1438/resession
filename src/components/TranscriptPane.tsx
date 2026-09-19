import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import "highlight.js/styles/github-dark.css";
import Highlight from "./Highlight";
import { invoke, isTauri } from "../api";
import type { Block, Event, SessionMeta } from "../types";

// 助手的 text 块按 markdown 渲染；用户消息保持纯文本。
// v1 已知限制：markdown 块内不做命中高亮（会破坏解析），纯文本块高亮
function BlockView({
  block,
  markdown,
  highlight,
}: {
  block: Block;
  markdown: boolean;
  highlight?: string;
}) {
  switch (block.kind) {
    case "text":
      return markdown ? (
        <div className="md">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeHighlight]}
          >
            {block.text}
          </ReactMarkdown>
        </div>
      ) : (
        <div className="evt-text">
          <Highlight text={block.text} query={highlight} />
        </div>
      );
    case "toolUse":
      return (
        <details className="tool">
          <summary>⚙ {block.name}</summary>
          <pre className="tool-body">{block.brief}</pre>
        </details>
      );
    case "toolResult":
      return (
        <details className="tool">
          <summary>↳ 结果</summary>
          <pre className="tool-body">{block.brief}</pre>
        </details>
      );
    default:
      return null;
  }
}

function EventView({
  event,
  dim,
  highlight,
}: {
  event: Event;
  dim?: boolean;
  highlight?: string;
}) {
  return (
    <div className={dim ? "evt sidechain-evt" : "evt"}>
      <span className={`evt-role role-${event.role}`}>
        {event.role === "user" ? "你" : event.role === "assistant" ? "Claude" : "系统"}
      </span>
      <div className="evt-blocks">
        {event.blocks.map((b, j) => (
          <BlockView
            key={j}
            block={b}
            markdown={event.role === "assistant"}
            highlight={highlight}
          />
        ))}
      </div>
    </div>
  );
}

// 连续的 sidechain 事件折叠为一个可展开块
function SidechainRun({
  events,
  highlight,
}: {
  events: Event[];
  highlight?: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="sidechain">
      <button className="sidechain-toggle" onClick={() => setOpen(!open)}>
        🤖 子 agent 执行了 {events.length} 条消息 {open ? "▲" : "▼"}
      </button>
      {open && events.map((e, i) => <EventView key={i} event={e} dim highlight={highlight} />)}
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

export default function TranscriptPane({
  session,
  highlight,
}: {
  session: SessionMeta;
  highlight?: string;
}) {
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
          <EventView key={i} event={item.event} highlight={highlight} />
        ) : (
          <SidechainRun key={i} events={item.events} highlight={highlight} />
        ),
      )}
    </div>
  );
}
