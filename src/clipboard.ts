// 必须从点击/键盘事件中调用：WKWebView 的异步 Clipboard API 可能不可用，
// 先通过同步 copy 命令保留用户手势，再回退到浏览器 Clipboard API。
export async function copyText(text: string): Promise<void> {
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
