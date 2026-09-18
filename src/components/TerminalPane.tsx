import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { invoke, isTauri } from "../api";
import type { SessionMeta } from "../types";

// 每次选中会话后挂载即打开 PTY，运行 provider 给出的原生 resume 命令；
// 组件卸载（切走/关闭）时关闭 PTY。内容原样透传，不做任何解释。
export default function TerminalPane({ session }: { session: SessionMeta }) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const term = new Terminal({
      fontFamily: "Consolas, 'Cascadia Mono', monospace",
      fontSize: 13,
      cursorBlink: true,
      theme: { background: "#101418" },
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
        ptyId = await invoke<string>("resume_session", { meta: session });
        if (disposed) return;
        const id = ptyId;

        term.onData((d) => {
          void invoke("pty_write", { id, data: d }).catch(() => {});
        });
        const onResize = () => {
          void invoke("pty_resize", {
            id,
            rows: term.rows,
            cols: term.cols,
          }).catch(() => {});
        };
        term.onResize(onResize);
        onResize();

        unlistens.push(
          await listen<{ id: string; data: string }>("pty-out", (e) => {
            if (e.payload.id === id) term.write(e.payload.data);
          }),
        );
        unlistens.push(
          await listen<{ id: string }>("pty-exit", (e) => {
            if (e.payload.id === id)
              term.writeln("\r\n\x1b[90m[进程已退出]\x1b[0m");
          }),
        );
      } catch (e) {
        term.writeln(`\x1b[31m启动失败: ${String(e)}\x1b[0m`);
      }
    })();

    return () => {
      disposed = true;
      unlistens.forEach((u) => u());
      if (ptyId) void invoke("pty_close", { id: ptyId }).catch(() => {});
      ro.disconnect();
      term.dispose();
    };
  }, []);

  return <div className="terminal" ref={hostRef} />;
}
