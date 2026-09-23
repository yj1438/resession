import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import "highlight.js/styles/github-dark.css";
import Highlight from "./Highlight";
import { invoke, isTauri } from "../api";
import type { Block, Event, SessionMeta } from "../types";

// 助手的 text 块按 markdown 渲染；用户消息保持纯文本。
// 已知限制：markdown 块内不做命中高亮（会破坏解析），纯文本块高亮
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

// memo：App 每 2s 的 tick 重渲染不应穿透到上万个 markdown 子树
const EventView = memo(function EventView({
  event,
  index,
  dim,
  highlight,
}: {
  event: Event;
  index?: number;
  dim?: boolean;
  highlight?: string;
}) {
  return (
    <div
      className={dim ? "evt sidechain-evt" : "evt"}
      data-event-index={index}
    >
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
});

// 连续的 sidechain 事件折叠为一个可展开块；搜索定位到段内时自动展开
const SidechainRun = memo(function SidechainRun({
  events,
  highlight,
  locateEventIndex,
}: {
  events: { index: number; event: Event }[];
  highlight?: string;
  locateEventIndex?: number;
}) {
  const [open, setOpen] = useState(false);

  const hitInside =
    locateEventIndex !== undefined &&
    events.some((e) => e.index === locateEventIndex);

  useEffect(() => {
    if (hitInside) setOpen(true);
  }, [hitInside]);

  return (
    <div className="sidechain">
      <button className="sidechain-toggle" onClick={() => setOpen(!open)}>
        🤖 子 agent 执行了 {events.length} 条消息 {open ? "▲" : "▼"}
      </button>
      {open &&
        events.map((e) => (
          <EventView
            key={e.index}
            event={e.event}
            index={e.index}
            dim
            highlight={highlight}
          />
        ))}
    </div>
  );
});

type IndexedEvent = { index: number; event: Event };
type Item =
  | { kind: "event"; index: number; event: Event }
  | { kind: "sidechain"; events: IndexedEvent[] };

// 把事件流按"主对话 / 连续 sidechain 段"分组；保留原始事件下标
// （与后端 SearchHit.event_index 同一坐标系，滚动定位依赖它）
function groupEvents(events: Event[]): Item[] {
  const items: Item[] = [];
  let pending: IndexedEvent[] = [];
  const flush = () => {
    if (pending.length > 0) {
      items.push({ kind: "sidechain", events: pending });
      pending = [];
    }
  };
  events.forEach((event, index) => {
    if (event.sidechain) {
      pending.push({ index, event });
    } else {
      flush();
      items.push({ kind: "event", index, event });
    }
  });
  flush();
  return items;
}

function itemRange(item: Item): [number, number] {
  return item.kind === "event"
    ? [item.index, item.index]
    : [item.events[0].index, item.events[item.events.length - 1].index];
}

const CHUNK = 250;
const LOCATE_BEFORE = 100;
const LOCATE_AFTER = 150;

export default function TranscriptPane({
  session,
  highlight,
  locateEventIndex,
  locateSequence = 0,
}: {
  session: SessionMeta;
  highlight?: string;
  locateEventIndex?: number;
  locateSequence?: number;
}) {
  const [events, setEvents] = useState<Event[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const retryRef = useRef<number | null>(null);
  // 窗口化渲染：只渲染 [start, end) 下标范围内的条目（大会话可流畅滚动）
  const [win, setWin] = useState<{ start: number; end: number }>({
    start: 0,
    end: CHUNK,
  });
  // 向上扩窗时的滚动锚点：渲染后按高度差补 scrollTop，视口不跳
  const anchorRef = useRef<number | null>(null);
  const topRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isTauri) return;
    setEvents(null);
    setError(null);
    setWin({ start: 0, end: CHUNK });
    // 仅按会话 id 加载一次：15s 重扫会替换 session 对象引用，
    // 若依赖引用会导致阅读中反复重载并滚回顶部
    invoke<Event[]>("load_transcript", { meta: session })
      .then(setEvents)
      .catch((e) => setError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.id]);

  const eventsLen = events?.length ?? 0;

  const items = useMemo(() => (events ? groupEvents(events) : []), [events]);
  const visibleItems = useMemo(
    () =>
      items.filter((it) => {
        const [lo, hi] = itemRange(it);
        return hi >= win.start && lo < win.end;
      }),
    [items, win.start, win.end],
  );

  const expandBottom = useCallback(() => {
    setWin((w) => ({ ...w, end: Math.min(eventsLen, w.end + CHUNK) }));
  }, [eventsLen]);

  const expandTop = useCallback(() => {
    const el = containerRef.current;
    if (el) anchorRef.current = el.scrollHeight;
    setWin((w) => ({ ...w, start: Math.max(0, w.start - CHUNK) }));
  }, []);

  // 窗口变化后：顶部扩展用高度差补滚动位置
  useLayoutEffect(() => {
    if (anchorRef.current === null) return;
    const el = containerRef.current;
    if (el) el.scrollTop += el.scrollHeight - anchorRef.current;
    anchorRef.current = null;
  }, [win]);

  // 哨兵 IntersectionObserver：接近视口边缘自动扩窗
  useEffect(() => {
    const el = containerRef.current;
    if (!el || events === null) return;
    const io = new IntersectionObserver(
      (entries) => {
        for (const en of entries) {
          if (!en.isIntersecting) continue;
          if (en.target === bottomRef.current) expandBottom();
          else if (en.target === topRef.current) expandTop();
        }
      },
      { root: el, rootMargin: "400px 0px" },
    );
    if (bottomRef.current) io.observe(bottomRef.current);
    if (topRef.current) io.observe(topRef.current);
    return () => io.disconnect();
  }, [events, expandBottom, expandTop]);

  // 滚动定位：把窗口开到命中下标附近（渲染后由重试循环滚到位）
  useEffect(() => {
    if (locateEventIndex === undefined || events === null) return;
    setWin((w) => {
      const s = Math.max(0, locateEventIndex - LOCATE_BEFORE);
      const e = Math.min(events.length, locateEventIndex + LOCATE_AFTER);
      if (w.start <= s && w.end >= e) return w;
      return { start: s, end: e };
    });
  }, [locateEventIndex, locateSequence, events]);

  // 定位滚动重试：窗口渲染 + sidechain 展开完成后找到节点，居中 + 闪烁
  useEffect(() => {
    if (locateEventIndex === undefined || events === null) return;
    let attempts = 0;
    let timer: number | null = null;
    const tryScroll = () => {
      const node = containerRef.current?.querySelector(
        `[data-event-index="${locateEventIndex}"]`,
      );
      if (node) {
        node.scrollIntoView({ block: "center", behavior: "smooth" });
        node.classList.add("locate-flash");
        window.setTimeout(() => node.classList.remove("locate-flash"), 2600);
        return;
      }
      if (attempts++ < 15 && retryRef.current !== null) {
        timer = window.setTimeout(tryScroll, 120);
      }
    };
    retryRef.current = 0;
    tryScroll();
    return () => {
      if (timer !== null) window.clearTimeout(timer);
      retryRef.current = null;
    };
  }, [locateEventIndex, locateSequence, events]);

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
    <div className="transcript" ref={containerRef}>
      <div className="chunk-sentinel" ref={topRef}>
        {win.start > 0 && (
          <span className="hint">
            ▲ 更早的 {win.start} 条已折叠，向上滚动自动加载
          </span>
        )}
      </div>
      {visibleItems.map((item) =>
        item.kind === "event" ? (
          <EventView
            key={item.index}
            event={item.event}
            index={item.index}
            highlight={highlight}
          />
        ) : (
          <SidechainRun
            key={item.events[0].index}
            events={item.events}
            highlight={highlight}
            locateEventIndex={locateEventIndex}
          />
        ),
      )}
      <div className="chunk-sentinel" ref={bottomRef}>
        {win.end < events.length && (
          <span className="hint">
            ▼ 已渲染 {win.end} / {events.length} 条，向下滚动自动加载
          </span>
        )}
      </div>
    </div>
  );
}
