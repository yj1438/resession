import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import type { SessionMeta } from "../types";

// M0：xterm 已就位但未接 PTY 后端；M1 由 invoke(resume) → portable-pty 双向桥
// 届时此处只增加 onData/emit 接线，终端实例与生命周期管理保持不变
export default function TerminalPane({ session }: { session: SessionMeta }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    if (!hostRef.current) return;
    const term = new Terminal({
      fontFamily: "Consolas, 'Cascadia Mono', monospace",
      fontSize: 13,
      theme: { background: "#101418" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(hostRef.current);
    fit.fit();
    termRef.current = term;

    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(hostRef.current);

    return () => {
      ro.disconnect();
      term.dispose();
      termRef.current = null;
    };
  }, []);

  useEffect(() => {
    termRef.current?.writeln("");
    termRef.current?.writeln(
      `\x1b[36m$\x1b[0m claude --resume ${session.id}`,
    );
    termRef.current?.writeln(
      `\x1b[33m[ReSession]\x1b[0m PTY 桥将在 M1 接入，届时此命令在真实终端中运行（cwd: ${session.cwd ?? "?"}）`,
    );
  }, [session.id, session.cwd]);

  return <div className="terminal" ref={hostRef} />;
}
