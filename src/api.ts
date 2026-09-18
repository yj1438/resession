// Tauri invoke 封装：纯浏览器开发时 isTauri = false，组件自动走 mock 分支
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export const isTauri = "__TAURI_INTERNALS__" in window;

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri) throw new Error("not running inside Tauri");
  return tauriInvoke<T>(cmd, args);
}
