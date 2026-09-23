import { useEffect, useState } from "react";

// 模块级单例 store：任意组件 import { reportError } 即可上报，
// 免去逐层传回调。ToastHost 只在 App 挂载一次。

export interface Toast {
  id: number;
  title: string;
  detail?: string;
}

type Listener = (items: Toast[]) => void;

const TOAST_MS = 7000;
const MAX_TOASTS = 4;

const items: Toast[] = [];
const timers = new Map<number, ReturnType<typeof setTimeout>>();
const listeners = new Set<Listener>();
let nextId = 1;

function notify() {
  const snapshot = [...items];
  for (const l of listeners) l(snapshot);
}

export function dismissToast(id: number) {
  const i = items.findIndex((t) => t.id === id);
  if (i >= 0) items.splice(i, 1);
  const t = timers.get(id);
  if (t) {
    clearTimeout(t);
    timers.delete(id);
  }
  notify();
}

/**
 * 统一错误上报：右下角 toast，7s 自动消失。
 * 相同 title+detail 去重（重置时限）——周期任务（如 15s 扫描）持续失败时
 * 只刷新一条，不会堆叠刷屏。
 */
export function reportError(title: string, detail?: string): void {
  const existing = items.find((t) => t.title === title && t.detail === detail);
  if (existing) {
    const prev = timers.get(existing.id);
    if (prev) clearTimeout(prev);
    timers.set(
      existing.id,
      setTimeout(() => dismissToast(existing.id), TOAST_MS),
    );
    return;
  }
  if (items.length >= MAX_TOASTS) dismissToast(items[0].id);
  const toast: Toast = { id: nextId++, title, detail };
  items.push(toast);
  timers.set(
    toast.id,
    setTimeout(() => dismissToast(toast.id), TOAST_MS),
  );
  notify();
}

export default function ToastHost() {
  const [list, setList] = useState<Toast[]>([]);
  useEffect(() => {
    const l: Listener = setList;
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  if (list.length === 0) return null;
  return (
    <div className="toast-host" aria-live="assertive">
      {list.map((t) => (
        <div className="toast" role="alert" key={t.id}>
          <div className="toast-body">
            <span className="toast-title">{t.title}</span>
            {t.detail && (
              <span className="toast-detail" title={t.detail}>
                {t.detail}
              </span>
            )}
          </div>
          <button
            className="toast-close"
            aria-label="关闭提示"
            onClick={() => dismissToast(t.id)}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
