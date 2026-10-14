import type { Terminal } from "@xterm/xterm";

// 菜单复制可能只产生 copy 事件，而不经过 xterm 的 keydown 回调。
// 在宿主捕获阶段接管有选区的复制，避免再次走 WebView 的默认复制路径。
export function bindTerminalCopy(
  host: HTMLElement,
  term: Pick<Terminal, "getSelection" | "attachCustomKeyEventHandler">,
  isMac: boolean,
  copy: (text: string, source: "key" | "copy") => void,
  nativeCopy = false,
): () => void {
  const onCopy = (event: ClipboardEvent) => {
    const selection = term.getSelection();
    if (!selection) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    copy(selection, "copy");
  };
  // 仅原生 Mac 通道接管 DOM copy；其他平台沿用 xterm 的 clipboardData，
  // 不在浏览器 copy 事件内部嵌套 execCommand("copy")。
  if (nativeCopy) host.addEventListener("copy", onCopy, { capture: true });

  term.attachCustomKeyEventHandler((event) => {
    const shortcut = isMac
      ? event.metaKey && !event.ctrlKey && !event.altKey
      : event.ctrlKey && event.shiftKey && !event.metaKey && !event.altKey;
    if (
      event.type !== "keydown" || !shortcut ||
      (event.key !== "c" && event.key !== "C")
    ) return true;
    const selection = term.getSelection();
    if (!selection) return true;
    event.preventDefault();
    copy(selection, "key");
    return false;
  });

  return () => {
    if (nativeCopy) host.removeEventListener("copy", onCopy, { capture: true });
    term.attachCustomKeyEventHandler(() => true);
  };
}
