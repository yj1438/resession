import { invoke, isTauri } from "./api";

// Mac App 直接写原生剪贴板，避免 WKWebView 的 copy 返回成功但实际内容为空。
// 其他平台/浏览器仍从点击或键盘事件同步复制，保留用户手势。
export async function copyText(text: string): Promise<void> {
  if (isTauri && /Mac/.test(navigator.platform)) {
    await invoke<void>("copy_text", { text });
    return;
  }
  const previousFocus = document.activeElement;
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.readOnly = true;
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  document.body.appendChild(textarea);

  let copied = false;
  try {
    textarea.focus({ preventScroll: true });
    textarea.select();
    copied = document.execCommand("copy");
  } catch {
    // 同步复制不可用时继续尝试异步接口；最终错误由调用方显示。
  } finally {
    textarea.remove();
    if (previousFocus instanceof HTMLElement) previousFocus.focus({ preventScroll: true });
  }
  if (copied) return;
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  throw new Error("系统剪贴板不可用，请重试复制");
}
