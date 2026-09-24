import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke, isTauri } from "../api";
import { PROVIDERS, type ProviderName } from "../providers";
import { reportError } from "./Toasts";

interface KnownProject {
  path: string;
  lastActive: string | null;
}

function relativeTime(iso: string | null): string {
  if (!iso) return "";
  const diff = Date.now() - new Date(iso).getTime();
  const min = Math.floor(diff / 60_000);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  return `${Math.floor(hr / 24)}d`;
}

// "➕ 新会话"面板：已知项目（按最近活跃）+ 系统文件夹选择器 + 手动路径
export default function NewSessionPanel({
  onPick,
  onClose,
}: {
  onPick: (cwd: string, provider: ProviderName) => void;
  onClose: () => void;
}) {
  const [projects, setProjects] = useState<KnownProject[]>([]);
  const [manualPath, setManualPath] = useState("");
  const [provider, setProvider] = useState<ProviderName>("claude");

  useEffect(() => {
    if (!isTauri) return;
    invoke<KnownProject[]>("known_projects")
      .then(setProjects)
      .catch((e) => reportError("获取已知项目失败", String(e)));
  }, []);

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") onPick(dir, provider);
  };

  return (
    <div className="np-backdrop" onClick={onClose}>
      <div className="np" onClick={(e) => e.stopPropagation()}>
        <div className="np-head">新会话 · 选择 Agent 和工作目录</div>
        <div className="provider-choice" role="group" aria-label="新会话 Agent">
          {PROVIDERS.map((item) => (
            <button
              key={item.value}
              className={provider === item.value ? "provider-choice-btn selected" : "provider-choice-btn"}
              aria-pressed={provider === item.value}
              onClick={() => setProvider(item.value)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <ul className="np-list">
          {projects.map((p) => (
            <li key={p.path} onClick={() => onPick(p.path, provider)}>
              <span className="np-path">{p.path}</span>
              <span className="np-time">{relativeTime(p.lastActive)}</span>
            </li>
          ))}
          {projects.length === 0 && (
            <li className="hint" onClick={(e) => e.stopPropagation()}>
              暂无已知项目，用下面两种方式选择目录
            </li>
          )}
        </ul>
        <div className="np-actions">
          <button className="np-pick" onClick={() => void pickFolder()}>
            📁 选择文件夹…
          </button>
          <input
            className="np-input"
            placeholder="或直接输入路径，回车打开"
            value={manualPath}
            onChange={(e) => setManualPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && manualPath.trim()) {
                onPick(manualPath.trim(), provider);
              }
            }}
          />
        </div>
      </div>
    </div>
  );
}
