import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { invoke, isTauri } from "../api";
import { providerLabel } from "../providers";
import type { SessionMeta } from "../types";

interface PtyEvent {
  id: string;
  offset: number;
  data: number[];
}

interface PtySnapshot {
  offset: number;
  data: number[];
  exited: boolean;
}

// VS Code Dark+ 16 色盘，配合后端注入的 TERM/COLORTERM 让 TUI 全彩渲染
const TERMINAL_THEME = {
  background: "#101418",
  foreground: "#d4d4d4",
  cursor: "#aeafad",
  selectionBackground: "#264f78",
  black: "#000000",
  red: "#cd3131",
  green: "#0dbc79",
  yellow: "#e5e510",
  blue: "#2472c8",
  magenta: "#bc3fbc",
  cyan: "#11a8cd",
  white: "#e5e5e5",
  brightBlack: "#666666",
  brightRed: "#f14c4c",
  brightGreen: "#23d18b",
  brightYellow: "#f5f543",
  brightBlue: "#3b8eea",
  brightMagenta: "#d670d6",
  brightCyan: "#29b8db",
  brightWhite: "#e5e5e5",
};

// 终端观看窗口。attachPtyId 用于附着已启动的 PTY；session 提供已有会话的标题，
// 也保留直接进入时的幂等 resume 兜底。PTY 生命周期归后端；切换视图仅断开观看，
// 退出后的最终输出保留到用户关闭视图。字节级传输由 xterm 处理 UTF-8 状态。
export default function TerminalPane({
  session,
  attachPtyId,
  onPtyStateChange,
  onCloseView,
}: {
  session?: SessionMeta;
  attachPtyId?: string;
  onPtyStateChange?: () => void;
  onCloseView?: () => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const ptyIdRef = useRef<string | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const term = new Terminal({
      fontFamily: "Consolas, 'Cascadia Mono', monospace",
      fontSize: 13,
      cursorBlink: true,
      theme: TERMINAL_THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();

    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(host);

    let disposed = false;
    // 已知 id 要尽早设置：新会话可能在两个事件监听注册期间就退出。
    let ptyId: string | null =
      attachPtyId ?? (session ? `${session.provider}:${session.id}` : null);
    const unlistens: UnlistenFn[] = [];
    const pendingOutput: PtyEvent[] = [];
    let snapshotReady = false;
    let nextOffset = 0;
    let exited = false;
    let exitShown = false;

    const writeOutput = (event: PtyEvent) => {
      // 快照可能已经包含实时事件的前缀；只写尚未回放的字节。
      const skip = Math.max(0, nextOffset - event.offset);
      if (skip >= event.data.length) return;
      term.write(Uint8Array.from(event.data.slice(skip)));
      nextOffset = event.offset + event.data.length;
    };

    const flushPending = (offset: number) => {
      nextOffset = offset;
      snapshotReady = true;
      pendingOutput.sort((a, b) => a.offset - b.offset).forEach(writeOutput);
      pendingOutput.length = 0;
    };

    const showExited = () => {
      if (disposed || exitShown) return;
      exitShown = true;
      term.writeln("\r\n\x1b[90m[进程已退出]\x1b[0m");
      ptyId = null;
      ptyIdRef.current = null;
      onPtyStateChange?.();
    };

    // TUI 启动时会查询终端能力（如 DA / 光标位置）。xterm 解析这些输出时
    // 通过 onData 回送响应；必须在任何快照或实时输出写入之前接好输入桥，
    // 否则首次查询的响应丢失，CLI 可能一直停在空白画面。
    term.onData((data) => {
      if (disposed || !ptyId) return;
      void invoke("pty_write", { id: ptyId, data }).catch(() => {});
    });

    (async () => {
      if (!isTauri) {
        term.writeln(`\x1b[33m[ReSession]\x1b[0m 浏览器模式无 PTY；请在 Tauri 窗口内使用`);
        return;
      }
      try {
        // 先挂监听再 attach，避免首帧输出落在监听之前
        unlistens.push(
          await listen<PtyEvent>("pty-out", (e) => {
            if (disposed || !ptyId || e.payload.id !== ptyId) return;
            if (snapshotReady) writeOutput(e.payload);
            else pendingOutput.push(e.payload);
          }),
        );
        unlistens.push(
          await listen<PtyEvent>("pty-exit", (e) => {
            if (disposed || !ptyId || e.payload.id !== ptyId) return;
            exited = true;
            if (snapshotReady) showExited();
          }),
        );

        if (!attachPtyId && session) {
          ptyId = await invoke<string>("resume_session", { meta: session });
        }
        if (disposed || !ptyId) return;
        ptyIdRef.current = ptyId;

        const onResize = () => {
          if (!ptyId) return;
          void invoke("pty_resize", {
            id: ptyId,
            rows: term.rows,
            cols: term.cols,
          }).catch(() => {});
        };
        term.onResize(onResize);
        onResize();

        // 实时事件先入队；快照先写入，再按字节偏移补上快照之后的事件。
        // 避免较旧快照覆盖新输出（尤其是 TUI 的同步输出控制序列）。
        const snap = await invoke<PtySnapshot>("pty_snapshot", { id: ptyId });
        if (disposed) return;
        if (snap.data.length > 0) term.write(Uint8Array.from(snap.data));
        flushPending(snap.offset + snap.data.length);
        if (snap.exited || exited) showExited();
        else onPtyStateChange?.();
      } catch (e) {
        if (disposed) return;
        // 极短命进程的快照若已不可用，至少保留监听期间收到的诊断输出。
        pendingOutput.sort((a, b) => a.offset - b.offset);
        flushPending(pendingOutput[0]?.offset ?? 0);
        if (exited || String(e).includes("pty not found")) showExited();
        else term.writeln(`\x1b[31m启动失败: ${String(e)}\x1b[0m`);
      }
    })();

    return () => {
      disposed = true;
      unlistens.forEach((u) => u());
      // 不 invoke pty_close：PTY 常驻，切会话/切标签不终止
      ro.disconnect();
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const closePty = () => {
    const id = ptyIdRef.current;
    if (id && isTauri) void invoke("pty_close", { id }).catch(() => {});
    onCloseView?.();
  };

  return (
    <div className="terminal-wrap">
      <div className="terminal-bar">
        <span className="hint mono">
          {session
            ? `原生 PTY · ${session.provider} resume ${session.id.slice(0, 8)}…`
            : `新会话 · 原生 ${providerLabel(attachPtyId?.split(":")[1] ?? "claude")}`}
        </span>
        <button className="term-close" onClick={closePty}>
          ⏹ 关闭终端
        </button>
      </div>
      <div className="terminal" ref={hostRef} />
    </div>
  );
}
