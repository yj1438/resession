import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke, isTauri } from "../api";
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
  onPick: (cwd: string) => void;
  onClose: () => void;
}) {
  const [projects, setProjects] = useState<KnownProject[]>([]);
  const [manualPath, setManualPath] = useState("");

  useEffect(() => {
    if (!isTauri) return;
    invoke<KnownProject[]>("known_projects")
      .then(setProjects)
      .catch((e) => reportError("获取已知项目失败", String(e)));
  }, []);

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") onPick(dir);
  };

  return (
    <div className="np-backdrop" onClick={onClose}>
      <div className="np" onClick={(e) => e.stopPropagation()}>
        <div className="np-head">新会话 · 在哪个目录启动原生 claude？</div>
        <ul className="np-list">
          {projects.map((p) => (
            <li key={p.path} onClick={() => onPick(p.path)}>
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
                onPick(manualPath.trim());
              }
            }}
          />
        </div>
      </div>
    </div>
  );
}
