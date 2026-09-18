import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { invoke, isTauri } from "../api";
import type { SessionMeta } from "../types";

interface PtyEvent {
  id: string;
  data: number[];
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

// PTY 生命周期归后端（以会话 id 为键常驻）；本组件只是"观看窗口"：
// 挂载 = 附着（幂等 spawn + 回放缓冲），卸载 = 仅断开观看，不关闭 PTY。
// 手动关闭走页头按钮。字节级传输（Uint8Array），xterm 自带 UTF-8 状态机。
export default function TerminalPane({
  session,
  onPtyStateChange,
}: {
  session: SessionMeta;
  onPtyStateChange?: () => void;
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
    let ptyId: string | null = null;
    const unlistens: UnlistenFn[] = [];

    (async () => {
      if (!isTauri) {
        term.writeln(`\x1b[36m$\x1b[0m claude --resume ${session.id}`);
        term.writeln(
          `\x1b[33m[ReSession]\x1b[0m 浏览器模式无 PTY；请在 Tauri 窗口内使用`,
        );
        return;
      }
      try {
        // 先挂监听再 spawn，避免首帧输出落在监听之前
        unlistens.push(
          await listen<PtyEvent>("pty-out", (e) => {
            if (ptyId && e.payload.id === ptyId)
              term.write(Uint8Array.from(e.payload.data));
          }),
        );
        unlistens.push(
          await listen<PtyEvent>("pty-exit", (e) => {
            if (ptyId && e.payload.id === ptyId) {
              term.writeln("\r\n\x1b[90m[进程已退出]\x1b[0m");
              ptyId = null;
              ptyIdRef.current = null;
              onPtyStateChange?.();
            }
          }),
        );

        ptyId = await invoke<string>("resume_session", { meta: session });
        if (disposed) return;
        ptyIdRef.current = ptyId;

        // 切回已打开的会话：回放缓冲，重建画面
        const snap = await invoke<number[]>("pty_snapshot", { id: ptyId });
        if (disposed) return;
        if (snap.length > 0) term.write(Uint8Array.from(snap));

        term.onData((d) => {
          void invoke("pty_write", { id: ptyId, data: d }).catch(() => {});
        });
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

        onPtyStateChange?.();
      } catch (e) {
        term.writeln(`\x1b[31m启动失败: ${String(e)}\x1b[0m`);
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
    if (!id || !isTauri) return;
    void invoke("pty_close", { id }).catch(() => {});
  };

  return (
    <div className="terminal-wrap">
      <div className="terminal-bar">
        <span className="hint mono">
          原生 PTY · claude --resume {session.id.slice(0, 8)}…
        </span>
        <button className="term-close" onClick={closePty}>
          ⏹ 关闭终端
        </button>
      </div>
      <div className="terminal" ref={hostRef} />
    </div>
  );
}
